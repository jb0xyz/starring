use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::header::{
    AUTHORIZATION, CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, COOKIE, HOST,
    REFERRER_POLICY, SET_COOKIE, STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS,
    X_FRAME_OPTIONS,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::FutureExt;
use serde::Serialize;
use url::form_urlencoded;
use zeroize::Zeroizing;

use super::HttpState;
use crate::config::valid_return_path;
use crate::{
    CsrfSecret, DiscordAuthorizationRequest, FacadeError, IdempotencyKey, OAuthCode, OAuthState,
    ProductControlFacade, ProductRequestId, SessionCredential,
};

pub(super) const SESSION_COOKIE: &str = "__Host-starring_session";
pub(super) const OAUTH_COOKIE: &str = "__Host-starring_oauth";
pub(super) const CSRF_COOKIE: &str = "__Host-starring_csrf";
const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");
const IDEMPOTENCY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");
const ORIGIN_HEADER: HeaderName = HeaderName::from_static("origin");
const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

pub(super) type RequestId = ProductRequestId;

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    r#type: &'static str,
    title: &'static str,
    status: u16,
    error: ErrorDetail<'a>,
}

#[derive(Serialize)]
struct ErrorDetail<'a> {
    code: &'static str,
    message: &'static str,
    request_id: &'a str,
    retryable: bool,
}

pub(super) async fn request_boundary(mut request: Request<Body>, next: Next) -> Response {
    mark_sensitive_request_headers(request.headers_mut());
    let request_id = request_id(request.headers());
    request.extensions_mut().insert(request_id.clone());
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        HeaderValue::from_str(request_id.as_str())
            .unwrap_or_else(|_| HeaderValue::from_static("invalid-request-id")),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'; base-uri 'none'"),
    );
    response
        .headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response.headers_mut().insert(
        STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000"),
    );
    response
        .headers_mut()
        .insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
        .headers_mut()
        .insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    response.headers_mut().insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), geolocation=(), microphone=()"),
    );
    response.headers_mut().insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    response.headers_mut().insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    response
}

pub(super) async fn resource_boundary<F>(
    State(state): State<HttpState<F>>,
    request: Request<Body>,
    next: Next,
) -> Response
where
    F: ProductControlFacade,
{
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .cloned()
        .unwrap_or_else(new_request_id);
    if !single_header_equals(request.headers(), HOST, state.config.public_host()) {
        return problem(
            StatusCode::BAD_REQUEST,
            "invalid_host",
            "The request host is invalid.",
            false,
            &request_id,
        );
    }
    if request.uri().path() == "/health/live" {
        return next.run(request).await;
    }
    if request.uri().path() == "/health/ready" {
        return next.run(request).await;
    }
    if !state.readiness_gate.is_ready() {
        return map_facade(
            FacadeError::new(crate::FacadeErrorCode::DependencyUnavailable),
            &request_id,
        );
    }
    let permit = match Arc::clone(&state.in_flight).try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            let mut response = problem(
                StatusCode::TOO_MANY_REQUESTS,
                "concurrency_exhausted",
                "The service is busy. Retry shortly.",
                true,
                &request_id,
            );
            response
                .headers_mut()
                .insert("retry-after", HeaderValue::from_static("1"));
            return response;
        }
    };
    if !state.readiness_gate.is_ready() {
        drop(permit);
        return map_facade(
            FacadeError::new(crate::FacadeErrorCode::DependencyUnavailable),
            &request_id,
        );
    }
    let future = std::panic::AssertUnwindSafe(next.run(request)).catch_unwind();
    let outcome = tokio::time::timeout(state.config.request_timeout(), future).await;
    drop(permit);
    bounded_outcome(outcome, &request_id)
}

fn bounded_outcome(
    outcome: Result<Result<Response, Box<dyn std::any::Any + Send>>, tokio::time::error::Elapsed>,
    request_id: &RequestId,
) -> Response {
    match outcome {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The request could not be completed.",
            false,
            request_id,
        ),
        Err(_) => problem(
            StatusCode::GATEWAY_TIMEOUT,
            "request_timeout",
            "The request deadline expired.",
            true,
            request_id,
        ),
    }
}

#[allow(clippy::result_large_err)]
pub(super) fn mutation_credential<F>(
    state: &HttpState<F>,
    headers: &HeaderMap,
    request_id: &RequestId,
) -> Result<(SessionCredential, CsrfSecret), Response>
where
    F: ProductControlFacade,
{
    if !single_header_equals(headers, ORIGIN_HEADER, state.config.public_origin()) {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "origin_required",
            "The request origin is not allowed.",
            false,
            request_id,
        ));
    }
    let credential = session_credential(headers, request_id)?;
    let header_csrf = match single_header(headers, CSRF_HEADER) {
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|raw| CsrfSecret::parse(raw).ok())
        {
            Some(value) => value,
            None => return Err(csrf_forbidden(request_id)),
        },
        None => return Err(csrf_forbidden(request_id)),
    };
    let cookie_csrf = csrf_cookie(headers).map_err(|_| csrf_forbidden(request_id))?;
    if !crate::secret::constant_time_secret_eq(
        header_csrf.expose_secret(),
        cookie_csrf.expose_secret(),
    ) {
        return Err(csrf_forbidden(request_id));
    }
    Ok((credential, header_csrf))
}

