use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use url::Url;

use super::*;

const REDIRECT_URI: &str = "https://starring.example/oauth/discord/callback";
const ACCESS_TOKEN: &str = "returned-access-token";
const REFRESH_TOKEN: &str = "returned-refresh-token";

struct HttpRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: String,
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
    delay: Duration,
}

impl HttpResponse {
    fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.as_bytes().to_vec(),
            delay: Duration::ZERO,
        }
    }

    fn delayed(status: u16, body: &str, delay: Duration) -> Self {
        Self {
            status,
            body: body.as_bytes().to_vec(),
            delay,
        }
    }

    fn bytes(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            delay: Duration::ZERO,
        }
    }
}

struct MockDiscord {
    base: Url,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
    task: JoinHandle<()>,
}

impl MockDiscord {
    async fn start(responses: Vec<HttpResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let base = Url::parse(&format!("http://{address}/")).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_request(&mut stream).await;
                recorded.lock().await.push(request);
                if !response.delay.is_zero() {
                    tokio::time::sleep(response.delay).await;
                }
                write_response(&mut stream, response).await;
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }

    async fn finish(self) -> Vec<HttpRequest> {
        self.task.await.unwrap();
        Arc::try_unwrap(self.requests).ok().unwrap().into_inner()
    }
}

async fn read_request(stream: &mut TcpStream) -> HttpRequest {
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        let mut chunk = [0_u8; 1_024];
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&chunk[..read]);
        assert!(bytes.len() <= 64 * 1_024);
    };
    let headers_text = std::str::from_utf8(&bytes[..header_end]).unwrap();
    let mut lines = headers_text.split("\r\n");
    let request_line = lines.next().unwrap();
    let mut request_parts = request_line.split(' ');
    let method = request_parts.next().unwrap().to_string();
    let path = request_parts.next().unwrap().to_string();
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').unwrap();
        headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
    }
    let content_length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(0);
    while bytes.len() - header_end < content_length {
        let mut chunk = [0_u8; 1_024];
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&chunk[..read]);
    }
    let body = std::str::from_utf8(&bytes[header_end..header_end + content_length])
        .unwrap()
        .to_string();
    HttpRequest {
        method,
        path,
        headers,
        body,
    }
}

async fn write_response(stream: &mut TcpStream, response: HttpResponse) {
    let reason = match response.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Response",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        reason,
        response.body.len()
    );
    if stream.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    if stream.write_all(&response.body).await.is_err() {
        return;
    }
    let _ = stream.shutdown().await;
}

fn application_id() -> DiscordApplicationIdV1 {
    DiscordApplicationIdV1::new(42).unwrap()
}

fn state() -> DiscordOAuthStateV1 {
    DiscordOAuthStateV1::from_owned(format!("{}A", "s".repeat(42))).unwrap()
}

fn code() -> DiscordAuthorizationCodeV1 {
    DiscordAuthorizationCodeV1::from_owned("authorization-code".to_string()).unwrap()
}

fn secret() -> DiscordOAuthClientSecretV1 {
    DiscordOAuthClientSecretV1::from_owned("client-secret".to_string()).unwrap()
}

fn token_response() -> String {
    format!(
        "{{\"access_token\":\"{ACCESS_TOKEN}\",\"refresh_token\":\"{REFRESH_TOKEN}\",\"token_type\":\"Bearer\",\"expires_in\":60,\"scope\":\"identify\"}}"
    )
}

fn user_response(id: &str, bot: bool, system: bool) -> String {
    format!(
        "{{\"id\":\"{id}\",\"username\":\"owner\",\"global_name\":\" Product Owner \",\"bot\":{bot},\"system\":{system}}}"
    )
}

fn client(base: Url, deadline: Duration) -> DiscordOAuthClient {
    let config =
        DiscordOAuthConfigV1::for_local_server(application_id(), REDIRECT_URI, deadline, base)
            .unwrap();
    DiscordOAuthClient::new(config).unwrap()
}

