use std::num::{NonZeroU32, NonZeroU64};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use authoring_application::{
    AuthenticatedActorV1, AuthorizedInstallationScopeV1, AuthorizedPromotionSnapshotError,
    AuthorizedPromotionSnapshotPort, AuthorizedPromotionSnapshotV1, CapabilityV1,
    FreshGuildAuthorityEvidence, OwnedSessionLoadError, PromotionAuthorityError,
    ResolvedPromotionAuthorityV1,
};
use authoring_promotion::{
    ApprovalPolicyV1, AuthoringSessionId, AutomationInstallationId, BindingRevision,
    PolicyRevision, SessionGeneration, TenantId,
};
use automation_ruleset::RuleSetKey;
use chrono::{DateTime, TimeDelta, Utc};
use design_harness::{
    DesignSession, LlmClient, LlmError, LlmResponse, Message, PreviewReadyArtifactError,
    PreviewReadyArtifactV1, SessionConfig, SessionSnapshot, ToolDefinition,
};
use discord_model::{GuildId, Permissions, UserId};
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingFingerprint};
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::types::Json;
use subtle::ConstantTimeEq;

use crate::bindings::decode_resource_bindings;
use crate::envelope::{
    build_snapshot_authenticated_data_v1, EncryptedSnapshotEnvelopeV1,
    SnapshotAuthenticatedDataInputV1, SnapshotEnvelopeCipher,
};
use crate::ProductDatabaseFailureV1;

