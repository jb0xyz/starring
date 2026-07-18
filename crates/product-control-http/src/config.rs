use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use axum::http::HeaderValue;
use url::Url;

const MIN_BODY_LIMIT: usize = 1_024;
const MAX_BODY_LIMIT: usize = 1_048_576;
const MAX_IN_FLIGHT: usize = 4_096;

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

#[derive(Clone)]
pub struct HttpBoundaryConfig {
    public_origin: HeaderValue,
    public_host: HeaderValue,
    oauth_callback_url: String,
    return_paths: Arc<BTreeSet<String>>,
    body_limit: usize,
    max_in_flight: usize,
    request_timeout: Duration,
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
