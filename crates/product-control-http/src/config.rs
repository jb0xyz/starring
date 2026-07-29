use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderValue;
use url::Url;

const MIN_BODY_LIMIT: usize = 1_024;
const MAX_BODY_LIMIT: usize = 1_048_576;
const MAX_IN_FLIGHT: usize = 4_096;
const OAUTH_START_BUDGET_CAPACITY: u32 = 10;
const OAUTH_START_BUDGET_REFILL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_AUTHORING_WORKER_CALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_AUTHORING_COORDINATION_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const MAX_AUTHORING_REQUEST_TIMEOUT: Duration = Duration::from_secs(32 * 60);
const MAX_AUTHORING_RETRY_AFTER_SECONDS: u64 = 60;
const DEFAULT_AUTHORING_MAX_IN_FLIGHT: usize = 64;
const MAX_AUTHORING_MAX_IN_FLIGHT: usize = 256;

#[derive(Clone, Copy)]
pub(crate) struct OAuthStartBudgetConfig {
    capacity: u32,
    refill_interval: Duration,
}

impl OAuthStartBudgetConfig {
    const fn production() -> Self {
        Self {
            capacity: OAUTH_START_BUDGET_CAPACITY,
            refill_interval: OAUTH_START_BUDGET_REFILL_INTERVAL,
        }
    }

    pub(crate) const fn capacity(self) -> u32 {
        self.capacity
    }

