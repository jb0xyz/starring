use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};
use std::str::FromStr;

use desired_state::ResourceKey;
use discord_model::ChannelId;
use resource_resolution::{
    installation_authority_payload_digest_v1, installation_authority_request_digest_v1,
    resource_binding_fingerprint_v2, InstallationAuthorityPayloadDigestV1,
    InstallationAuthorityPayloadIdentityV1, InstallationAuthorityPolicyV1,
    InstallationAuthorityRequestDigestV1, InstallationAuthorityRequestIdentityV1,
    InstallationAuthorityScopeV1, ResourceBindingFingerprint, ResourceBindingMap,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Connection, FromRow, Row};

use crate::command::binding_key;
use crate::keychain::read_admin_database_url;
use crate::{AuthorityAdvanceCommandV1, AuthorityOperatorErrorV1};

const DATABASE_NAME: &str = "starring_runtime_staging";
const ADMIN_DATABASE_NAME: &str = "postgres";
const DATABASE_HOST: &str = "127.0.0.1";
const DATABASE_PORT: u16 = 5432;
const CLUSTER_ADMIN_ROLE: &str = "starring_cluster_admin";
const EXPECTED_SOURCE_REVISION: i64 = 1;
const EXPECTED_SUCCESSOR_REVISION: i64 = 2;
const MAX_SERIALIZATION_ATTEMPTS: usize = 2;

const CLUSTER_PREFLIGHT_SQL: &str = r#"
SELECT
    pg_catalog.current_database() = 'starring_runtime_staging',
    current_user = session_user
        AND current_user = 'starring_cluster_admin',
    control.system_identifier::TEXT = $1,
    pg_catalog.inet_client_addr() = '127.0.0.1'::PG_CATALOG.INET
        AND pg_catalog.inet_server_addr() = '127.0.0.1'::PG_CATALOG.INET
        AND pg_catalog.inet_server_port() = 5432,
    NOT COALESCE((
        SELECT ssl
        FROM pg_catalog.pg_stat_ssl
        WHERE pid = pg_catalog.pg_backend_pid()
    ), TRUE),
    pg_catalog.current_setting('server_version_num')::INTEGER
        BETWEEN 160000 AND 169999,
    administrator.rolcanlogin
        AND administrator.rolsuper
        AND NOT administrator.rolcreatedb
        AND NOT administrator.rolcreaterole
        AND NOT administrator.rolinherit
        AND NOT administrator.rolreplication
        AND NOT administrator.rolbypassrls
        AND administrator.rolconnlimit = 2
        AND administrator.rolvaliduntil =
            'infinity'::PG_CATALOG.TIMESTAMPTZ,
    NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_auth_members AS membership
        WHERE membership.roleid = administrator.oid
            OR membership.member = administrator.oid
    )
        AND NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole = administrator.oid
        ),
    owner.oid IS NOT NULL
        AND NOT owner.rolcanlogin
        AND NOT owner.rolsuper
        AND NOT owner.rolcreatedb
        AND NOT owner.rolcreaterole
        AND NOT owner.rolinherit
        AND NOT owner.rolreplication
        AND NOT owner.rolbypassrls
        AND owner.rolconnlimit = 0
        AND owner.rolpassword IS NULL
        AND NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_auth_members AS membership
            WHERE membership.roleid = owner.oid
                OR membership.member = owner.oid
        )
        AND NOT EXISTS (
            SELECT 1
            FROM pg_catalog.pg_db_role_setting AS setting
            WHERE setting.setrole = owner.oid
        ),
    pg_catalog.to_regclass(
        'public.automation_installations'
    ) IS NOT NULL
        AND pg_catalog.to_regclass(
            'public.automation_installation_authority_versions'
        ) IS NOT NULL
        AND EXISTS (
            SELECT 1
            FROM public._sqlx_migrations AS migration
            WHERE migration.version = 202607300001
                AND migration.success
        ),
    (
        SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_trigger AS trigger
        WHERE trigger.tgrelid =
                'public.automation_installation_authority_versions'
                    ::PG_CATALOG.REGCLASS
            AND NOT trigger.tgisinternal
            AND trigger.tgenabled = 'O'
            AND trigger.tgname IN (
                'installation_authority_enforce_sequence',
                'installation_authority_assert_head',
                'installation_authority_reject_mutation'
            )
    ) = 3
FROM pg_catalog.pg_control_system() AS control
INNER JOIN pg_catalog.pg_authid AS administrator
    ON administrator.rolname = current_user
LEFT JOIN pg_catalog.pg_authid AS owner
    ON owner.rolname = 'starring_owner'
"#;

const INSTALLATION_FOR_UPDATE_SQL: &str = r#"
SELECT
    installation.installation_id,
    installation.tenant_id,
    installation.discord_application_id,
    installation.discord_guild_id,
    installation.ruleset_key,
    installation.lifecycle_state,
    installation.current_authority_revision,
    tenant.lifecycle_state AS tenant_lifecycle_state
FROM public.automation_installations AS installation
INNER JOIN public.product_tenants AS tenant
    ON tenant.tenant_id = installation.tenant_id
