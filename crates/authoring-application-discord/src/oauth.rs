use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Duration;

use discord_model::UserId;
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use url::Url;
use zeroize::{Zeroize, Zeroizing};

use crate::DiscordApplicationIdV1;

const AUTHORIZATION_ENDPOINT: &str = "https://discord.com/oauth2/authorize";
const TOKEN_ENDPOINT: &str = "https://discord.com/api/v10/oauth2/token";
const CURRENT_USER_ENDPOINT: &str = "https://discord.com/api/v10/users/@me";
const REVOCATION_ENDPOINT: &str = "https://discord.com/api/v10/oauth2/token/revoke";
const DEFAULT_DEADLINE: Duration = Duration::from_secs(5);
const MAX_DEADLINE: Duration = Duration::from_secs(5);
const MAX_REVOCATION_RESERVE: Duration = Duration::from_secs(2);
const MAX_RESPONSE_BYTES: usize = 16 * 1024;
const OAUTH_STATE_BYTES: usize = 43;
const MAX_AUTHORIZATION_CODE_BYTES: usize = 1_024;
const MAX_CLIENT_SECRET_BYTES: usize = 1_024;
const MAX_CREDENTIAL_BYTES: usize = 8 * 1024;
const MAX_DISPLAY_NAME_BYTES: usize = 512;
const MAX_DISPLAY_NAME_SCALARS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiscordOAuthConfigError {
    #[error("Discord OAuth redirect URI is invalid")]
    InvalidRedirectUri,
    #[error("Discord OAuth request deadline is invalid")]
    InvalidDeadline,
    #[error("Discord OAuth HTTP client configuration failed")]
    HttpClient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiscordOAuthSecretError {
    #[error("Discord OAuth state is invalid")]
    InvalidState,
    #[error("Discord OAuth authorization code is invalid")]
    InvalidAuthorizationCode,
    #[error("Discord OAuth client secret is invalid")]
    InvalidClientSecret,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiscordOAuthError {
    #[error("Discord rejected the OAuth exchange")]
    ExchangeRejected,
    #[error("Discord OAuth dependency is unavailable")]
    Unavailable,
    #[error("Discord OAuth request timed out")]
    Timeout,
    #[error("Discord returned an invalid OAuth response")]
    InvalidResponse,
    #[error("Discord OAuth response exceeded the size limit")]
    ResponseTooLarge,
    #[error("Discord OAuth credential revocation failed")]
    RevocationFailed,
}

pub struct DiscordOAuthStateV1(Zeroizing<String>);

impl DiscordOAuthStateV1 {
    pub fn from_owned(value: String) -> Result<Self, DiscordOAuthSecretError> {
        if canonical_secret(&value) {
            Ok(Self(Zeroizing::new(value)))
        } else {
            Err(DiscordOAuthSecretError::InvalidState)
        }
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for DiscordOAuthStateV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DiscordOAuthStateV1(<redacted>)")
    }
}

pub struct DiscordAuthorizationCodeV1(Zeroizing<String>);

impl DiscordAuthorizationCodeV1 {
    pub fn from_owned(value: String) -> Result<Self, DiscordOAuthSecretError> {
        if !value.is_empty()
            && value.len() <= MAX_AUTHORIZATION_CODE_BYTES
            && !value.chars().any(char::is_control)
        {
            Ok(Self(Zeroizing::new(value)))
        } else {
            Err(DiscordOAuthSecretError::InvalidAuthorizationCode)
        }
    }

    fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for DiscordAuthorizationCodeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DiscordAuthorizationCodeV1(<redacted>)")
    }
}

pub struct DiscordOAuthClientSecretV1(Zeroizing<String>);

impl DiscordOAuthClientSecretV1 {
    pub fn from_owned(value: String) -> Result<Self, DiscordOAuthSecretError> {
        if !value.is_empty()
            && value.len() <= MAX_CLIENT_SECRET_BYTES
            && !value.chars().any(char::is_control)
        {
            Ok(Self(Zeroizing::new(value)))
        } else {
            Err(DiscordOAuthSecretError::InvalidClientSecret)
        }
    }

    fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for DiscordOAuthClientSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DiscordOAuthClientSecretV1(<redacted>)")
    }
}

#[derive(Clone)]
struct DiscordOAuthEndpointsV1 {
    authorization: Url,
    token: Url,
    current_user: Url,
    revocation: Url,
    permit_plaintext: bool,
}

