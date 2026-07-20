use std::num::NonZeroU64;

use authoring_application::{
    AuthorizedDeploymentStatusV1, DeploymentFailureCodeV1, DeploymentStatusPortError,
    DeploymentStatusProjectionV1, ExactLiveProjectionV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_runtime_convergence::{
    RuntimeDeploymentPhaseV1, RuntimeFailureKindV1, RuntimePendingConditionV1,
};
use automation_runtime_convergence_postgres::{
    DeploymentAvailabilityV1, RuntimeConvergenceStoreError, RuntimeDeploymentStatusV1,
};

use crate::ProductDatabaseFailureV1;

use super::row::indeterminate;

pub(super) fn validate_runtime_projection(
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    status: &RuntimeDeploymentStatusV1,
) -> Result<(), DeploymentStatusPortError> {
    let scope = request.scope();
    let exact = request.exact_deployment();
    let identity = &status.snapshot.identity;
    let target = &status.snapshot.target;
    if identity.tenant_id.as_str() != scope.tenant_id().as_str()
        || identity.installation_id.as_str() != scope.installation_id().as_str()
        || identity.deployment_id.as_str() != exact.deployment_reference()
        || identity.promotion_id.as_str() != exact.promotion_id().as_str()
        || target.guild_id != scope.guild_id()
        || status.desired_target_digest() != exact.target_digest()
    {
        return Err(indeterminate());
    }
    match (&status.availability, &status.live) {
        (DeploymentAvailabilityV1::Live, Some(live))
            if matches!(status.snapshot.phase, RuntimeDeploymentPhaseV1::Live)
                && status.snapshot.live.as_ref().is_some_and(|attestation| {
                    attestation.runtime_generation == live.runtime_generation
                        && attestation.process_instance_id == live.process_instance_id
                        && attestation.target == status.snapshot.target
                })
                && live.runtime_generation == status.snapshot.runtime_generation
                && live.last_heartbeat_at <= status.observed_at
                && status.observed_at < live.expires_at => {}
        (DeploymentAvailabilityV1::Live, _) | (_, Some(_)) => return Err(indeterminate()),
        _ => {}
    }
    Ok(())
}

pub(super) fn project_status(
    exact: &authoring_application::ExactDeploymentSelectorV1,
    status: &RuntimeDeploymentStatusV1,
) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
    match status.availability {
        DeploymentAvailabilityV1::Live => {
            let revision =
                NonZeroU64::new(status.snapshot.revision.get()).ok_or_else(indeterminate)?;
            Ok(DeploymentStatusProjectionV1::ExactLive(
                ExactLiveProjectionV1::from_exact_attestation(exact.clone(), revision),
            ))
        }
        DeploymentAvailabilityV1::RuntimePending => match &status.snapshot.phase {
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Retryable { failure, .. },
            } => Ok(DeploymentStatusProjectionV1::Failed {
                retryable: true,
                failure_code: public_runtime_failure_code(failure.kind)
                    .as_str()
                    .to_string(),
            }),
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { .. },
            } => Err(indeterminate()),
            _ => Ok(DeploymentStatusProjectionV1::Pending),
        },
        DeploymentAvailabilityV1::Blocked => Ok(DeploymentStatusProjectionV1::Failed {
            retryable: false,
            failure_code: public_blocked_failure_code(status)?.as_str().to_string(),
        }),
        DeploymentAvailabilityV1::Superseded | DeploymentAvailabilityV1::Cancelled => {
            Ok(DeploymentStatusProjectionV1::Failed {
                retryable: false,
                failure_code: public_status_reason_code(status.reason_code)?
                    .as_str()
                    .to_string(),
            })
        }
    }
}

fn public_blocked_failure_code(
    status: &RuntimeDeploymentStatusV1,
) -> Result<DeploymentFailureCodeV1, DeploymentStatusPortError> {
    let phase_failure = match &status.snapshot.phase {
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { failure },
        } => Some(failure.kind),
        _ => None,
    };
    blocked_failure_code(status.reason_code, phase_failure)
}

fn blocked_failure_code(
    reason_code: &str,
    phase_failure: Option<RuntimeFailureKindV1>,
) -> Result<DeploymentFailureCodeV1, DeploymentStatusPortError> {
    if reason_code == "deployment_blocked" {
        return phase_failure
            .map(public_runtime_failure_code)
            .ok_or_else(indeterminate);
    }
    public_status_reason_code(reason_code)
}

