use std::time::Duration;

use authoring_application::{
    AuthenticatedSessionFingerprintV1, AuthenticationBackendFailureV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, MutationAuthenticationPort,
};
use authoring_promotion::PrincipalId;
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::types::Json;
use subtle::ConstantTimeEq;

use crate::digest::digest_opaque_session_credential_v1;
use crate::{ProductDatabaseFailureV1, ProductSessionDigestV1};

const DEFAULT_IDLE_LIFETIME_SECONDS: u64 = 30 * 60;
const DEFAULT_TOUCH_INTERVAL_SECONDS: u64 = 5 * 60;
const DEFAULT_STATEMENT_TIMEOUT_MILLIS: u64 = 2_000;
const MAX_IDLE_LIFETIME_SECONDS: u64 = 30 * 60;
const MAX_STATEMENT_TIMEOUT_MILLIS: u64 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationConfigError {
    #[error("product authentication idle lifetime must be positive")]
    IdleLifetimeZero,
    #[error("product authentication idle lifetime exceeds the supported maximum")]
    IdleLifetimeTooLong,
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
        if idle_lifetime > Duration::from_secs(MAX_IDLE_LIFETIME_SECONDS) {
            return Err(AuthenticationConfigError::IdleLifetimeTooLong);
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

    fn idle_lifetime_seconds(self) -> f64 {
        self.idle_lifetime.as_secs_f64()
    }

    fn touch_interval(self) -> TimeDelta {
        TimeDelta::from_std(self.touch_interval)
            .expect("validated product authentication touch interval remains in chrono range")
    }

    pub(crate) fn statement_timeout(self) -> String {
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
    discord_user_id: String,
    identity_revision: i64,
    display_profile: Json<Value>,
    disabled: bool,
    csrf_digest: Vec<u8>,
    oauth_state_digest: Option<Vec<u8>>,
    last_seen_at: DateTime<Utc>,
    idle_expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl AuthenticationRow {
    fn authenticate(
        &self,
        session_fingerprint: ProductSessionDigestV1,
        csrf_digest: Option<&[u8; 32]>,
        database_now: DateTime<Utc>,
    ) -> Result<ActiveProductSessionV1, SessionValidationError> {
        let persisted_csrf: [u8; 32] = self
            .csrf_digest
            .as_slice()
            .try_into()
            .map_err(|_| SessionValidationError::Invariant)?;
        if let Some(expected) = csrf_digest {
            if persisted_csrf.ct_eq(expected).unwrap_u8() != 1 {
                return Err(SessionValidationError::InvalidCsrf);
            }
        }
        if self.disabled {
            return Err(SessionValidationError::InvalidCredential);
        }
        if self.revoked_at.is_some() {
            return Err(SessionValidationError::Revoked);
        }
        if self.oauth_state_digest.as_deref().map(<[u8]>::len) != Some(32) {
            return Err(SessionValidationError::Invariant);
        }
        if database_now >= self.idle_expires_at || database_now >= self.absolute_expires_at {
            return Err(SessionValidationError::Expired);
        }
        if self.last_seen_at > database_now || self.idle_expires_at > self.absolute_expires_at {
            return Err(SessionValidationError::Invariant);
        }
        let principal_id = PrincipalId::parse(&self.principal_id)
            .map_err(|_| SessionValidationError::Invariant)?;
        let identity_revision = u64::try_from(self.identity_revision)
            .ok()
            .filter(|revision| *revision != 0)
            .ok_or(SessionValidationError::Invariant)?;
        if canonical_snowflake(&self.discord_user_id).is_none() {
            return Err(SessionValidationError::Invariant);
        }
        Ok(ActiveProductSessionV1 {
            principal_id,
            session_fingerprint,
            discord_user_id: self.discord_user_id.clone(),
            identity_revision,
            display_profile: self.display_profile.0.clone(),
            absolute_expires_at: self.absolute_expires_at,
        })
    }

    fn touch_due(&self, config: PostgresAuthenticationConfig, database_now: DateTime<Utc>) -> bool {
        database_now - self.last_seen_at >= config.touch_interval()
    }
}

#[derive(Clone)]
pub(crate) struct ActiveProductSessionV1 {
    pub principal_id: PrincipalId,
    pub session_fingerprint: ProductSessionDigestV1,
    pub discord_user_id: String,
    pub identity_revision: u64,
    pub display_profile: Value,
    pub absolute_expires_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionValidationError {
    InvalidCredential,
    InvalidCsrf,
    Expired,
    Revoked,
    Database(ProductDatabaseFailureV1),
    Invariant,
}

pub(crate) async fn load_active_product_session(
    pool: &PgPool,
    config: PostgresAuthenticationConfig,
    credential: &str,
    csrf: Option<&str>,
) -> Result<ActiveProductSessionV1, SessionValidationError> {
    let digest = digest_opaque_session_credential_v1(credential)
        .map_err(|_| SessionValidationError::InvalidCredential)?;
    let csrf_digest = csrf
        .map(digest_opaque_session_credential_v1)
        .transpose()
        .map_err(|_| SessionValidationError::InvalidCsrf)?;
    let mut transaction = pool.begin().await.map_err(database_session_error)?;
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, true)")
        .bind(config.statement_timeout())
        .execute(&mut *transaction)
        .await
        .map_err(database_session_error)?;
    let row = sqlx::query_as::<_, AuthenticationRow>(
        "SELECT authentication_session.principal_id, principal.discord_user_id, \
         principal.identity_revision, principal.display_profile, principal.disabled, \
         authentication_session.csrf_digest, authentication_session.oauth_state_digest, \
         authentication_session.last_seen_at, \
         authentication_session.idle_expires_at, authentication_session.absolute_expires_at, \
         authentication_session.revoked_at \
         FROM public.product_auth_sessions AS authentication_session \
         INNER JOIN public.product_principals AS principal \
         ON principal.principal_id = authentication_session.principal_id \
         WHERE authentication_session.session_digest = $1 \
         FOR SHARE OF authentication_session, principal",
    )
    .bind(digest.as_bytes().as_slice())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(database_session_error)?;
    let Some(row) = row else {
        transaction
            .rollback()
            .await
            .map_err(database_session_error)?;
        return Err(SessionValidationError::InvalidCredential);
    };
    let database_now = sqlx::query_scalar("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await;
    let database_now = match database_now {
        Ok(database_now) => database_now,
        Err(error) => {
            let failure = database_session_error(error);
            transaction
                .rollback()
                .await
                .map_err(database_session_error)?;
            return Err(failure);
        }
    };
    let active = match row.authenticate(
        digest.clone(),
        csrf_digest.as_ref().map(|digest| digest.as_bytes()),
        database_now,
    ) {
        Ok(active) => active,
        Err(error) => {
            transaction
                .rollback()
                .await
                .map_err(database_session_error)?;
            return Err(error);
        }
    };
    let touch_due = row.touch_due(config, database_now);
    transaction.commit().await.map_err(database_session_error)?;
    if touch_due {
        touch_active_product_session(pool, config, digest.as_bytes(), &row).await?;
    }
    Ok(active)
}

async fn touch_active_product_session(
    pool: &PgPool,
    config: PostgresAuthenticationConfig,
    session_digest: &[u8; 32],
    observed: &AuthenticationRow,
) -> Result<(), SessionValidationError> {
    let mut transaction = pool.begin().await.map_err(database_session_error)?;
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, true)")
        .bind(config.statement_timeout())
        .execute(&mut *transaction)
        .await
        .map_err(database_session_error)?;
    let idle_lifetime_seconds = config.idle_lifetime_seconds();
    let result = sqlx::query(
        "WITH locked_session AS MATERIALIZED ( \
           SELECT session_digest FROM public.product_auth_sessions \
           WHERE session_digest = $1 AND revoked_at IS NULL \
           AND last_seen_at = $2 AND idle_expires_at = $3 AND absolute_expires_at = $4 \
           FOR UPDATE \
         ), touch_clock AS MATERIALIZED ( \
           SELECT pg_catalog.clock_timestamp() AS touched_at FROM locked_session \
         ) \
         UPDATE public.product_auth_sessions AS authentication_session \
         SET last_seen_at = touch_clock.touched_at, \
          idle_expires_at = LEAST(authentication_session.absolute_expires_at, \
           touch_clock.touched_at + pg_catalog.make_interval(secs => $5::DOUBLE PRECISION)) \
         FROM locked_session, touch_clock \
         WHERE authentication_session.session_digest = locked_session.session_digest \
         AND touch_clock.touched_at < authentication_session.idle_expires_at \
         AND touch_clock.touched_at < authentication_session.absolute_expires_at",
    )
    .bind(session_digest.as_slice())
    .bind(observed.last_seen_at)
    .bind(observed.idle_expires_at)
    .bind(observed.absolute_expires_at)
    .bind(idle_lifetime_seconds)
    .execute(&mut *transaction)
    .await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let failure = database_session_error(error);
            transaction
                .rollback()
                .await
                .map_err(database_session_error)?;
            return Err(failure);
        }
    };
    if result.rows_affected() > 1 {
        transaction
            .rollback()
            .await
            .map_err(database_session_error)?;
        return Err(SessionValidationError::Invariant);
    }
    transaction.commit().await.map_err(database_session_error)?;
    Ok(())
}

impl AuthenticationPort for PostgresAuthentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        let active = load_active_product_session(&self.pool, self.config, credential, None)
            .await
            .map_err(map_session_error)?;
        let session_fingerprint = *active.session_fingerprint.as_bytes();
        Ok(AuthenticationClaimsV1::from_authentication(
            active.principal_id,
            AuthenticatedSessionFingerprintV1::from_sha256_digest(session_fingerprint),
        ))
    }
}

