use std::num::NonZeroU64;
use std::time::Duration;

use authoring_application::{AuthenticatedActorV1, InstallationSelectorV1};
use authoring_application_discord::{
    DiscordApplicationIdV1, DiscordAuthoritySourceError, InstallationAuthorityRecordV1,
    InstallationAuthoritySource,
};
use authoring_promotion::{AutomationInstallationId, TenantId};
use chrono::{DateTime, Utc};
use discord_model::{GuildId, UserId};
use sqlx::postgres::PgPool;
use subtle::ConstantTimeEq;

const DEFAULT_STATEMENT_TIMEOUT_MILLIS: u64 = 2_000;
const MAX_STATEMENT_TIMEOUT_MILLIS: u64 = 60_000;
const AUTHORITY_DIGEST_LENGTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InstallationAuthoritySourceConfigError {
    #[error("installation authority statement timeout is outside the supported range")]
    InvalidStatementTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostgresInstallationAuthoritySourceConfig {
    statement_timeout: Duration,
}

impl PostgresInstallationAuthoritySourceConfig {
    pub fn new(
        statement_timeout: Duration,
    ) -> Result<Self, InstallationAuthoritySourceConfigError> {
        let statement_timeout_millis = statement_timeout.as_millis();
        if statement_timeout_millis == 0
            || statement_timeout > Duration::from_millis(MAX_STATEMENT_TIMEOUT_MILLIS)
            || !statement_timeout.subsec_nanos().is_multiple_of(1_000_000)
        {
            return Err(InstallationAuthoritySourceConfigError::InvalidStatementTimeout);
        }
        Ok(Self { statement_timeout })
    }

    fn statement_timeout(self) -> String {
        format!("{}ms", self.statement_timeout.as_millis())
    }
}

impl Default for PostgresInstallationAuthoritySourceConfig {
    fn default() -> Self {
        Self::new(Duration::from_millis(DEFAULT_STATEMENT_TIMEOUT_MILLIS))
            .expect("default installation authority source configuration is valid")
    }
}

#[derive(Clone)]
pub struct PostgresInstallationAuthoritySource {
    pool: PgPool,
    config: PostgresInstallationAuthoritySourceConfig,
}

impl PostgresInstallationAuthoritySource {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            config: PostgresInstallationAuthoritySourceConfig::default(),
        }
    }

    pub fn with_config(pool: PgPool, config: PostgresInstallationAuthoritySourceConfig) -> Self {
        Self { pool, config }
    }
}

#[derive(sqlx::FromRow)]
struct InstallationAuthorityRow {
    principal_id: String,
    acting_user_id: String,
    principal_disabled: bool,
    session_digest: Vec<u8>,
    session_principal_id: String,
    oauth_state_digest: Option<Vec<u8>>,
    last_seen_at: DateTime<Utc>,
    idle_expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    installation_tenant_id: String,
    installation_id: String,
    tenant_id: Option<String>,
    tenant_lifecycle_state: Option<String>,
    installation_lifecycle_state: String,
    discord_application_id: String,
    discord_guild_id: String,
    current_authority_revision: i64,
    authority_tenant_id: Option<String>,
    authority_installation_id: Option<String>,
    authority_revision: Option<i64>,
    authority_payload_digest: Option<String>,
    database_now: DateTime<Utc>,
}

