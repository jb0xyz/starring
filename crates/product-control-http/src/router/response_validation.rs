use crate::facade::{valid_digest, valid_resource_id};
use crate::{
    ApplyView, ApprovalPreviewView, AuthoringSessionViewV1, AuthoringTurnDispositionV1,
    AuthoringTurnViewV1, CurrentPrincipalView, DecisionView, DeploymentOperationalStateV2,
    DeploymentOperationalViewV2, DeploymentOperatorActionV2, DeploymentRetryStateV2,
    DeploymentRuntimePhaseV2, DeploymentServingFreshnessStateV2, DeploymentState, DeploymentView,
    LifecycleCancellationView, ProductState, PromotionView,
};

const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_AUTHORING_RESPONSE_BYTES: usize = 384 * 1_024;

pub(super) fn valid_authoring_turn_view(
    view: &AuthoringTurnViewV1,
    session_id: &str,
    expected_generation: u64,
) -> bool {
    if view.session_id != session_id
        || !valid_resource_id(&view.session_id)
        || view.projection.validate_preview_integrity().is_err()
        || !bounded_authoring_response(view)
    {
        return false;
    }
    let durable = !matches!(
        view.projection.state(),
        authoring_application::SafeAuthoringTurnStateV1::Unsupported
            | authoring_application::SafeAuthoringTurnStateV1::Rejected
    );
    match view.disposition {
        Some(AuthoringTurnDispositionV1::Created)
        | Some(AuthoringTurnDispositionV1::ExactReplay) => {
            durable
                && expected_generation
                    .checked_add(1)
                    .filter(|generation| *generation <= MAX_SAFE_JSON_INTEGER)
                    == view.generation
        }
        None => {
            view.generation.is_none()
                && matches!(
                    view.projection.state(),
                    authoring_application::SafeAuthoringTurnStateV1::Unsupported
                        | authoring_application::SafeAuthoringTurnStateV1::Rejected
                )
        }
    }
}

pub(super) fn valid_authoring_session_view(
    view: &AuthoringSessionViewV1,
    session_id: &str,
) -> bool {
    view.session_id == session_id
        && valid_resource_id(&view.session_id)
        && (1..=MAX_SAFE_JSON_INTEGER).contains(&view.observed_generation)
        && !matches!(
            view.projection.state(),
            authoring_application::SafeAuthoringTurnStateV1::Unsupported
                | authoring_application::SafeAuthoringTurnStateV1::Rejected
        )
        && view.projection.validate_preview_integrity().is_ok()
        && bounded_authoring_response(view)
}

fn bounded_authoring_response<T: serde::Serialize>(view: &T) -> bool {
    serde_json::to_vec(view).is_ok_and(|body| body.len() <= MAX_AUTHORING_RESPONSE_BYTES)
}