WHERE installation.installation_id = $1
FOR UPDATE OF installation
"#;

const INSTALLATION_READ_SQL: &str = r#"
SELECT
    installation.installation_id,
    installation.tenant_id,
    installation.discord_application_id,
    installation.discord_guild_id,
    installation.ruleset_key,
    installation.lifecycle_state,
    installation.current_authority_revision,
    tenant.lifecycle_state AS tenant_lifecycle_state
FROM public.automation_installations AS installation
INNER JOIN public.product_tenants AS tenant
    ON tenant.tenant_id = installation.tenant_id
WHERE installation.installation_id = $1
"#;

const AUTHORITIES_SQL: &str = r#"
SELECT
    authority.installation_id,
    authority.revision,
    authority.tenant_id,
    authority.binding_revision,
    authority.resource_bindings,
    authority.binding_fingerprint,
    authority.policy_revision,
    authority.required_approvals,
    authority.activation_ttl_seconds,
    authority.authority_payload_digest,
    authority.created_by_principal_id,
    authority.created_by_request_digest,
    authority.created_at::PG_CATALOG.TEXT AS created_at
FROM public.automation_installation_authority_versions AS authority
WHERE authority.installation_id = $1
ORDER BY authority.revision
"#;

const INSERT_SUCCESSOR_SQL: &str = r#"
INSERT INTO public.automation_installation_authority_versions (
    installation_id,
    revision,
    tenant_id,
    binding_revision,
    resource_bindings,
    binding_fingerprint,
    policy_revision,
    required_approvals,
    activation_ttl_seconds,
    authority_payload_digest,
    created_by_principal_id,
    created_by_request_digest
) VALUES (
    $1,
    2,
    $2,
    2,
    $3,
    $4,
    $5,
    $6,
    $7,
    $8,
    $9,
    $10
)
"#;

const ADVANCE_HEAD_SQL: &str = r#"
UPDATE public.automation_installations
SET
    current_authority_revision = 2,
    updated_at = pg_catalog.clock_timestamp()
WHERE tenant_id = $1
    AND installation_id = $2
    AND current_authority_revision = 1
    AND lifecycle_state = 'active'
"#;

const PRISTINE_PRODUCT_STATE_SQL: &str = r#"
SELECT
    NOT EXISTS (
        SELECT 1
        FROM public.authoring_sessions AS session
        WHERE session.tenant_id = $1 AND session.installation_id = $2
    )
    AND NOT EXISTS (
        SELECT 1
        FROM public.authoring_promotions AS promotion
        WHERE promotion.tenant_id = $1
    )
    AND NOT EXISTS (
        SELECT 1
        FROM public.runtime_deployments AS deployment
        WHERE deployment.tenant_id = $1 AND deployment.installation_id = $2
    )
    AND NOT EXISTS (
        SELECT 1
        FROM public.activation_requests AS activation
        WHERE activation.guild_id = $3 AND activation.ruleset_key = $4
    )
    AND NOT EXISTS (
        SELECT 1
        FROM public.automation_ruleset_versions AS artifact
        WHERE artifact.guild_id = $3 AND artifact.ruleset_key = $4
    )
    AND NOT EXISTS (
        SELECT 1
        FROM public.automation_ruleset_activations AS active
        WHERE active.guild_id = $3 AND active.ruleset_key = $4
    )
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorityAdvanceOutcomeV1 {
    Advanced,
    ExactReplay,
    RecoveredCommitted,
}

impl AuthorityAdvanceOutcomeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Advanced => "advanced",
            Self::ExactReplay => "exact_replay",
            Self::RecoveredCommitted => "recovered_committed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityAdvanceReportV1 {
    outcome: AuthorityAdvanceOutcomeV1,
    installation_id: String,
    authority_revision: u64,
    channel_id: u64,
}

impl AuthorityAdvanceReportV1 {
    pub const fn outcome(&self) -> AuthorityAdvanceOutcomeV1 {
        self.outcome
    }

    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    pub const fn authority_revision(&self) -> u64 {
        self.authority_revision
    }

    pub const fn binding_key(&self) -> &'static str {
        binding_key()
    }

    pub const fn channel_id(&self) -> u64 {
        self.channel_id
    }
}

#[derive(Clone, PartialEq, Eq, FromRow)]
struct InstallationRowV1 {
    installation_id: String,
    tenant_id: String,
    discord_application_id: String,
    discord_guild_id: String,
    ruleset_key: String,
    lifecycle_state: String,
    current_authority_revision: i64,
    tenant_lifecycle_state: String,
}

#[derive(Clone, PartialEq, Eq, FromRow)]
struct AuthorityRowV1 {
    installation_id: String,
    revision: i64,
    tenant_id: String,
    binding_revision: i64,
    resource_bindings: Value,
    binding_fingerprint: String,
    policy_revision: i64,
    required_approvals: i32,
    activation_ttl_seconds: i64,
    authority_payload_digest: String,
    created_by_principal_id: String,
    created_by_request_digest: String,
    created_at: String,
}

