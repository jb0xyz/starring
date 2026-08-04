use std::sync::Arc;
use std::time::Duration;
use std::{collections::BTreeSet, iter};

use futures_util::StreamExt;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT_ENCODING, AUTHORIZATION, CONNECTION, CONTENT_LENGTH,
    HOST, TRANSFER_ENCODING, UPGRADE,
};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::{TokioIo, TokioTimer};
use percent_encoding::percent_decode_str;
use reqwest::redirect::Policy;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;

use crate::state::{IndeterminateClaim, ResourceKind, ResourceReservation, SharedState};
use crate::Config;

const MAX_HTTP_CONNECTIONS: usize = 16;
const MAX_REQUEST_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 24 * 1024;
const MAX_AUTHORIZATION_BYTES: usize = 4096;
const MAX_AUDIT_REASON_BYTES: usize = 256;
const MAX_REQUEST_BODY_BYTES: usize = 128 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 2048;
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(15);
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DOWNSTREAM_BODY_TIMEOUT: Duration = Duration::from_secs(5);
const DOWNSTREAM_CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);
const HTTP_CONNECTION_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("http_bind_failed")]
    Bind,
    #[error("http_accept_failed")]
    Accept,
    #[error("http_client_failed")]
    Client,
    #[error("http_connection_failed")]
    Connection,
    #[error("http_connection_drain_failed")]
    ConnectionDrain,
}

#[derive(Debug, Error)]
enum ProxyServiceError {
    #[error("upstream_unavailable")]
    Upstream,
    #[error("indeterminate_disconnect")]
    InjectedDisconnect,
}

struct ValidatedRequest {
    method: Method,
    path_and_query: String,
    headers: HeaderMap,
    body: Bytes,
    create_role: bool,
    create_channel: bool,
    create_message_channel: Option<String>,
    current_user: bool,
    delete_resource: Option<DeleteResource>,
    decoded_audit_reason: Option<String>,
}

enum DeleteResource {
    Role(String),
    Channel(String),
    Message {
        channel_id: String,
        message_id: String,
    },
}

impl DeleteResource {
    fn unknown_code(&self) -> u64 {
        match self {
            Self::Role(_) => 10011,
            Self::Channel(_) => 10003,
            Self::Message { .. } => 10008,
        }
    }
}

pub async fn serve(
    config: Config,
    state: Arc<SharedState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), HttpError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .connect_timeout(UPSTREAM_CONNECT_TIMEOUT)
        .timeout(UPSTREAM_TIMEOUT)
        .build()
        .map_err(|_| HttpError::Client)?;
    let listener = TcpListener::bind(config.http_listen())
        .await
        .map_err(|_| HttpError::Bind)?;
    state.mark_effect_http_listener_ready();
    let permits = Arc::new(Semaphore::new(MAX_HTTP_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            joined = connections.join_next(), if !connections.is_empty() => {
                if joined.is_some_and(|result| result.is_err()) {
                    return Err(HttpError::Connection);
                }
            }
            accepted = listener.accept() => {
                let (stream, address) = accepted.map_err(|_| HttpError::Accept)?;
                if !address.ip().is_loopback() {
                    continue;
                }
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                    continue;
                };
                let config = config.clone();
                let state = Arc::clone(&state);
                let client = client.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let service = service_fn(move |request| {
                        proxy_request(request, config.clone(), Arc::clone(&state), client.clone())
                    });
                    let connection = http1::Builder::new()
                        .timer(TokioTimer::new())
                        .header_read_timeout(DOWNSTREAM_BODY_TIMEOUT)
                        .keep_alive(false)
                        .max_headers(MAX_REQUEST_HEADERS)
                        .max_buf_size(MAX_HEADER_BYTES)
                        .serve_connection(TokioIo::new(stream), service);
                    let _ = tokio::time::timeout(DOWNSTREAM_CONNECTION_TIMEOUT, connection).await;
                });
            }
        }
    }
    let drained = tokio::time::timeout(HTTP_CONNECTION_DRAIN_TIMEOUT, async {
        let mut failed = false;
        while let Some(joined) = connections.join_next().await {
            failed |= joined.is_err();
        }
        failed
    })
    .await;
    match drained {
        Ok(false) => Ok(()),
        Ok(true) => Err(HttpError::Connection),
        Err(_) => {
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            Err(HttpError::ConnectionDrain)
        }
    }
}

