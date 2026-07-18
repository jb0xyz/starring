use std::time::Duration;

use authoring_application::{AuthenticatedIdentityV1, AuthenticationError, AuthenticationPort};
use authoring_promotion::PrincipalId;
use chrono::{DateTime, TimeDelta, Utc};
use sqlx::postgres::PgPool;

use crate::digest::digest_opaque_session_credential_v1;

const DEFAULT_IDLE_LIFETIME_SECONDS: u64 = 30 * 60;
const DEFAULT_TOUCH_INTERVAL_SECONDS: u64 = 5 * 60;
const DEFAULT_STATEMENT_TIMEOUT_MILLIS: u64 = 2_000;
const MAX_STATEMENT_TIMEOUT_MILLIS: u64 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationConfigError {
    #[error("product authentication idle lifetime must be positive")]
    IdleLifetimeZero,
    #[error(
        "product authentication touch interval must be positive and shorter than the idle lifetime"
    )]
    InvalidTouchInterval,
    #[error("product authentication statement timeout is outside the supported range")]
    InvalidStatementTimeout,
    #[error("product authentication duration exceeds the supported range")]
    DurationOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostgresAuthenticationConfig {
    idle_lifetime: Duration,
    touch_interval: Duration,
    statement_timeout: Duration,
}

impl PostgresAuthenticationConfig {
    pub fn new(
        idle_lifetime: Duration,
        touch_interval: Duration,
        statement_timeout: Duration,
    ) -> Result<Self, AuthenticationConfigError> {
        if idle_lifetime.is_zero() {
            return Err(AuthenticationConfigError::IdleLifetimeZero);
        }
        if touch_interval.is_zero() || touch_interval >= idle_lifetime {
            return Err(AuthenticationConfigError::InvalidTouchInterval);
        }
        let statement_timeout_millis = statement_timeout.as_millis();
        if statement_timeout_millis == 0
            || statement_timeout_millis > u128::from(MAX_STATEMENT_TIMEOUT_MILLIS)
        {
            return Err(AuthenticationConfigError::InvalidStatementTimeout);
        }
        TimeDelta::from_std(idle_lifetime)
            .map_err(|_| AuthenticationConfigError::DurationOverflow)?;
        TimeDelta::from_std(touch_interval)
            .map_err(|_| AuthenticationConfigError::DurationOverflow)?;
        Ok(Self {
            idle_lifetime,
            touch_interval,
            statement_timeout,
        })
    }

    fn idle_lifetime(self) -> Result<TimeDelta, AuthenticationError> {
        TimeDelta::from_std(self.idle_lifetime)
            .map_err(|_| backend("product authentication idle lifetime exceeds chrono range"))
    }

    fn touch_interval(self) -> Result<TimeDelta, AuthenticationError> {
        TimeDelta::from_std(self.touch_interval)
            .map_err(|_| backend("product authentication touch interval exceeds chrono range"))
    }

    fn statement_timeout(self) -> String {
        format!("{}ms", self.statement_timeout.as_millis())
    }
}

impl Default for PostgresAuthenticationConfig {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(DEFAULT_IDLE_LIFETIME_SECONDS),
            Duration::from_secs(DEFAULT_TOUCH_INTERVAL_SECONDS),
            Duration::from_millis(DEFAULT_STATEMENT_TIMEOUT_MILLIS),
        )
        .expect("default product authentication configuration is valid")
    }
}

#[derive(Clone)]
pub struct PostgresAuthentication {
    pool: PgPool,
    config: PostgresAuthenticationConfig,
}

impl PostgresAuthentication {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            config: PostgresAuthenticationConfig::default(),
        }
    }

    pub fn with_config(pool: PgPool, config: PostgresAuthenticationConfig) -> Self {
        Self { pool, config }
    }
}

#[derive(sqlx::FromRow)]
struct AuthenticationRow {
    principal_id: String,
    disabled: bool,
    last_seen_at: DateTime<Utc>,
    idle_expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    database_now: DateTime<Utc>,
}

impl AuthenticationRow {
    fn authenticate(
        &self,
        config: PostgresAuthenticationConfig,
    ) -> Result<PrincipalId, AuthenticationError> {
        if self.disabled {
            return Err(AuthenticationError::InvalidCredential);
        }
        if self.revoked_at.is_some() {
            return Err(AuthenticationError::Revoked);
        }
        if self.database_now >= self.idle_expires_at
            || self.database_now >= self.absolute_expires_at
        {
            return Err(AuthenticationError::Expired);
        }
        if self.last_seen_at > self.database_now || self.idle_expires_at > self.absolute_expires_at
        {
            return Err(backend(
                "persisted product authentication session is inconsistent",
            ));
        }
        PrincipalId::parse(&self.principal_id)
            .map_err(|_| backend("persisted product principal identifier is invalid"))
            .and_then(|principal_id| {
                config.idle_lifetime()?;
                config.touch_interval()?;
                Ok(principal_id)
            })
    }

    fn touch_due(&self, config: PostgresAuthenticationConfig) -> Result<bool, AuthenticationError> {
        Ok(self.database_now - self.last_seen_at >= config.touch_interval()?)
    }