#[derive(Clone)]
struct ExpectedSuccessorV1 {
    tenant_id: String,
    installation_id: String,
    resource_bindings: Value,
    binding_fingerprint: ResourceBindingFingerprint,
    policy_revision: i64,
    required_approvals: i32,
    activation_ttl_seconds: i64,
    authority_payload_digest: InstallationAuthorityPayloadDigestV1,
    created_by_principal_id: String,
    created_by_request_digest: InstallationAuthorityRequestDigestV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredResourceBindingsV1 {
    #[serde(default)]
    role_bindings: BTreeMap<String, String>,
    #[serde(default)]
    channel_bindings: BTreeMap<String, String>,
}

enum AttemptFailureV1 {
    Retryable,
    Terminal(AuthorityOperatorErrorV1),
}

struct AttemptResultV1 {
    outcome: AuthorityAdvanceOutcomeV1,
    revision_one: AuthorityRowV1,
    commit_indeterminate: bool,
}

pub async fn advance_authority(
    command: &AuthorityAdvanceCommandV1,
) -> Result<AuthorityAdvanceReportV1, AuthorityOperatorErrorV1> {
    let mut terminal_result = None;
    for attempt in 0..MAX_SERIALIZATION_ATTEMPTS {
        let mut connection = connect_target(command, "starring-authority-advance").await?;
        match advance_once(&mut connection, command).await {
            Ok(result) => {
                terminal_result = Some(result);
                break;
            }
            Err(AttemptFailureV1::Retryable) if attempt + 1 < MAX_SERIALIZATION_ATTEMPTS => {}
            Err(AttemptFailureV1::Retryable) => return Err(AuthorityOperatorErrorV1::DatabaseBusy),
            Err(AttemptFailureV1::Terminal(error)) => return Err(error),
        }
    }
    let result = terminal_result.ok_or(AuthorityOperatorErrorV1::DatabaseBusy)?;
    let verification = verify_committed(command, &result.revision_one).await;
    if result.commit_indeterminate {
        verification.map_err(|_| AuthorityOperatorErrorV1::CommitIndeterminate)?;
    } else {
        verification?;
    }
    Ok(AuthorityAdvanceReportV1 {
        outcome: match (result.outcome, result.commit_indeterminate) {
            (AuthorityAdvanceOutcomeV1::Advanced, true) => {
                AuthorityAdvanceOutcomeV1::RecoveredCommitted
            }
            (outcome, _) => outcome,
        },
        installation_id: command.installation_id().to_string(),
        authority_revision: EXPECTED_SUCCESSOR_REVISION as u64,
        channel_id: command.channel_id(),
    })
}

async fn advance_once(
    connection: &mut PgConnection,
    command: &AuthorityAdvanceCommandV1,
) -> Result<AttemptResultV1, AttemptFailureV1> {
    let mut transaction = connection
        .begin()
        .await
        .map_err(classify_transaction_error)?;
    sqlx::raw_sql(
        "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE; \
         SET LOCAL lock_timeout = '5s'; \
         SET LOCAL statement_timeout = '30s'; \
         SET LOCAL idle_in_transaction_session_timeout = '30s'; \
         SET LOCAL search_path = pg_catalog, public; \
         SET LOCAL ROLE starring_owner; \
         SET CONSTRAINTS ALL DEFERRED",
    )
    .execute(&mut *transaction)
    .await
    .map_err(classify_transaction_error)?;

    let installation_count: i64 =
        sqlx::query_scalar("SELECT pg_catalog.count(*) FROM public.automation_installations")
            .fetch_one(&mut *transaction)
            .await
            .map_err(classify_transaction_error)?;
    if installation_count != 1 {
        return Err(AttemptFailureV1::Terminal(
            AuthorityOperatorErrorV1::AuthorityPrecondition,
        ));
    }
    let installation = sqlx::query_as::<_, InstallationRowV1>(INSTALLATION_FOR_UPDATE_SQL)
        .bind(command.installation_id())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(classify_transaction_error)?
        .ok_or(AttemptFailureV1::Terminal(
            AuthorityOperatorErrorV1::AuthorityPrecondition,
        ))?;
    validate_installation(&installation, command).map_err(AttemptFailureV1::Terminal)?;
    let authorities = sqlx::query_as::<_, AuthorityRowV1>(AUTHORITIES_SQL)
        .bind(command.installation_id())
        .fetch_all(&mut *transaction)
        .await
        .map_err(classify_transaction_error)?;
    let revision_one = authorities
        .first()
        .filter(|authority| authority.revision == EXPECTED_SOURCE_REVISION)
        .cloned()
        .ok_or(AttemptFailureV1::Terminal(
            AuthorityOperatorErrorV1::AuthorityPrecondition,
        ))?;
    let expected =
        expected_successor(&revision_one, command).map_err(AttemptFailureV1::Terminal)?;
    let outcome = match installation.current_authority_revision {
        EXPECTED_SOURCE_REVISION => {
            if authorities.len() != 1 {
                return Err(AttemptFailureV1::Terminal(
                    AuthorityOperatorErrorV1::AuthorityPrecondition,
                ));
            }
            let empty_product_state = pristine_product_state(&mut transaction, &installation)
                .await
                .map_err(classify_transaction_error)?;
            if !empty_product_state {
                return Err(AttemptFailureV1::Terminal(
                    AuthorityOperatorErrorV1::AuthorityPrecondition,
                ));
            }
            let active_principal =
                active_principal(&mut transaction, &expected.created_by_principal_id)
                    .await
                    .map_err(classify_transaction_error)?;
            if !active_principal {
                return Err(AttemptFailureV1::Terminal(
                    AuthorityOperatorErrorV1::AuthorityPrecondition,
                ));
            }
            sqlx::query(INSERT_SUCCESSOR_SQL)
                .bind(&expected.installation_id)
                .bind(&expected.tenant_id)
                .bind(&expected.resource_bindings)
                .bind(expected.binding_fingerprint.as_str())
                .bind(expected.policy_revision)
                .bind(expected.required_approvals)
                .bind(expected.activation_ttl_seconds)
                .bind(expected.authority_payload_digest.as_str())
                .bind(&expected.created_by_principal_id)
                .bind(expected.created_by_request_digest.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(classify_transaction_error)?;
            let advanced = sqlx::query(ADVANCE_HEAD_SQL)
                .bind(&expected.tenant_id)
                .bind(&expected.installation_id)
                .execute(&mut *transaction)
                .await
                .map_err(classify_transaction_error)?;
            if advanced.rows_affected() != 1 {
                return Err(AttemptFailureV1::Terminal(
                    AuthorityOperatorErrorV1::AuthorityPrecondition,
                ));
            }
            AuthorityAdvanceOutcomeV1::Advanced
        }
        EXPECTED_SUCCESSOR_REVISION => {
            if authorities.len() != 2 || !authority_matches_expected(&authorities[1], &expected) {
                return Err(AttemptFailureV1::Terminal(
                    AuthorityOperatorErrorV1::AuthorityConflict,
                ));
            }
            AuthorityAdvanceOutcomeV1::ExactReplay
        }
        _ => {
            return Err(AttemptFailureV1::Terminal(
                AuthorityOperatorErrorV1::AuthorityConflict,
            ))
        }
    };
    if let Err(error) = sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *transaction)
        .await
    {
        return Err(classify_transaction_error(error));
    }
    match transaction.commit().await {
        Ok(()) => Ok(AttemptResultV1 {
            outcome,
            revision_one,
            commit_indeterminate: false,
        }),
        Err(error) if serialization_failure(&error) => Err(AttemptFailureV1::Retryable),
        Err(_) => Ok(AttemptResultV1 {
            outcome,
            revision_one,
            commit_indeterminate: true,
        }),
    }
}

async fn verify_committed(
    command: &AuthorityAdvanceCommandV1,
    captured_revision_one: &AuthorityRowV1,
) -> Result<(), AuthorityOperatorErrorV1> {
    let mut connection = connect_target(command, "starring-authority-postverify").await?;
    let mut transaction = connection
        .begin()
        .await
        .map_err(|_| AuthorityOperatorErrorV1::Verification)?;
    sqlx::raw_sql(
        "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY DEFERRABLE; \
         SET LOCAL lock_timeout = '5s'; \
         SET LOCAL statement_timeout = '30s'; \
         SET LOCAL idle_in_transaction_session_timeout = '30s'; \
         SET LOCAL search_path = pg_catalog, public; \
         SET LOCAL ROLE starring_owner",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|_| AuthorityOperatorErrorV1::Verification)?;
    let installation_count: i64 =
        sqlx::query_scalar("SELECT pg_catalog.count(*) FROM public.automation_installations")
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| AuthorityOperatorErrorV1::Verification)?;
    let installation = sqlx::query_as::<_, InstallationRowV1>(INSTALLATION_READ_SQL)
        .bind(command.installation_id())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| AuthorityOperatorErrorV1::Verification)?
        .ok_or(AuthorityOperatorErrorV1::Verification)?;
    let authorities = sqlx::query_as::<_, AuthorityRowV1>(AUTHORITIES_SQL)
        .bind(command.installation_id())
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| AuthorityOperatorErrorV1::Verification)?;
    if installation_count != 1
        || validate_installation(&installation, command).is_err()
        || installation.current_authority_revision != EXPECTED_SUCCESSOR_REVISION
        || authorities.len() != 2
        || authorities[0] != *captured_revision_one
    {
        return Err(AuthorityOperatorErrorV1::Verification);
    }
    let expected = expected_successor(&authorities[0], command)
        .map_err(|_| AuthorityOperatorErrorV1::Verification)?;
    if !authority_matches_expected(&authorities[1], &expected)
        || !stored_successor_recomputes(&authorities[1], &authorities[0])
    {
        return Err(AuthorityOperatorErrorV1::Verification);
    }
    if !active_principal(&mut transaction, &expected.created_by_principal_id)
        .await
        .map_err(|_| AuthorityOperatorErrorV1::Verification)?
    {
        return Err(AuthorityOperatorErrorV1::Verification);
    }
    transaction
        .commit()
        .await
        .map_err(|_| AuthorityOperatorErrorV1::Verification)
}