const DEFAULT_STATEMENT_TIMEOUT_MILLIS: u64 = 2_000;
const DEFAULT_FRESH_AUTHORITY_MILLIS: u64 = 5_000;
const DEFAULT_MAX_PLAINTEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_STATEMENT_TIMEOUT_MILLIS: u64 = 60_000;
const MAX_FRESH_AUTHORITY_MILLIS: u64 = 60_000;
const MAX_PLAINTEXT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizedSnapshotConfigError {
    #[error("authorized snapshot statement timeout is outside the supported range")]
    InvalidStatementTimeout,
    #[error("fresh guild authority lifetime is outside the supported range")]
    InvalidFreshAuthorityLifetime,
    #[error("authorized snapshot plaintext bound is outside the supported range")]
    InvalidPlaintextBound,
    #[error("authorized snapshot duration exceeds the supported range")]
    DurationOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostgresAuthorizedPromotionSnapshotsConfig {
    statement_timeout: Duration,
    fresh_authority_lifetime: Duration,
    max_plaintext_bytes: usize,
}

impl PostgresAuthorizedPromotionSnapshotsConfig {
    pub fn new(
        statement_timeout: Duration,
        fresh_authority_lifetime: Duration,
        max_plaintext_bytes: usize,
    ) -> Result<Self, AuthorizedSnapshotConfigError> {
        let statement_timeout_millis = statement_timeout.as_millis();
        if statement_timeout_millis == 0
            || statement_timeout_millis > u128::from(MAX_STATEMENT_TIMEOUT_MILLIS)
        {
            return Err(AuthorizedSnapshotConfigError::InvalidStatementTimeout);
        }
        let authority_millis = fresh_authority_lifetime.as_millis();
        if authority_millis == 0 || authority_millis > u128::from(MAX_FRESH_AUTHORITY_MILLIS) {
            return Err(AuthorizedSnapshotConfigError::InvalidFreshAuthorityLifetime);
        }
        if max_plaintext_bytes == 0 || max_plaintext_bytes > MAX_PLAINTEXT_BYTES {
            return Err(AuthorizedSnapshotConfigError::InvalidPlaintextBound);
        }
        TimeDelta::from_std(fresh_authority_lifetime)
            .map_err(|_| AuthorizedSnapshotConfigError::DurationOverflow)?;
        Ok(Self {
            statement_timeout,
            fresh_authority_lifetime,
            max_plaintext_bytes,
        })
    }

    fn statement_timeout(self) -> String {
        format!("{}ms", self.statement_timeout.as_millis())
    }

    fn fresh_authority_lifetime(self) -> Result<TimeDelta, PromotionAuthorityError> {
        TimeDelta::from_std(self.fresh_authority_lifetime)
            .map_err(|_| authority_backend("fresh authority lifetime exceeds chrono range"))
    }
}

impl Default for PostgresAuthorizedPromotionSnapshotsConfig {
    fn default() -> Self {
        Self::new(
            Duration::from_millis(DEFAULT_STATEMENT_TIMEOUT_MILLIS),
            Duration::from_millis(DEFAULT_FRESH_AUTHORITY_MILLIS),
            DEFAULT_MAX_PLAINTEXT_BYTES,
        )
        .expect("default authorized snapshot configuration is valid")
    }
}

#[derive(Clone)]
pub struct PostgresAuthorizedPromotionSnapshots<C> {
    pool: PgPool,
    cipher: C,
    config: PostgresAuthorizedPromotionSnapshotsConfig,
}

impl<C> PostgresAuthorizedPromotionSnapshots<C> {
    pub fn new(pool: PgPool, cipher: C) -> Self {
        Self {
            pool,
            cipher,
            config: PostgresAuthorizedPromotionSnapshotsConfig::default(),
        }
    }

    pub fn with_config(
        pool: PgPool,
        cipher: C,
        config: PostgresAuthorizedPromotionSnapshotsConfig,
    ) -> Self {
        Self {
            pool,
            cipher,
            config,
        }
    }
}

#[derive(sqlx::FromRow)]
struct AtomicSnapshotRow {
    session_tenant_id: String,
    session_installation_id: String,
    owner_principal_id: String,
    owner_discord_user_id: String,
    owner_disabled: bool,
    actor_session_digest: Vec<u8>,
    current_generation: i64,
    session_lifecycle_state: String,
    tenant_lifecycle_state: String,
    installation_tenant_id: String,
    discord_application_id: String,
    discord_guild_id: String,
    ruleset_key: String,
    installation_lifecycle_state: String,
    current_authority_revision: i64,
    generation: i64,
    snapshot_schema_version: i64,
    snapshot_ciphertext: Vec<u8>,
    snapshot_nonce: Vec<u8>,
    encryption_key_id: String,
    encryption_suite: String,
    encryption_suite_version: i16,
    authenticated_metadata_digest: String,
    generation_resource_bindings: Json<Value>,
    generation_binding_fingerprint: String,
    installation_authority_revision: i64,
    generation_stage: String,
    candidate_revision: Option<i64>,
    candidate_hash: Option<String>,
    harness_contract_revision: i64,
    authority_tenant_id: String,
    binding_revision: i64,
    authority_resource_bindings: Json<Value>,
    authority_binding_fingerprint: String,
    policy_revision: i64,
    required_approvals: i32,
    activation_ttl_seconds: i64,
    authority_payload_digest: String,
    database_now: DateTime<Utc>,
}

struct CopiedAuthorizedSnapshot {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    session_id: AuthoringSessionId,
    generation: SessionGeneration,
    snapshot_schema_version: u32,
    authenticated_metadata_digest: String,
    binding_fingerprint: ResourceBindingFingerprint,
    bindings: resource_resolution::ResourceBindingMap,
    envelope: EncryptedSnapshotEnvelopeV1,
    candidate_revision: u64,
    candidate_hash: String,
    authority: ResolvedPromotionAuthorityV1,
}

impl<C, E> AuthorizedPromotionSnapshotPort<E> for PostgresAuthorizedPromotionSnapshots<C>
where
    C: SnapshotEnvelopeCipher,
    E: FreshGuildAuthorityEvidence,
{
    async fn load_atomic_authorized_snapshot(
        &self,
        actor: &AuthenticatedActorV1,
        scope: &AuthorizedInstallationScopeV1,
        evidence: &E,
        session_id: &AuthoringSessionId,
        expected_generation: SessionGeneration,
    ) -> Result<AuthorizedPromotionSnapshotV1, AuthorizedPromotionSnapshotError> {
        let mut transaction = self.pool.begin().await.map_err(session_database_backend)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(session_database_backend)?;
        sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, true)")
            .bind(self.config.statement_timeout())
            .execute(&mut *transaction)
            .await
            .map_err(session_database_backend)?;
        let row = fetch_atomic_snapshot(&mut transaction, session_id, actor, scope)
            .await?
            .ok_or(OwnedSessionLoadError::NotFound)?;
        let copied = validate_and_copy_row(
            row,
            actor,
            scope,
            evidence,
            session_id,
            expected_generation,
            self.config,
        )?;
        transaction
            .commit()
            .await
            .map_err(session_database_backend)?;
        let artifact = materialize_snapshot(&self.cipher, &copied, self.config).await?;
        Ok(AuthorizedPromotionSnapshotV1::from_atomic_authorization(
            artifact,
            copied.authority,
        ))
    }
}