impl DiscordOAuthEndpointsV1 {
    fn production() -> Self {
        Self {
            authorization: Url::parse(AUTHORIZATION_ENDPOINT)
                .expect("fixed Discord authorization endpoint must be valid"),
            token: Url::parse(TOKEN_ENDPOINT).expect("fixed Discord token endpoint must be valid"),
            current_user: Url::parse(CURRENT_USER_ENDPOINT)
                .expect("fixed Discord current-user endpoint must be valid"),
            revocation: Url::parse(REVOCATION_ENDPOINT)
                .expect("fixed Discord revocation endpoint must be valid"),
            permit_plaintext: false,
        }
    }

    #[cfg(test)]
    fn local(base: &Url) -> Self {
        Self {
            authorization: base.join("oauth2/authorize").unwrap(),
            token: base.join("api/v10/oauth2/token").unwrap(),
            current_user: base.join("api/v10/users/@me").unwrap(),
            revocation: base.join("api/v10/oauth2/token/revoke").unwrap(),
            permit_plaintext: true,
        }
    }
}

#[derive(Clone)]
pub struct DiscordOAuthConfigV1 {
    client_id: DiscordApplicationIdV1,
    redirect_uri: String,
    login_deadline: Duration,
    endpoints: DiscordOAuthEndpointsV1,
}

impl DiscordOAuthConfigV1 {
    pub fn new(
        client_id: DiscordApplicationIdV1,
        redirect_uri: &str,
    ) -> Result<Self, DiscordOAuthConfigError> {
        Self::with_deadline(client_id, redirect_uri, DEFAULT_DEADLINE)
    }

    pub fn with_deadline(
        client_id: DiscordApplicationIdV1,
        redirect_uri: &str,
        login_deadline: Duration,
    ) -> Result<Self, DiscordOAuthConfigError> {
        validate_redirect_uri(redirect_uri)?;
        validate_deadline(login_deadline)?;
        Ok(Self {
            client_id,
            redirect_uri: redirect_uri.to_string(),
            login_deadline,
            endpoints: DiscordOAuthEndpointsV1::production(),
        })
    }

    pub fn client_id(&self) -> DiscordApplicationIdV1 {
        self.client_id
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn login_deadline(&self) -> Duration {
        self.login_deadline
    }

    #[cfg(test)]
    fn for_local_server(
        client_id: DiscordApplicationIdV1,
        redirect_uri: &str,
        login_deadline: Duration,
        base: Url,
    ) -> Result<Self, DiscordOAuthConfigError> {
        validate_redirect_uri(redirect_uri)?;
        validate_deadline(login_deadline)?;
        Ok(Self {
            client_id,
            redirect_uri: redirect_uri.to_string(),
            login_deadline,
            endpoints: DiscordOAuthEndpointsV1::local(&base),
        })
    }
}

impl Debug for DiscordOAuthConfigV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiscordOAuthConfigV1")
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .field("login_deadline", &self.login_deadline)
            .finish_non_exhaustive()
    }
}

#[derive(PartialEq, Eq)]
pub struct VerifiedDiscordIdentityV1 {
    user_id: UserId,
    display_name: String,
}

impl VerifiedDiscordIdentityV1 {
    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

impl Debug for VerifiedDiscordIdentityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedDiscordIdentityV1")
            .field("user_id", &self.user_id)
            .field("display_name", &"<redacted>")
            .finish()
    }
}

pub struct DiscordOAuthClient {
    http: Client,
    config: DiscordOAuthConfigV1,
}

pub trait DiscordIdentifyOAuthPort {
    fn authorization_url(&self, state: &DiscordOAuthStateV1) -> Url;

    fn exchange_identify<'a>(
        &'a self,
        authorization_code: &'a DiscordAuthorizationCodeV1,
        client_secret: &'a DiscordOAuthClientSecretV1,
    ) -> impl Future<Output = Result<VerifiedDiscordIdentityV1, DiscordOAuthError>> + Send + 'a;
}

impl DiscordOAuthClient {
    pub fn new(config: DiscordOAuthConfigV1) -> Result<Self, DiscordOAuthConfigError> {
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(config.login_deadline)
            .no_proxy()
            .user_agent("starring-product-control/1");
        if !config.endpoints.permit_plaintext {
            builder = builder.https_only(true);
        }
        let http = builder
            .build()
            .map_err(|_| DiscordOAuthConfigError::HttpClient)?;
        Ok(Self { http, config })
    }

    pub fn authorization_url(&self, state: &DiscordOAuthStateV1) -> Url {
        let mut url = self.config.endpoints.authorization.clone();
        url.query_pairs_mut()
            .append_pair("client_id", &self.config.client_id.to_string())
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", "identify")
            .append_pair("state", state.expose_secret());
        url
    }