fn public_runtime_failure_code(kind: RuntimeFailureKindV1) -> DeploymentFailureCodeV1 {
    match kind {
        RuntimeFailureKindV1::EnvironmentUnavailable => {
            DeploymentFailureCodeV1::RuntimeEnvironmentUnavailable
        }
        RuntimeFailureKindV1::ActivationNotObservable => {
            DeploymentFailureCodeV1::ActivationNotObservable
        }
        RuntimeFailureKindV1::PanelReconciliation => {
            DeploymentFailureCodeV1::PanelReconciliationFailed
        }
        RuntimeFailureKindV1::GatewayStart => DeploymentFailureCodeV1::GatewayStartFailed,
        RuntimeFailureKindV1::GatewayReadyTimeout => DeploymentFailureCodeV1::GatewayReadyTimeout,
        RuntimeFailureKindV1::InvariantViolation => {
            DeploymentFailureCodeV1::RuntimeInvariantViolation
        }
    }
}

fn public_status_reason_code(
    value: &str,
) -> Result<DeploymentFailureCodeV1, DeploymentStatusPortError> {
    match value {
        "deployment_blocked" => Ok(DeploymentFailureCodeV1::DeploymentBlocked),
        "active_target_changed" => Ok(DeploymentFailureCodeV1::ActiveTargetChanged),
        "binding_authority_changed" => Ok(DeploymentFailureCodeV1::BindingAuthorityChanged),
        "product_authority_inactive" => Ok(DeploymentFailureCodeV1::ProductAuthorityInactive),
        "product_authority_not_current" => Ok(DeploymentFailureCodeV1::ProductAuthorityNotCurrent),
        "deployment_superseded" => Ok(DeploymentFailureCodeV1::DeploymentSuperseded),
        "deployment_cancelled" => Ok(DeploymentFailureCodeV1::DeploymentCancelled),
        _ => Err(indeterminate()),
    }
}

pub(super) fn map_database_error(error: ProductDatabaseFailureV1) -> DeploymentStatusPortError {
    match error {
        ProductDatabaseFailureV1::Timeout
        | ProductDatabaseFailureV1::Retryable
        | ProductDatabaseFailureV1::Unavailable => DeploymentStatusPortError::Backend(
            "runtime deployment status backend is unavailable".to_string(),
        ),
    }
}

pub(super) fn map_projector_error(
    error: RuntimeConvergenceStoreError,
) -> DeploymentStatusPortError {
    match error {
        RuntimeConvergenceStoreError::NotFound => DeploymentStatusPortError::NotFound,
        RuntimeConvergenceStoreError::DatabaseTimeout
        | RuntimeConvergenceStoreError::DatabaseConcurrency
        | RuntimeConvergenceStoreError::DatabaseUnavailable
        | RuntimeConvergenceStoreError::DatabaseFailure => DeploymentStatusPortError::Backend(
            "runtime deployment status backend is unavailable".to_string(),
        ),
        _ => indeterminate(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_failure_evidence_never_crosses_the_product_boundary() {
        let cases = [
            (
                RuntimeFailureKindV1::EnvironmentUnavailable,
                "runtime_environment_unavailable",
            ),
            (
                RuntimeFailureKindV1::ActivationNotObservable,
                "activation_not_observable",
            ),
            (
                RuntimeFailureKindV1::PanelReconciliation,
                "panel_reconciliation_failed",
            ),
            (RuntimeFailureKindV1::GatewayStart, "gateway_start_failed"),
            (
                RuntimeFailureKindV1::GatewayReadyTimeout,
                "gateway_ready_timeout",
            ),
            (
                RuntimeFailureKindV1::InvariantViolation,
                "runtime_invariant_violation",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(public_runtime_failure_code(kind).as_str(), expected);
        }
        assert!(public_status_reason_code(&"a".repeat(64)).is_err());
        assert!(public_status_reason_code("private_internal_identifier").is_err());
        assert_eq!(
            public_status_reason_code("binding_authority_changed")
                .unwrap()
                .as_str(),
            "binding_authority_changed"
        );
    }

    #[test]
    fn current_authority_failure_takes_precedence_over_stale_runtime_failure() {
        assert_eq!(
            blocked_failure_code(
                "product_authority_inactive",
                Some(RuntimeFailureKindV1::GatewayStart)
            )
            .unwrap()
            .as_str(),
            "product_authority_inactive"
        );
        assert_eq!(
            blocked_failure_code(
                "deployment_blocked",
                Some(RuntimeFailureKindV1::GatewayStart)
            )
            .unwrap()
            .as_str(),
            "gateway_start_failed"
        );
        assert!(blocked_failure_code("deployment_blocked", None).is_err());
    }

    #[test]
    fn database_and_projector_errors_are_redacted() {
        assert_eq!(
            map_database_error(ProductDatabaseFailureV1::Unavailable),
            DeploymentStatusPortError::Backend(
                "runtime deployment status backend is unavailable".to_string()
            )
        );
        assert_eq!(
            map_projector_error(RuntimeConvergenceStoreError::InvalidPersistedState(
                "sensitive database detail"
            )),
            DeploymentStatusPortError::Indeterminate(
                "runtime deployment status projection is inconsistent".to_string()
            )
        );
    }
}