async fn fetch_atomic_snapshot(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: &AuthoringSessionId,
    actor: &AuthenticatedActorV1,
    scope: &AuthorizedInstallationScopeV1,
) -> Result<Option<AtomicSnapshotRow>, AuthorizedPromotionSnapshotError> {
    sqlx::query_as::<_, AtomicSnapshotRow>(
        "SELECT authoring_session.tenant_id AS session_tenant_id, \
         authoring_session.installation_id AS session_installation_id, \
         authoring_session.owner_principal_id, principal.discord_user_id AS owner_discord_user_id, \
         principal.disabled AS owner_disabled, \
         actor_session.session_digest AS actor_session_digest, \
         authoring_session.current_generation, \
         authoring_session.lifecycle_state AS session_lifecycle_state, \
         tenant.lifecycle_state AS tenant_lifecycle_state, \
         installation.tenant_id AS installation_tenant_id, \
         installation.discord_application_id, installation.discord_guild_id, \
         installation.ruleset_key, \
         installation.lifecycle_state AS installation_lifecycle_state, \
         installation.current_authority_revision, generation.generation, \
         generation.snapshot_schema_version, generation.snapshot_ciphertext, \
         generation.snapshot_nonce, generation.encryption_key_id, generation.encryption_suite, \
         generation.encryption_suite_version, generation.authenticated_metadata_digest, \
         generation.resource_bindings AS generation_resource_bindings, \
         generation.binding_fingerprint AS generation_binding_fingerprint, \
         generation.installation_authority_revision, generation.stage AS generation_stage, \
         generation.candidate_revision, generation.candidate_hash, \
         generation.harness_contract_revision, \
         authority.tenant_id AS authority_tenant_id, authority.binding_revision, \
         authority.resource_bindings AS authority_resource_bindings, \
         authority.binding_fingerprint AS authority_binding_fingerprint, \
         authority.policy_revision, authority.required_approvals, \
         authority.activation_ttl_seconds, authority.authority_payload_digest, \
         CURRENT_TIMESTAMP AS database_now \
         FROM public.authoring_sessions AS authoring_session \
         INNER JOIN public.product_principals AS principal \
         ON principal.principal_id = authoring_session.owner_principal_id \
         INNER JOIN public.product_auth_sessions AS actor_session \
         ON actor_session.principal_id = authoring_session.owner_principal_id \
         AND actor_session.session_digest = $2 \
         AND actor_session.oauth_state_digest IS NOT NULL \
         AND actor_session.revoked_at IS NULL \
         AND CURRENT_TIMESTAMP < actor_session.idle_expires_at \
         AND CURRENT_TIMESTAMP < actor_session.absolute_expires_at \
         INNER JOIN public.product_tenants AS tenant \
         ON tenant.tenant_id = authoring_session.tenant_id \
         INNER JOIN public.automation_installations AS installation \
         ON installation.tenant_id = authoring_session.tenant_id \
         AND installation.installation_id = authoring_session.installation_id \
         INNER JOIN public.authoring_session_generations AS generation \
         ON generation.tenant_id = authoring_session.tenant_id \
         AND generation.installation_id = authoring_session.installation_id \
         AND generation.session_id = authoring_session.session_id \
         AND generation.generation = authoring_session.current_generation \
         INNER JOIN public.automation_installation_authority_versions AS authority \
         ON authority.tenant_id = generation.tenant_id \
         AND authority.installation_id = generation.installation_id \
         AND authority.revision = generation.installation_authority_revision \
         WHERE authoring_session.session_id = $1 \
         AND authoring_session.tenant_id = $3 \
         AND authoring_session.installation_id = $4",
    )
    .bind(session_id.as_str())
    .bind(actor.session_fingerprint().as_bytes().as_slice())
    .bind(scope.tenant_id().as_str())
    .bind(scope.installation_id().as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| session_database_backend(error).into())
}