    pub async fn exchange_identify(
        &self,
        authorization_code: &DiscordAuthorizationCodeV1,
        client_secret: &DiscordOAuthClientSecretV1,
    ) -> Result<VerifiedDiscordIdentityV1, DiscordOAuthError> {
        let operation_deadline = Instant::now() + self.config.login_deadline;
        let revocation_reserve = (self.config.login_deadline / 2).min(MAX_REVOCATION_RESERVE);
        let identity_deadline = operation_deadline - revocation_reserve;
        let token_response = tokio::time::timeout_at(
            identity_deadline,
            self.exchange_authorization_code(authorization_code, client_secret, identity_deadline),
        )
        .await
        .map_err(|_| DiscordOAuthError::Timeout)??;
        let identity_result = match token_response.identity_access_token() {
            Ok(access_token) => {
                match tokio::time::timeout_at(
                    identity_deadline,
                    self.fetch_current_user(access_token, identity_deadline),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(DiscordOAuthError::Timeout),
                }
            }
            Err(error) => Err(error),
        };
        let revocation_result = tokio::time::timeout_at(
            operation_deadline,
            self.revoke_all_credentials(&token_response, client_secret, operation_deadline),
        )
        .await
        .map_err(|_| DiscordOAuthError::RevocationFailed)?;
        if revocation_result.is_err() {
            return Err(DiscordOAuthError::RevocationFailed);
        }
        identity_result
    }

    async fn exchange_authorization_code(
        &self,
        authorization_code: &DiscordAuthorizationCodeV1,
        client_secret: &DiscordOAuthClientSecretV1,
        deadline: Instant,
    ) -> Result<OAuthTokenResponse, DiscordOAuthError> {
        let client_id = self.config.client_id.to_string();
        let form = TokenExchangeForm {
            client_id: &client_id,
            client_secret: client_secret.expose_secret(),
            grant_type: "authorization_code",
            code: authorization_code.expose_secret(),
            redirect_uri: &self.config.redirect_uri,
        };
        let response = self
            .http
            .post(self.config.endpoints.token.clone())
            .timeout(remaining(deadline, DiscordOAuthError::Timeout)?)
            .form(&form)
            .send()
            .await
            .map_err(classify_network_error)?;
        match response.status() {
            StatusCode::OK => bounded_token_response(response).await,
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Err(DiscordOAuthError::ExchangeRejected)
            }
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                Err(DiscordOAuthError::Unavailable)
            }
            _ => Err(DiscordOAuthError::InvalidResponse),
        }
    }

    async fn fetch_current_user(
        &self,
        access_token: &OAuthCredential,
        deadline: Instant,
    ) -> Result<VerifiedDiscordIdentityV1, DiscordOAuthError> {
        let response = self
            .http
            .get(self.config.endpoints.current_user.clone())
            .timeout(remaining(deadline, DiscordOAuthError::Timeout)?)
            .bearer_auth(access_token.expose_secret())
            .send()
            .await
            .map_err(classify_network_error)?;
        let raw: CurrentUserResponse = match response.status() {
            StatusCode::OK => bounded_json(response).await?,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(DiscordOAuthError::ExchangeRejected);
            }
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                return Err(DiscordOAuthError::Unavailable);
            }
            _ => return Err(DiscordOAuthError::InvalidResponse),
        };
        raw.verify()
    }

    async fn revoke_all_credentials(
        &self,
        token_response: &OAuthTokenResponse,
        client_secret: &DiscordOAuthClientSecretV1,
        deadline: Instant,
    ) -> Result<(), DiscordOAuthError> {
        let access = async {
            match token_response.access_token.as_ref() {
                Some(token) => {
                    self.revoke_credential(token, "access_token", client_secret, deadline)
                        .await
                }
                None => Ok(()),
            }
        };
        let refresh = async {
            match token_response.refresh_token.as_ref() {
                Some(token) => {
                    self.revoke_credential(token, "refresh_token", client_secret, deadline)
                        .await
                }
                None => Ok(()),
            }
        };
        let (access, refresh) = tokio::join!(access, refresh);
        if access.is_err() || refresh.is_err() {
            Err(DiscordOAuthError::RevocationFailed)
        } else {
            Ok(())
        }
    }

    async fn revoke_credential(
        &self,
        credential: &OAuthCredential,
        token_type_hint: &'static str,
        client_secret: &DiscordOAuthClientSecretV1,
        deadline: Instant,
    ) -> Result<(), DiscordOAuthError> {
        let client_id = self.config.client_id.to_string();
        let form = TokenRevocationForm {
            token: credential.expose_secret(),
            token_type_hint,
        };
        let response = self
            .http
            .post(self.config.endpoints.revocation.clone())
            .timeout(remaining(deadline, DiscordOAuthError::RevocationFailed)?)
            .basic_auth(client_id, Some(client_secret.expose_secret()))
            .form(&form)
            .send()
            .await
            .map_err(|_| DiscordOAuthError::RevocationFailed)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(DiscordOAuthError::RevocationFailed)
        }
    }
}