impl InstallationAuthorityRow {
    fn project(
        self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
    ) -> Result<InstallationAuthorityRecordV1, DiscordAuthoritySourceError> {
        let persisted_session_digest: [u8; 32] = self
            .session_digest
            .as_slice()
            .try_into()
            .map_err(|_| DiscordAuthoritySourceError::InvalidRecord)?;
        if persisted_session_digest
            .ct_eq(actor.session_fingerprint().as_bytes())
            .unwrap_u8()
            != 1
            || self.principal_id != actor.principal_id().as_str()
            || self.session_principal_id != actor.principal_id().as_str()
            || self.session_principal_id != self.principal_id
            || self.installation_id != installation.installation_id().as_str()
        {
            return Err(DiscordAuthoritySourceError::InvalidRecord);
        }
        if self.oauth_state_digest.as_deref().map(<[u8]>::len) != Some(32) {
            return match self.oauth_state_digest {
                None => Err(DiscordAuthoritySourceError::NotFound),
                Some(_) => Err(DiscordAuthoritySourceError::InvalidRecord),
            };
        }
        if self.last_seen_at > self.database_now
            || self.last_seen_at >= self.idle_expires_at
            || self.idle_expires_at > self.absolute_expires_at
            || self
                .revoked_at
                .is_some_and(|revoked_at| revoked_at < self.last_seen_at)
        {
            return Err(DiscordAuthoritySourceError::InvalidRecord);
        }
        if self.principal_disabled
            || self.revoked_at.is_some()
            || self.database_now >= self.idle_expires_at
            || self.database_now >= self.absolute_expires_at
        {
            return Err(DiscordAuthoritySourceError::NotFound);
        }
        let tenant_id = TenantId::parse(&self.installation_tenant_id)
            .map_err(|_| DiscordAuthoritySourceError::InvalidRecord)?;
        let installation_id = AutomationInstallationId::parse(&self.installation_id)
            .map_err(|_| DiscordAuthoritySourceError::InvalidRecord)?;
        let persisted_tenant_id = self
            .tenant_id
            .as_deref()
            .ok_or(DiscordAuthoritySourceError::InvalidRecord)?;
        if persisted_tenant_id != tenant_id.as_str() {
            return Err(DiscordAuthoritySourceError::InvalidRecord);
        }
        if !active_lifecycle(self.tenant_lifecycle_state.as_deref())?
            || !active_lifecycle(Some(&self.installation_lifecycle_state))?
        {
            return Err(DiscordAuthoritySourceError::NotFound);
        }
        let application_id = DiscordApplicationIdV1::new(
            canonical_snowflake(&self.discord_application_id)
                .ok_or(DiscordAuthoritySourceError::InvalidRecord)?,
        )
        .map_err(|_| DiscordAuthoritySourceError::InvalidRecord)?;
        let guild_id = GuildId(
            canonical_snowflake(&self.discord_guild_id)
                .ok_or(DiscordAuthoritySourceError::InvalidRecord)?,
        );
        let acting_user_id = UserId(
            canonical_snowflake(&self.acting_user_id)
                .ok_or(DiscordAuthoritySourceError::InvalidRecord)?,
        );
        let current_authority_revision = positive_revision(self.current_authority_revision)?;
        let authority_tenant_id = self
            .authority_tenant_id
            .ok_or(DiscordAuthoritySourceError::InvalidRecord)?;
        let authority_installation_id = self
            .authority_installation_id
            .ok_or(DiscordAuthoritySourceError::InvalidRecord)?;
        let authority_revision = positive_revision(
            self.authority_revision
                .ok_or(DiscordAuthoritySourceError::InvalidRecord)?,
        )?;
        let authority_digest = self
            .authority_payload_digest
            .filter(|digest| canonical_authority_digest(digest))
            .ok_or(DiscordAuthoritySourceError::InvalidRecord)?;
        if authority_tenant_id != tenant_id.as_str()
            || authority_installation_id != installation_id.as_str()
            || authority_revision != current_authority_revision
        {
            return Err(DiscordAuthoritySourceError::InvalidRecord);
        }
        Ok(InstallationAuthorityRecordV1 {
            tenant_id,
            installation_id,
            application_id,
            guild_id,
            acting_user_id,
            authority_revision,
            authority_digest,
        })
    }
}

impl InstallationAuthoritySource for PostgresInstallationAuthoritySource {
    async fn load_for_actor(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
    ) -> Result<InstallationAuthorityRecordV1, DiscordAuthoritySourceError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| DiscordAuthoritySourceError::Unavailable)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(|_| DiscordAuthoritySourceError::Unavailable)?;
        let timeout = self.config.statement_timeout();
        sqlx::query(
            "SELECT pg_catalog.set_config('statement_timeout', $1, true), \
             pg_catalog.set_config('idle_in_transaction_session_timeout', $1, true)",
        )
        .bind(timeout)
        .execute(&mut *transaction)
        .await
        .map_err(|_| DiscordAuthoritySourceError::Unavailable)?;
        let row = sqlx::query_as::<_, InstallationAuthorityRow>(
            "WITH request_clock AS MATERIALIZED ( \
               SELECT pg_catalog.clock_timestamp() AS database_now \
             ) \
             SELECT principal.principal_id, principal.discord_user_id AS acting_user_id, \
              principal.disabled AS principal_disabled, actor_session.session_digest, \
              actor_session.principal_id AS session_principal_id, \
              actor_session.oauth_state_digest, actor_session.last_seen_at, \
              actor_session.idle_expires_at, actor_session.absolute_expires_at, \
              actor_session.revoked_at, \
              installation.tenant_id AS installation_tenant_id, \
              installation.installation_id, tenant.tenant_id, \
              tenant.lifecycle_state AS tenant_lifecycle_state, \
              installation.lifecycle_state AS installation_lifecycle_state, \
              installation.discord_application_id, \
              installation.discord_guild_id, installation.current_authority_revision, \
              authority.tenant_id AS authority_tenant_id, \
              authority.installation_id AS authority_installation_id, \
              authority.revision AS authority_revision, \
              authority.authority_payload_digest, request_clock.database_now \
             FROM public.automation_installations AS installation \
             LEFT JOIN public.product_tenants AS tenant \
              ON tenant.tenant_id = installation.tenant_id \
             INNER JOIN public.product_principals AS principal \
              ON principal.principal_id = $2 \
             INNER JOIN public.product_auth_sessions AS actor_session \
              ON actor_session.principal_id = principal.principal_id \
              AND actor_session.session_digest = $3 \
             LEFT JOIN public.automation_installation_authority_versions AS authority \
              ON authority.tenant_id = installation.tenant_id \
              AND authority.installation_id = installation.installation_id \
              AND authority.revision = installation.current_authority_revision \
             CROSS JOIN request_clock \
             WHERE installation.installation_id = $1",
        )
        .bind(installation.installation_id().as_str())
        .bind(actor.principal_id().as_str())
        .bind(actor.session_fingerprint().as_bytes().as_slice())
        .fetch_optional(&mut *transaction)
        .await;
        let row = match row {
            Ok(row) => row,
            Err(error) => {
                let source_error = query_error(&error);
                transaction
                    .rollback()
                    .await
                    .map_err(|_| DiscordAuthoritySourceError::Unavailable)?;
                return Err(source_error);
            }
        };
        let Some(row) = row else {
            transaction
                .rollback()
                .await
                .map_err(|_| DiscordAuthoritySourceError::Unavailable)?;
            return Err(DiscordAuthoritySourceError::NotFound);
        };
        let record = match row.project(actor, installation) {
            Ok(record) => record,
            Err(error) => {
                transaction
                    .rollback()
                    .await
                    .map_err(|_| DiscordAuthoritySourceError::Unavailable)?;
                return Err(error);
            }
        };
        transaction
            .commit()
            .await
            .map_err(|_| DiscordAuthoritySourceError::Unavailable)?;
        Ok(record)
    }
}