async fn connect_target(
    command: &AuthorityAdvanceCommandV1,
    application_name: &str,
) -> Result<PgConnection, AuthorityOperatorErrorV1> {
    let secret = read_admin_database_url()?;
    let options = exact_admin_target_connect_options(&secret, application_name)?;
    drop(secret);
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|_| AuthorityOperatorErrorV1::DatabaseConnection)?;
    verify_cluster(&mut connection, command.system_identifier()).await?;
    Ok(connection)
}

fn exact_admin_target_connect_options(
    value: &[u8],
    application_name: &str,
) -> Result<PgConnectOptions, AuthorityOperatorErrorV1> {
    let value =
        std::str::from_utf8(value).map_err(|_| AuthorityOperatorErrorV1::DatabaseUrlShape)?;
    let prefix = format!("postgresql://{CLUSTER_ADMIN_ROLE}:");
    let suffix = format!("@{DATABASE_HOST}:{DATABASE_PORT}/{ADMIN_DATABASE_NAME}?sslmode=disable");
    let password = value
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(&suffix))
        .ok_or(AuthorityOperatorErrorV1::DatabaseUrlShape)?;
    if password.len() != 43
        || !password
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(AuthorityOperatorErrorV1::DatabaseUrlShape);
    }
    let options = PgConnectOptions::from_str(value)
        .map_err(|_| AuthorityOperatorErrorV1::DatabaseUrlShape)?;
    Ok(options
        .host(DATABASE_HOST)
        .port(DATABASE_PORT)
        .username(CLUSTER_ADMIN_ROLE)
        .database(DATABASE_NAME)
        .ssl_mode(PgSslMode::Disable)
        .application_name(application_name))
}