impl DiscordIdentifyOAuthPort for DiscordOAuthClient {
    fn authorization_url(&self, state: &DiscordOAuthStateV1) -> Url {
        DiscordOAuthClient::authorization_url(self, state)
    }

    async fn exchange_identify(
        &self,
        authorization_code: &DiscordAuthorizationCodeV1,
        client_secret: &DiscordOAuthClientSecretV1,
    ) -> Result<VerifiedDiscordIdentityV1, DiscordOAuthError> {
        DiscordOAuthClient::exchange_identify(self, authorization_code, client_secret).await
    }
}

#[derive(Serialize)]
struct TokenExchangeForm<'a> {
    client_id: &'a str,
    client_secret: &'a str,
    grant_type: &'static str,
    code: &'a str,
    redirect_uri: &'a str,
}

#[derive(Serialize)]
struct TokenRevocationForm<'a> {
    token: &'a str,
    token_type_hint: &'static str,
}

struct OAuthCredential(Zeroizing<String>);

impl OAuthCredential {
    fn expose_secret(&self) -> &str {
        self.0.as_str()
    }

    fn valid(&self) -> bool {
        !self.0.is_empty()
            && self.0.len() <= MAX_CREDENTIAL_BYTES
            && !self.0.chars().any(char::is_control)
    }
}

impl Debug for OAuthCredential {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OAuthCredential(<redacted>)")
    }
}

struct OAuthTokenResponse {
    access_token: Option<OAuthCredential>,
    refresh_token: Option<OAuthCredential>,
    token_type: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
    metadata_valid: bool,
}

impl OAuthTokenResponse {
    fn identity_access_token(&self) -> Result<&OAuthCredential, DiscordOAuthError> {
        let access_token = self
            .access_token
            .as_ref()
            .ok_or(DiscordOAuthError::InvalidResponse)?;
        if !self.metadata_valid
            || !access_token.valid()
            || self
                .refresh_token
                .as_ref()
                .is_some_and(|token| !token.valid())
            || self.token_type.as_deref() != Some("Bearer")
            || self.expires_in.is_none_or(|seconds| seconds == 0)
            || self.scope.as_deref() != Some("identify")
        {
            return Err(DiscordOAuthError::InvalidResponse);
        }
        Ok(access_token)
    }
}

#[derive(Deserialize)]
struct CurrentUserResponse {
    id: String,
    username: String,
    global_name: Option<String>,
    bot: Option<bool>,
    system: Option<bool>,
}

impl CurrentUserResponse {
    fn verify(self) -> Result<VerifiedDiscordIdentityV1, DiscordOAuthError> {
        let parsed_id = self
            .id
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0 && value.to_string() == self.id)
            .ok_or(DiscordOAuthError::InvalidResponse)?;
        if self.bot.unwrap_or(false) || self.system.unwrap_or(false) {
            return Err(DiscordOAuthError::InvalidResponse);
        }
        let display_name = self
            .global_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.username)
            .trim();
        if display_name.is_empty()
            || display_name.len() > MAX_DISPLAY_NAME_BYTES
            || display_name.chars().count() > MAX_DISPLAY_NAME_SCALARS
            || display_name.chars().any(char::is_control)
        {
            return Err(DiscordOAuthError::InvalidResponse);
        }
        Ok(VerifiedDiscordIdentityV1 {
            user_id: UserId(parsed_id),
            display_name: display_name.to_string(),
        })
    }
}

async fn bounded_json<T>(mut response: Response) -> Result<T, DiscordOAuthError>
where
    T: for<'de> Deserialize<'de>,
{
    if !response_is_json(&response) {
        return Err(DiscordOAuthError::InvalidResponse);
    }
    let body = bounded_body(&mut response).await?;
    serde_json::from_slice(body.as_slice()).map_err(|_| DiscordOAuthError::InvalidResponse)
}

async fn bounded_token_response(
    mut response: Response,
) -> Result<OAuthTokenResponse, DiscordOAuthError> {
    if !response_is_json(&response) {
        return Err(DiscordOAuthError::InvalidResponse);
    }
    let body = bounded_body(&mut response).await?;
    decode_token_response(body.as_slice())
}