    fn extended_idle_expiry(
        &self,
        config: PostgresAuthenticationConfig,
    ) -> Result<DateTime<Utc>, AuthenticationError> {
        let candidate = self
            .database_now
            .checked_add_signed(config.idle_lifetime()?)
            .ok_or_else(|| backend("product authentication idle expiry overflow"))?;
        Ok(self
            .idle_expires_at
            .max(candidate.min(self.absolute_expires_at)))
    }
}

impl AuthenticationPort for PostgresAuthentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticatedIdentityV1, AuthenticationError> {
        let digest = digest_opaque_session_credential_v1(credential)
            .map_err(|_| AuthenticationError::InvalidCredential)?;
        let mut transaction = self.pool.begin().await.map_err(backend)?;
        sqlx::query("SELECT set_config('statement_timeout', $1, true)")
            .bind(self.config.statement_timeout())
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
        let row = sqlx::query_as::<_, AuthenticationRow>(
            "SELECT authentication_session.principal_id, principal.disabled, \
             authentication_session.last_seen_at, authentication_session.idle_expires_at, \
             authentication_session.absolute_expires_at, authentication_session.revoked_at, \
             CURRENT_TIMESTAMP AS database_now \
             FROM product_auth_sessions AS authentication_session \
             INNER JOIN product_principals AS principal \
             ON principal.principal_id = authentication_session.principal_id \
             WHERE authentication_session.session_digest = $1 \
             FOR UPDATE OF authentication_session, principal",
        )
        .bind(digest.as_bytes().as_slice())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(backend)?
        .ok_or(AuthenticationError::InvalidCredential)?;
        let principal_id = row.authenticate(self.config)?;
        if row.touch_due(self.config)? {
            let idle_expires_at = row.extended_idle_expiry(self.config)?;
            let result = sqlx::query(
                "UPDATE product_auth_sessions \
                 SET last_seen_at = $2, idle_expires_at = $3 \
                 WHERE session_digest = $1 \
                 AND revoked_at IS NULL \
                 AND last_seen_at = $4 \
                 AND idle_expires_at = $5 \
                 AND absolute_expires_at = $6",
            )
            .bind(digest.as_bytes().as_slice())
            .bind(row.database_now)
            .bind(idle_expires_at)
            .bind(row.last_seen_at)
            .bind(row.idle_expires_at)
            .bind(row.absolute_expires_at)
            .execute(&mut *transaction)
            .await
            .map_err(backend)?;
            if result.rows_affected() != 1 {
                return Err(backend(
                    "product authentication session touch lost its row lock",
                ));
            }
        }
        transaction.commit().await.map_err(backend)?;
        Ok(AuthenticatedIdentityV1::from_authentication(principal_id))
    }
}

fn backend(error: impl std::fmt::Display) -> AuthenticationError {
    AuthenticationError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn row() -> AuthenticationRow {
        AuthenticationRow {
            principal_id: "principal-1".to_string(),
            disabled: false,
            last_seen_at: Utc.with_ymd_and_hms(2026, 7, 19, 11, 50, 0).unwrap(),
            idle_expires_at: Utc.with_ymd_and_hms(2026, 7, 19, 12, 10, 0).unwrap(),
            absolute_expires_at: Utc.with_ymd_and_hms(2026, 7, 19, 20, 0, 0).unwrap(),
            revoked_at: None,
            database_now: Utc.with_ymd_and_hms(2026, 7, 19, 12, 0, 0).unwrap(),
        }
    }

    #[test]
    fn configuration_rejects_unbounded_and_unsafe_intervals() {
        assert_eq!(
            PostgresAuthenticationConfig::new(
                Duration::ZERO,
                Duration::from_secs(1),
                Duration::from_secs(1)
            ),
            Err(AuthenticationConfigError::IdleLifetimeZero)
        );
        assert_eq!(
            PostgresAuthenticationConfig::new(
                Duration::from_secs(30),
                Duration::from_secs(30),
                Duration::from_secs(1)
            ),
            Err(AuthenticationConfigError::InvalidTouchInterval)
        );
        assert_eq!(
            PostgresAuthenticationConfig::new(
                Duration::from_secs(30),
                Duration::from_secs(5),
                Duration::from_secs(61)
            ),
            Err(AuthenticationConfigError::InvalidStatementTimeout)
        );
    }

    #[test]
    fn classification_fails_closed_for_disabled_revoked_and_expired_rows() {
        let config = PostgresAuthenticationConfig::default();
        let mut disabled = row();
        disabled.disabled = true;
        assert_eq!(
            disabled.authenticate(config),
            Err(AuthenticationError::InvalidCredential)
        );
        let mut revoked = row();
        revoked.revoked_at = Some(revoked.database_now);
        assert_eq!(
            revoked.authenticate(config),
            Err(AuthenticationError::Revoked)
        );
        let mut expired = row();
        expired.idle_expires_at = expired.database_now;
        assert_eq!(
            expired.authenticate(config),
            Err(AuthenticationError::Expired)
        );
    }

    #[test]
    fn active_row_uses_database_time_for_bounded_touch() {
        let row = row();
        let config = PostgresAuthenticationConfig::default();
        assert_eq!(row.authenticate(config).unwrap().as_str(), "principal-1");
        assert!(row.touch_due(config).unwrap());
        assert_eq!(
            row.extended_idle_expiry(config).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 19, 12, 30, 0).unwrap()
        );
    }
}