async fn verify_cluster(
    connection: &mut PgConnection,
    system_identifier: &str,
) -> Result<(), AuthorityOperatorErrorV1> {
    let row = sqlx::query(CLUSTER_PREFLIGHT_SQL)
        .bind(system_identifier)
        .fetch_optional(connection)
        .await
        .map_err(|_| AuthorityOperatorErrorV1::ClusterContract)?
        .ok_or(AuthorityOperatorErrorV1::ClusterContract)?;
    for index in 0..11 {
        let valid: bool = row
            .try_get(index)
            .map_err(|_| AuthorityOperatorErrorV1::ClusterContract)?;
        if !valid {
            return Err(AuthorityOperatorErrorV1::ClusterContract);
        }
    }
    Ok(())
}

fn validate_installation(
    installation: &InstallationRowV1,
    command: &AuthorityAdvanceCommandV1,
) -> Result<(), AuthorityOperatorErrorV1> {
    if installation.installation_id != command.installation_id()
        || installation.tenant_id.is_empty()
        || canonical_snowflake(&installation.discord_application_id).is_none()
        || canonical_snowflake(&installation.discord_guild_id).is_none()
        || installation.ruleset_key.is_empty()
        || installation.lifecycle_state != "active"
        || installation.tenant_lifecycle_state != "active"
        || !matches!(
            installation.current_authority_revision,
            EXPECTED_SOURCE_REVISION | EXPECTED_SUCCESSOR_REVISION
        )
    {
        return Err(AuthorityOperatorErrorV1::AuthorityPrecondition);
    }
    Ok(())
}