fn positive_revision(value: i64) -> Result<NonZeroU64, DiscordAuthoritySourceError> {
    u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or(DiscordAuthoritySourceError::InvalidRecord)
}

fn active_lifecycle(value: Option<&str>) -> Result<bool, DiscordAuthoritySourceError> {
    match value {
        Some("active") => Ok(true),
        Some("provisioning" | "suspended" | "disabled") => Ok(false),
        Some(_) | None => Err(DiscordAuthoritySourceError::InvalidRecord),
    }
}

fn query_error(error: &sqlx::Error) -> DiscordAuthoritySourceError {
    match error {
        sqlx::Error::ColumnDecode { .. } | sqlx::Error::Decode(_) => {
            DiscordAuthoritySourceError::InvalidRecord
        }
        _ => DiscordAuthoritySourceError::Unavailable,
    }
}

fn canonical_snowflake(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed != 0 && parsed.to_string() == value).then_some(parsed)
}

fn canonical_authority_digest(value: &str) -> bool {
    value.len() == AUTHORITY_DIGEST_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn configuration_rejects_zero_submillisecond_and_unbounded_timeouts() {
        assert_eq!(
            PostgresInstallationAuthoritySourceConfig::new(Duration::ZERO),
            Err(InstallationAuthoritySourceConfigError::InvalidStatementTimeout)
        );
        assert_eq!(
            PostgresInstallationAuthoritySourceConfig::new(Duration::from_nanos(1)),
            Err(InstallationAuthoritySourceConfigError::InvalidStatementTimeout)
        );
        assert_eq!(
            PostgresInstallationAuthoritySourceConfig::new(Duration::from_millis(
                MAX_STATEMENT_TIMEOUT_MILLIS + 1
            )),
            Err(InstallationAuthoritySourceConfigError::InvalidStatementTimeout)
        );
        assert_eq!(
            PostgresInstallationAuthoritySourceConfig::new(
                Duration::from_millis(1) + Duration::from_nanos(1)
            ),
            Err(InstallationAuthoritySourceConfigError::InvalidStatementTimeout)
        );
        assert_eq!(
            PostgresInstallationAuthoritySourceConfig::new(
                Duration::from_secs(60) + Duration::from_nanos(1)
            ),
            Err(InstallationAuthoritySourceConfigError::InvalidStatementTimeout)
        );
        assert!(PostgresInstallationAuthoritySourceConfig::new(Duration::from_secs(60)).is_ok());
    }

    #[test]
    fn canonical_external_identifiers_are_strict() {
        assert_eq!(canonical_snowflake("1"), Some(1));
        assert_eq!(canonical_snowflake(&u64::MAX.to_string()), Some(u64::MAX));
        for invalid in ["", "0", "01", "+1", " 1", "18446744073709551616"] {
            assert_eq!(canonical_snowflake(invalid), None);
        }
        assert!(canonical_authority_digest(&"a".repeat(64)));
        for invalid in ["a".repeat(63), "A".repeat(64), "g".repeat(64)] {
            assert!(!canonical_authority_digest(&invalid));
        }
    }

    #[test]
    fn query_failures_are_classified_without_exposing_database_details() {
        let invalid = query_error(&sqlx::Error::Decode(Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            "sensitive row",
        ))));
        assert_eq!(invalid, DiscordAuthoritySourceError::InvalidRecord);
        assert!(!invalid.to_string().contains("sensitive row"));
        assert_eq!(
            query_error(&sqlx::Error::PoolClosed),
            DiscordAuthoritySourceError::Unavailable
        );
    }

    #[test]
    fn lifecycle_classification_distinguishes_inactive_from_corrupt() {
        assert_eq!(active_lifecycle(Some("active")), Ok(true));
        for inactive in ["provisioning", "suspended", "disabled"] {
            assert_eq!(active_lifecycle(Some(inactive)), Ok(false));
        }
        assert_eq!(
            active_lifecycle(Some("unknown")),
            Err(DiscordAuthoritySourceError::InvalidRecord)
        );
        assert_eq!(
            active_lifecycle(None),
            Err(DiscordAuthoritySourceError::InvalidRecord)
        );
    }
}