async fn proxy_request(
    request: Request<Incoming>,
    config: Config,
    state: Arc<SharedState>,
    client: reqwest::Client,
) -> Result<Response<Full<Bytes>>, ProxyServiceError> {
    let validated = match tokio::time::timeout(
        DOWNSTREAM_BODY_TIMEOUT,
        validate_request(request, &config, &state),
    )
    .await
    {
        Ok(Ok(validated)) => validated,
        Ok(Err(status)) => {
            state.record_rejected_http();
            return Ok(empty_response(status));
        }
        Err(_) => {
            state.record_rejected_http();
            return Ok(empty_response(StatusCode::REQUEST_TIMEOUT));
        }
    };
    let mut reservation: Option<ResourceReservation> = if validated.create_role {
        state.reserve_resource(ResourceKind::Role)
    } else if validated.create_channel {
        state.reserve_resource(ResourceKind::Channel)
    } else if let Some(channel_id) = validated.create_message_channel.as_ref() {
        state.reserve_resource(ResourceKind::Message {
            channel_id: channel_id.clone(),
        })
    } else {
        None
    };
    if (validated.create_role
        || validated.create_channel
        || validated.create_message_channel.is_some())
        && reservation.is_none()
    {
        state.record_rejected_http();
        return Ok(empty_response(StatusCode::SERVICE_UNAVAILABLE));
    }
    let mut claim = if validated.create_role {
        validated
            .decoded_audit_reason
            .as_deref()
            .and_then(|reason| state.claim_indeterminate(reason))
    } else {
        None
    };
    state.record_forwarded_http();
    let upstream_url = format!("{}{}", config.http_upstream(), validated.path_and_query);
    let upstream = match client
        .request(validated.method.clone(), upstream_url)
        .headers(validated.headers.clone())
        .body(validated.body.clone())
        .send()
        .await
    {
        Ok(upstream) => upstream,
        Err(_) => {
            restore_claim(&state, claim.take());
            eprintln!("d2_transport_http_failure=upstream_request");
            return Err(ProxyServiceError::Upstream);
        }
    };
    let status = upstream.status();
    if validate_response_headers(upstream.headers()).is_err() {
        restore_claim(&state, claim.take());
        eprintln!("d2_transport_http_failure=response_headers");
        return Err(ProxyServiceError::Upstream);
    }
    if upstream
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64)
    {
        restore_claim(&state, claim.take());
        eprintln!("d2_transport_http_failure=response_content_length");
        return Err(ProxyServiceError::Upstream);
    }
    let headers = upstream.headers().clone();
    let mut body = Vec::new();
    let mut stream = upstream.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => {
                restore_claim(&state, claim.take());
                eprintln!("d2_transport_http_failure=response_stream");
                return Err(ProxyServiceError::Upstream);
            }
        };
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BODY_BYTES {
            restore_claim(&state, claim.take());
            eprintln!("d2_transport_http_failure=response_body_limit");
            return Err(ProxyServiceError::Upstream);
        }
        body.extend_from_slice(&chunk);
    }
    if status.is_success() && reservation.is_some() {
        let Some(id) = extract_response_id(&body) else {
            restore_claim(&state, claim.take());
            eprintln!("d2_transport_http_failure=resource_identity");
            return Err(ProxyServiceError::Upstream);
        };
        if !reservation.take().expect("resource reservation").commit(id) {
            restore_claim(&state, claim.take());
            eprintln!("d2_transport_http_failure=resource_commit");
            return Err(ProxyServiceError::Upstream);
        }
    }
    let current_user_identity_matches =
        extract_response_id(&body).as_deref() == Some(config.bot_user_id());
    if validated.current_user && (!status.is_success() || !current_user_identity_matches) {
        restore_claim(&state, claim.take());
        eprintln!(
            "d2_transport_http_failure=current_user_identity status={} identity_matches={}",
            status.as_u16(),
            current_user_identity_matches
        );
        return Err(ProxyServiceError::Upstream);
    }
    if let Some(resource) = validated.delete_resource {
        let exact_not_found = status == StatusCode::NOT_FOUND
            && exact_unknown_resource(&body, resource.unknown_code());
        if status == StatusCode::NOT_FOUND && !exact_not_found {
            restore_claim(&state, claim.take());
            eprintln!("d2_transport_http_failure=resource_delete_confirmation");
            return Err(ProxyServiceError::Upstream);
        }
        if status.is_success() || exact_not_found {
            let recorded = match resource {
                DeleteResource::Role(id) => state.remove_role(&id),
                DeleteResource::Channel(id) => state.remove_channel(&id),
                DeleteResource::Message {
                    channel_id,
                    message_id,
                } => state.remove_message(&channel_id, &message_id),
            };
            if !recorded {
                restore_claim(&state, claim.take());
                eprintln!("d2_transport_http_failure=resource_delete_commit");
                return Err(ProxyServiceError::Upstream);
            }
        }
    }
    if let Some(claim) = claim.take() {
        if state.finish_indeterminate(claim, Some(status.as_u16())) {
            return Err(ProxyServiceError::InjectedDisconnect);
        }
    }
    build_response(status, &headers, Bytes::from(body))
}

fn restore_claim(state: &SharedState, claim: Option<IndeterminateClaim>) {
    if let Some(claim) = claim {
        let _ = state.finish_indeterminate(claim, None);
    }
}

