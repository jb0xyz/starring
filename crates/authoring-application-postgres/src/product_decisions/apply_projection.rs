use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};

use authoring_application::{
    AuthorizedApplyProductV1, FreshGuildAuthorityEvidence, ProductCandidateErrorCodeV1,
    ProductControlPortError,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_ruleset::{
    RuleSetContentHash, RuleSetKey, RuleSetSchemaVersion, RuleSetVersion, RuleSetVersionId,
    RuleSetVersionIdentity,
};
use automation_ruleset_activation::{
    assess_product_target_v1, check_product_readiness_v1, product_approval_context_digest_v1,
    validate_product_target_v1, ActivationApprovalContextV1, ActivationEnvironment,
    ActivationRequestId, ActivationTarget, ObservedActive, ProductPreflightErrorCodeV1,
    ProductReadinessAssessmentV1, ProductTargetAssessmentV1,
};
use automation_ruleset_readiness::{
    build_readiness_context, GuildRoleHierarchyV1, GuildRoleStateV1,
};
use automation_runtime_convergence::{
    ActivationRequestId as RuntimeActivationRequestId, BindingRevision, DeploymentId,
    InstallationId, PromotionId, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_convergence_postgres::{
    prepare_requested_deployment_v1, EnqueueDeploymentV1, PreparedRequestedDeploymentV1,
};
use chrono::{DateTime, Utc};
use discord_model::{GuildId, UserId};
use resource_resolution::{
    resource_binding_fingerprint_v2, ResourceBindingFingerprint, ResourceBindingMap,
};
use serde::Deserialize;
use serde_json::Value;

use super::digest::ApplyDigests;
use crate::bindings::decode_resource_bindings;

pub(super) struct PreparedProductApplyV1 {
    pub deployment: PreparedRequestedDeploymentV1,
    pub activation_notices: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedApplyProjectionV1 {
    version: u32,
    requested_at: DateTime<Utc>,
    operation: LockedApplyOperationV1,
    server: LockedApplyServerV1,
    active: Option<RuleSetVersionIdentity>,
    target_is_active: bool,
    runtime_generation: RuntimeGeneration,
    previous_runtime: Option<RuntimeProcessIdentityV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedApplyOperationV1 {
    endpoint_domain: String,
    semantic_request_digest: String,
    request_id: String,
    receipt_id: String,
    audit_event_id: String,
    apply_attempt_id: String,
    deployment_id: DeploymentId,
    product_session_binding_v1: String,
    session_subject_binding_v1: String,
    active_idempotency_key_digest: String,
    idempotency_key_digest_candidates: Vec<String>,
    idempotency_digest_key_id_candidates: Vec<String>,
    idempotency_digest_key_fingerprint_candidates: Vec<String>,
    idempotency_digest_key_id: String,
    authority_observation_digest: String,
    authority_observed_at: DateTime<Utc>,
    authority_expires_at: DateTime<Utc>,
    effective_permission_bits: String,
    guild_owner: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedApplyServerV1 {
    scope: LockedApplyScopeV1,
    activation: LockedApplyActivationV1,
    authority: LockedApplyAuthorityV1,
    target: LockedApplyTargetV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedApplyScopeV1 {
    tenant_id: TenantId,
    installation_id: InstallationId,
    promotion_id: PromotionId,
    principal_id: String,
    acting_user_id: UserId,
    discord_application_id: String,
    guild_id: GuildId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedApplyActivationV1 {
    request_id: RuntimeActivationRequestId,
    product_revision: u64,
    state: String,
    requester_id: UserId,
    required_approvals: NonZeroU32,
    approval_count: u64,
    expires_at: DateTime<Utc>,
    approval_payload_digest: String,
    approval_context_digest: String,
    approval_context: ActivationApprovalContextV1,
    observed_active_version: Option<RuleSetVersionId>,
    observed_active_hash: Option<RuleSetContentHash>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedApplyAuthorityV1 {
    revision: NonZeroU64,
    payload_digest: String,
    binding_revision: BindingRevision,
    binding_fingerprint: ResourceBindingFingerprint,
    policy_revision: NonZeroU64,
    required_approvals: NonZeroU32,
    activation_ttl_seconds: NonZeroU64,
    resource_bindings: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedApplyTargetV1 {
    guild_id: GuildId,
    ruleset_key: RuleSetKey,
    version: RuleSetVersionId,
    content_hash: RuleSetContentHash,
    schema_version: RuleSetSchemaVersion,
    definition: Value,
    created_by: UserId,
}

pub(super) fn prepare_product_apply_v1(
    value: Value,
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
) -> Result<PreparedProductApplyV1, ProductControlPortError> {
    let projection = serde_json::from_value::<LockedApplyProjectionV1>(value)
        .map_err(|_| invalid_projection())?;
    validate_operation(&projection, request, digests)?;
    let bindings = validate_server(&projection, request)?;
    let environment = build_environment(
        request,
        bindings,
        projection.server.authority.binding_revision.get(),
    )?;
    let definition = serde_json::from_value(projection.server.target.definition.clone())
        .map_err(|_| candidate(ProductCandidateErrorCodeV1::StructurallyInvalid))?;
    let artifact = RuleSetVersion {
        guild_id: projection.server.target.guild_id,
        ruleset_key: projection.server.target.ruleset_key.clone(),
        version: projection.server.target.version,
        schema_version: projection.server.target.schema_version,
        definition,
        content_hash: projection.server.target.content_hash,
        created_by: projection.server.target.created_by,
    };
    let activation_target = ActivationTarget {
        guild_id: artifact.guild_id,
        ruleset_key: artifact.ruleset_key.clone(),
        version: artifact.version,
        content_hash: artifact.content_hash,
    };
    let validated =
        validate_product_target_v1(&activation_target, &artifact).map_err(map_candidate_error)?;
    let context = product_context(&projection.server.activation.approval_context)?;
    let target = match assess_product_target_v1(validated, context, projection.active.as_ref()) {
        ProductTargetAssessmentV1::Ready(target) => target,
        ProductTargetAssessmentV1::Superseded { .. } => return Err(invalid_projection()),
    };
    let ready = match check_product_readiness_v1(target, &artifact, &environment)
        .map_err(map_candidate_error)?
    {
        ProductReadinessAssessmentV1::Ready(ready) => ready,
        ProductReadinessAssessmentV1::Superseded { .. } => return Err(invalid_projection()),
    };
    if ready.target_is_active() != projection.target_is_active {
        return Err(invalid_projection());
    }
    let activation_notices =
        serde_json::to_value(ready.into_activation_notices()).map_err(|_| invalid_projection())?;
    validate_activation_notices(&activation_notices)?;
    let deployment = prepare_requested_deployment_v1(
        EnqueueDeploymentV1 {
            identity: RuntimeDeploymentIdentityV1 {
                deployment_id: projection.operation.deployment_id,
                tenant_id: projection.server.scope.tenant_id,
                installation_id: projection.server.scope.installation_id,
                promotion_id: projection.server.scope.promotion_id,
                activation_request_id: projection.server.activation.request_id,
            },
            target: RuntimeDeploymentTargetV1 {
                guild_id: projection.server.target.guild_id,
                ruleset_key: projection.server.target.ruleset_key,
                version: projection.server.target.version,
                content_hash: projection.server.target.content_hash,
                binding_revision: projection.server.authority.binding_revision,
                binding_fingerprint: projection.server.authority.binding_fingerprint,
            },
            runtime_generation: projection.runtime_generation,
            previous_runtime: projection.previous_runtime,
            installation_authority_revision: projection.server.authority.revision.get(),
        },
        projection.requested_at,
    )
    .map_err(|_| invalid_projection())?;
    Ok(PreparedProductApplyV1 {
        deployment,
        activation_notices,
    })
}

fn validate_operation(
    projection: &LockedApplyProjectionV1,
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
) -> Result<(), ProductControlPortError> {
    let operation = &projection.operation;
    let evidence = request.evidence();
    if projection.version != 1
        || operation.endpoint_domain != "product_apply_v1"
        || operation.semantic_request_digest != digests.semantic_request
        || operation.request_id != request.request_id().as_str()
        || operation.receipt_id != digests.receipt_id
        || operation.audit_event_id != digests.audit_event_id
        || operation.apply_attempt_id != digests.apply_attempt_id
        || operation.deployment_id.as_str() != digests.deployment_id
        || operation.active_idempotency_key_digest != digests.active_idempotency
        || operation.idempotency_key_digest_candidates != digests.idempotency_candidates
        || operation.idempotency_digest_key_id_candidates != digests.idempotency_candidate_key_ids
        || operation.idempotency_digest_key_fingerprint_candidates
            != digests.idempotency_candidate_key_fingerprints
        || operation.idempotency_digest_key_id != digests.active_key_id
        || operation.authority_observation_digest != evidence.observation_digest()
        || operation.authority_observed_at != evidence.observed_at()
        || operation.authority_expires_at != evidence.expires_at()
        || operation.effective_permission_bits != evidence.effective_permissions_bits().to_string()
        || operation.guild_owner != evidence.guild_owner()
        || !is_lower_hex(&operation.product_session_binding_v1, 32)
        || !is_lower_hex(&operation.session_subject_binding_v1, 32)
    {
        return Err(invalid_projection());
    }
    Ok(())
}

fn validate_server(
    projection: &LockedApplyProjectionV1,
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<ResourceBindingMap, ProductControlPortError> {
    let scope = &projection.server.scope;
    let activation = &projection.server.activation;
    let authority = &projection.server.authority;
    let target = &projection.server.target;
    let evidence = request.evidence();
    if scope.tenant_id.as_str() != request.scope().tenant_id().as_str()
        || scope.installation_id.as_str() != request.scope().installation_id().as_str()
        || scope.promotion_id.as_str() != request.command().promotion.promotion_id().as_str()
        || scope.principal_id != request.actor().principal_id().as_str()
        || scope.acting_user_id != request.scope().acting_user_id()
        || scope.discord_application_id != evidence.discord_application_id().get().to_string()
        || scope.guild_id != request.scope().guild_id()
        || activation.product_revision != request.command().expected_revision.get()
        || activation.state != "approved"
        || activation.approval_count < u64::from(activation.required_approvals.get())
        || activation.expires_at <= projection.requested_at
        || activation.approval_payload_digest != request.command().expected_payload_digest.as_str()
        || authority.revision != evidence.installation_authority_revision()
        || authority.payload_digest != evidence.installation_authority_digest()
        || target.guild_id != request.scope().guild_id()
        || target.guild_id != scope.guild_id
    {
        return Err(invalid_projection());
    }
    let bindings = decode_resource_bindings(authority.resource_bindings.clone())
        .map_err(|_| invalid_projection())?;
    if resource_binding_fingerprint_v2(&bindings) != authority.binding_fingerprint {
        return Err(invalid_projection());
    }
    let context = product_context(&activation.approval_context)?;
    let request_id = ActivationRequestId::parse(activation.request_id.as_str())
        .map_err(|_| invalid_projection())?;
    let activation_target = ActivationTarget {
        guild_id: target.guild_id,
        ruleset_key: target.ruleset_key.clone(),
        version: target.version,
        content_hash: target.content_hash,
    };
    let observed_active = match (
        activation.observed_active_version,
        activation.observed_active_hash,
    ) {
        (Some(version), Some(content_hash)) => Some(ObservedActive {
            version,
            content_hash,
        }),
        (None, None) => None,
        _ => return Err(invalid_projection()),
    };
    if context.promotion_id.as_str() != scope.promotion_id.as_str()
        || context.approval_payload_digest.as_str() != activation.approval_payload_digest
        || context.approval_context_digest.as_str() != activation.approval_context_digest
        || !context.binding.validate(target.guild_id)
        || !context.policy.validate()
        || context.binding.revision.get() != authority.binding_revision.get()
        || context.policy.revision != authority.policy_revision
        || context.policy.required_approvals != authority.required_approvals
        || context.policy.ttl_seconds != authority.activation_ttl_seconds
        || activation.required_approvals != context.policy.required_approvals
        || observed_active != context.baseline.as_observed()
        || product_approval_context_digest_v1(
            &request_id,
            &activation_target,
            activation.requester_id,
            context,
        ) != context.approval_context_digest
    {
        return Err(invalid_projection());
    }
    Ok(bindings)
}

fn build_environment(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    bindings: ResourceBindingMap,
    binding_revision: u64,
) -> Result<ActivationEnvironment, ProductControlPortError> {
    let runtime = request
        .evidence()
        .apply_runtime_environment()
        .ok_or(ProductControlPortError::InvalidState)?;
    if runtime.guild_id() != request.scope().guild_id() {
        return Err(ProductControlPortError::ScopeMismatch);
    }
    let (guild_capabilities, role_permissions) = build_readiness_context(
        runtime.guild_id(),
        &bindings,
        runtime.guild_role_permissions(),
        runtime.bot_role_ids(),
    )
    .map_err(|_| candidate(ProductCandidateErrorCodeV1::RoleHierarchyIncomplete))?;
    let roles = runtime
        .guild_roles()
        .iter()
        .map(|(role_id, role)| {
            (
                *role_id,
                GuildRoleStateV1 {
                    position: role.position,
                    managed: role.managed,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let role_hierarchy =
        GuildRoleHierarchyV1::new(runtime.guild_id(), roles, runtime.bot_role_ids().to_vec())
            .map_err(|_| candidate(ProductCandidateErrorCodeV1::RoleHierarchyIncomplete))?;
    Ok(ActivationEnvironment {
        binding_revision: NonZeroU64::new(binding_revision),
        bindings,
        guild_capabilities,
        role_permissions,
        role_hierarchy: Some(role_hierarchy),
    })
}

fn product_context(
    context: &ActivationApprovalContextV1,
) -> Result<&automation_ruleset_activation::ProductApprovalContextV1, ProductControlPortError> {
    match context {
        ActivationApprovalContextV1::ProductAuthoring { context } => Ok(context),
        ActivationApprovalContextV1::LegacyManual => Err(invalid_projection()),
    }
}

fn validate_activation_notices(value: &Value) -> Result<(), ProductControlPortError> {
    let values = value.as_array().ok_or_else(invalid_projection)?;
    let encoded = serde_json::to_vec(value).map_err(|_| invalid_projection())?;
    if values.len() > 128
        || encoded.len() > 16_384
        || values.iter().any(|value| {
            value
                .as_str()
                .is_none_or(|value| value.chars().count() > 1_024)
        })
    {
        return Err(candidate(ProductCandidateErrorCodeV1::StructurallyInvalid));
    }
    Ok(())
}

fn map_candidate_error(
    error: automation_ruleset_activation::ProductPreflightErrorV1,
) -> ProductControlPortError {
    candidate(match error.code() {
        ProductPreflightErrorCodeV1::TargetCorrupt => ProductCandidateErrorCodeV1::TargetCorrupt,
        ProductPreflightErrorCodeV1::BindingRevisionUnavailable => {
            ProductCandidateErrorCodeV1::BindingRevisionUnavailable
        }
        ProductPreflightErrorCodeV1::UnsupportedSchema => {
            ProductCandidateErrorCodeV1::UnsupportedSchema
        }
        ProductPreflightErrorCodeV1::StructurallyInvalid => {
            ProductCandidateErrorCodeV1::StructurallyInvalid
        }
        ProductPreflightErrorCodeV1::HashComputationFailed => {
            ProductCandidateErrorCodeV1::HashComputationFailed
        }
        ProductPreflightErrorCodeV1::HashMismatch => ProductCandidateErrorCodeV1::HashMismatch,
        ProductPreflightErrorCodeV1::BindingInvalid => ProductCandidateErrorCodeV1::BindingInvalid,
        ProductPreflightErrorCodeV1::BlockingPolicy => ProductCandidateErrorCodeV1::BlockingPolicy,
        ProductPreflightErrorCodeV1::MissingCapabilities => {
            ProductCandidateErrorCodeV1::MissingCapabilities
        }
        ProductPreflightErrorCodeV1::RoleHierarchyUnavailable => {
            ProductCandidateErrorCodeV1::RoleHierarchyUnavailable
        }
        ProductPreflightErrorCodeV1::RoleHierarchyIncomplete => {
            ProductCandidateErrorCodeV1::RoleHierarchyIncomplete
        }
        ProductPreflightErrorCodeV1::RoleUnmanageable => {
            ProductCandidateErrorCodeV1::RoleUnmanageable
        }
    })
}

fn candidate(code: ProductCandidateErrorCodeV1) -> ProductControlPortError {
    ProductControlPortError::InvalidServerCandidate(code)
}

fn invalid_projection() -> ProductControlPortError {
    ProductControlPortError::Backend(
        "product apply lock returned an invalid server projection".to_string(),
    )
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