fn validate_and_copy_row<E: FreshGuildAuthorityEvidence>(
    row: AtomicSnapshotRow,
    actor: &AuthenticatedActorV1,
    scope: &AuthorizedInstallationScopeV1,
    evidence: &E,
    session_id: &AuthoringSessionId,
    expected_generation: SessionGeneration,
    config: PostgresAuthorizedPromotionSnapshotsConfig,
) -> Result<CopiedAuthorizedSnapshot, AuthorizedPromotionSnapshotError> {
    if row.owner_principal_id != actor.principal_id().as_str() {
        return Err(OwnedSessionLoadError::NotOwned.into());
    }
    let persisted_session_digest: [u8; 32] = row
        .actor_session_digest
        .as_slice()
        .try_into()
        .map_err(|_| session_backend("persisted actor session digest is invalid"))?;
    if persisted_session_digest
        .ct_eq(actor.session_fingerprint().as_bytes())
        .unwrap_u8()
        != 1
    {
        return Err(OwnedSessionLoadError::NotOwned.into());
    }
    if row.owner_disabled {
        return Err(PromotionAuthorityError::Forbidden.into());
    }
    let current_generation = positive_u64(row.current_generation)
        .ok_or_else(|| session_backend("persisted session generation is invalid"))?;
    if current_generation != expected_generation.get() || row.generation != row.current_generation {
        return Err(OwnedSessionLoadError::GenerationMismatch.into());
    }
    if row.session_lifecycle_state != "active" || row.generation_stage != "preview_ready" {
        return Err(OwnedSessionLoadError::NotPreviewReady.into());
    }
    if row.tenant_lifecycle_state != "active" || row.installation_lifecycle_state != "active" {
        return Err(PromotionAuthorityError::Forbidden.into());
    }
    validate_scope(&row, scope)?;
    validate_evidence(&row, scope, evidence, config)?;
    if row.current_authority_revision != row.installation_authority_revision {
        return Err(PromotionAuthorityError::GenerationMismatch.into());
    }
    if row.harness_contract_revision <= 0 {
        return Err(session_backend("persisted harness contract revision is invalid").into());
    }
    let tenant_id = TenantId::parse(&row.session_tenant_id)
        .map_err(|_| authority_backend("persisted tenant identifier is invalid"))?;
    let installation_id = AutomationInstallationId::parse(&row.session_installation_id)
        .map_err(|_| authority_backend("persisted installation identifier is invalid"))?;
    let generation = SessionGeneration::new(current_generation)
        .map_err(|_| session_backend("persisted session generation is invalid"))?;
    let snapshot_schema_version = u32::try_from(row.snapshot_schema_version)
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| session_backend("persisted snapshot schema version is invalid"))?;
    let generation_bindings =
        decode_resource_bindings(row.generation_resource_bindings.0).map_err(session_backend)?;
    let authority_bindings =
        decode_resource_bindings(row.authority_resource_bindings.0).map_err(authority_backend)?;
    if generation_bindings != authority_bindings {
        return Err(PromotionAuthorityError::GenerationMismatch.into());
    }
    let binding_fingerprint = resource_binding_fingerprint_v2(&generation_bindings);
    if row.generation_binding_fingerprint != binding_fingerprint.as_str()
        || row.authority_binding_fingerprint != binding_fingerprint.as_str()
    {
        return Err(PromotionAuthorityError::GenerationMismatch.into());
    }
    let encryption_suite_version = u16::try_from(row.encryption_suite_version)
        .map_err(|_| session_backend("persisted encryption suite version is invalid"))?;
    let envelope = EncryptedSnapshotEnvelopeV1::from_persisted_parts(
        row.snapshot_ciphertext,
        row.snapshot_nonce,
        row.encryption_key_id,
        row.encryption_suite,
        encryption_suite_version,
    )
    .map_err(|_| session_backend("persisted snapshot envelope is invalid"))?;
    let candidate_revision = row
        .candidate_revision
        .and_then(positive_u64)
        .ok_or(OwnedSessionLoadError::NotPreviewReady)?;
    let candidate_hash = row
        .candidate_hash
        .filter(|value| is_lower_hex_digest(value))
        .ok_or(OwnedSessionLoadError::NotPreviewReady)?;
    let guild_id = parse_guild_id(&row.discord_guild_id)?;
    let requester = parse_user_id(&row.owner_discord_user_id)?;
    let ruleset_key = RuleSetKey::parse(&row.ruleset_key)
        .map_err(|_| authority_backend("persisted RuleSet key is invalid"))?;
    let binding_revision = BindingRevision::new(
        positive_u64(row.binding_revision)
            .ok_or_else(|| authority_backend("persisted binding revision is invalid"))?,
    )
    .map_err(|_| authority_backend("persisted binding revision is invalid"))?;
    let policy_revision = PolicyRevision::new(
        positive_u64(row.policy_revision)
            .ok_or_else(|| authority_backend("persisted policy revision is invalid"))?,
    )
    .map_err(|_| authority_backend("persisted policy revision is invalid"))?;
    let required_approvals = u32::try_from(row.required_approvals)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| authority_backend("persisted approval quorum is invalid"))?;
    let ttl_seconds = positive_u64(row.activation_ttl_seconds)
        .and_then(NonZeroU64::new)
        .ok_or_else(|| authority_backend("persisted activation TTL is invalid"))?;
    if !is_lower_hex_digest(&row.authority_payload_digest)
        || !is_lower_hex_digest(&row.authenticated_metadata_digest)
    {
        return Err(authority_backend("persisted authority digest is invalid").into());
    }
    Ok(CopiedAuthorizedSnapshot {
        tenant_id,
        installation_id: installation_id.clone(),
        session_id: session_id.clone(),
        generation,
        snapshot_schema_version,
        authenticated_metadata_digest: row.authenticated_metadata_digest,
        binding_fingerprint,
        bindings: generation_bindings,
        envelope,
        candidate_revision,
        candidate_hash,
        authority: ResolvedPromotionAuthorityV1 {
            guild_id,
            installation_id,
            ruleset_key,
            requester,
            binding_revision,
            policy: ApprovalPolicyV1 {
                revision: policy_revision,
                required_approvals,
                ttl_seconds,
            },
        },
    })
}

