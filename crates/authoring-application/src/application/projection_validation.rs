use authoring_promotion::approval_payload_digest_v1;

use crate::status::validate_exact_live;
use crate::{
    DeploymentOperationalObservationV2, DeploymentStatusObservationV1,
    DeploymentStatusProjectionV1, DeploymentStatusV1, ProductApplicationError,
    ProductApprovalPreviewObservationV1, ProductApprovalPreviewV1, ProductDecisionPhaseV1,
};

pub(super) fn validate_approval_phase(
    phase: &ProductDecisionPhaseV1,
) -> Result<(), ProductApplicationError> {
    if matches!(
        phase,
        ProductDecisionPhaseV1::PendingApproval | ProductDecisionPhaseV1::Approved
    ) {
        Ok(())
    } else {
        Err(ProductApplicationError::InvalidProjection)
    }
}

pub(super) fn validate_preview(
    scope: &crate::AuthorizedInstallationScopeV1,
    promotion: &crate::PromotionSelectorV1,
    preview: &ProductApprovalPreviewV1,
) -> Result<(), ProductApplicationError> {
    let payload = preview.payload();
    if preview.installation_id() != scope.installation_id()
        || preview.guild_id() != scope.guild_id()
        || payload.promotion_id != *promotion.promotion_id()
        || payload.authority.tenant_id != *scope.tenant_id()
        || payload.authority.installation_id != *scope.installation_id()
        || payload.authority.guild_id != scope.guild_id()
    {
        return Err(ProductApplicationError::InvalidProjection);
    }
    if let ProductDecisionPhaseV1::Applied { exact_deployment } = preview.phase() {
        if exact_deployment.installation_id() != scope.installation_id()
            || exact_deployment.promotion_id() != promotion.promotion_id()
        {
            return Err(ProductApplicationError::InvalidProjection);
        }
    }
    let digest = approval_payload_digest_v1(payload)
        .map_err(|_| ProductApplicationError::InvalidProjection)?;
    if digest.to_string() != preview.payload_digest().as_str() {
        return Err(ProductApplicationError::InvalidProjection);
    }
    Ok(())
}

pub(super) fn validate_preview_observation(
    scope: &crate::AuthorizedInstallationScopeV1,
    promotion: &crate::PromotionSelectorV1,
    observation: &ProductApprovalPreviewObservationV1,
) -> Result<(), ProductApplicationError> {
    validate_preview(scope, promotion, observation.preview())?;
    match observation.preview().phase() {
        ProductDecisionPhaseV1::PendingApproval | ProductDecisionPhaseV1::Approved
            if observation.observed_at() >= observation.activation_expires_at() =>
        {
            Err(ProductApplicationError::InvalidProjection)
        }
        ProductDecisionPhaseV1::Expired
            if observation.observed_at() < observation.activation_expires_at() =>
        {
            Err(ProductApplicationError::InvalidProjection)
        }
        _ => Ok(()),
    }
}

pub(super) fn deployment_status(
    exact_deployment: &crate::ExactDeploymentSelectorV1,
    status: DeploymentStatusProjectionV1,
) -> Result<DeploymentStatusV1, ProductApplicationError> {
    match status {
        DeploymentStatusProjectionV1::NotRequested => Ok(DeploymentStatusV1::NotRequested),
        DeploymentStatusProjectionV1::Pending => Ok(DeploymentStatusV1::Pending),
        DeploymentStatusProjectionV1::Failed {
            retryable,
            failure_code,
        } => Ok(DeploymentStatusV1::Failed {
            retryable,
            failure_code,
        }),
        DeploymentStatusProjectionV1::ExactLive(live)
            if live.exact_deployment() == exact_deployment =>
        {
            Ok(DeploymentStatusV1::Live {
                attestation_revision: live.attestation_revision(),
            })
        }
        DeploymentStatusProjectionV1::ExactLive(_) => {
            Err(ProductApplicationError::InvalidProjection)
        }
    }
}

pub(super) fn validate_runtime_projection(
    expected: &crate::ExactDeploymentSelectorV1,
    status: &DeploymentStatusProjectionV1,
) -> Result<(), ProductApplicationError> {
    match status {
        DeploymentStatusProjectionV1::Failed { failure_code, .. }
            if crate::DeploymentFailureCodeV1::parse(failure_code).is_err() =>
        {
            Err(ProductApplicationError::InvalidProjection)
        }
        DeploymentStatusProjectionV1::ExactLive(_) if !validate_exact_live(expected, status) => {
            Err(ProductApplicationError::InvalidProjection)
        }
        _ => Ok(()),
    }
}

pub(super) fn validate_runtime_observation(
    expected: &crate::ExactDeploymentSelectorV1,
    decision_observed_at: std::time::SystemTime,
    observation: &DeploymentStatusObservationV1,
) -> Result<(), ProductApplicationError> {
    if observation.observed_at() < decision_observed_at {
        return Err(ProductApplicationError::InvalidProjection);
    }
    validate_runtime_projection(expected, observation.projection())
}

pub(super) fn validate_runtime_operational_observation(
    expected: &crate::ExactDeploymentSelectorV1,
    decision_observed_at: std::time::SystemTime,
    observation: &DeploymentOperationalObservationV2,
) -> Result<(), ProductApplicationError> {
    validate_runtime_observation(expected, decision_observed_at, observation.base())
}
