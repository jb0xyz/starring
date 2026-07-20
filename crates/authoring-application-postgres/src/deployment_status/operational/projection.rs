use std::num::NonZeroU64;

use authoring_application::{
    AuthorizedDeploymentStatusV1, DeploymentAttestationObservationV2, DeploymentConvergencePhaseV2,
    DeploymentOperationalObservationV2, DeploymentOperationalProjectionV2,
    DeploymentOperatorActionV2, DeploymentRetryObservationV2, DeploymentServingFreshnessV2,
    DeploymentStatusObservationV1, DeploymentStatusPortError,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_runtime_convergence::{RuntimeDeploymentPhaseV1, RuntimePendingConditionV1};
use automation_runtime_convergence_postgres::{
    RuntimeDeploymentStatusV2, RuntimeServingFreshnessV2,
};

use super::super::projection::{project_status, validate_runtime_projection};
use super::super::row::indeterminate;

pub(super) fn project_operational_status(
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    status: RuntimeDeploymentStatusV2,
) -> Result<DeploymentOperationalObservationV2, DeploymentStatusPortError> {
    validate_runtime_projection(request, &status.status)?;
    let projection = project_status(request.exact_deployment(), &status.status)?;
    let (last_heartbeat_at, lease_expires_at) = status
        .status
        .live
        .as_ref()
        .map(|live| {
            (
                Some(live.last_heartbeat_at.into()),
                Some(live.expires_at.into()),
            )
        })
        .unwrap_or((None, None));
    let base = DeploymentStatusObservationV1::from_server_projection(
        projection,
        status.status.observed_at.into(),
        last_heartbeat_at,
        lease_expires_at,
    )
    .map_err(|_| indeterminate())?;
    let phase = deployment_phase(&status);
    let retry = deployment_retry(&status, phase)?;
    let operator_action = operator_action(&status);
    let attestation = status
        .attestation
        .map(|attestation| {
            let revision =
                NonZeroU64::new(attestation.deployment_revision.get()).ok_or_else(indeterminate)?;
            Ok::<_, DeploymentStatusPortError>(DeploymentAttestationObservationV2::new(
                revision,
                attestation.convergence_attempt,
            ))
        })
        .transpose()?;
    let serving = serving_freshness(&status)?;
    DeploymentOperationalObservationV2::from_server_projection(
        base,
        DeploymentOperationalProjectionV2 {
            phase,
            current_attempt: status.convergence_attempt.get(),
            last_failure_attempt: status.last_failure_attempt,
            retry,
            operator_action,
            attestation,
            serving,
        },
    )
    .map_err(|_| indeterminate())
}

fn deployment_phase(status: &RuntimeDeploymentStatusV2) -> DeploymentConvergencePhaseV2 {
    match status.status.reason_code {
        "product_authority_inactive" | "product_authority_not_current" => {
            return DeploymentConvergencePhaseV2::AuthorityBlocked
        }
        "active_target_changed" | "binding_authority_changed" => {
            return DeploymentConvergencePhaseV2::Superseded
        }
        _ => {}
    }
    match &status.status.snapshot.phase {
        RuntimeDeploymentPhaseV1::Requested => DeploymentConvergencePhaseV2::Requested,
        RuntimeDeploymentPhaseV1::PreflightReady => DeploymentConvergencePhaseV2::PreflightReady,
        RuntimeDeploymentPhaseV1::DrainRequested => DeploymentConvergencePhaseV2::DrainRequested,
        RuntimeDeploymentPhaseV1::Drained => DeploymentConvergencePhaseV2::Drained,
        RuntimeDeploymentPhaseV1::ActivationApplying => {
            DeploymentConvergencePhaseV2::ActivationApplying
        }
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Ready,
        } => DeploymentConvergencePhaseV2::RuntimeReady,
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition:
                RuntimePendingConditionV1::Retryable {
                    retry_not_before, ..
                },
        } if status.status.observed_at < *retry_not_before => {
            DeploymentConvergencePhaseV2::RetryWaiting
        }
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Retryable { .. },
        } => DeploymentConvergencePhaseV2::RetryDue,
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { .. },
        } => DeploymentConvergencePhaseV2::OperatorBlocked,
        RuntimeDeploymentPhaseV1::ReconcilingPanels => {
            DeploymentConvergencePhaseV2::ReconcilingPanels
        }
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady => {
            DeploymentConvergencePhaseV2::AwaitingGatewayReady
        }
        RuntimeDeploymentPhaseV1::Live => DeploymentConvergencePhaseV2::Live,
        RuntimeDeploymentPhaseV1::Superseded { .. } => DeploymentConvergencePhaseV2::Superseded,
        RuntimeDeploymentPhaseV1::Cancelled { .. } => DeploymentConvergencePhaseV2::Cancelled,
    }
}