impl MutationAuthenticationPort for PostgresAuthentication {
    type CsrfProof = str;

    async fn authenticate_mutation(
        &self,
        credential: &Self::Credential,
        csrf: &Self::CsrfProof,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        let active = load_active_product_session(&self.pool, self.config, credential, Some(csrf))
            .await
            .map_err(map_mutation_session_error)?;
        let session_fingerprint = *active.session_fingerprint.as_bytes();
        Ok(AuthenticationClaimsV1::from_authentication(
            active.principal_id,
            AuthenticatedSessionFingerprintV1::from_sha256_digest(session_fingerprint),
        ))
    }
}

fn map_session_error(error: SessionValidationError) -> AuthenticationError {
    match error {
        SessionValidationError::InvalidCredential | SessionValidationError::InvalidCsrf => {
            AuthenticationError::InvalidCredential
        }
        SessionValidationError::Expired => AuthenticationError::Expired,
        SessionValidationError::Revoked => AuthenticationError::Revoked,
        SessionValidationError::Database(ProductDatabaseFailureV1::Timeout) => {
            AuthenticationBackendFailureV1::Timeout.into()
        }
        SessionValidationError::Database(ProductDatabaseFailureV1::Retryable) => {
            AuthenticationBackendFailureV1::Retryable.into()
        }
        SessionValidationError::Database(ProductDatabaseFailureV1::Unavailable)
        | SessionValidationError::Invariant => AuthenticationBackendFailureV1::Unavailable.into(),
    }
}