fn expected_successor(
    revision_one: &AuthorityRowV1,
    command: &AuthorityAdvanceCommandV1,
) -> Result<ExpectedSuccessorV1, AuthorityOperatorErrorV1> {
    let predecessor_digest =
        InstallationAuthorityPayloadDigestV1::parse(revision_one.authority_payload_digest.clone())
            .map_err(|_| AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    if revision_one.installation_id != command.installation_id()
        || revision_one.revision != EXPECTED_SOURCE_REVISION
        || revision_one.binding_revision != EXPECTED_SOURCE_REVISION
        || revision_one.policy_revision <= 0
        || revision_one.required_approvals <= 0
        || revision_one.activation_ttl_seconds <= 0
        || InstallationAuthorityRequestDigestV1::parse(
            revision_one.created_by_request_digest.clone(),
        )
        .is_err()
    {
        return Err(AuthorityOperatorErrorV1::AuthorityPrecondition);
    }
    let source_bindings = decode_resource_bindings(&revision_one.resource_bindings)?;
    if source_bindings != ResourceBindingMap::default()
        || resource_binding_fingerprint_v2(&source_bindings).as_str()
            != revision_one.binding_fingerprint
    {
        return Err(AuthorityOperatorErrorV1::AuthorityPrecondition);
    }
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        ResourceKey(binding_key().to_string()),
        ChannelId(command.channel_id()),
    );
    let binding_fingerprint = resource_binding_fingerprint_v2(&bindings);
    let resource_bindings = encode_resource_bindings(&bindings)?;
    let revision = NonZeroU64::new(EXPECTED_SUCCESSOR_REVISION as u64)
        .ok_or(AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let binding_revision = NonZeroU64::new(EXPECTED_SUCCESSOR_REVISION as u64)
        .ok_or(AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let policy_revision = u64::try_from(revision_one.policy_revision)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or(AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let required_approvals = u32::try_from(revision_one.required_approvals)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or(AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let activation_ttl_seconds = u64::try_from(revision_one.activation_ttl_seconds)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or(AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let scope =
        InstallationAuthorityScopeV1::new(&revision_one.tenant_id, &revision_one.installation_id)
            .map_err(|_| AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let policy = InstallationAuthorityPolicyV1::new(
        policy_revision,
        required_approvals,
        activation_ttl_seconds,
    )
    .map_err(|_| AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let payload = InstallationAuthorityPayloadIdentityV1::new(
        scope,
        revision,
        binding_revision,
        &binding_fingerprint,
        policy,
    )
    .map_err(|_| AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let authority_payload_digest = installation_authority_payload_digest_v1(&payload);
    let request = InstallationAuthorityRequestIdentityV1::new(
        NonZeroU64::new(EXPECTED_SOURCE_REVISION as u64)
            .ok_or(AuthorityOperatorErrorV1::AuthorityPrecondition)?,
        &predecessor_digest,
        &payload,
        &revision_one.created_by_principal_id,
    )
    .map_err(|_| AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let created_by_request_digest = installation_authority_request_digest_v1(&request);
    Ok(ExpectedSuccessorV1 {
        tenant_id: revision_one.tenant_id.clone(),
        installation_id: revision_one.installation_id.clone(),
        resource_bindings,
        binding_fingerprint,
        policy_revision: revision_one.policy_revision,
        required_approvals: revision_one.required_approvals,
        activation_ttl_seconds: revision_one.activation_ttl_seconds,
        authority_payload_digest,
        created_by_principal_id: revision_one.created_by_principal_id.clone(),
        created_by_request_digest,
    })
}

fn authority_matches_expected(authority: &AuthorityRowV1, expected: &ExpectedSuccessorV1) -> bool {
    authority.installation_id == expected.installation_id
        && authority.revision == EXPECTED_SUCCESSOR_REVISION
        && authority.tenant_id == expected.tenant_id
        && authority.binding_revision == EXPECTED_SUCCESSOR_REVISION
        && authority.resource_bindings == expected.resource_bindings
        && authority.binding_fingerprint == expected.binding_fingerprint.as_str()
        && authority.policy_revision == expected.policy_revision
        && authority.required_approvals == expected.required_approvals
        && authority.activation_ttl_seconds == expected.activation_ttl_seconds
        && authority.authority_payload_digest == expected.authority_payload_digest.as_str()
        && authority.created_by_principal_id == expected.created_by_principal_id
        && authority.created_by_request_digest == expected.created_by_request_digest.as_str()
}

fn stored_successor_recomputes(authority: &AuthorityRowV1, predecessor: &AuthorityRowV1) -> bool {
    let Ok(bindings) = decode_resource_bindings(&authority.resource_bindings) else {
        return false;
    };
    let fingerprint = resource_binding_fingerprint_v2(&bindings);
    if fingerprint.as_str() != authority.binding_fingerprint
        || !bindings.role_bindings.is_empty()
        || bindings.channel_bindings.len() != 1
        || bindings
            .channel_bindings
            .keys()
            .next()
            .map(|key| key.0.as_str())
            != Some(binding_key())
    {
        return false;
    }
    let Some(revision) = u64::try_from(authority.revision)
        .ok()
        .and_then(NonZeroU64::new)
    else {
        return false;
    };
    let Some(binding_revision) = u64::try_from(authority.binding_revision)
        .ok()
        .and_then(NonZeroU64::new)
    else {
        return false;
    };
    let Some(policy_revision) = u64::try_from(authority.policy_revision)
        .ok()
        .and_then(NonZeroU64::new)
    else {
        return false;
    };
    let Some(required_approvals) = u32::try_from(authority.required_approvals)
        .ok()
        .and_then(NonZeroU32::new)
    else {
        return false;
    };
    let Some(activation_ttl_seconds) = u64::try_from(authority.activation_ttl_seconds)
        .ok()
        .and_then(NonZeroU64::new)
    else {
        return false;
    };
    let Ok(scope) =
        InstallationAuthorityScopeV1::new(&authority.tenant_id, &authority.installation_id)
    else {
        return false;
    };
    let Ok(policy) = InstallationAuthorityPolicyV1::new(
        policy_revision,
        required_approvals,
        activation_ttl_seconds,
    ) else {
        return false;
    };
    let Ok(payload) = InstallationAuthorityPayloadIdentityV1::new(
        scope,
        revision,
        binding_revision,
        &fingerprint,
        policy,
    ) else {
        return false;
    };
    let payload_digest = installation_authority_payload_digest_v1(&payload);
    let Ok(predecessor_digest) =
        InstallationAuthorityPayloadDigestV1::parse(predecessor.authority_payload_digest.clone())
    else {
        return false;
    };
    let Ok(request) = InstallationAuthorityRequestIdentityV1::new(
        NonZeroU64::new(EXPECTED_SOURCE_REVISION as u64).expect("source revision is nonzero"),
        &predecessor_digest,
        &payload,
        &authority.created_by_principal_id,
    ) else {
        return false;
    };
    let request_digest = installation_authority_request_digest_v1(&request);
    authority.authority_payload_digest == payload_digest.as_str()
        && authority.created_by_request_digest == request_digest.as_str()
}

async fn active_principal(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    principal_id: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT pg_catalog.count(*) = 1 \
         FROM public.product_principals AS principal \
         WHERE principal.principal_id = $1 AND NOT principal.disabled",
    )
    .bind(principal_id)
    .fetch_one(&mut **transaction)
    .await
}

async fn pristine_product_state(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    installation: &InstallationRowV1,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(PRISTINE_PRODUCT_STATE_SQL)
        .bind(&installation.tenant_id)
        .bind(&installation.installation_id)
        .bind(&installation.discord_guild_id)
        .bind(&installation.ruleset_key)
        .fetch_one(&mut **transaction)
        .await
}

fn decode_resource_bindings(value: &Value) -> Result<ResourceBindingMap, AuthorityOperatorErrorV1> {
    let stored = serde_json::from_value::<StoredResourceBindingsV1>(value.clone())
        .map_err(|_| AuthorityOperatorErrorV1::AuthorityPrecondition)?;
    let mut bindings = ResourceBindingMap::default();
    for (key, value) in stored.role_bindings {
        let parsed =
            canonical_snowflake(&value).ok_or(AuthorityOperatorErrorV1::AuthorityPrecondition)?;
        bindings
            .role_bindings
            .insert(ResourceKey(key), discord_model::RoleId(parsed));
    }
    for (key, value) in stored.channel_bindings {
        let parsed =
            canonical_snowflake(&value).ok_or(AuthorityOperatorErrorV1::AuthorityPrecondition)?;
        bindings
            .channel_bindings
            .insert(ResourceKey(key), ChannelId(parsed));
    }
    Ok(bindings)
}

fn encode_resource_bindings(
    bindings: &ResourceBindingMap,
) -> Result<Value, AuthorityOperatorErrorV1> {
    serde_json::to_value(StoredResourceBindingsV1 {
        role_bindings: bindings
            .role_bindings
            .iter()
            .map(|(key, value)| (key.0.clone(), value.to_string()))
            .collect(),
        channel_bindings: bindings
            .channel_bindings
            .iter()
            .map(|(key, value)| (key.0.clone(), value.to_string()))
            .collect(),
    })
    .map_err(|_| AuthorityOperatorErrorV1::AuthorityPrecondition)
}

fn canonical_snowflake(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed != 0 && parsed.to_string() == value).then_some(parsed)
}

fn classify_transaction_error(error: sqlx::Error) -> AttemptFailureV1 {
    if serialization_failure(&error) {
        AttemptFailureV1::Retryable
    } else {
        AttemptFailureV1::Terminal(AuthorityOperatorErrorV1::DatabaseMutation)
    }
}

fn serialization_failure(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| matches!(code.as_ref(), "40001" | "40P01" | "55P03" | "57014"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{AuthorityAdvanceCommandValuesV1, AuthorityOperatorErrorV1};

    fn command(channel_id: &str) -> AuthorityAdvanceCommandV1 {
        AuthorityAdvanceCommandV1::parse(AuthorityAdvanceCommandValuesV1 {
            system_identifier: "7663763942264209752".to_string(),
            installation_id: "installation.staging".to_string(),
            channel_id: channel_id.to_string(),
            acknowledgement: format!(
                "starring-staging-authority-advance-v1:7663763942264209752:installation.staging:1:2:community_hub:{channel_id}:reviewed-discord-text-channel"
            ),
        })
        .unwrap()
    }

    fn revision_one() -> AuthorityRowV1 {
        let bindings = ResourceBindingMap::default();
        AuthorityRowV1 {
            installation_id: "installation.staging".to_string(),
            revision: 1,
            tenant_id: "tenant.staging".to_string(),
            binding_revision: 1,
            resource_bindings: json!({
                "role_bindings": {},
                "channel_bindings": {}
            }),
            binding_fingerprint: resource_binding_fingerprint_v2(&bindings).into_string(),
            policy_revision: 7,
            required_approvals: 1,
            activation_ttl_seconds: 7_200,
            authority_payload_digest: "a".repeat(64),
            created_by_principal_id: "discord:1056857223529250906".to_string(),
            created_by_request_digest: "b".repeat(64),
            created_at: "2026-07-30 00:00:00+09".to_string(),
        }
    }

    #[test]
    fn successor_preserves_policy_and_adds_only_the_reviewed_channel() {
        let source = revision_one();
        let expected = expected_successor(&source, &command("123456789012345678")).unwrap();
        assert_eq!(expected.policy_revision, source.policy_revision);
        assert_eq!(expected.required_approvals, source.required_approvals);
        assert_eq!(
            expected.activation_ttl_seconds,
            source.activation_ttl_seconds
        );
        assert_eq!(
            expected.resource_bindings,
            json!({
                "role_bindings": {},
                "channel_bindings": {
                    "community_hub": "123456789012345678"
                }
            })
        );
        let row = AuthorityRowV1 {
            installation_id: expected.installation_id.clone(),
            revision: 2,
            tenant_id: expected.tenant_id.clone(),
            binding_revision: 2,
            resource_bindings: expected.resource_bindings.clone(),
            binding_fingerprint: expected.binding_fingerprint.to_string(),
            policy_revision: expected.policy_revision,
            required_approvals: expected.required_approvals,
            activation_ttl_seconds: expected.activation_ttl_seconds,
            authority_payload_digest: expected.authority_payload_digest.to_string(),
            created_by_principal_id: expected.created_by_principal_id.clone(),
            created_by_request_digest: expected.created_by_request_digest.to_string(),
            created_at: "2026-07-30 00:00:01+09".to_string(),
        };
        assert!(authority_matches_expected(&row, &expected));
        assert!(stored_successor_recomputes(&row, &source));
    }

    #[test]
    fn different_channel_is_a_distinct_authority_and_request_identity() {
        let source = revision_one();
        let first = expected_successor(&source, &command("123456789012345678")).unwrap();
        let second = expected_successor(&source, &command("123456789012345679")).unwrap();
        assert_ne!(first.binding_fingerprint, second.binding_fingerprint);
        assert_ne!(
            first.authority_payload_digest,
            second.authority_payload_digest
        );
        assert_ne!(
            first.created_by_request_digest,
            second.created_by_request_digest
        );
    }

    #[test]
    fn source_authority_must_be_exact_empty_revision_one() {
        let mut source = revision_one();
        source.binding_revision = 2;
        assert_eq!(
            expected_successor(&source, &command("123456789012345678")).err(),
            Some(AuthorityOperatorErrorV1::AuthorityPrecondition)
        );

        let mut source = revision_one();
        source.resource_bindings = json!({
            "role_bindings": {},
            "channel_bindings": {"other": "700"}
        });
        assert_eq!(
            expected_successor(&source, &command("123456789012345678")).err(),
            Some(AuthorityOperatorErrorV1::AuthorityPrecondition)
        );
    }

    #[test]
    fn exact_admin_url_parser_accepts_only_the_fixed_keychain_shape() {
        let password = "A".repeat(43);
        let valid = format!(
            "postgresql://{CLUSTER_ADMIN_ROLE}:{password}@{DATABASE_HOST}:{DATABASE_PORT}/{ADMIN_DATABASE_NAME}?sslmode=disable"
        );
        assert!(exact_admin_target_connect_options(valid.as_bytes(), "test").is_ok());
        for invalid in [
            valid.replace(CLUSTER_ADMIN_ROLE, "other"),
            valid.replace(DATABASE_HOST, "localhost"),
            valid.replace(ADMIN_DATABASE_NAME, DATABASE_NAME),
            valid.replace("sslmode=disable", "sslmode=require"),
            valid.replace(&password, "short"),
        ] {
            assert!(exact_admin_target_connect_options(invalid.as_bytes(), "test").is_err());
        }
    }

    #[test]
    fn cluster_preflight_uses_a_schema_qualified_atomic_type_name() {
        assert!(CLUSTER_PREFLIGHT_SQL.contains("'infinity'::PG_CATALOG.TIMESTAMPTZ"));
        assert!(!CLUSTER_PREFLIGHT_SQL.contains("PG_CATALOG.TIMESTAMP WITH TIME ZONE"));
    }

    #[test]
    fn mutation_sql_owns_one_serializable_insert_and_cas_boundary() {
        for required in [
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
            "SET LOCAL ROLE starring_owner",
            "SET CONSTRAINTS ALL DEFERRED",
            "SET CONSTRAINTS ALL IMMEDIATE",
        ] {
            let source = include_str!("postgres.rs");
            assert!(source.contains(required));
        }
        assert!(INSERT_SUCCESSOR_SQL
            .contains("INSERT INTO public.automation_installation_authority_versions"));
        assert!(ADVANCE_HEAD_SQL.contains("AND current_authority_revision = 1"));
        assert!(ADVANCE_HEAD_SQL.contains("SET\n    current_authority_revision = 2"));
    }

    #[test]
    fn pristine_state_rejects_slot_artifacts_and_active_pointers() {
        assert!(PRISTINE_PRODUCT_STATE_SQL.contains("public.automation_ruleset_versions"));
        assert!(PRISTINE_PRODUCT_STATE_SQL.contains("public.automation_ruleset_activations"));
        assert_eq!(
            PRISTINE_PRODUCT_STATE_SQL.matches("guild_id = $3").count(),
            3
        );
        assert_eq!(
            PRISTINE_PRODUCT_STATE_SQL
                .matches("ruleset_key = $4")
                .count(),
            3
        );
    }
}