async fn validate_request(
    request: Request<Incoming>,
    config: &Config,
    state: &SharedState,
) -> Result<ValidatedRequest, StatusCode> {
    let (parts, mut body) = request.into_parts();
    validate_uri(&parts.uri, config)?;
    validate_method_and_path(&parts.method, &parts.uri, config, state)?;
    validate_headers(&parts.headers, config)?;
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| StatusCode::BAD_REQUEST)?;
        let data = frame.into_data().map_err(|_| StatusCode::BAD_REQUEST)?;
        if bytes.len().saturating_add(data.len()) > MAX_REQUEST_BODY_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        bytes.extend_from_slice(&data);
    }
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .ok_or(StatusCode::BAD_REQUEST)?
        .to_owned();
    let create_role = parts.method == Method::POST
        && parts.uri.path() == format!("/api/v10/guilds/{}/roles", config.guild_id())
        && parts.uri.query().is_none();
    let create_channel = parts.method == Method::POST
        && parts.uri.path() == format!("/api/v10/guilds/{}/channels", config.guild_id())
        && parts.uri.query().is_none();
    let create_message_channel = if parts.method == Method::POST && parts.uri.query().is_none() {
        match parts.uri.path().split('/').collect::<Vec<_>>().as_slice() {
            ["", "api", "v10", "channels", channel, "messages"] => Some((*channel).to_owned()),
            _ => None,
        }
    } else {
        None
    };
    let current_user = parts.method == Method::GET
        && parts.uri.path() == "/api/v10/users/@me"
        && parts.uri.query().is_none();
    let delete_resource = if parts.method == Method::DELETE && parts.uri.query().is_none() {
        match parts.uri.path().split('/').collect::<Vec<_>>().as_slice() {
            ["", "api", "v10", "guilds", _, "roles", role] => {
                Some(DeleteResource::Role((*role).to_owned()))
            }
            ["", "api", "v10", "channels", channel] => {
                Some(DeleteResource::Channel((*channel).to_owned()))
            }
            ["", "api", "v10", "channels", channel, "messages", message] => {
                Some(DeleteResource::Message {
                    channel_id: (*channel).to_owned(),
                    message_id: (*message).to_owned(),
                })
            }
            _ => None,
        }
    } else {
        None
    };
    let decoded_audit_reason = parts
        .headers
        .get("x-audit-log-reason")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| percent_decode_str(value).decode_utf8().ok())
        .map(|value| value.into_owned());
    let headers = forward_request_headers(&parts.headers);
    Ok(ValidatedRequest {
        method: parts.method,
        path_and_query,
        headers,
        body: Bytes::from(bytes),
        create_role,
        create_channel,
        create_message_channel,
        current_user,
        delete_resource,
        decoded_audit_reason,
    })
}

fn validate_uri(uri: &Uri, config: &Config) -> Result<(), StatusCode> {
    if uri.scheme().is_some() || uri.authority().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if path_and_query.len() > MAX_PATH_BYTES
        || !uri.path().starts_with("/api/v10/")
        || uri.path().contains("//")
        || uri
            .path()
            .split('/')
            .any(|segment| matches!(segment, "." | ".."))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let segments = uri.path().split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["", "api", "v10", "guilds", guild, ..] if *guild == config.guild_id() => Ok(()),
        ["", "api", "v10", "channels", channel, ..] if crate::config::valid_snowflake(channel) => {
            Ok(())
        }
        ["", "api", "v10", "users", "@me"] => Ok(()),
        _ => Err(StatusCode::FORBIDDEN),
    }
}

fn validate_method_and_path(
    method: &Method,
    uri: &Uri,
    config: &Config,
    state: &SharedState,
) -> Result<(), StatusCode> {
    let segments = uri.path().split('/').collect::<Vec<_>>();
    let allowed = match segments.as_slice() {
        ["", "api", "v10", "guilds", guild, "roles"] => {
            *guild == config.guild_id()
                && (*method == Method::GET || (*method == Method::POST && uri.query().is_none()))
        }
        ["", "api", "v10", "guilds", guild, "roles", role] => {
            *guild == config.guild_id()
                && state.owns_role(role)
                && (*method == Method::GET
                    || (matches!(*method, Method::PATCH | Method::DELETE) && uri.query().is_none()))
        }
        ["", "api", "v10", "guilds", guild, "channels"] => {
            *guild == config.guild_id()
                && (*method == Method::GET || (*method == Method::POST && uri.query().is_none()))
        }
        ["", "api", "v10", "guilds", guild, "audit-logs"] => {
            *guild == config.guild_id() && *method == Method::GET
        }
        ["", "api", "v10", "guilds", guild, "members", member] => {
            *guild == config.guild_id()
                && (*member == config.bot_user_id() || *member == config.actor_id())
                && *method == Method::GET
        }
        ["", "api", "v10", "guilds", guild, "members", actor, "roles", role] => {
            *guild == config.guild_id()
                && *actor == config.actor_id()
                && state.owns_role(role)
                && matches!(*method, Method::PUT | Method::DELETE)
                && uri.query().is_none()
        }
        ["", "api", "v10", "channels", channel] => {
            state.owns_channel(channel)
                && (*method == Method::GET
                    || (matches!(*method, Method::PATCH | Method::DELETE) && uri.query().is_none()))
        }
        ["", "api", "v10", "channels", channel, "permissions", target] => {
            state.owns_channel(channel)
                && (state.owns_role(target)
                    || *target == config.guild_id()
                    || *target == config.actor_id())
                && matches!(*method, Method::PUT | Method::DELETE)
                && uri.query().is_none()
        }
        ["", "api", "v10", "channels", channel, "messages"] => {
            uri.query().is_none()
                && ((*method == Method::POST && state.admits_message_creation(channel))
                    || (*method == Method::GET && state.owns_channel(channel)))
        }
        ["", "api", "v10", "channels", channel, "messages", message] => {
            uri.query().is_none()
                && state.owns_message(channel, message)
                && ((state.owns_channel(channel)
                    && matches!(*method, Method::GET | Method::PATCH | Method::DELETE))
                    || (*channel == config.hub_channel_id()
                        && matches!(*method, Method::GET | Method::DELETE)))
        }
        ["", "api", "v10", "users", "@me"] => *method == Method::GET && uri.query().is_none(),
        _ => false,
    };
    if allowed {
        Ok(())
    } else if matches!(
        *method,
        Method::GET | Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        Err(StatusCode::FORBIDDEN)
    } else {
        Err(StatusCode::METHOD_NOT_ALLOWED)
    }
}

