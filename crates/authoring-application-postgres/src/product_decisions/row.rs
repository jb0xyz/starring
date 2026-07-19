use authoring_application::{
    ApprovalPayloadDigestV1, AuthorizedInstallationScopeV1, CapabilityV1,
    ExactDeploymentSelectorV1, FreshGuildAuthorityEvidence, ProductApprovalPreviewV1,
    ProductControlPortError, ProductDecisionPhaseV1, ProductDecisionProjectionV1,
    ProductRevisionV1, PromotionSelectorV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use authoring_promotion::{approval_payload_digest_v1, PromotionRecordV1, PromotionStageV1};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use serde_json::Value;
use sqlx::types::Json;

#[derive(sqlx::FromRow)]
pub(crate) struct ProductDecisionRow {
    pub activation_request_id: String,
    pub activation_tenant_id: String,
    pub activation_installation_id: String,
    pub activation_guild_id: String,
    pub activation_ruleset_key: String,
    pub activation_requester_id: String,
    pub activation_required_approvals: i32,
    pub activation_state: String,
    pub activation_created_at: DateTime<Utc>,
    pub activation_expires_at: DateTime<Utc>,
    pub activation_promotion_request_digest: String,
    pub activation_approval_payload_digest: String,
    pub activation_approval_context: Json<Value>,
    pub activation_product_revision: i64,
    pub approval_count: i64,
    pub promotion_tenant_id: String,
    pub promotion_stage: String,
    pub promotion_request_digest: String,
    pub promotion_record: Json<Value>,
    pub tenant_lifecycle_state: String,
    pub installation_application_id: String,
    pub installation_guild_id: String,
    pub installation_ruleset_key: String,
    pub installation_lifecycle_state: String,
    pub installation_current_authority_revision: i64,
    pub authority_binding_revision: i64,
    pub authority_binding_fingerprint: String,
    pub authority_policy_revision: i64,
    pub authority_required_approvals: i32,
    pub authority_activation_ttl_seconds: i64,
    pub authority_payload_digest: String,
    pub actor_discord_user_id: String,
    pub actor_disabled: bool,
    pub actor_session_revoked_at: Option<DateTime<Utc>>,
    pub actor_session_idle_expires_at: DateTime<Utc>,
    pub actor_session_absolute_expires_at: DateTime<Utc>,
    pub runtime_deployment_id: Option<String>,
    pub runtime_desired_target_digest: Option<String>,
    pub database_now: DateTime<Utc>,
}

pub(crate) struct ValidatedDecisionRow {
    pub preview: ProductApprovalPreviewV1,
    pub projection: ProductDecisionProjectionV1,
}

pub(crate) fn validate_decision_row(
    row: ProductDecisionRow,
    scope: &AuthorizedInstallationScopeV1,
    evidence: &FreshDiscordAuthorityEvidenceV1,
    promotion: &PromotionSelectorV1,
    expected_capability: CapabilityV1,
) -> Result<ValidatedDecisionRow, ProductControlPortError> {
    validate_authority(&row, scope, evidence, expected_capability)?;
    let record = serde_json::from_value::<PromotionRecordV1>(row.promotion_record.0.clone())
        .map_err(|_| invalid_persistence())?;
    record.validate().map_err(|_| invalid_persistence())?;
    validate_record_and_activation(&row, &record, promotion)?;
    let payload = record
        .product_approval_payload()
        .ok_or(ProductControlPortError::InvalidState)?;
    let payload_digest = approval_payload_digest_v1(&payload).map_err(|_| invalid_persistence())?;
    if payload_digest.as_str() != row.activation_approval_payload_digest {
        return Err(invalid_persistence());
    }
    let revision = u64::try_from(row.activation_product_revision)
        .ok()
        .and_then(|value| ProductRevisionV1::new(value).ok())
        .ok_or_else(invalid_persistence)?;
    let guild_id = parse_guild(&row.activation_guild_id)?;
    let phase = phase(&row, promotion)?;
    let payload_digest = ApprovalPayloadDigestV1::parse(payload_digest.as_str())
        .map_err(|_| invalid_persistence())?;
    let preview = ProductApprovalPreviewV1::from_server_projection(
        scope.installation_id().clone(),
        guild_id,
        payload,
        payload_digest,
        revision,
        phase.clone(),
    );
    let projection = ProductDecisionProjectionV1::from_server_projection(
        scope.tenant_id().clone(),
        scope.installation_id().clone(),
        guild_id,
        promotion.promotion_id().clone(),
        revision,
        phase,
    );
    Ok(ValidatedDecisionRow {
        preview,
        projection,
    })
}

fn validate_authority(
    row: &ProductDecisionRow,
    scope: &AuthorizedInstallationScopeV1,
    evidence: &FreshDiscordAuthorityEvidenceV1,
    expected_capability: CapabilityV1,
) -> Result<(), ProductControlPortError> {
    let observed_at = evidence.observed_at();
    let expires_at = evidence.expires_at();
    let evidence_matches = evidence.capability() == expected_capability
        && evidence.tenant_id() == scope.tenant_id()
        && evidence.installation_id() == scope.installation_id()
        && evidence.guild_id() == scope.guild_id()
        && evidence.acting_user_id() == scope.acting_user_id()
        && evidence.discord_application_id().get().to_string() == row.installation_application_id
        && evidence.installation_authority_revision().get().to_string()
            == row.installation_current_authority_revision.to_string()
        && evidence.installation_authority_digest() == row.authority_payload_digest
        && observed_at <= row.database_now
        && row.database_now < expires_at
        && expires_at <= observed_at + chrono::Duration::seconds(5);
    if !evidence_matches
        || row.tenant_lifecycle_state != "active"
        || row.installation_lifecycle_state != "active"
        || row.actor_disabled
        || row.actor_session_revoked_at.is_some()
        || row.database_now >= row.actor_session_idle_expires_at
        || row.database_now >= row.actor_session_absolute_expires_at
    {
        return Err(ProductControlPortError::InvalidState);
    }
    if row.activation_tenant_id != scope.tenant_id().as_str()
        || row.activation_installation_id != scope.installation_id().as_str()
        || row.activation_guild_id != scope.guild_id().to_string()
        || row.installation_guild_id != row.activation_guild_id
        || row.installation_ruleset_key != row.activation_ruleset_key
        || row.actor_discord_user_id != scope.acting_user_id().to_string()
    {
        return Err(ProductControlPortError::ScopeMismatch);
    }
    Ok(())
}

fn validate_record_and_activation(
    row: &ProductDecisionRow,
    record: &PromotionRecordV1,
    promotion: &PromotionSelectorV1,
) -> Result<(), ProductControlPortError> {
    if &record.id != promotion.promotion_id()
        || record.intent.authority.tenant_id.as_str() != row.promotion_tenant_id
        || record.intent.authority.installation_id.as_str() != row.activation_installation_id
        || record.intent.authority.guild_id.to_string() != row.activation_guild_id
        || record.intent.authority.ruleset_key.as_str() != row.activation_ruleset_key
        || record.request_digest.as_str() != row.promotion_request_digest
        || row.promotion_request_digest != row.activation_promotion_request_digest
        || row.activation_required_approvals != row.authority_required_approvals
    {
        return Err(invalid_persistence());
    }
    let activation = match &record.stage {
        PromotionStageV1::ActivationPending { activation, .. }
        | PromotionStageV1::Expired { activation, .. } => activation,
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
            return Err(ProductControlPortError::InvalidState)
        }
    };
    let expected_context =
        serde_json::to_value(&activation.approval_context).map_err(|_| invalid_persistence())?;
    if row.activation_approval_context["context"] != expected_context
        || i64::try_from(activation.approval_context.binding.revision.get()).ok()
            != Some(row.authority_binding_revision)
        || activation.approval_context.binding.fingerprint.as_str()
            != row.authority_binding_fingerprint
        || i64::try_from(activation.approval_context.policy.revision.get()).ok()
            != Some(row.authority_policy_revision)
        || i32::try_from(activation.approval_context.policy.required_approvals.get()).ok()
            != Some(row.authority_required_approvals)
        || i64::try_from(activation.approval_context.policy.ttl_seconds.get()).ok()
            != Some(row.authority_activation_ttl_seconds)
        || activation.request_id.as_str() != row.activation_request_id
        || activation.target.guild_id.to_string() != row.activation_guild_id
        || activation.target.ruleset_key.as_str() != row.activation_ruleset_key
        || activation.requester.to_string() != row.activation_requester_id
        || activation.required_approvals.get().to_string()
            != row.activation_required_approvals.to_string()
        || activation.approval_context.approval_payload_digest.as_str()
            != row.activation_approval_payload_digest
        || record.stage_name() != row.promotion_stage
    {
        return Err(invalid_persistence());
    }
    if row.approval_count < 0
        || row.approval_count > i64::from(row.activation_required_approvals)
        || row.activation_created_at >= row.activation_expires_at
    {
        return Err(invalid_persistence());
    }
    Ok(())
}

