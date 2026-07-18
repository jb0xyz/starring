use crate::facade::{valid_digest, valid_resource_id};
use crate::{
    ApplyView, ApprovalPreviewView, CsrfSecret, CurrentPrincipalView, DecisionView,
    DeploymentState, DeploymentView, ProductState, PromotionView,
};

pub(super) fn valid_current_principal(view: &CurrentPrincipalView) -> bool {
    valid_resource_id(&view.principal_id)
        && CsrfSecret::parse(&view.csrf_token).is_ok()
        && !view.display_name.is_empty()
        && view.display_name.len() <= 1_024
        && view.display_name.chars().count() <= 256
        && !view.display_name.chars().any(char::is_control)
}

pub(super) fn valid_promotion_view(view: &PromotionView, installation_id: &str) -> bool {
    view.installation_id == installation_id
        && valid_digest(&view.promotion_id)
        && view.revision > 0
        && valid_digest(&view.payload_digest)
        && (view.replayed || view.state == ProductState::PendingApproval)
}

pub(super) fn valid_decision_view(
    view: &DecisionView,
    installation_id: &str,
    promotion_id: &str,
) -> bool {
    view.installation_id == installation_id
        && view.promotion_id == promotion_id
        && view.revision > 0
}

pub(super) fn valid_approval_view(
    view: &DecisionView,
    installation_id: &str,
    promotion_id: &str,
) -> bool {
    valid_decision_view(view, installation_id, promotion_id) && view.state == ProductState::Approved
}

pub(super) fn valid_rejection_view(
    view: &DecisionView,
    installation_id: &str,
    promotion_id: &str,
) -> bool {
    valid_decision_view(view, installation_id, promotion_id) && view.state == ProductState::Rejected
}

pub(super) fn valid_preview_view(
    view: &ApprovalPreviewView,
    installation_id: &str,
    promotion_id: &str,
) -> bool {
    valid_decision_identity(
        &view.installation_id,
        &view.promotion_id,
        installation_id,
        promotion_id,
    ) && view.revision > 0
        && valid_digest(&view.payload_digest)
        && matches!(
            view.state,
            ProductState::PendingApproval | ProductState::Approved
        )
        && view.summary.target_version > 0
        && valid_digest(&view.summary.target_content_hash)
        && valid_digest(&view.summary.binding_fingerprint)
        && view.summary.required_approvals > 0
}

pub(super) fn valid_apply_view(
    view: &ApplyView,
    installation_id: &str,
    promotion_id: &str,
) -> bool {
    valid_decision_identity(
        &view.installation_id,
        &view.promotion_id,
        installation_id,
        promotion_id,
    ) && matches!(
        view.state,
        ProductState::RuntimePending | ProductState::Live
    )
}

pub(super) fn valid_deployment_view(
    view: &DeploymentView,
    installation_id: &str,
    promotion_id: &str,
) -> bool {
    if !valid_decision_identity(
        &view.installation_id,
        &view.promotion_id,
        installation_id,
        promotion_id,
    ) {
        return false;
    }
    let failure_code_valid = view.failure_code.as_ref().is_some_and(|code| {
        !code.is_empty()
            && code.len() <= 64
            && code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    });
    match view.state {
        DeploymentState::Failed => {
            failure_code_valid
                && view.attestation_revision.is_none()
                && view.last_serving_heartbeat.is_none()
                && view.serving_lease_expires_at.is_none()
        }
        DeploymentState::Live => {
            !view.retryable
                && view.failure_code.is_none()
                && view
                    .attestation_revision
                    .is_some_and(|revision| revision > 0)
                && view.last_serving_heartbeat.is_some_and(|heartbeat| {
                    heartbeat <= view.observed_at
                        && view.serving_lease_expires_at.is_some_and(|expires_at| {
                            expires_at > view.observed_at && heartbeat < expires_at
                        })
                })
        }
        DeploymentState::Pending => {
            view.failure_code.is_none()
                && view.attestation_revision.is_none()
                && view.last_serving_heartbeat.is_none()
                && view.serving_lease_expires_at.is_none()
        }
        DeploymentState::NotApplicable | DeploymentState::NotRequested => {
            !view.retryable
                && view.failure_code.is_none()
                && view.attestation_revision.is_none()
                && view.last_serving_heartbeat.is_none()
                && view.serving_lease_expires_at.is_none()
        }
    }
}

fn valid_decision_identity(
    actual_installation: &str,
    actual_promotion: &str,
    expected_installation: &str,
    expected_promotion: &str,
) -> bool {
    actual_installation == expected_installation && actual_promotion == expected_promotion
}