#[allow(clippy::result_large_err)]
pub(super) fn session_credential(
    headers: &HeaderMap,
    request_id: &RequestId,
) -> Result<SessionCredential, Response> {
    let raw =
        cookie_secret(headers, SESSION_COOKIE).map_err(|_| authentication_required(request_id))?;
    SessionCredential::parse(&raw).map_err(|_| authentication_required(request_id))
}

pub(super) fn csrf_cookie(headers: &HeaderMap) -> Result<CsrfSecret, CookieReadError> {
    let raw = cookie_secret(headers, CSRF_COOKIE)?;
    CsrfSecret::parse(&raw).map_err(|_| CookieReadError::Invalid)
}

#[allow(clippy::result_large_err)]
pub(super) fn require_json(headers: &HeaderMap, request_id: &RequestId) -> Result<(), Response> {
    let expected = HeaderValue::from_static("application/json");
    if single_header_equals(headers, CONTENT_TYPE, &expected) {
        Ok(())
    } else {
        Err(problem(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "json_content_type_required",
            "Content-Type must be application/json.",
            false,
            request_id,
        ))
    }
}

#[allow(clippy::result_large_err)]
pub(super) fn parse_json<T>(
    body: Result<Json<T>, JsonRejection>,
    request_id: &RequestId,
) -> Result<T, Response> {
    match body {
        Ok(Json(value)) => Ok(value),
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => Err(problem(
            StatusCode::PAYLOAD_TOO_LARGE,
            "body_too_large",
            "The request body is too large.",
            false,
            request_id,
        )),
        Err(_) => Err(problem(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "The JSON body is malformed or contains unknown fields.",
            false,
            request_id,
        )),
    }
}

#[allow(clippy::result_large_err)]
pub(super) fn idempotency_key(
    headers: &HeaderMap,
    request_id: &RequestId,
) -> Result<IdempotencyKey, Response> {
    let value = single_header(headers, IDEMPOTENCY_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| IdempotencyKey::parse(value).ok());
    value.ok_or_else(|| {
        problem(
            StatusCode::BAD_REQUEST,
            "invalid_idempotency_key",
            "A single valid Idempotency-Key header is required.",
            false,
            request_id,
        )
    })
}

fn single_header(headers: &HeaderMap, name: HeaderName) -> Option<&HeaderValue> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?;
    if values.next().is_some() {
        None
    } else {
        Some(first)
    }
}

fn single_header_equals(headers: &HeaderMap, name: HeaderName, expected: &HeaderValue) -> bool {
    single_header(headers, name).is_some_and(|actual| actual == expected)
}

#[derive(Clone, Copy)]
pub(super) enum CookieReadError {
    Missing,
    Duplicate,
    Invalid,
}

pub(super) fn cookie_secret(
    headers: &HeaderMap,
    name: &str,
) -> Result<Zeroizing<String>, CookieReadError> {
    let value = single_header(headers, COOKIE).ok_or(CookieReadError::Missing)?;
    let value = value.to_str().map_err(|_| CookieReadError::Invalid)?;
    if value.len() > 4_096 {
        return Err(CookieReadError::Invalid);
    }
    let mut found = None;
    for pair in value.split(';') {
        let (key, value) = pair
            .trim()
            .split_once('=')
            .ok_or(CookieReadError::Invalid)?;
        if key == name {
            if found.is_some() {
                return Err(CookieReadError::Duplicate);
            }
            found = Some(Zeroizing::new(value.to_string()));
        }
    }
    found.ok_or(CookieReadError::Missing)
}

pub(super) fn parse_start_query(query: Option<&str>) -> Result<Option<String>, ()> {
    let Some(query) = query else {
        return Ok(None);
    };
    if query.len() > 2_048 {
        return Err(());
    }
    let pairs = form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
    match pairs.as_slice() {
        [] => Ok(None),
        [(key, value)] if key == "return_to" && valid_return_path(value) => {
            Ok(Some(value.to_string()))
        }
        _ => Err(()),
    }
}

