mod readiness;

use std::time::Duration;

use authoring_application::{
    AuthenticatedSessionFingerprintV1, AuthenticationBackendFailureV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, MutationAuthenticationPort,
};
use authoring_promotion::PrincipalId;
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPool;
use sqlx::types::Json;
use subtle::ConstantTimeEq;

use crate::digest::digest_opaque_session_credential_v1;
use crate::{ProductDatabaseFailureV1, ProductSessionDigestV1};

pub use readiness::AuthenticationReadinessErrorV1;

const DEFAULT_IDLE_LIFETIME_SECONDS: u64 = 30 * 60;
const DEFAULT_TOUCH_INTERVAL_SECONDS: u64 = 5 * 60;
const DEFAULT_STATEMENT_TIMEOUT_MILLIS: u64 = 2_000;
const MAX_IDLE_LIFETIME_SECONDS: u64 = 30 * 60;
const MAX_STATEMENT_TIMEOUT_MILLIS: u64 = 60_000;
const MIN_TOUCH_INTERVAL_SECONDS: u64 = 1;
const SESSION_READ_QUERY: &str = "SELECT * FROM public.starring_product_session_read_v1($1)";
const MUTATION_SESSION_READ_QUERY: &str =
    "SELECT * FROM public.starring_product_session_mutation_read_v1($1)";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationConfigError {
    #[error("product authentication idle lifetime must be positive")]
    IdleLifetimeZero,
    #[error("product authentication idle lifetime exceeds the supported maximum")]
    IdleLifetimeTooLong,
    #[error(
        "product authentication touch interval must be at least one second and shorter than the idle lifetime"
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
        if touch_interval < Duration::from_secs(MIN_TOUCH_INTERVAL_SECONDS)
            || touch_interval >= idle_lifetime
        {
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

    fn idle_lifetime(self) -> TimeDelta {
        TimeDelta::from_std(self.idle_lifetime)
            .expect("validated product authentication idle lifetime remains in chrono range")
    }

    fn touch_interval_seconds(self) -> f64 {
        self.touch_interval.as_secs_f64()
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
    principal_disabled: bool,
    csrf_digest_length: i32,
    oauth_state_digest_length: Option<i32>,
    csrf_comparison_tag: Option<Vec<u8>>,
    last_seen_at: DateTime<Utc>,
    idle_expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl AuthenticationRow {
    fn authenticate(
        &self,
        session_fingerprint: ProductSessionDigestV1,
        proof: SessionProofModeV1<'_>,
        database_now: DateTime<Utc>,
    ) -> Result<ActiveProductSessionV1, SessionValidationError> {
        if self.csrf_digest_length != 32 {
            return Err(SessionValidationError::Invariant);
        }
        match proof {
            SessionProofModeV1::SessionOnly => {
                if self.csrf_comparison_tag.is_some() {
                    return Err(SessionValidationError::Invariant);
                }
            }
            SessionProofModeV1::Mutation(expected_csrf_digest) => {
                let persisted_tag: [u8; 32] = self
                    .csrf_comparison_tag
                    .as_deref()
                    .ok_or(SessionValidationError::Invariant)?
                    .try_into()
                    .map_err(|_| SessionValidationError::Invariant)?;
                let expected_tag = csrf_comparison_tag(
                    session_fingerprint.as_bytes(),
                    expected_csrf_digest.as_bytes(),
                );
                if persisted_tag.ct_eq(&expected_tag).unwrap_u8() != 1 {
                    return Err(SessionValidationError::InvalidCsrf);
                }
            }
        }
        if self.principal_disabled {
            return Err(SessionValidationError::InvalidCredential);
        }
        if self.revoked_at.is_some() {
            return Err(SessionValidationError::Revoked);
        }
        if self.oauth_state_digest_length != Some(32) {
            return Err(SessionValidationError::Invariant);
        }
        if database_now >= self.idle_expires_at || database_now >= self.absolute_expires_at {
            return Err(SessionValidationError::Expired);
        }
        if self.last_seen_at > database_now
            || self.idle_expires_at > self.absolute_expires_at
            || self.idle_expires_at - self.last_seen_at > TimeDelta::minutes(30)
        {
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
        self.idle_expires_at - self.last_seen_at <= config.idle_lifetime()
            && database_now - self.last_seen_at >= config.touch_interval()
    }
}

#[derive(Clone, Copy)]
enum SessionProofModeV1<'a> {
    SessionOnly,
    Mutation(&'a ProductSessionDigestV1),
}

fn csrf_comparison_tag(session_digest: &[u8; 32], csrf_digest: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(session_digest);
    digest.update(csrf_digest);
    digest.finalize().into()
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
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, true), \
         pg_catalog.set_config('lock_timeout', $1, true), \
         pg_catalog.set_config('idle_in_transaction_session_timeout', $1, true)",
    )
    .bind(config.statement_timeout())
    .execute(&mut *transaction)
    .await
    .map_err(database_session_error)?;
    let query = if csrf_digest.is_some() {
        MUTATION_SESSION_READ_QUERY
    } else {
        SESSION_READ_QUERY
    };
    let row = sqlx::query_as::<_, AuthenticationRow>(query)
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
        match csrf_digest.as_ref() {
            Some(digest) => SessionProofModeV1::Mutation(digest),
            None => SessionProofModeV1::SessionOnly,
        },
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
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, true), \
         pg_catalog.set_config('lock_timeout', $1, true), \
         pg_catalog.set_config('idle_in_transaction_session_timeout', $1, true)",
    )
    .bind(config.statement_timeout())
    .execute(&mut *transaction)
    .await
    .map_err(database_session_error)?;
    let touch_interval_seconds = config.touch_interval_seconds();
    let result = sqlx::query_scalar::<_, i64>(
        "SELECT public.starring_product_session_touch_v1($1, $2, $3, $4, $5)",
    )
    .bind(session_digest.as_slice())
    .bind(observed.last_seen_at)
    .bind(observed.idle_expires_at)
    .bind(observed.absolute_expires_at)
    .bind(touch_interval_seconds)
    .fetch_one(&mut *transaction)
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
    if !(0..=1).contains(&result) {
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
            principal_disabled: false,
            csrf_digest_length: 32,
            oauth_state_digest_length: Some(32),
            csrf_comparison_tag: None,
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

    fn csrf_fingerprint(value: u8) -> ProductSessionDigestV1 {
        ProductSessionDigestV1::from_digest_bytes([value; 32])
    }

    fn mutation_row(csrf_digest: &ProductSessionDigestV1) -> AuthenticationRow {
        let mut row = row();
        row.csrf_comparison_tag =
            Some(csrf_comparison_tag(fingerprint().as_bytes(), csrf_digest.as_bytes()).to_vec());
        row
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
                Duration::from_millis(999),
                Duration::from_secs(1)
            ),
            Err(AuthenticationConfigError::InvalidTouchInterval)
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
        disabled.principal_disabled = true;
        assert!(matches!(
            disabled.authenticate(
                fingerprint(),
                SessionProofModeV1::SessionOnly,
                database_now()
            ),
            Err(SessionValidationError::InvalidCredential)
        ));
        let mut revoked = row();
        revoked.revoked_at = Some(database_now());
        assert!(matches!(
            revoked.authenticate(
                fingerprint(),
                SessionProofModeV1::SessionOnly,
                database_now()
            ),
            Err(SessionValidationError::Revoked)
        ));
        let persisted_csrf = csrf_fingerprint(7);
        let wrong_csrf = csrf_fingerprint(8);
        let mut revoked = mutation_row(&persisted_csrf);
        revoked.revoked_at = Some(database_now());
        assert!(matches!(
            revoked.authenticate(
                fingerprint(),
                SessionProofModeV1::Mutation(&wrong_csrf),
                database_now()
            ),
            Err(SessionValidationError::InvalidCsrf)
        ));
        let mut expired = row();
        expired.idle_expires_at = database_now();
        assert!(matches!(
            expired.authenticate(
                fingerprint(),
                SessionProofModeV1::SessionOnly,
                database_now()
            ),
            Err(SessionValidationError::Expired)
        ));
        let mut unbounded = row();
        unbounded.idle_expires_at = unbounded.last_seen_at + TimeDelta::minutes(31);
        assert!(matches!(
            unbounded.authenticate(
                fingerprint(),
                SessionProofModeV1::SessionOnly,
                database_now()
            ),
            Err(SessionValidationError::Invariant)
        ));
    }

    #[test]
    fn active_row_uses_database_time_for_bounded_touch() {
        let row = row();
        let config = PostgresAuthenticationConfig::default();
        assert_eq!(
            row.authenticate(
                fingerprint(),
                SessionProofModeV1::SessionOnly,
                database_now()
            )
            .unwrap()
            .principal_id
            .as_str(),
            "principal-1"
        );
        assert!(row.touch_due(config, database_now()));
        let tightened = PostgresAuthenticationConfig::new(
            Duration::from_secs(10 * 60),
            Duration::from_secs(5 * 60),
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(!row.touch_due(tightened, database_now()));
    }

    #[test]
    fn csrf_digest_comparison_is_exact() {
        let persisted_csrf = csrf_fingerprint(7);
        let matching_csrf = csrf_fingerprint(7);
        let wrong_csrf = csrf_fingerprint(8);
        let row = mutation_row(&persisted_csrf);
        assert!(row
            .authenticate(
                fingerprint(),
                SessionProofModeV1::Mutation(&matching_csrf),
                database_now()
            )
            .is_ok());
        assert!(matches!(
            row.authenticate(
                fingerprint(),
                SessionProofModeV1::Mutation(&wrong_csrf),
                database_now()
            ),
            Err(SessionValidationError::InvalidCsrf)
        ));
        assert!(matches!(
            row.authenticate(
                fingerprint(),
                SessionProofModeV1::SessionOnly,
                database_now()
            ),
            Err(SessionValidationError::Invariant)
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
