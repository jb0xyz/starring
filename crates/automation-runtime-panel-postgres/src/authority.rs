use automation_ruleset::content_hash;
use automation_ruleset::{
    RuleSetContentHash, RuleSetKey, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_controller::RuntimeExecutionGuardV1;
use automation_runtime_convergence::{
    ControllerId, DeploymentId, DeploymentRevision, FencingToken, InstallationId,
    RuntimeDeploymentPhaseV1, RuntimeGeneration, TenantId,
};
use automation_runtime_convergence_postgres::RuntimeExactTargetV1;
use discord_model::GuildId;
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingFingerprint};

use crate::RuntimePanelPersistenceErrorV1;

macro_rules! bind_runtime_panel_authority {
    ($query:expr, $authority:expr, $session_id:expr) => {{
        let authority = $authority;
        $query
            .bind(authority.tenant_id.as_str())
            .bind(authority.installation_id.as_str())
            .bind(authority.deployment_id.as_str())
            .bind(authority.deployment_revision.get() as i64)
            .bind(authority.controller_id.as_str())
            .bind(authority.controller_fencing_token.get() as i64)
            .bind(i64::from(authority.convergence_attempt))
            .bind(authority.runtime_generation.get() as i64)
            .bind(authority.guild_id.to_string())
            .bind(authority.ruleset_key.as_str())
            .bind(i64::from(authority.target_version.get()))
            .bind(authority.content_hash.to_hex())
            .bind(authority.binding_revision as i64)
            .bind(authority.binding_fingerprint.as_str())
            .bind(authority.installation_authority_revision as i64)
            .bind(authority.current_authority_revision as i64)
            .bind($session_id.as_str())
    }};
}

pub(crate) use bind_runtime_panel_authority;

pub(crate) struct RuntimePanelAuthorityV1 {
    pub(crate) tenant_id: TenantId,
    pub(crate) installation_id: InstallationId,
    pub(crate) deployment_id: DeploymentId,
    pub(crate) deployment_revision: DeploymentRevision,
    pub(crate) controller_id: ControllerId,
    pub(crate) controller_fencing_token: FencingToken,
    pub(crate) convergence_attempt: u32,
    pub(crate) runtime_generation: RuntimeGeneration,
    pub(crate) guild_id: GuildId,
    pub(crate) ruleset_key: RuleSetKey,
    pub(crate) target_version: RuleSetVersionId,
    pub(crate) content_hash: RuleSetContentHash,
    pub(crate) binding_revision: u64,
    pub(crate) binding_fingerprint: ResourceBindingFingerprint,
    pub(crate) installation_authority_revision: u64,
    pub(crate) current_authority_revision: u64,
}

impl RuntimePanelAuthorityV1 {
    pub(crate) fn new(
        guard: RuntimeExecutionGuardV1,
        exact: RuntimeExactTargetV1,
    ) -> Result<Self, RuntimePanelPersistenceErrorV1> {
        let snapshot = &exact.snapshot;
        let target = &snapshot.target;
        let artifact = &exact.artifact;
        let lease = snapshot
            .controller_lease
            .as_ref()
            .ok_or(RuntimePanelPersistenceErrorV1::InvalidAuthority)?;
        let calculated_hash = content_hash(artifact.schema_version, &artifact.definition)
            .map_err(|_| RuntimePanelPersistenceErrorV1::InvalidAuthority)?;
        let calculated_fingerprint = resource_binding_fingerprint_v2(&exact.bindings);
        if !guard.scope.matches(&snapshot.identity)
            || guard.expected_revision != snapshot.revision
            || guard.controller_id != lease.controller_id
            || guard.fencing_token != lease.fencing_token
            || guard.runtime_generation != snapshot.runtime_generation
            || !matches!(snapshot.phase, RuntimeDeploymentPhaseV1::ReconcilingPanels)
            || lease.acquired_at >= lease.expires_at
            || target.guild_id.0 == 0
            || artifact.guild_id != target.guild_id
            || artifact.ruleset_key != target.ruleset_key
            || artifact.version != target.version
            || artifact.schema_version != CURRENT_RULESET_SCHEMA_VERSION
            || artifact.content_hash != target.content_hash
            || calculated_hash != target.content_hash
            || automation_core::validate_structural(&artifact.definition).is_err()
            || calculated_fingerprint != target.binding_fingerprint
            || exact.installation_authority_revision == 0
            || exact.current_authority_revision == 0
            || [
                guard.expected_revision.get(),
                guard.fencing_token.get(),
                guard.runtime_generation.get(),
                target.binding_revision.get(),
                exact.installation_authority_revision,
                exact.current_authority_revision,
            ]
            .into_iter()
            .any(|value| i64::try_from(value).is_err())
        {
            return Err(RuntimePanelPersistenceErrorV1::InvalidAuthority);
        }
        Ok(Self {
            tenant_id: guard.scope.tenant_id,
            installation_id: guard.scope.installation_id,
            deployment_id: guard.scope.deployment_id,
            deployment_revision: guard.expected_revision,
            controller_id: guard.controller_id,
            controller_fencing_token: guard.fencing_token,
            convergence_attempt: guard.convergence_attempt.get(),
            runtime_generation: guard.runtime_generation,
            guild_id: target.guild_id,
            ruleset_key: target.ruleset_key.clone(),
            target_version: target.version,
            content_hash: target.content_hash,
            binding_revision: target.binding_revision.get(),
            binding_fingerprint: target.binding_fingerprint.clone(),
            installation_authority_revision: exact.installation_authority_revision,
            current_authority_revision: exact.current_authority_revision,
        })
    }
}