fn form(body: &str) -> BTreeMap<String, String> {
    url::form_urlencoded::parse(body.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

#[test]
fn authorization_url_and_secret_boundaries_are_exact() {
    let config = DiscordOAuthConfigV1::new(application_id(), REDIRECT_URI).unwrap();
    assert_eq!(config.login_deadline(), Duration::from_secs(5));
    let client = DiscordOAuthClient::new(config).unwrap();
    let state = state();
    let url = client.authorization_url(&state);
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("discord.com"));
    assert_eq!(url.path(), "/oauth2/authorize");
    assert_eq!(
        url.query_pairs().collect::<BTreeMap<_, _>>(),
        BTreeMap::from([
            ("client_id".into(), "42".into()),
            ("redirect_uri".into(), REDIRECT_URI.into()),
            ("response_type".into(), "code".into()),
            ("scope".into(), "identify".into()),
            ("state".into(), state.expose_secret().into()),
        ])
    );
    assert_eq!(format!("{state:?}"), "DiscordOAuthStateV1(<redacted>)");
    assert_eq!(
        format!("{:?}", code()),
        "DiscordAuthorizationCodeV1(<redacted>)"
    );
    assert_eq!(
        format!("{:?}", secret()),
        "DiscordOAuthClientSecretV1(<redacted>)"
    );
    for invalid in [
        "http://starring.example/oauth/discord/callback",
        "https://user@starring.example/oauth/discord/callback",
        "https://starring.example/oauth/discord/callback?next=/",
        "https://starring.example:443/oauth/discord/callback",
    ] {
        assert_eq!(
            DiscordOAuthConfigV1::new(application_id(), invalid).unwrap_err(),
            DiscordOAuthConfigError::InvalidRedirectUri
        );
    }
}

#[tokio::test]
async fn exchange_fetches_identity_and_revokes_every_returned_credential() {
    let mock = MockDiscord::start(vec![
        HttpResponse::json(200, &token_response()),
        HttpResponse::json(200, &user_response("123", false, false)),
        HttpResponse::json(200, "{}"),
        HttpResponse::json(200, "{}"),
    ])
    .await;
    let client = client(mock.base.clone(), Duration::from_secs(1));
    let identity = client.exchange_identify(&code(), &secret()).await.unwrap();
    assert_eq!(identity.user_id(), UserId(123));
    assert_eq!(identity.display_name(), "Product Owner");
    assert_eq!(
        format!("{identity:?}"),
        "VerifiedDiscordIdentityV1 { user_id: UserId(123), display_name: \"<redacted>\" }"
    );
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/v10/oauth2/token");
    assert_eq!(
        form(&requests[0].body),
        BTreeMap::from([
            ("client_id".to_string(), "42".to_string()),
            ("client_secret".to_string(), "client-secret".to_string()),
            ("grant_type".to_string(), "authorization_code".to_string()),
            ("code".to_string(), "authorization-code".to_string()),
            ("redirect_uri".to_string(), REDIRECT_URI.to_string()),
        ])
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/v10/users/@me");
    assert_eq!(
        requests[1].headers.get("authorization").map(String::as_str),
        Some("Bearer returned-access-token")
    );
    let mut revoked = BTreeMap::new();
    for request in [&requests[2], &requests[3]] {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/v10/oauth2/token/revoke");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Basic NDI6Y2xpZW50LXNlY3JldA==")
        );
        let fields = form(&request.body);
        revoked.insert(
            fields.get("token_type_hint").unwrap().clone(),
            fields.get("token").unwrap().clone(),
        );
    }
    assert_eq!(
        revoked,
        BTreeMap::from([
            ("access_token".to_string(), ACCESS_TOKEN.to_string()),
            ("refresh_token".to_string(), REFRESH_TOKEN.to_string()),
        ])
    );
}

#[tokio::test]
async fn malformed_and_oversized_token_responses_are_redacted_and_bounded() {
    for (response, expected) in [
        (
            HttpResponse::json(200, "{not-json"),
            DiscordOAuthError::InvalidResponse,
        ),
        (
            HttpResponse::bytes(200, vec![b'x'; MAX_RESPONSE_BYTES + 1]),
            DiscordOAuthError::ResponseTooLarge,
        ),
    ] {
        let mock = MockDiscord::start(vec![response]).await;
        let client = client(mock.base.clone(), Duration::from_secs(1));
        let error = client
            .exchange_identify(&code(), &secret())
            .await
            .unwrap_err();
        assert_eq!(error, expected);
        assert!(!format!("{error:?}").contains("authorization-code"));
        assert_eq!(mock.finish().await.len(), 1);
    }
}

#[tokio::test]
async fn invalid_or_oversized_user_responses_still_revoke_credentials() {
    for (user, expected) in [
        (
            HttpResponse::json(200, &user_response("00123", false, false)),
            DiscordOAuthError::InvalidResponse,
        ),
        (
            HttpResponse::bytes(200, vec![b'x'; MAX_RESPONSE_BYTES + 1]),
            DiscordOAuthError::ResponseTooLarge,
        ),
    ] {
        let mock = MockDiscord::start(vec![
            HttpResponse::json(200, &token_response()),
            user,
            HttpResponse::json(200, "{}"),
            HttpResponse::json(200, "{}"),
        ])
        .await;
        let client = client(mock.base.clone(), Duration::from_secs(1));
        assert_eq!(
            client.exchange_identify(&code(), &secret()).await,
            Err(expected)
        );
        let requests = mock.finish().await;
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[2].path, "/api/v10/oauth2/token/revoke");
        assert_eq!(requests[3].path, "/api/v10/oauth2/token/revoke");
    }
}