fn phase(
    row: &ProductDecisionRow,
    promotion: &PromotionSelectorV1,
) -> Result<ProductDecisionPhaseV1, ProductControlPortError> {
    if matches!(row.activation_state.as_str(), "pending" | "approved")
        && row.activation_expires_at <= row.database_now
    {
        return Ok(ProductDecisionPhaseV1::Expired);
    }
    match row.activation_state.as_str() {
        "pending" if row.approval_count < i64::from(row.activation_required_approvals) => {
            Ok(ProductDecisionPhaseV1::PendingApproval)
        }
        "approved" if row.approval_count >= i64::from(row.activation_required_approvals) => {
            Ok(ProductDecisionPhaseV1::Approved)
        }
        "applying" => Ok(ProductDecisionPhaseV1::Applying),
        "applied" => {
            let deployment_id = row
                .runtime_deployment_id
                .as_ref()
                .ok_or_else(invalid_persistence)?;
            let target_digest = row
                .runtime_desired_target_digest
                .as_ref()
                .ok_or_else(invalid_persistence)?;
            let exact = ExactDeploymentSelectorV1::from_server_projection(
                authoring_promotion::AutomationInstallationId::parse(
                    &row.activation_installation_id,
                )
                .map_err(|_| invalid_persistence())?,
                promotion.promotion_id().clone(),
                deployment_id,
                target_digest,
            )
            .map_err(|_| invalid_persistence())?;
            Ok(ProductDecisionPhaseV1::Applied {
                exact_deployment: exact,
            })
        }
        "rejected" => Ok(ProductDecisionPhaseV1::Rejected),
        "expired" => Ok(ProductDecisionPhaseV1::Expired),
        "superseded" => Ok(ProductDecisionPhaseV1::Superseded),
        "withdrawn" => Ok(ProductDecisionPhaseV1::Withdrawn),
        _ => Err(invalid_persistence()),
    }
}

