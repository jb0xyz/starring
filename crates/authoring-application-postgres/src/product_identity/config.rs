use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use url::Url;

use crate::{AuthenticationConfigError, PostgresAuthenticationConfig};

const MAX_OAUTH_FLOW_LIFETIME_SECONDS: u64 = 10 * 60;
const MAX_SESSION_ABSOLUTE_LIFETIME_SECONDS: u64 = 12 * 60 * 60;
const MAX_SESSION_IDLE_LIFETIME_SECONDS: u64 = 30 * 60;
const MAX_RETURN_PATHS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductIdentityConfigError {
    #[error("product OAuth redirect URI is invalid")]
    InvalidRedirectUri,
    #[error("product OAuth return path allowlist is invalid")]
    InvalidReturnPaths,
    #[error("product OAuth flow lifetime is invalid")]
    InvalidOAuthFlowLifetime,
    #[error("product session absolute lifetime is invalid")]
    InvalidSessionAbsoluteLifetime,
    #[error("product session idle lifetime is invalid")]
    InvalidSessionIdleLifetime,
    #[error("product authentication configuration is invalid")]
    InvalidAuthentication,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductIdentityLifetimesV1 {
    oauth_flow: Duration,
    session_idle: Duration,
    session_absolute: Duration,
    touch_interval: Duration,
    statement_timeout: Duration,
}

impl ProductIdentityLifetimesV1 {
    pub fn new(
        oauth_flow: Duration,
        session_idle: Duration,
        session_absolute: Duration,
        touch_interval: Duration,
        statement_timeout: Duration,
    ) -> Result<Self, ProductIdentityConfigError> {
        if oauth_flow.as_secs() == 0
            || oauth_flow > Duration::from_secs(MAX_OAUTH_FLOW_LIFETIME_SECONDS)
        {
            return Err(ProductIdentityConfigError::InvalidOAuthFlowLifetime);
        }
        if session_absolute.as_secs() == 0
            || session_absolute > Duration::from_secs(MAX_SESSION_ABSOLUTE_LIFETIME_SECONDS)
        {
            return Err(ProductIdentityConfigError::InvalidSessionAbsoluteLifetime);
        }
        if session_idle.is_zero()
            || session_idle > session_absolute
            || session_idle > Duration::from_secs(MAX_SESSION_IDLE_LIFETIME_SECONDS)
        {
            return Err(ProductIdentityConfigError::InvalidSessionIdleLifetime);
        }
        PostgresAuthenticationConfig::new(session_idle, touch_interval, statement_timeout)
            .map_err(map_authentication_config)?;
        Ok(Self {
            oauth_flow,
            session_idle,
            session_absolute,
            touch_interval,
            statement_timeout,
        })
    }

    pub fn production() -> Self {
        Self::new(
            Duration::from_secs(10 * 60),
            Duration::from_secs(30 * 60),
            Duration::from_secs(12 * 60 * 60),
            Duration::from_secs(5 * 60),
            Duration::from_secs(2),
        )
        .expect("production product identity lifetimes are valid")
    }

    pub(crate) fn oauth_flow(self) -> Duration {
        self.oauth_flow
    }

    pub(crate) fn session_idle(self) -> Duration {
        self.session_idle
    }

    pub(crate) fn session_absolute(self) -> Duration {
        self.session_absolute
    }

    pub(crate) fn authentication(self) -> PostgresAuthenticationConfig {
        PostgresAuthenticationConfig::new(
            self.session_idle,
            self.touch_interval,
            self.statement_timeout,
        )
        .expect("validated product identity authentication configuration remains valid")
    }
}

#[derive(Clone, Debug)]
pub struct PostgresProductIdentityConfig {
    redirect_uri: String,
    allowed_return_paths: Arc<BTreeSet<String>>,
    lifetimes: ProductIdentityLifetimesV1,
}

impl PostgresProductIdentityConfig {
    pub fn new(
        redirect_uri: &str,
        allowed_return_paths: impl IntoIterator<Item = String>,
        lifetimes: ProductIdentityLifetimesV1,
    ) -> Result<Self, ProductIdentityConfigError> {
        if !valid_redirect_uri(redirect_uri) {
            return Err(ProductIdentityConfigError::InvalidRedirectUri);
        }
        let allowed_return_paths = allowed_return_paths.into_iter().collect::<BTreeSet<_>>();
        if allowed_return_paths.is_empty()
            || allowed_return_paths.len() > MAX_RETURN_PATHS
            || allowed_return_paths
                .iter()
                .any(|path| !valid_return_path(path))
        {
            return Err(ProductIdentityConfigError::InvalidReturnPaths);
        }
        Ok(Self {
            redirect_uri: redirect_uri.to_string(),
            allowed_return_paths: Arc::new(allowed_return_paths),
            lifetimes,
        })
    }

    pub fn production(
        redirect_uri: &str,
        allowed_return_paths: impl IntoIterator<Item = String>,
    ) -> Result<Self, ProductIdentityConfigError> {
        Self::new(
            redirect_uri,
            allowed_return_paths,
            ProductIdentityLifetimesV1::production(),
        )
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn allows_return_path(&self, return_path: &str) -> bool {
        self.allowed_return_paths.contains(return_path)
    }

    pub(crate) fn allowed_return_paths(&self) -> Vec<String> {
        self.allowed_return_paths.iter().cloned().collect()
    }

    pub(crate) fn lifetimes(&self) -> ProductIdentityLifetimesV1 {
        self.lifetimes
    }
}

fn valid_redirect_uri(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 2_048
        || value != value.trim()
        || value.chars().any(char::is_whitespace)
    {
        return false;
    }
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().starts_with('/')
        && url.as_str() == value
}

fn valid_return_path(value: &str) -> bool {
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
                .all(|segment| !segment.is_empty() && !matches!(segment, "." | "..")))
}

fn map_authentication_config(_error: AuthenticationConfigError) -> ProductIdentityConfigError {
    ProductIdentityConfigError::InvalidAuthentication
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_configuration_accepts_exact_callback_and_paths() {
        let config = PostgresProductIdentityConfig::production(
            "https://starring.example/oauth/discord/callback",
            ["/app".to_string(), "/".to_string()],
        )
        .unwrap();
        assert!(config.allows_return_path("/app"));
        assert!(!config.allows_return_path("/admin"));
    }

    #[test]
    fn configuration_rejects_ambiguous_redirects_and_paths() {
        for redirect in [
            "http://starring.example/oauth/discord/callback",
            "https://starring.example/oauth/discord/callback?next=/app",
            "https://user@starring.example/oauth/discord/callback",
            "https://starring.example/oauth/discord/callback#fragment",
        ] {
            assert!(
                PostgresProductIdentityConfig::production(redirect, ["/app".to_string()]).is_err()
            );
        }
        for path in [
            "",
            "app",
            "//app",
            "/app/",
            "/app//settings",
            "/app?next=/admin",
        ] {
            assert!(PostgresProductIdentityConfig::production(
                "https://starring.example/oauth/discord/callback",
                [path.to_string()]
            )
            .is_err());
        }
    }

    #[test]
    fn lifetimes_reject_values_outside_product_limits() {
        assert_eq!(
            ProductIdentityLifetimesV1::new(
                Duration::from_millis(999),
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(5),
                Duration::from_secs(1),
            ),
            Err(ProductIdentityConfigError::InvalidOAuthFlowLifetime)
        );
        assert_eq!(
            ProductIdentityLifetimesV1::new(
                Duration::from_secs(601),
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(5),
                Duration::from_secs(1),
            ),
            Err(ProductIdentityConfigError::InvalidOAuthFlowLifetime)
        );
        assert_eq!(
            ProductIdentityLifetimesV1::new(
                Duration::from_secs(60),
                Duration::from_secs(30),
                Duration::from_secs(43_201),
                Duration::from_secs(5),
                Duration::from_secs(1),
            ),
            Err(ProductIdentityConfigError::InvalidSessionAbsoluteLifetime)
        );
        assert_eq!(
            ProductIdentityLifetimesV1::new(
                Duration::from_secs(60),
                Duration::from_secs(MAX_SESSION_IDLE_LIFETIME_SECONDS + 1),
                Duration::from_secs(3_600),
                Duration::from_secs(5),
                Duration::from_secs(1),
            ),
            Err(ProductIdentityConfigError::InvalidSessionIdleLifetime)
        );
        assert_eq!(
            ProductIdentityLifetimesV1::new(
                Duration::from_secs(60),
                Duration::from_millis(500),
                Duration::from_millis(999),
                Duration::from_millis(100),
                Duration::from_secs(1),
            ),
            Err(ProductIdentityConfigError::InvalidSessionAbsoluteLifetime)
        );
    }
}