fn validate_headers(headers: &HeaderMap, config: &Config) -> Result<(), StatusCode> {
    if headers.len() > MAX_REQUEST_HEADERS {
        return Err(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
    }
    let total = headers.iter().try_fold(0_usize, |total, (name, value)| {
        total
            .checked_add(name.as_str().len())?
            .checked_add(value.as_bytes().len())
    });
    if total.is_none_or(|total| total > MAX_HEADER_BYTES) {
        return Err(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
    }
    let host = exactly_one_header(headers, HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if host != config.http_listen().to_string() {
        return Err(StatusCode::FORBIDDEN);
    }
    let authorization = exactly_one_header(headers, AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if authorization.len() > MAX_AUTHORIZATION_BYTES
        || !authorization.starts_with("Bot ")
        || authorization.len() <= 4
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let audit_reasons = headers.get_all("x-audit-log-reason");
    let mut audit_reasons = audit_reasons.iter();
    if let Some(reason) = audit_reasons.next() {
        if audit_reasons.next().is_some() || reason.as_bytes().len() > MAX_AUDIT_REASON_BYTES {
            return Err(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
        }
    }
    Ok(())
}

fn forward_request_headers(source: &HeaderMap) -> HeaderMap {
    let mut destination = HeaderMap::new();
    let connection_headers = connection_header_names(source);
    for (name, value) in source {
        if !hop_by_hop(name)
            && *name != HOST
            && *name != CONTENT_LENGTH
            && *name != ACCEPT_ENCODING
            && !connection_headers.contains(name.as_str())
            && !name.as_str().starts_with("x-forwarded-")
        {
            destination.append(name.clone(), value.clone());
        }
    }
    destination.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    destination
}

fn build_response(
    status: reqwest::StatusCode,
    source_headers: &reqwest::header::HeaderMap,
    body: Bytes,
) -> Result<Response<Full<Bytes>>, ProxyServiceError> {
    let mut builder = Response::builder().status(status.as_u16());
    let headers = builder.headers_mut().ok_or(ProxyServiceError::Upstream)?;
    let connection_headers = connection_header_names(source_headers);
    for (name, value) in source_headers {
        let name = HeaderName::from_bytes(name.as_str().as_bytes())
            .map_err(|_| ProxyServiceError::Upstream)?;
        if hop_by_hop(&name) || name == CONTENT_LENGTH || connection_headers.contains(name.as_str())
        {
            continue;
        }
        let value =
            HeaderValue::from_bytes(value.as_bytes()).map_err(|_| ProxyServiceError::Upstream)?;
        headers.append(name, value);
    }
    headers.insert(CONTENT_LENGTH, HeaderValue::from(body.len()));
    builder
        .body(Full::new(body))
        .map_err(|_| ProxyServiceError::Upstream)
}

fn validate_response_headers(headers: &HeaderMap) -> Result<(), ()> {
    if headers.len() > MAX_REQUEST_HEADERS {
        return Err(());
    }
    let total = headers.iter().try_fold(0_usize, |total, (name, value)| {
        total
            .checked_add(name.as_str().len())?
            .checked_add(value.as_bytes().len())
    });
    if total.is_none_or(|total| total > MAX_HEADER_BYTES) {
        Err(())
    } else {
        Ok(())
    }
}

fn exactly_one_header(headers: &HeaderMap, name: HeaderName) -> Option<&HeaderValue> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn connection_header_names(headers: &HeaderMap) -> BTreeSet<String> {
    headers
        .get_all(CONNECTION)
        .iter()
        .flat_map(|value| value.to_str().ok().into_iter())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .flat_map(|value| iter::once(value.to_ascii_lowercase()))
        .collect()
}

fn hop_by_hop(name: &HeaderName) -> bool {
    *name == CONNECTION
        || *name == TRANSFER_ENCODING
        || *name == UPGRADE
        || matches!(
            name.as_str(),
            "keep-alive" | "proxy-authenticate" | "proxy-authorization" | "te" | "trailer"
        )
}

fn extract_response_id(body: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let id = value.get("id")?.as_str()?;
    crate::config::valid_snowflake(id).then(|| id.to_owned())
}

fn exact_unknown_resource(body: &[u8], expected_code: u64) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("code").and_then(serde_json::Value::as_u64))
        == Some(expected_code)
}

fn empty_response(status: StatusCode) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(CONTENT_LENGTH, 0)
        .body(Full::new(Bytes::new()))
        .expect("static empty response")
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::fs;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use hyper::service::service_fn;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::watch;

    use super::*;

    async fn reserve_address() -> SocketAddrV4 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        match address {
            std::net::SocketAddr::V4(address) => address,
            _ => panic!("expected IPv4"),
        }
    }

    async fn fake_upstream(address: SocketAddrV4) {
        let listener = TcpListener::bind(address).await.unwrap();
        let next_role_id = Arc::new(AtomicU64::new(11));
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let next_role_id = Arc::clone(&next_role_id);
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| {
                    let next_role_id = Arc::clone(&next_role_id);
                    async move {
                        assert!(request.headers().contains_key(AUTHORIZATION));
                        let response = match request.uri().path() {
                            "/api/v10/users/@me" => Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from_static(br#"{"id":"6"}"#)))
                                .unwrap(),
                            "/api/v10/guilds/7/roles" => {
                                let reason = request
                                    .headers()
                                    .get("x-audit-log-reason")
                                    .and_then(|value| value.to_str().ok())
                                    .unwrap();
                                let body = if reason.ends_with(&"b".repeat(64)) {
                                    "{}".to_owned()
                                } else {
                                    let id = next_role_id.fetch_add(1, Ordering::Relaxed);
                                    format!(r#"{{"id":"{id}","name":"room"}}"#)
                                };
                                Response::builder()
                                    .status(StatusCode::CREATED)
                                    .header("content-type", "application/json")
                                    .header("x-ratelimit-remaining", "4")
                                    .body(Full::new(Bytes::from(body)))
                                    .unwrap()
                            }
                            "/api/v10/channels/5/messages" => {
                                assert_eq!(request.method(), Method::POST);
                                Response::builder()
                                    .status(StatusCode::CREATED)
                                    .header("content-type", "application/json")
                                    .body(Full::new(Bytes::from_static(br#"{"id":"15"}"#)))
                                    .unwrap()
                            }
                            "/api/v10/channels/5/messages/15" => {
                                assert_eq!(request.method(), Method::DELETE);
                                Response::builder()
                                    .status(StatusCode::NO_CONTENT)
                                    .body(Full::new(Bytes::new()))
                                    .unwrap()
                            }
                            "/api/v10/channels/5/messages/16" => {
                                assert_eq!(request.method(), Method::DELETE);
                                Response::builder()
                                    .status(StatusCode::NOT_FOUND)
                                    .header("content-type", "application/json")
                                    .body(Full::new(Bytes::from_static(
                                        br#"{"message":"Unknown Message","code":10008}"#,
                                    )))
                                    .unwrap()
                            }
                            "/api/v10/channels/5/messages/17" => {
                                assert_eq!(request.method(), Method::DELETE);
                                Response::builder()
                                    .status(StatusCode::NOT_FOUND)
                                    .header("content-type", "application/json")
                                    .body(Full::new(Bytes::from_static(
                                        br#"{"message":"Unknown Guild","code":10004}"#,
                                    )))
                                    .unwrap()
                            }
                            _ => panic!("unexpected fake upstream path"),
                        };
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    }

    fn raw_create_role_request(proxy: SocketAddrV4, reason: &str) -> Vec<u8> {
        format!(
            "POST /api/v10/guilds/7/roles HTTP/1.1\r\nHost: {proxy}\r\nAuthorization: Bot secret-test-value\r\nX-Audit-Log-Reason: {reason}\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{{\"name\":\"room\"}}"
        )
        .into_bytes()
    }

    fn raw_current_user_request(proxy: SocketAddrV4) -> Vec<u8> {
        format!(
            "GET /api/v10/users/@me HTTP/1.1\r\nHost: {proxy}\r\nAuthorization: Bot secret-test-value\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    fn raw_create_hub_message_request(proxy: SocketAddrV4) -> Vec<u8> {
        format!(
            "POST /api/v10/channels/5/messages HTTP/1.1\r\nHost: {proxy}\r\nAuthorization: Bot secret-test-value\r\nContent-Type: application/json\r\nContent-Length: 18\r\nConnection: close\r\n\r\n{{\"content\":\"room\"}}"
        )
        .into_bytes()
    }

    fn raw_delete_hub_message_request(proxy: SocketAddrV4) -> Vec<u8> {
        format!(
            "DELETE /api/v10/channels/5/messages/15 HTTP/1.1\r\nHost: {proxy}\r\nAuthorization: Bot secret-test-value\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    fn raw_confirm_missing_hub_message_request(proxy: SocketAddrV4) -> Vec<u8> {
        format!(
            "DELETE /api/v10/channels/5/messages/16 HTTP/1.1\r\nHost: {proxy}\r\nAuthorization: Bot secret-test-value\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    fn raw_mismatched_missing_hub_message_request(proxy: SocketAddrV4) -> Vec<u8> {
        format!(
            "DELETE /api/v10/channels/5/messages/17 HTTP/1.1\r\nHost: {proxy}\r\nAuthorization: Bot secret-test-value\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    #[tokio::test]
    async fn fake_upstream_success_is_consumed_then_downstream_gets_zero_bytes_once() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let upstream_address = reserve_address().await;
        let proxy_address = reserve_address().await;
        let gateway_address = reserve_address().await;
        let upstream = tokio::spawn(fake_upstream(upstream_address));
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            gateway_address,
            proxy_address,
            format!("ws://{gateway_address}"),
            format!("http://{upstream_address}"),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        let reason = format!("starring-effect-v1:{}", "a".repeat(64));
        assert_eq!(
            state.arm_next_indeterminate("d2:indeterminate:1"),
            crate::state::ArmOutcome::Armed
        );
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server_config = config.clone();
        let server_state = Arc::clone(&state);
        let server = tokio::spawn(async move {
            serve(server_config, server_state, shutdown_rx)
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        let mut identity = TcpStream::connect(proxy_address).await.unwrap();
        identity
            .write_all(&raw_current_user_request(proxy_address))
            .await
            .unwrap();
        let mut identity_response = Vec::new();
        identity.read_to_end(&mut identity_response).await.unwrap();
        assert!(String::from_utf8(identity_response)
            .unwrap()
            .starts_with("HTTP/1.1 200 OK\r\n"));
        let malformed_reason = format!("starring-effect-v1:{}", "b".repeat(64));
        let mut malformed = TcpStream::connect(proxy_address).await.unwrap();
        malformed
            .write_all(&raw_create_role_request(proxy_address, &malformed_reason))
            .await
            .unwrap();
        let mut malformed_response = Vec::new();
        malformed
            .read_to_end(&mut malformed_response)
            .await
            .unwrap();
        assert!(malformed_response.is_empty());
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["effect_http"]["indeterminate_injections"], 0);
        assert_eq!(snapshot["effect_http"]["indeterminate_armed"], true);
        assert_eq!(snapshot["effect_http"]["owned_role_count"], 0);
        let mut stream = TcpStream::connect(proxy_address).await.unwrap();
        stream
            .write_all(&raw_create_role_request(proxy_address, &reason))
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        assert!(response.is_empty());
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["effect_http"]["indeterminate_injections"], 1);
        assert_eq!(snapshot["effect_http"]["owned_role_count"], 1);
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains("secret-test-value"));
        assert!(!serialized.contains(&reason));
        let mut second = TcpStream::connect(proxy_address).await.unwrap();
        second
            .write_all(&raw_create_role_request(proxy_address, &reason))
            .await
            .unwrap();
        let mut second_response = Vec::new();
        second.read_to_end(&mut second_response).await.unwrap();
        let text = String::from_utf8(second_response).unwrap();
        assert!(text.starts_with("HTTP/1.1 201 Created\r\n"));
        assert!(text
            .to_ascii_lowercase()
            .contains("content-type: application/json"));
        assert!(text
            .to_ascii_lowercase()
            .contains("x-ratelimit-remaining: 4"));
        assert_eq!(
            state.arm_next_indeterminate("d2:indeterminate:2"),
            crate::state::ArmOutcome::Busy
        );
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["effect_http"]["indeterminate_injections"], 1);
        assert_eq!(snapshot["effect_http"]["indeterminate_armed"], false);
        assert_eq!(snapshot["effect_http"]["owned_role_count"], 2);
        shutdown_tx.send(true).unwrap();
        server.await.unwrap();
        upstream.abort();
    }

    #[tokio::test]
    async fn pinned_hub_message_is_tracked_and_can_be_deleted_without_channel_ownership() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let upstream_address = reserve_address().await;
        let proxy_address = reserve_address().await;
        let gateway_address = reserve_address().await;
        let upstream = tokio::spawn(fake_upstream(upstream_address));
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            gateway_address,
            proxy_address,
            format!("ws://{gateway_address}"),
            format!("http://{upstream_address}"),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server_config = config.clone();
        let server_state = Arc::clone(&state);
        let server = tokio::spawn(async move {
            serve(server_config, server_state, shutdown_rx)
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        let mut create = TcpStream::connect(proxy_address).await.unwrap();
        create
            .write_all(&raw_create_hub_message_request(proxy_address))
            .await
            .unwrap();
        let mut create_response = Vec::new();
        create.read_to_end(&mut create_response).await.unwrap();
        assert!(String::from_utf8(create_response)
            .unwrap()
            .starts_with("HTTP/1.1 201 Created\r\n"));
        assert!(!state.owns_channel("5"));
        assert!(state.owns_message("5", "15"));
        let mut delete = TcpStream::connect(proxy_address).await.unwrap();
        delete
            .write_all(&raw_delete_hub_message_request(proxy_address))
            .await
            .unwrap();
        let mut delete_response = Vec::new();
        delete.read_to_end(&mut delete_response).await.unwrap();
        assert!(String::from_utf8(delete_response)
            .unwrap()
            .starts_with("HTTP/1.1 204 No Content\r\n"));
        assert!(!state.owns_message("5", "15"));
        assert!(state
            .reserve_resource(ResourceKind::Message {
                channel_id: "5".to_owned()
            })
            .unwrap()
            .commit("16".to_owned()));
        let mut confirm_missing = TcpStream::connect(proxy_address).await.unwrap();
        confirm_missing
            .write_all(&raw_confirm_missing_hub_message_request(proxy_address))
            .await
            .unwrap();
        let mut confirm_missing_response = Vec::new();
        confirm_missing
            .read_to_end(&mut confirm_missing_response)
            .await
            .unwrap();
        assert!(String::from_utf8(confirm_missing_response)
            .unwrap()
            .starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert!(!state.owns_message("5", "16"));
        assert!(state
            .reserve_resource(ResourceKind::Message {
                channel_id: "5".to_owned()
            })
            .unwrap()
            .commit("17".to_owned()));
        let mut mismatched_missing = TcpStream::connect(proxy_address).await.unwrap();
        mismatched_missing
            .write_all(&raw_mismatched_missing_hub_message_request(proxy_address))
            .await
            .unwrap();
        let mut mismatched_missing_response = Vec::new();
        mismatched_missing
            .read_to_end(&mut mismatched_missing_response)
            .await
            .unwrap();
        assert!(mismatched_missing_response.is_empty());
        assert!(state.owns_message("5", "17"));
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["effect_http"]["forwarded_requests"], 4);
        assert_eq!(snapshot["effect_http"]["rejected_requests"], 0);
        assert_eq!(snapshot["effect_http"]["owned_channel_count"], 0);
        assert_eq!(snapshot["effect_http"]["owned_message_count"], 1);
        let inventory = serde_json::to_value(state.resource_inventory().unwrap()).unwrap();
        assert_eq!(inventory["history"].as_array().unwrap().len(), 3);
        assert_eq!(
            inventory["history"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["state"] == "deleted")
                .count(),
            2
        );
        assert_eq!(inventory["active"].as_array().unwrap().len(), 1);
        assert_eq!(inventory["active"][0]["resource_id"], "17");
        shutdown_tx.send(true).unwrap();
        server.await.unwrap();
        upstream.abort();
    }

    #[tokio::test]
    async fn mismatched_guild_and_host_fail_before_fake_upstream() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            "127.0.0.1:21001".parse().unwrap(),
            "127.0.0.1:21002".parse().unwrap(),
            "ws://127.0.0.1:22001".to_owned(),
            "http://127.0.0.1:22002".to_owned(),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        assert_eq!(
            validate_uri(&"/api/v10/guilds/9/roles".parse().unwrap(), &config),
            Err(StatusCode::FORBIDDEN)
        );
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_static("discord.com"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bot value"));
        assert_eq!(
            validate_headers(&headers, &config),
            Err(StatusCode::FORBIDDEN)
        );
        for path in [
            "/api/v10/users/@me",
            "/api/v10/guilds/7/roles",
            "/api/v10/guilds/7/channels",
            "/api/v10/guilds/7/members/6",
            "/api/v10/guilds/7/members/8",
        ] {
            assert_eq!(
                validate_method_and_path(&Method::GET, &path.parse().unwrap(), &config, &state),
                Ok(())
            );
        }
        assert_eq!(
            validate_method_and_path(
                &Method::GET,
                &"/api/v10/guilds/7/members/9".parse().unwrap(),
                &config,
                &state
            ),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            validate_method_and_path(
                &Method::POST,
                &"/api/v10/channels/11/messages".parse().unwrap(),
                &config,
                &state
            ),
            Err(StatusCode::FORBIDDEN)
        );
        for method in [Method::GET, Method::PATCH, Method::DELETE] {
            assert_eq!(
                validate_method_and_path(
                    &method,
                    &"/api/v10/channels/5".parse().unwrap(),
                    &config,
                    &state
                ),
                Err(StatusCode::FORBIDDEN)
            );
        }
        assert_eq!(
            validate_method_and_path(
                &Method::PUT,
                &"/api/v10/channels/5/permissions/7".parse().unwrap(),
                &config,
                &state
            ),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            validate_method_and_path(
                &Method::POST,
                &"/api/v10/channels/5/messages".parse().unwrap(),
                &config,
                &state
            ),
            Ok(())
        );
        assert_eq!(
            validate_method_and_path(
                &Method::GET,
                &"/api/v10/channels/5/messages".parse().unwrap(),
                &config,
                &state
            ),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            validate_method_and_path(
                &Method::POST,
                &"/api/v10/channels/5/messages?limit=1".parse().unwrap(),
                &config,
                &state
            ),
            Err(StatusCode::FORBIDDEN)
        );
        assert!(state
            .reserve_resource(ResourceKind::Message {
                channel_id: "5".to_owned()
            })
            .unwrap()
            .commit("15".to_owned()));
        for method in [Method::GET, Method::DELETE] {
            assert_eq!(
                validate_method_and_path(
                    &method,
                    &"/api/v10/channels/5/messages/15".parse().unwrap(),
                    &config,
                    &state
                ),
                Ok(())
            );
        }
        assert_eq!(
            validate_method_and_path(
                &Method::PATCH,
                &"/api/v10/channels/5/messages/15".parse().unwrap(),
                &config,
                &state
            ),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            validate_method_and_path(
                &Method::DELETE,
                &"/api/v10/channels/5/messages/16".parse().unwrap(),
                &config,
                &state
            ),
            Err(StatusCode::FORBIDDEN)
        );
        assert!(state
            .reserve_resource(ResourceKind::Channel)
            .unwrap()
            .commit("11".to_owned()));
        assert!(state
            .reserve_resource(ResourceKind::Role)
            .unwrap()
            .commit("12".to_owned()));
        assert_eq!(
            validate_method_and_path(
                &Method::POST,
                &"/api/v10/channels/11/messages".parse().unwrap(),
                &config,
                &state
            ),
            Ok(())
        );
        assert!(state
            .reserve_resource(ResourceKind::Message {
                channel_id: "11".to_owned()
            })
            .unwrap()
            .commit("13".to_owned()));
        assert_eq!(
            validate_method_and_path(
                &Method::DELETE,
                &"/api/v10/channels/11/messages/13".parse().unwrap(),
                &config,
                &state
            ),
            Ok(())
        );
        assert_eq!(
            validate_method_and_path(
                &Method::DELETE,
                &"/api/v10/channels/11/messages/14".parse().unwrap(),
                &config,
                &state
            ),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn tracked_mutations_reject_query_suffixes() {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "5",
            "8",
            "6",
            "127.0.0.1:21001".parse().unwrap(),
            "127.0.0.1:21002".parse().unwrap(),
            "ws://127.0.0.1:22001".to_owned(),
            "http://127.0.0.1:22002".to_owned(),
        );
        let state = Arc::new(SharedState::new(&config).unwrap());
        for path in [
            "/api/v10/guilds/7/roles?unexpected=1",
            "/api/v10/guilds/7/channels?unexpected=1",
        ] {
            assert_eq!(
                validate_method_and_path(&Method::POST, &path.parse().unwrap(), &config, &state),
                Err(StatusCode::FORBIDDEN)
            );
        }
        assert!(state
            .reserve_resource(ResourceKind::Role)
            .unwrap()
            .commit("12".to_owned()));
        assert!(state
            .reserve_resource(ResourceKind::Channel)
            .unwrap()
            .commit("11".to_owned()));
        assert!(state
            .reserve_resource(ResourceKind::Message {
                channel_id: "11".to_owned()
            })
            .unwrap()
            .commit("13".to_owned()));
        for (method, path) in [
            (Method::PATCH, "/api/v10/guilds/7/roles/12?unexpected=1"),
            (Method::DELETE, "/api/v10/guilds/7/roles/12?unexpected=1"),
            (Method::PATCH, "/api/v10/channels/11?unexpected=1"),
            (Method::DELETE, "/api/v10/channels/11?unexpected=1"),
            (
                Method::PUT,
                "/api/v10/guilds/7/members/8/roles/12?unexpected=1",
            ),
            (
                Method::DELETE,
                "/api/v10/guilds/7/members/8/roles/12?unexpected=1",
            ),
            (
                Method::PUT,
                "/api/v10/channels/11/permissions/12?unexpected=1",
            ),
            (
                Method::DELETE,
                "/api/v10/channels/11/permissions/12?unexpected=1",
            ),
            (Method::POST, "/api/v10/channels/11/messages?unexpected=1"),
            (
                Method::DELETE,
                "/api/v10/channels/11/messages/13?unexpected=1",
            ),
        ] {
            assert_eq!(
                validate_method_and_path(&method, &path.parse().unwrap(), &config, &state),
                Err(StatusCode::FORBIDDEN)
            );
        }
    }

    #[test]
    fn forwarded_requests_negotiate_identity_encoding() {
        let mut source = HeaderMap::new();
        source.insert(HOST, HeaderValue::from_static("127.0.0.1:21002"));
        source.insert(AUTHORIZATION, HeaderValue::from_static("Bot value"));
        source.insert(ACCEPT_ENCODING, HeaderValue::from_static("br"));
        source.insert("user-agent", HeaderValue::from_static("twilight-http"));
        let forwarded = forward_request_headers(&source);
        assert_eq!(
            forwarded.get(ACCEPT_ENCODING),
            Some(&HeaderValue::from_static("identity"))
        );
        assert_eq!(
            forwarded.get(AUTHORIZATION),
            Some(&HeaderValue::from_static("Bot value"))
        );
        assert_eq!(
            forwarded.get("user-agent"),
            Some(&HeaderValue::from_static("twilight-http"))
        );
        assert!(!forwarded.contains_key(HOST));
    }

    #[test]
    fn only_exact_discord_unknown_resource_codes_confirm_deletion() {
        assert!(exact_unknown_resource(
            br#"{"message":"Unknown Channel","code":10003}"#,
            10003
        ));
        assert!(exact_unknown_resource(
            br#"{"message":"Unknown Message","code":10008}"#,
            10008
        ));
        assert!(exact_unknown_resource(
            br#"{"message":"Unknown Role","code":10011}"#,
            10011
        ));
        assert!(!exact_unknown_resource(
            br#"{"message":"Unknown Guild","code":10004}"#,
            10003
        ));
        assert!(!exact_unknown_resource(b"", 10008));
    }
}