fn map_mutation_session_error(error: SessionValidationError) -> AuthenticationError {
    match error {
        SessionValidationError::InvalidCsrf => AuthenticationError::InvalidCsrf,
        other => map_session_error(other),
    }
}

fn database_session_error(error: sqlx::Error) -> SessionValidationError {
    SessionValidationError::Database(ProductDatabaseFailureV1::classify(&error))
}

fn canonical_snowflake(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed != 0 && parsed.to_string() == value).then_some(parsed)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn row() -> AuthenticationRow {
        AuthenticationRow {
            principal_id: "principal-1".to_string(),
            discord_user_id: "100".to_string(),
            identity_revision: 1,
            display_profile: Json(serde_json::json!({"display_name": "Principal"})),
            disabled: false,
            csrf_digest: vec![7_u8; 32],
            oauth_state_digest: Some(vec![9_u8; 32]),
            last_seen_at: Utc.with_ymd_and_hms(2026, 7, 19, 11, 50, 0).unwrap(),
            idle_expires_at: Utc.with_ymd_and_hms(2026, 7, 19, 12, 10, 0).unwrap(),
            absolute_expires_at: Utc.with_ymd_and_hms(2026, 7, 19, 20, 0, 0).unwrap(),
            revoked_at: None,
        }
    }

    fn database_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 19, 12, 0, 0).unwrap()
    }

    fn fingerprint() -> ProductSessionDigestV1 {
        ProductSessionDigestV1::from_digest_bytes([3_u8; 32])
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
                Duration::from_secs(MAX_IDLE_LIFETIME_SECONDS + 1),
                Duration::from_secs(5),
                Duration::from_secs(1)
            ),
            Err(AuthenticationConfigError::IdleLifetimeTooLong)
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
        let mut disabled = row();
        disabled.disabled = true;
        assert!(matches!(
            disabled.authenticate(fingerprint(), None, database_now()),
            Err(SessionValidationError::InvalidCredential)
        ));
        let mut revoked = row();
        revoked.revoked_at = Some(database_now());
        assert!(matches!(
            revoked.authenticate(fingerprint(), None, database_now()),
            Err(SessionValidationError::Revoked)
        ));
        assert!(matches!(
            revoked.authenticate(fingerprint(), Some(&[8_u8; 32]), database_now()),
            Err(SessionValidationError::InvalidCsrf)
        ));
        let mut expired = row();
        expired.idle_expires_at = database_now();
        assert!(matches!(
            expired.authenticate(fingerprint(), None, database_now()),
            Err(SessionValidationError::Expired)
        ));
    }

    #[test]
    fn active_row_uses_database_time_for_bounded_touch() {
        let row = row();
        let config = PostgresAuthenticationConfig::default();
        assert_eq!(
            row.authenticate(fingerprint(), None, database_now())
                .unwrap()
                .principal_id
                .as_str(),
            "principal-1"
        );
        assert!(row.touch_due(config, database_now()));
    }

    #[test]
    fn csrf_digest_comparison_is_exact() {
        let row = row();
        assert!(row
            .authenticate(fingerprint(), Some(&[7_u8; 32]), database_now())
            .is_ok());
        assert!(matches!(
            row.authenticate(fingerprint(), Some(&[8_u8; 32]), database_now()),
            Err(SessionValidationError::InvalidCsrf)
        ));
    }

    #[test]
    fn csrf_failure_is_only_exposed_by_mutation_authentication() {
        assert_eq!(
            map_session_error(SessionValidationError::InvalidCsrf),
            AuthenticationError::InvalidCredential
        );
        assert_eq!(
            map_mutation_session_error(SessionValidationError::InvalidCsrf),
            AuthenticationError::InvalidCsrf
        );
    }
}