fn validate_scope(
    row: &AtomicSnapshotRow,
    scope: &AuthorizedInstallationScopeV1,
) -> Result<(), PromotionAuthorityError> {
    if row.session_tenant_id != scope.tenant_id().as_str()
        || row.installation_tenant_id != scope.tenant_id().as_str()
        || row.authority_tenant_id != scope.tenant_id().as_str()
        || row.session_installation_id != scope.installation_id().as_str()
        || row.owner_discord_user_id != scope.acting_user_id().to_string()
        || row.discord_guild_id != scope.guild_id().to_string()
    {
        return Err(PromotionAuthorityError::ScopeMismatch);
    }
    Ok(())
}

fn validate_evidence<E: FreshGuildAuthorityEvidence>(
    row: &AtomicSnapshotRow,
    scope: &AuthorizedInstallationScopeV1,
    evidence: &E,
    config: PostgresAuthorizedPromotionSnapshotsConfig,
) -> Result<(), PromotionAuthorityError> {
    if evidence.tenant_id() != scope.tenant_id()
        || evidence.installation_id() != scope.installation_id()
        || evidence.discord_application_id().get().to_string() != row.discord_application_id
        || evidence.guild_id() != scope.guild_id()
        || evidence.acting_user_id() != scope.acting_user_id()
        || evidence.capability() != CapabilityV1::Promote
        || evidence.installation_authority_revision().get()
            != positive_u64(row.current_authority_revision).unwrap_or(0)
        || evidence.installation_authority_digest() != row.authority_payload_digest
        || !is_lower_hex_digest(evidence.installation_authority_digest())
        || !is_lower_hex_digest(evidence.observation_digest())
    {
        return Err(PromotionAuthorityError::ScopeMismatch);
    }
    let permissions = Permissions::from_bits_retain(evidence.effective_permissions_bits());
    if !evidence.guild_owner()
        && !permissions.intersects(Permissions::ADMINISTRATOR | Permissions::MANAGE_GUILD)
    {
        return Err(PromotionAuthorityError::Forbidden);
    }
    let observed_at = system_time_to_utc(evidence.observed_at())
        .ok_or_else(|| authority_backend("fresh authority observation time is invalid"))?;
    let expires_at = system_time_to_utc(evidence.expires_at())
        .ok_or_else(|| authority_backend("fresh authority expiry time is invalid"))?;
    let maximum_expiry = observed_at
        .checked_add_signed(config.fresh_authority_lifetime()?)
        .ok_or_else(|| authority_backend("fresh authority expiry overflow"))?;
    if observed_at > row.database_now
        || expires_at <= observed_at
        || row.database_now >= expires_at
        || row.database_now - observed_at > config.fresh_authority_lifetime()?
        || expires_at > maximum_expiry
    {
        return Err(PromotionAuthorityError::Forbidden);
    }
    Ok(())
}

async fn materialize_snapshot<C: SnapshotEnvelopeCipher>(
    cipher: &C,
    copied: &CopiedAuthorizedSnapshot,
    config: PostgresAuthorizedPromotionSnapshotsConfig,
) -> Result<PreviewReadyArtifactV1, AuthorizedPromotionSnapshotError> {
    let authenticated_data =
        build_snapshot_authenticated_data_v1(SnapshotAuthenticatedDataInputV1 {
            tenant_id: &copied.tenant_id,
            installation_id: &copied.installation_id,
            session_id: &copied.session_id,
            generation: copied.generation,
            snapshot_schema_version: copied.snapshot_schema_version,
            binding_fingerprint: &copied.binding_fingerprint,
            encryption_key_id: copied.envelope.encryption_key_id(),
            encryption_suite: copied.envelope.encryption_suite(),
            encryption_suite_version: copied.envelope.encryption_suite_version(),
        })
        .map_err(|_| session_backend("persisted snapshot authenticated metadata is invalid"))?;
    if copied.authenticated_metadata_digest != authenticated_data.digest_hex() {
        return Err(session_backend("snapshot authenticated metadata digest mismatch").into());
    }
    let plaintext = cipher
        .decrypt(&copied.envelope, authenticated_data.as_bytes())
        .await
        .map_err(|_| session_backend("snapshot envelope could not be authenticated"))?;
    if plaintext.is_empty() || plaintext.len() > config.max_plaintext_bytes {
        return Err(
            session_backend("decrypted snapshot exceeds the durable plaintext bound").into(),
        );
    }
    let snapshot = serde_json::from_slice::<SessionSnapshot>(&plaintext)
        .map_err(|_| session_backend("decrypted authoring snapshot is invalid"))?;
    if snapshot.schema_version != copied.snapshot_schema_version {
        return Err(session_backend("snapshot schema projection does not match plaintext").into());
    }
    snapshot
        .validate_durable_size()
        .map_err(|_| session_backend("decrypted authoring snapshot violates durable bounds"))?;
    let restored = DesignSession::restore_intent_recipe(
        NoLlmClient,
        SessionConfig::default(),
        snapshot,
        copied.bindings.clone(),
    )
    .map_err(|_| session_backend("decrypted authoring snapshot failed restoration"))?;
    let artifact = restored
        .export_preview_ready_artifact()
        .map_err(map_artifact_error)?;
    if artifact.context_fingerprint() != &copied.binding_fingerprint
        || artifact.receipt().candidate_revision != copied.candidate_revision
        || artifact.receipt().candidate_ruleset_hash != copied.candidate_hash
    {
        return Err(session_backend("snapshot preview projection does not match artifact").into());
    }
    Ok(artifact)
}