pub(super) fn valid_current_principal(view: &CurrentPrincipalView) -> bool {
    valid_resource_id(&view.principal_id)
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
    valid_decision_view(view, installation_id, promotion_id)
        && matches!(
            view.state,
            ProductState::PendingApproval | ProductState::Approved
        )
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

pub(super) fn valid_lifecycle_cancellation_view(
    view: &LifecycleCancellationView,
    installation_id: &str,
    promotion_id: &str,
) -> bool {
    valid_decision_identity(
        &view.installation_id,
        &view.promotion_id,
        installation_id,
        promotion_id,
    ) && view.revision > 0
        && view.state == ProductState::Approved
        && valid_runtime_id(&view.drain_intent_id)
        && valid_runtime_id(&view.product_operation_id)
        && view.drain_intent_id != view.product_operation_id
        && view.source_intent_revision > 0
        && view
            .source_intent_revision
            .checked_add(1)
            .is_some_and(|value| value == view.terminal_intent_revision)
        && valid_digest(&view.terminal_state_digest)
        && view.source_runtime_deployment_revision > 0
        && view
            .source_runtime_deployment_revision
            .checked_add(1)
            .is_some_and(|value| value == view.resulting_runtime_deployment_revision)
        && view.source_slot_writer_epoch > 0
        && view
            .source_slot_writer_epoch
            .checked_add(1)
            .is_some_and(|value| value == view.successor_slot_writer_epoch)
}

fn valid_runtime_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

pub(super) fn valid_deployment_operational_view_v2(
    view: &DeploymentOperationalViewV2,
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
    match view.state {
        DeploymentOperationalStateV2::NotApplicable => view.runtime.is_none(),
        DeploymentOperationalStateV2::Pending => view.runtime.as_ref().is_some_and(|runtime| {
            valid_runtime_common(view, runtime)
                && runtime.failure.is_none()
                && runtime.retry.is_none()
                && runtime.operator_action.is_none()
                && matches!(
                    runtime.phase,
                    DeploymentRuntimePhaseV2::Requested
                        | DeploymentRuntimePhaseV2::PreflightReady
                        | DeploymentRuntimePhaseV2::DrainRequested
                        | DeploymentRuntimePhaseV2::Drained
                        | DeploymentRuntimePhaseV2::ActivationApplying
                        | DeploymentRuntimePhaseV2::RuntimeReady
                        | DeploymentRuntimePhaseV2::ReconcilingPanels
                        | DeploymentRuntimePhaseV2::AwaitingGatewayReady
                        | DeploymentRuntimePhaseV2::Live
                )
                && valid_serving(runtime, view.state)
        }),
        DeploymentOperationalStateV2::Failed => view.runtime.as_ref().is_some_and(|runtime| {
            valid_runtime_common(view, runtime)
                && runtime
                    .failure
                    .as_ref()
                    .is_some_and(|failure| valid_failure(runtime, failure))
                && valid_serving(runtime, view.state)
        }),
        DeploymentOperationalStateV2::Live => view.runtime.as_ref().is_some_and(|runtime| {
            valid_runtime_common(view, runtime)
                && runtime.phase == DeploymentRuntimePhaseV2::Live
                && runtime.current_attempt > 0
                && runtime.failure.is_none()
                && runtime.retry.is_none()
                && runtime.operator_action.is_none()
                && valid_serving(runtime, view.state)
        }),
    }
}

fn valid_runtime_common(
    view: &DeploymentOperationalViewV2,
    runtime: &crate::RuntimeDeploymentOperationalViewV2,
) -> bool {
    if runtime.observed_at < view.decision_observed_at
        || (runtime.current_attempt == 0
            && !matches!(
                runtime.phase,
                DeploymentRuntimePhaseV2::Requested
                    | DeploymentRuntimePhaseV2::AuthorityBlocked
                    | DeploymentRuntimePhaseV2::Superseded
            ))
    {
        return false;
    }
    runtime
        .last_failure_attempt
        .is_none_or(|attempt| attempt > 0 && attempt <= runtime.current_attempt)
        && runtime.attestation.as_ref().is_none_or(|attestation| {
            attestation.deployment_revision > 0
                && attestation.convergence_attempt > 0
                && attestation.convergence_attempt == runtime.current_attempt
        })
}

fn valid_failure(
    runtime: &crate::RuntimeDeploymentOperationalViewV2,
    failure: &crate::DeploymentFailureViewV2,
) -> bool {
    if authoring_application::DeploymentFailureCodeV1::parse(&failure.code).is_err() {
        return false;
    }
    match runtime.phase {
        DeploymentRuntimePhaseV2::RetryWaiting | DeploymentRuntimePhaseV2::RetryDue => {
            failure.retryable
                && runtime_failure_code(&failure.code)
                && runtime.operator_action.is_none()
                && valid_retry(runtime)
        }
        DeploymentRuntimePhaseV2::OperatorBlocked => {
            !failure.retryable
                && blocked_runtime_failure_code(&failure.code)
                && runtime.retry.is_none()
                && runtime.current_attempt > 0
                && runtime.last_failure_attempt == Some(runtime.current_attempt)
                && runtime.operator_action
                    == Some(DeploymentOperatorActionV2::RecoverBlockedDeployment)
        }
        DeploymentRuntimePhaseV2::AuthorityBlocked => {
            !failure.retryable
                && matches!(
                    failure.code.as_str(),
                    "product_authority_inactive" | "product_authority_not_current"
                )
                && runtime.retry.is_none()
                && runtime.operator_action
                    == Some(DeploymentOperatorActionV2::RestoreProductAuthority)
        }
        DeploymentRuntimePhaseV2::Superseded => {
            !failure.retryable
                && matches!(
                    failure.code.as_str(),
                    "active_target_changed" | "binding_authority_changed" | "deployment_superseded"
                )
                && runtime.retry.is_none()
                && runtime.operator_action.is_none()
        }
        DeploymentRuntimePhaseV2::Cancelled => {
            !failure.retryable
                && failure.code == "deployment_cancelled"
                && runtime.retry.is_none()
                && runtime.operator_action.is_none()
        }
        _ => false,
    }
}

fn valid_retry(runtime: &crate::RuntimeDeploymentOperationalViewV2) -> bool {
    runtime.retry.as_ref().is_some_and(|retry| {
        retry.failure_attempt > 0
            && retry.failure_attempt == runtime.current_attempt
            && runtime.last_failure_attempt == Some(retry.failure_attempt)
            && match (runtime.phase, retry.state) {
                (DeploymentRuntimePhaseV2::RetryWaiting, DeploymentRetryStateV2::Waiting) => {
                    runtime.observed_at < retry.retry_not_before
                }
                (DeploymentRuntimePhaseV2::RetryDue, DeploymentRetryStateV2::Due) => {
                    runtime.observed_at >= retry.retry_not_before
                }
                _ => false,
            }
    })
}

fn valid_serving(
    runtime: &crate::RuntimeDeploymentOperationalViewV2,
    state: DeploymentOperationalStateV2,
) -> bool {
    let heartbeat = runtime.serving.last_heartbeat_at;
    let expires_at = runtime.serving.lease_expires_at;
    match runtime.serving.state {
        DeploymentServingFreshnessStateV2::NotExpected => {
            runtime.phase != DeploymentRuntimePhaseV2::Live
                && runtime.attestation.is_none()
                && heartbeat.is_none()
                && expires_at.is_none()
                && state != DeploymentOperationalStateV2::Live
        }
        DeploymentServingFreshnessStateV2::AttestationMissing => {
            runtime.phase == DeploymentRuntimePhaseV2::Live
                && runtime.attestation.is_none()
                && heartbeat.is_none()
                && expires_at.is_none()
                && state == DeploymentOperationalStateV2::Pending
        }
        DeploymentServingFreshnessStateV2::LeaseMissing
        | DeploymentServingFreshnessStateV2::IdentityMismatch => {
            runtime.phase == DeploymentRuntimePhaseV2::Live
                && runtime.attestation.is_some()
                && heartbeat.is_none()
                && expires_at.is_none()
                && state == DeploymentOperationalStateV2::Pending
        }
        DeploymentServingFreshnessStateV2::Disconnected => {
            runtime.phase == DeploymentRuntimePhaseV2::Live
                && runtime.attestation.is_some()
                && state == DeploymentOperationalStateV2::Pending
                && heartbeat.is_some_and(|heartbeat| {
                    heartbeat <= runtime.observed_at
                        && expires_at.is_some_and(|expires_at| heartbeat <= expires_at)
                })
        }
        DeploymentServingFreshnessStateV2::Expired => {
            runtime.phase == DeploymentRuntimePhaseV2::Live
                && runtime.attestation.is_some()
                && state == DeploymentOperationalStateV2::Pending
                && heartbeat.is_some_and(|heartbeat| {
                    expires_at.is_some_and(|expires_at| {
                        heartbeat <= expires_at && expires_at <= runtime.observed_at
                    })
                })
        }
        DeploymentServingFreshnessStateV2::Fresh => {
            runtime.phase == DeploymentRuntimePhaseV2::Live
                && runtime.attestation.is_some()
                && state == DeploymentOperationalStateV2::Live
                && heartbeat.is_some_and(|heartbeat| {
                    heartbeat <= runtime.observed_at
                        && expires_at.is_some_and(|expires_at| runtime.observed_at < expires_at)
                })
        }
    }
}

fn runtime_failure_code(code: &str) -> bool {
    matches!(
        code,
        "runtime_environment_unavailable"
            | "activation_not_observable"
            | "panel_reconciliation_failed"
            | "gateway_start_failed"
            | "gateway_ready_timeout"
            | "runtime_invariant_violation"
    )
}

fn blocked_runtime_failure_code(code: &str) -> bool {
    runtime_failure_code(code) || code == "deployment_blocked"
}

fn valid_decision_identity(
    actual_installation: &str,
    actual_promotion: &str,
    expected_installation: &str,
    expected_promotion: &str,
) -> bool {
    actual_installation == expected_installation && actual_promotion == expected_promotion
}