fn parse_guild(value: &str) -> Result<GuildId, ProductControlPortError> {
    let parsed = value
        .parse::<GuildId>()
        .map_err(|_| invalid_persistence())?;
    if parsed.0 == 0 || parsed.to_string() != value {
        return Err(invalid_persistence());
    }
    Ok(parsed)
}

fn invalid_persistence() -> ProductControlPortError {
    ProductControlPortError::Backend(
        "persisted product decision violates its integrity contract".to_string(),
    )
}

trait PromotionStageName {
    fn stage_name(&self) -> &'static str;
}

impl PromotionStageName for PromotionRecordV1 {
    fn stage_name(&self) -> &'static str {
        match &self.stage {
            PromotionStageV1::Prepared => "prepared",
            PromotionStageV1::Published { .. } => "published",
            PromotionStageV1::ActivationPending { .. } => "activation_pending",
            PromotionStageV1::Expired { .. } => "expired",
        }
    }
}

pub(crate) fn approval_phase_from_database(
    state: &str,
) -> Result<ProductDecisionPhaseV1, ProductControlPortError> {
    match state {
        "pending" => Ok(ProductDecisionPhaseV1::PendingApproval),
        "approved" => Ok(ProductDecisionPhaseV1::Approved),
        _ => Err(invalid_persistence()),
    }
}

pub(crate) fn approval_revision_from_database(
    revision: i64,
) -> Result<ProductRevisionV1, ProductControlPortError> {
    u64::try_from(revision)
        .ok()
        .and_then(|value| ProductRevisionV1::new(value).ok())
        .ok_or_else(invalid_persistence)
}

pub(crate) fn approval_guild_from_database(
    guild_id: &str,
) -> Result<GuildId, ProductControlPortError> {
    parse_guild(guild_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_result_accepts_only_product_approval_phases() {
        assert_eq!(
            approval_phase_from_database("pending").unwrap(),
            ProductDecisionPhaseV1::PendingApproval
        );
        assert_eq!(
            approval_phase_from_database("approved").unwrap(),
            ProductDecisionPhaseV1::Approved
        );
        assert!(approval_phase_from_database("applied").is_err());
    }

    #[test]
    fn persisted_identities_are_canonical() {
        assert_eq!(approval_revision_from_database(2).unwrap().get(), 2);
        assert_eq!(approval_guild_from_database("42").unwrap(), GuildId(42));
        assert!(approval_revision_from_database(0).is_err());
        assert!(approval_guild_from_database("01").is_err());
    }
}