#[derive(Clone, Copy)]
struct NoLlmClient;

impl LlmClient for NoLlmClient {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        Err(LlmError::Client(
            "model access is unavailable while restoring a durable snapshot".to_string(),
        ))
    }
}

fn map_artifact_error(error: PreviewReadyArtifactError) -> AuthorizedPromotionSnapshotError {
    match error {
        PreviewReadyArtifactError::IntentRecipeDisabled
        | PreviewReadyArtifactError::NotPreviewReady { .. } => {
            OwnedSessionLoadError::NotPreviewReady.into()
        }
        PreviewReadyArtifactError::InvalidSession(_)
        | PreviewReadyArtifactError::PreviewRendering { .. } => {
            session_backend("decrypted authoring snapshot could not export a safe preview").into()
        }
    }
}

fn parse_guild_id(value: &str) -> Result<GuildId, PromotionAuthorityError> {
    canonical_snowflake(value)
        .map(GuildId)
        .ok_or_else(|| authority_backend("persisted Discord guild identifier is invalid"))
}

fn parse_user_id(value: &str) -> Result<UserId, PromotionAuthorityError> {
    canonical_snowflake(value)
        .map(UserId)
        .ok_or_else(|| authority_backend("persisted Discord user identifier is invalid"))
}

fn canonical_snowflake(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed != 0 && parsed.to_string() == value).then_some(parsed)
}

fn system_time_to_utc(value: SystemTime) -> Option<DateTime<Utc>> {
    let duration = value.duration_since(UNIX_EPOCH).ok()?;
    let seconds = i64::try_from(duration.as_secs()).ok()?;
    DateTime::from_timestamp(seconds, duration.subsec_nanos())
}

fn positive_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|value| *value != 0)
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn session_backend(error: impl std::fmt::Display) -> OwnedSessionLoadError {
    OwnedSessionLoadError::Backend(error.to_string())
}

fn session_database_backend(error: sqlx::Error) -> OwnedSessionLoadError {
    session_backend(ProductDatabaseFailureV1::classify(&error))
}