    pub(crate) const fn refill_interval(self) -> Duration {
        self.refill_interval
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HttpBoundaryConfigError {
    #[error("public origin is invalid")]
    InvalidPublicOrigin,
    #[error("request body limit is outside the supported range")]
    InvalidBodyLimit,
    #[error("request concurrency limit is outside the supported range")]
    InvalidConcurrencyLimit,
    #[error("request timeout must be positive")]
    InvalidRequestTimeout,
    #[error("OAuth return path allowlist is invalid")]
    InvalidReturnPaths,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringHttpBoundaryConfigErrorV1 {
    #[error("authoring worker call timeout is invalid")]
    InvalidWorkerCallTimeout,
    #[error("authoring coordination timeout is invalid")]
    InvalidCoordinationTimeout,
    #[error("authoring request timeout cannot be represented")]
    InvalidRequestTimeout,
    #[error("authoring retry delay is invalid")]
    InvalidRetryAfter,
    #[error("authoring concurrency limit is invalid")]
    InvalidConcurrencyLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthoringHttpBoundaryConfigV1 {
    request_timeout: Duration,
    retry_after_seconds: u64,
    max_in_flight: usize,
}

impl AuthoringHttpBoundaryConfigV1 {
    pub fn new(
        worker_call_timeout: Duration,
        coordination_timeout: Duration,
        retry_after_seconds: u64,
    ) -> Result<Self, AuthoringHttpBoundaryConfigErrorV1> {
        if worker_call_timeout.is_zero() || worker_call_timeout > MAX_AUTHORING_WORKER_CALL_TIMEOUT
        {
            return Err(AuthoringHttpBoundaryConfigErrorV1::InvalidWorkerCallTimeout);
        }
        if coordination_timeout.is_zero()
            || coordination_timeout > MAX_AUTHORING_COORDINATION_TIMEOUT
        {
            return Err(AuthoringHttpBoundaryConfigErrorV1::InvalidCoordinationTimeout);
        }
        let request_timeout = worker_call_timeout
            .checked_mul(authoring_application::AUTHORING_MAX_MODEL_CALLS_V1)
            .and_then(|timeout| timeout.checked_add(coordination_timeout))
            .filter(|timeout| *timeout <= MAX_AUTHORING_REQUEST_TIMEOUT)
            .ok_or(AuthoringHttpBoundaryConfigErrorV1::InvalidRequestTimeout)?;
        if retry_after_seconds == 0 || retry_after_seconds > MAX_AUTHORING_RETRY_AFTER_SECONDS {
            return Err(AuthoringHttpBoundaryConfigErrorV1::InvalidRetryAfter);
        }
        Ok(Self {
            request_timeout,
            retry_after_seconds,
            max_in_flight: DEFAULT_AUTHORING_MAX_IN_FLIGHT,
        })
    }

    pub fn production(
        worker_call_timeout: Duration,
    ) -> Result<Self, AuthoringHttpBoundaryConfigErrorV1> {
        Self::new(worker_call_timeout, Duration::from_secs(30), 1)
    }

    pub fn request_timeout(self) -> Duration {
        self.request_timeout
    }

    pub fn retry_after_seconds(self) -> u64 {
        self.retry_after_seconds
    }

    pub fn with_max_in_flight(
        mut self,
        max_in_flight: usize,
    ) -> Result<Self, AuthoringHttpBoundaryConfigErrorV1> {
        if max_in_flight == 0 || max_in_flight > MAX_AUTHORING_MAX_IN_FLIGHT {
            return Err(AuthoringHttpBoundaryConfigErrorV1::InvalidConcurrencyLimit);
        }
        self.max_in_flight = max_in_flight;
        Ok(self)
    }

    pub fn max_in_flight(self) -> usize {
        self.max_in_flight
    }
}

#[derive(Clone)]
pub struct HttpBoundaryConfig {
    public_origin: HeaderValue,
    public_host: HeaderValue,
    oauth_callback_url: String,
    return_paths: Arc<BTreeSet<String>>,
    body_limit: usize,
    max_in_flight: usize,
    request_timeout: Duration,
    oauth_start_budget: OAuthStartBudgetConfig,
}

impl HttpBoundaryConfig {
    pub fn new(
        public_origin: &str,
        body_limit: usize,
        max_in_flight: usize,
        request_timeout: Duration,
        return_paths: impl IntoIterator<Item = String>,
    ) -> Result<Self, HttpBoundaryConfigError> {
        let url =
            Url::parse(public_origin).map_err(|_| HttpBoundaryConfigError::InvalidPublicOrigin)?;
        if url.scheme() != "https"
            || url.username() != ""
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(HttpBoundaryConfigError::InvalidPublicOrigin);
        }
        let host = url
            .host_str()
            .ok_or(HttpBoundaryConfigError::InvalidPublicOrigin)?;
        let origin = url.origin().ascii_serialization();
        let public_host = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
        let public_origin = HeaderValue::from_str(&origin)
            .map_err(|_| HttpBoundaryConfigError::InvalidPublicOrigin)?;
        let public_host = HeaderValue::from_str(&public_host)
            .map_err(|_| HttpBoundaryConfigError::InvalidPublicOrigin)?;
        if !(MIN_BODY_LIMIT..=MAX_BODY_LIMIT).contains(&body_limit) {
            return Err(HttpBoundaryConfigError::InvalidBodyLimit);
        }
        if max_in_flight == 0 || max_in_flight > MAX_IN_FLIGHT {
            return Err(HttpBoundaryConfigError::InvalidConcurrencyLimit);
        }
        if request_timeout.is_zero() {
            return Err(HttpBoundaryConfigError::InvalidRequestTimeout);
        }
        let return_paths = return_paths.into_iter().collect::<BTreeSet<_>>();
        if return_paths.is_empty()
            || return_paths.len() > 64
            || return_paths.iter().any(|path| !valid_return_path(path))
        {
            return Err(HttpBoundaryConfigError::InvalidReturnPaths);
        }
        Ok(Self {
            public_origin,
            public_host,
            oauth_callback_url: format!("{origin}/oauth/discord/callback"),
            return_paths: Arc::new(return_paths),
            body_limit,
            max_in_flight,
            request_timeout,
            oauth_start_budget: OAuthStartBudgetConfig::production(),
        })
    }

    pub fn production(
        public_origin: &str,
        return_paths: impl IntoIterator<Item = String>,
    ) -> Result<Self, HttpBoundaryConfigError> {
        Self::new(
            public_origin,
            64 * 1_024,
            256,
            Duration::from_secs(10),
            return_paths,
        )
    }

    pub(crate) fn public_origin(&self) -> &HeaderValue {
        &self.public_origin
    }

    pub(crate) fn public_host(&self) -> &HeaderValue {
        &self.public_host
    }

    pub(crate) fn oauth_callback_url(&self) -> &str {
        &self.oauth_callback_url
    }

    pub(crate) fn allows_return_path(&self, path: &str) -> bool {
        self.return_paths.contains(path)
    }

    pub(crate) fn body_limit(&self) -> usize {
        self.body_limit
    }

    pub(crate) fn max_in_flight(&self) -> usize {
        self.max_in_flight
    }

    pub(crate) fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub(crate) fn oauth_start_budget(&self) -> OAuthStartBudgetConfig {
        self.oauth_start_budget
    }
}

pub(crate) fn valid_return_path(value: &str) -> bool {
    (value == "/" || !value.ends_with('/'))
        && !value.is_empty()
        && value.len() <= 256
        && value.is_ascii()
        && value.starts_with('/')
        && !value.starts_with("//")
        && !value.contains("//")
        && value
            .bytes()
            .all(|byte| byte == b'/' || byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && (value == "/"
            || value
                .split('/')
                .skip(1)
                .all(|segment| !matches!(segment, "." | "..") && !segment.is_empty()))
}