#[tokio::test]
async fn bot_system_and_noncanonical_identities_are_rejected_after_revocation() {
    for user in [
        user_response("123", true, false),
        user_response("123", false, true),
        user_response("0", false, false),
    ] {
        let mock = MockDiscord::start(vec![
            HttpResponse::json(200, &token_response()),
            HttpResponse::json(200, &user),
            HttpResponse::json(200, "{}"),
            HttpResponse::json(200, "{}"),
        ])
        .await;
        let client = client(mock.base.clone(), Duration::from_secs(1));
        assert_eq!(
            client.exchange_identify(&code(), &secret()).await,
            Err(DiscordOAuthError::InvalidResponse)
        );
        assert_eq!(mock.finish().await.len(), 4);
    }
}

#[tokio::test]
async fn missing_identify_scope_skips_profile_fetch_but_revokes_credentials() {
    let token = token_response().replace("\"identify\"", "\"guilds\"");
    let mock = MockDiscord::start(vec![
        HttpResponse::json(200, &token),
        HttpResponse::json(200, "{}"),
        HttpResponse::json(200, "{}"),
    ])
    .await;
    let client = client(mock.base.clone(), Duration::from_secs(1));
    assert_eq!(
        client.exchange_identify(&code(), &secret()).await,
        Err(DiscordOAuthError::InvalidResponse)
    );
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].path, "/api/v10/oauth2/token/revoke");
    assert_eq!(requests[2].path, "/api/v10/oauth2/token/revoke");
}

#[tokio::test]
async fn malformed_token_metadata_still_revokes_every_parsed_credential() {
    let token = token_response().replace("\"expires_in\":60", "\"expires_in\":\"broken\"");
    let mock = MockDiscord::start(vec![
        HttpResponse::json(200, &token),
        HttpResponse::json(200, "{}"),
        HttpResponse::json(200, "{}"),
    ])
    .await;
    let client = client(mock.base.clone(), Duration::from_secs(1));
    assert_eq!(
        client.exchange_identify(&code(), &secret()).await,
        Err(DiscordOAuthError::InvalidResponse)
    );
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 3);
    assert!(requests[1..]
        .iter()
        .all(|request| request.path == "/api/v10/oauth2/token/revoke"));
}

#[tokio::test]
async fn profile_budget_timeout_preserves_cleanup_reserve_and_revokes_before_return() {
    let mock = MockDiscord::start(vec![
        HttpResponse::json(200, &token_response()),
        HttpResponse::delayed(
            200,
            &user_response("123", false, false),
            Duration::from_millis(130),
        ),
        HttpResponse::json(200, "{}"),
        HttpResponse::json(200, "{}"),
    ])
    .await;
    let client = client(mock.base.clone(), Duration::from_millis(200));
    let started = tokio::time::Instant::now();
    assert_eq!(
        client.exchange_identify(&code(), &secret()).await,
        Err(DiscordOAuthError::Timeout)
    );
    assert!(started.elapsed() <= Duration::from_millis(200));
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 4);
    assert!(requests[2..]
        .iter()
        .all(|request| request.path == "/api/v10/oauth2/token/revoke"));
}

#[tokio::test]
async fn upstream_failures_and_deadlines_are_stable() {
    let unavailable = MockDiscord::start(vec![HttpResponse::json(500, "sensitive-body")]).await;
    let unavailable_client = client(unavailable.base.clone(), Duration::from_secs(1));
    assert_eq!(
        unavailable_client
            .exchange_identify(&code(), &secret())
            .await,
        Err(DiscordOAuthError::Unavailable)
    );
    assert_eq!(unavailable.finish().await.len(), 1);

    let timeout = MockDiscord::start(vec![HttpResponse::delayed(
        200,
        &token_response(),
        Duration::from_millis(80),
    )])
    .await;
    let timeout_client = client(timeout.base.clone(), Duration::from_millis(20));
    assert_eq!(
        timeout_client.exchange_identify(&code(), &secret()).await,
        Err(DiscordOAuthError::Timeout)
    );
    assert_eq!(timeout.finish().await.len(), 1);
}

#[tokio::test]
async fn profile_failure_is_followed_by_revocation_and_revocation_failure_wins() {
    let mock = MockDiscord::start(vec![
        HttpResponse::json(200, &token_response()),
        HttpResponse::json(500, "sensitive-profile-error"),
        HttpResponse::json(500, "sensitive-revocation-error"),
        HttpResponse::json(200, "{}"),
    ])
    .await;
    let client = client(mock.base.clone(), Duration::from_secs(1));
    let error = client
        .exchange_identify(&code(), &secret())
        .await
        .unwrap_err();
    assert_eq!(error, DiscordOAuthError::RevocationFailed);
    assert_eq!(
        error.to_string(),
        "Discord OAuth credential revocation failed"
    );
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[2].path, "/api/v10/oauth2/token/revoke");
    assert_eq!(requests[3].path, "/api/v10/oauth2/token/revoke");
}