fn authority_backend(error: impl std::fmt::Display) -> PromotionAuthorityError {
    PromotionAuthorityError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    struct Evidence {
        tenant_id: TenantId,
        installation_id: AutomationInstallationId,
        observed_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        guild_owner: bool,
        effective_permissions_bits: u64,
    }

    impl FreshGuildAuthorityEvidence for Evidence {
        fn tenant_id(&self) -> &TenantId {
            &self.tenant_id
        }

        fn installation_id(&self) -> &AutomationInstallationId {
            &self.installation_id
        }

        fn discord_application_id(&self) -> NonZeroU64 {
            NonZeroU64::new(900).unwrap()
        }

        fn guild_id(&self) -> GuildId {
            GuildId(901)
        }

        fn acting_user_id(&self) -> UserId {
            UserId(100)
        }

        fn capability(&self) -> CapabilityV1 {
            CapabilityV1::Promote
        }

        fn guild_owner(&self) -> bool {
            self.guild_owner
        }

        fn effective_permissions_bits(&self) -> u64 {
            self.effective_permissions_bits
        }

        fn installation_authority_revision(&self) -> NonZeroU64 {
            NonZeroU64::new(1).unwrap()
        }

        fn installation_authority_digest(&self) -> &str {
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
        }

        fn observation_digest(&self) -> &str {
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }

        fn observed_at(&self) -> SystemTime {
            self.observed_at.into()
        }

        fn expires_at(&self) -> SystemTime {
            self.expires_at.into()
        }
    }

    fn row(now: DateTime<Utc>) -> AtomicSnapshotRow {
        AtomicSnapshotRow {
            session_tenant_id: "tenant-1".to_string(),
            session_installation_id: "installation-1".to_string(),
            owner_principal_id: "principal-1".to_string(),
            owner_discord_user_id: "100".to_string(),
            owner_disabled: false,
            actor_session_digest: vec![5_u8; 32],
            current_generation: 1,
            session_lifecycle_state: "active".to_string(),
            tenant_lifecycle_state: "active".to_string(),
            installation_tenant_id: "tenant-1".to_string(),
            discord_application_id: "900".to_string(),
            discord_guild_id: "901".to_string(),
            ruleset_key: "studyroom".to_string(),
            installation_lifecycle_state: "active".to_string(),
            current_authority_revision: 1,
            generation: 1,
            snapshot_schema_version: 8,
            snapshot_ciphertext: vec![0; 16],
            snapshot_nonce: vec![0; 24],
            encryption_key_id: "key-1".to_string(),
            encryption_suite: "xchacha20_poly1305".to_string(),
            encryption_suite_version: 1,
            authenticated_metadata_digest: "b".repeat(64),
            generation_resource_bindings: Json(serde_json::json!({
                "role_bindings": {},
                "channel_bindings": {"community_hub": "700"}
            })),
            generation_binding_fingerprint: resource_binding_fingerprint_v2(
                &decode_resource_bindings(serde_json::json!({
                    "role_bindings": {},
                    "channel_bindings": {"community_hub": "700"}
                }))
                .unwrap(),
            )
            .into_string(),
            installation_authority_revision: 1,
            generation_stage: "preview_ready".to_string(),
            candidate_revision: Some(1),
            candidate_hash: Some("c".repeat(64)),
            harness_contract_revision: 1,
            authority_tenant_id: "tenant-1".to_string(),
            binding_revision: 1,
            authority_resource_bindings: Json(serde_json::json!({
                "role_bindings": {},
                "channel_bindings": {"community_hub": "700"}
            })),
            authority_binding_fingerprint: resource_binding_fingerprint_v2(
                &decode_resource_bindings(serde_json::json!({
                    "role_bindings": {},
                    "channel_bindings": {"community_hub": "700"}
                }))
                .unwrap(),
            )
            .into_string(),
            policy_revision: 1,
            required_approvals: 1,
            activation_ttl_seconds: 3600,
            authority_payload_digest: "d".repeat(64),
            database_now: now,
        }
    }

    fn scope() -> AuthorizedInstallationScopeV1 {
        AuthorizedInstallationScopeV1::from_fresh_authority(
            TenantId::parse("tenant-1").unwrap(),
            AutomationInstallationId::parse("installation-1").unwrap(),
            GuildId(901),
            UserId(100),
        )
    }

    #[test]
    fn evidence_is_bound_to_database_time_and_exact_scope() {
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 12, 0, 0).unwrap();
        let evidence = Evidence {
            tenant_id: TenantId::parse("tenant-1").unwrap(),
            installation_id: AutomationInstallationId::parse("installation-1").unwrap(),
            observed_at: now - TimeDelta::seconds(1),
            expires_at: now + TimeDelta::seconds(4),
            guild_owner: true,
            effective_permissions_bits: 0,
        };
        assert!(validate_evidence(
            &row(now),
            &scope(),
            &evidence,
            PostgresAuthorizedPromotionSnapshotsConfig::default()
        )
        .is_ok());
        let stale = Evidence {
            tenant_id: evidence.tenant_id.clone(),
            installation_id: evidence.installation_id.clone(),
            observed_at: now - TimeDelta::seconds(6),
            expires_at: now + TimeDelta::seconds(1),
            guild_owner: true,
            effective_permissions_bits: 0,
        };
        assert_eq!(
            validate_evidence(
                &row(now),
                &scope(),
                &stale,
                PostgresAuthorizedPromotionSnapshotsConfig::default()
            ),
            Err(PromotionAuthorityError::Forbidden)
        );
        let future = Evidence {
            tenant_id: evidence.tenant_id.clone(),
            installation_id: evidence.installation_id.clone(),
            observed_at: now + TimeDelta::milliseconds(1),
            expires_at: now + TimeDelta::seconds(5),
            guild_owner: true,
            effective_permissions_bits: 0,
        };
        assert_eq!(
            validate_evidence(
                &row(now),
                &scope(),
                &future,
                PostgresAuthorizedPromotionSnapshotsConfig::default()
            ),
            Err(PromotionAuthorityError::Forbidden)
        );
        let overlong = Evidence {
            tenant_id: evidence.tenant_id.clone(),
            installation_id: evidence.installation_id.clone(),
            observed_at: now - TimeDelta::seconds(1),
            expires_at: now + TimeDelta::seconds(5),
            guild_owner: true,
            effective_permissions_bits: 0,
        };
        assert_eq!(
            validate_evidence(
                &row(now),
                &scope(),
                &overlong,
                PostgresAuthorizedPromotionSnapshotsConfig::default()
            ),
            Err(PromotionAuthorityError::Forbidden)
        );
        let expired = Evidence {
            tenant_id: evidence.tenant_id,
            installation_id: evidence.installation_id,
            observed_at: now - TimeDelta::seconds(1),
            expires_at: now,
            guild_owner: true,
            effective_permissions_bits: 0,
        };
        assert_eq!(
            validate_evidence(
                &row(now),
                &scope(),
                &expired,
                PostgresAuthorizedPromotionSnapshotsConfig::default()
            ),
            Err(PromotionAuthorityError::Forbidden)
        );
    }

    #[test]
    fn scope_and_authority_revision_mismatches_fail_closed() {
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 12, 0, 0).unwrap();
        let mut mismatched_scope = row(now);
        mismatched_scope.session_tenant_id = "tenant-2".to_string();
        assert_eq!(
            validate_scope(&mismatched_scope, &scope()),
            Err(PromotionAuthorityError::ScopeMismatch)
        );
        let evidence = Evidence {
            tenant_id: TenantId::parse("tenant-1").unwrap(),
            installation_id: AutomationInstallationId::parse("installation-1").unwrap(),
            observed_at: now - TimeDelta::seconds(1),
            expires_at: now + TimeDelta::seconds(4),
            guild_owner: true,
            effective_permissions_bits: 0,
        };
        let mut mismatched_revision = row(now);
        mismatched_revision.current_authority_revision = 2;
        assert_eq!(
            validate_evidence(
                &mismatched_revision,
                &scope(),
                &evidence,
                PostgresAuthorizedPromotionSnapshotsConfig::default()
            ),
            Err(PromotionAuthorityError::ScopeMismatch)
        );
        let mut mismatched_digest = row(now);
        mismatched_digest.authority_payload_digest = "e".repeat(64);
        assert_eq!(
            validate_evidence(
                &mismatched_digest,
                &scope(),
                &evidence,
                PostgresAuthorizedPromotionSnapshotsConfig::default()
            ),
            Err(PromotionAuthorityError::ScopeMismatch)
        );
    }

    #[test]
    fn non_owner_requires_manager_permissions() {
        let now = Utc.with_ymd_and_hms(2026, 7, 19, 12, 0, 0).unwrap();
        let denied = Evidence {
            tenant_id: TenantId::parse("tenant-1").unwrap(),
            installation_id: AutomationInstallationId::parse("installation-1").unwrap(),
            observed_at: now - TimeDelta::seconds(1),
            expires_at: now + TimeDelta::seconds(4),
            guild_owner: false,
            effective_permissions_bits: Permissions::VIEW_CHANNEL.bits(),
        };
        assert_eq!(
            validate_evidence(
                &row(now),
                &scope(),
                &denied,
                PostgresAuthorizedPromotionSnapshotsConfig::default()
            ),
            Err(PromotionAuthorityError::Forbidden)
        );
        let manager = Evidence {
            effective_permissions_bits: Permissions::MANAGE_GUILD.bits(),
            ..denied
        };
        assert!(validate_evidence(
            &row(now),
            &scope(),
            &manager,
            PostgresAuthorizedPromotionSnapshotsConfig::default()
        )
        .is_ok());
    }

    #[test]
    fn configuration_bounds_database_and_decryption_work() {
        assert_eq!(
            PostgresAuthorizedPromotionSnapshotsConfig::new(
                Duration::ZERO,
                Duration::from_secs(5),
                1
            ),
            Err(AuthorizedSnapshotConfigError::InvalidStatementTimeout)
        );
        assert_eq!(
            PostgresAuthorizedPromotionSnapshotsConfig::new(
                Duration::from_secs(1),
                Duration::from_secs(61),
                1
            ),
            Err(AuthorizedSnapshotConfigError::InvalidFreshAuthorityLifetime)
        );
        assert_eq!(
            PostgresAuthorizedPromotionSnapshotsConfig::new(
                Duration::from_secs(1),
                Duration::from_secs(5),
                MAX_PLAINTEXT_BYTES + 1
            ),
            Err(AuthorizedSnapshotConfigError::InvalidPlaintextBound)
        );
    }
}