fn deployment_retry(
    status: &RuntimeDeploymentStatusV2,
    phase: DeploymentConvergencePhaseV2,
) -> Result<Option<DeploymentRetryObservationV2>, DeploymentStatusPortError> {
    let RuntimeDeploymentPhaseV1::RuntimePending {
        condition:
            RuntimePendingConditionV1::Retryable {
                attempt,
                retry_not_before,
                ..
            },
    } = &status.status.snapshot.phase
    else {
        return Ok(None);
    };
    let retry = match phase {
        DeploymentConvergencePhaseV2::RetryWaiting => DeploymentRetryObservationV2::Waiting {
            failure_attempt: *attempt,
            retry_not_before: (*retry_not_before).into(),
        },
        DeploymentConvergencePhaseV2::RetryDue => DeploymentRetryObservationV2::Due {
            failure_attempt: *attempt,
            retry_not_before: (*retry_not_before).into(),
        },
        DeploymentConvergencePhaseV2::AuthorityBlocked
        | DeploymentConvergencePhaseV2::Superseded => return Ok(None),
        _ => return Err(indeterminate()),
    };
    Ok(Some(retry))
}

fn operator_action(status: &RuntimeDeploymentStatusV2) -> Option<DeploymentOperatorActionV2> {
    match status.status.reason_code {
        "deployment_blocked" => Some(DeploymentOperatorActionV2::RecoverBlockedDeployment),
        "product_authority_inactive" | "product_authority_not_current" => {
            Some(DeploymentOperatorActionV2::RestoreProductAuthority)
        }
        _ => None,
    }
}

fn serving_freshness(
    status: &RuntimeDeploymentStatusV2,
) -> Result<DeploymentServingFreshnessV2, DeploymentStatusPortError> {
    let heartbeat = status.serving.last_heartbeat_at.map(Into::into);
    let expires = status.serving.expires_at.map(Into::into);
    match (status.serving.freshness, heartbeat, expires) {
        (RuntimeServingFreshnessV2::NotExpected, None, None) => {
            Ok(DeploymentServingFreshnessV2::NotExpected)
        }
        (RuntimeServingFreshnessV2::AttestationMissing, None, None) => {
            Ok(DeploymentServingFreshnessV2::AttestationMissing)
        }
        (RuntimeServingFreshnessV2::LeaseMissing, None, None) => {
            Ok(DeploymentServingFreshnessV2::LeaseMissing)
        }
        (RuntimeServingFreshnessV2::IdentityMismatch, None, None) => {
            Ok(DeploymentServingFreshnessV2::IdentityMismatch)
        }
        (
            RuntimeServingFreshnessV2::Disconnected,
            Some(last_heartbeat_at),
            Some(lease_expires_at),
        ) => Ok(DeploymentServingFreshnessV2::Disconnected {
            last_heartbeat_at,
            lease_expires_at,
        }),
        (RuntimeServingFreshnessV2::Expired, Some(last_heartbeat_at), Some(lease_expires_at)) => {
            Ok(DeploymentServingFreshnessV2::Expired {
                last_heartbeat_at,
                lease_expires_at,
            })
        }
        (RuntimeServingFreshnessV2::Fresh, Some(last_heartbeat_at), Some(lease_expires_at)) => {
            Ok(DeploymentServingFreshnessV2::Fresh {
                last_heartbeat_at,
                lease_expires_at,
            })
        }
        _ => Err(indeterminate()),
    }
}