async fn bounded_body(response: &mut Response) -> Result<Zeroizing<Vec<u8>>, DiscordOAuthError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(DiscordOAuthError::ResponseTooLarge);
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await.map_err(classify_network_error)? {
        if body
            .len()
            .checked_add(chunk.len())
            .is_none_or(|length| length > MAX_RESPONSE_BYTES)
        {
            return Err(DiscordOAuthError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn decode_token_response(body: &[u8]) -> Result<OAuthTokenResponse, DiscordOAuthError> {
    let mut value = serde_json::from_slice::<serde_json::Value>(body)
        .map_err(|_| DiscordOAuthError::InvalidResponse)?;
    let result = if let Some(object) = value.as_object_mut() {
        let (access_token, access_valid) = take_credential(object.remove("access_token"), true);
        let (refresh_token, refresh_valid) = take_credential(object.remove("refresh_token"), false);
        let (token_type, token_type_valid) = take_string(object.remove("token_type"));
        let (scope, scope_valid) = take_string(object.remove("scope"));
        let (expires_in, expires_valid) = take_u64(object.remove("expires_in"));
        Ok(OAuthTokenResponse {
            access_token,
            refresh_token,
            token_type,
            expires_in,
            scope,
            metadata_valid: access_valid
                && refresh_valid
                && token_type_valid
                && scope_valid
                && expires_valid,
        })
    } else {
        Err(DiscordOAuthError::InvalidResponse)
    };
    zeroize_json_strings(&mut value);
    result
}

fn take_credential(
    value: Option<serde_json::Value>,
    required: bool,
) -> (Option<OAuthCredential>, bool) {
    match value {
        Some(serde_json::Value::String(value)) => {
            (Some(OAuthCredential(Zeroizing::new(value))), true)
        }
        Some(serde_json::Value::Null) if !required => (None, true),
        None if !required => (None, true),
        Some(mut value) => {
            zeroize_json_strings(&mut value);
            (None, false)
        }
        None => (None, false),
    }
}

fn take_string(value: Option<serde_json::Value>) -> (Option<String>, bool) {
    match value {
        Some(serde_json::Value::String(value)) => (Some(value), true),
        Some(mut value) => {
            zeroize_json_strings(&mut value);
            (None, false)
        }
        None => (None, false),
    }
}

fn take_u64(value: Option<serde_json::Value>) -> (Option<u64>, bool) {
    match value {
        Some(serde_json::Value::Number(value)) => (value.as_u64(), value.as_u64().is_some()),
        Some(mut value) => {
            zeroize_json_strings(&mut value);
            (None, false)
        }
        None => (None, false),
    }
}

fn zeroize_json_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(value) => value.zeroize(),
        serde_json::Value::Array(values) => {
            for value in values {
                zeroize_json_strings(value);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                zeroize_json_strings(value);
            }
        }
        _ => {}
    }
}

fn response_is_json(response: &Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

fn classify_network_error(error: reqwest::Error) -> DiscordOAuthError {
    if error.is_timeout() {
        DiscordOAuthError::Timeout
    } else {
        DiscordOAuthError::Unavailable
    }
}

fn remaining(deadline: Instant, elapsed: DiscordOAuthError) -> Result<Duration, DiscordOAuthError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(elapsed)
    } else {
        Ok(remaining)
    }
}

fn validate_redirect_uri(value: &str) -> Result<(), DiscordOAuthConfigError> {
    let url = Url::parse(value).map_err(|_| DiscordOAuthConfigError::InvalidRedirectUri)?;
    if url.scheme() != "https"
        || !url.has_host()
        || url.username() != ""
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.as_str() != value
    {
        return Err(DiscordOAuthConfigError::InvalidRedirectUri);
    }
    Ok(())
}

fn validate_deadline(value: Duration) -> Result<(), DiscordOAuthConfigError> {
    if value.is_zero() || value > MAX_DEADLINE {
        Err(DiscordOAuthConfigError::InvalidDeadline)
    } else {
        Ok(())
    }
}

fn canonical_secret(value: &str) -> bool {
    value.len() == OAUTH_STATE_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && value.as_bytes().last().is_some_and(|byte| {
            matches!(
                byte,
                b'A' | b'E'
                    | b'I'
                    | b'M'
                    | b'Q'
                    | b'U'
                    | b'Y'
                    | b'c'
                    | b'g'
                    | b'k'
                    | b'o'
                    | b's'
                    | b'w'
                    | b'0'
                    | b'4'
                    | b'8'
            )
        })
}

#[cfg(test)]
#[path = "oauth_tests.rs"]
mod tests;