pub(super) fn parse_callback_query(
    query: Option<&str>,
) -> Result<(OAuthCode, crate::OAuthState), ()> {
    let query = query.ok_or(())?;
    if query.len() > 4_096 {
        return Err(());
    }
    let mut code = None;
    let mut state = None;
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        let value = Zeroizing::new(value.into_owned());
        match key.as_ref() {
            "code" if code.is_none() => {
                code = Some(OAuthCode::parse(value.as_str()).map_err(|_| ())?)
            }
            "state" if state.is_none() => {
                state = Some(crate::OAuthState::parse(value.as_str()).map_err(|_| ())?)
            }
            _ => return Err(()),
        }
    }
    Ok((code.ok_or(())?, state.ok_or(())?))
}

pub(super) fn discord_authorization_location(
    request: &DiscordAuthorizationRequest,
    state: &OAuthState,
    expected_callback: &str,
) -> Option<Zeroizing<String>> {
    if !request
        .client_id
        .parse::<u64>()
        .is_ok_and(|parsed| parsed > 0 && parsed.to_string() == request.client_id)
        || request.callback_url != expected_callback
    {
        return None;
    }
    let mut location = Zeroizing::new(String::with_capacity(
        128 + request.client_id.len() + request.callback_url.len() + state.expose_secret().len(),
    ));
    location.push_str("https://discord.com/oauth2/authorize?client_id=");
    push_form_value(&mut location, &request.client_id);
    location.push_str("&redirect_uri=");
    push_form_value(&mut location, &request.callback_url);
    location.push_str("&response_type=code&scope=identify&state=");
    push_form_value(&mut location, state.expose_secret());
    Some(location)
}

fn push_form_value(destination: &mut String, value: &str) {
    for fragment in form_urlencoded::byte_serialize(value.as_bytes()) {
        destination.push_str(fragment);
    }
}

pub(super) fn secure_http_only_cookie(
    name: &str,
    value: &str,
    max_age_seconds: u32,
) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "{name}={value}; Path=/; Max-Age={max_age_seconds}; Secure; HttpOnly; SameSite=Lax"
    ))
}

pub(super) fn secure_csrf_cookie(value: &str, max_age_seconds: u32) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "{CSRF_COOKIE}={value}; Path=/; Max-Age={max_age_seconds}; Secure; SameSite=Strict"
    ))
}

pub(super) fn clear_cookie(name: &str) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "{name}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax"
    ))
}

pub(super) fn clear_csrf_cookie() -> Zeroizing<String> {
    Zeroizing::new(format!(
        "{CSRF_COOKIE}=; Path=/; Max-Age=0; Secure; SameSite=Strict"
    ))
}

pub(super) fn append_cookie(response: &mut Response, cookie: Zeroizing<String>) {
    if let Ok(mut value) = HeaderValue::from_str(cookie.as_str()) {
        value.set_sensitive(true);
        response.headers_mut().append(SET_COOKIE, value);
    }
}

fn mark_sensitive_request_headers(headers: &mut HeaderMap) {
    for (name, value) in headers.iter_mut() {
        if name == COOKIE
            || name == AUTHORIZATION
            || name == CSRF_HEADER
            || name == IDEMPOTENCY_HEADER
        {
            value.set_sensitive(true);
        }
    }
}

fn request_id(headers: &HeaderMap) -> RequestId {
    single_header(headers, REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| ProductRequestId::parse(value).ok())
        .unwrap_or_else(new_request_id)
}

fn new_request_id() -> RequestId {
    ProductRequestId::generated()
}

fn csrf_forbidden(request_id: &RequestId) -> Response {
    problem(
        StatusCode::FORBIDDEN,
        "csrf_required",
        "A valid CSRF proof is required.",
        false,
        request_id,
    )
}

pub(super) fn authentication_required(request_id: &RequestId) -> Response {
    problem(
        StatusCode::UNAUTHORIZED,
        "authentication_required",
        "A valid product session is required.",
        false,
        request_id,
    )
}

pub(super) fn malformed_query(request_id: &RequestId) -> Response {
    problem(
        StatusCode::BAD_REQUEST,
        "invalid_query",
        "The query is malformed.",
        false,
        request_id,
    )
}

pub(super) fn invalid_input(request_id: &RequestId) -> Response {
    problem(
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "A path, header, or request value is invalid.",
        false,
        request_id,
    )
}

pub(super) fn map_facade(error: FacadeError, request_id: &RequestId) -> Response {
    problem(
        error.status(),
        error.code(),
        error.message(),
        error.retryable(),
        request_id,
    )
}

pub(super) fn problem(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retryable: bool,
    request_id: &RequestId,
) -> Response {
    let body = ErrorEnvelope {
        r#type: "about:blank",
        title: status.canonical_reason().unwrap_or("Request failed"),
        status: status.as_u16(),
        error: ErrorDetail {
            code,
            message,
            request_id: request_id.as_str(),
            retryable,
        },
    };
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    response
}
