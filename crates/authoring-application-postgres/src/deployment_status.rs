use std::num::NonZeroU64;

use authoring_application::{
    AuthorizedDeploymentStatusV1, CapabilityV1, DeploymentStatusPort, DeploymentStatusPortError,
    DeploymentStatusProjectionV1, ExactLiveProjectionV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_runtime_convergence::{
    DeploymentId, InstallationId, RuntimeDeploymentPhaseV1, RuntimeFailureKindV1,
    RuntimePendingConditionV1, TenantId,
};
use automation_runtime_convergence_postgres::{
    DeploymentAvailabilityV1, PostgresRuntimeConvergence, RuntimeConvergenceStoreError,
    RuntimeDeploymentScopeV1, RuntimeDeploymentStatusV1,
};

const MAX_READ_AUTHORITY_LIFETIME: chrono::Duration = chrono::Duration::seconds(30);
const MAX_APPLY_AUTHORITY_LIFETIME: chrono::Duration = chrono::Duration::seconds(5);

#[derive(Clone)]
pub struct PostgresProductDeploymentStatuses {
    runtime: PostgresRuntimeConvergence,
}

impl PostgresProductDeploymentStatuses {
    pub fn new(runtime: PostgresRuntimeConvergence) -> Self {
        Self { runtime }
    }
}

impl DeploymentStatusPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductDeploymentStatuses {
    async fn load_exact_deployment_status(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
        validate_request_scope(&request)?;
        let scope = runtime_scope(&request)?;
        let status = self.runtime.status(&scope).await.map_err(map_store_error)?;
        validate_runtime_projection(&request, &status)?;
        project_status(request.exact_deployment(), &status)
    }
}

fn validate_request_scope(
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<(), DeploymentStatusPortError> {
    let evidence = request.evidence();
    let scope = request.scope();
    let exact = request.exact_deployment();
    if status_authority_lifetime(evidence.capability()).is_none()
        || evidence.tenant_id() != scope.tenant_id()
        || evidence.installation_id() != scope.installation_id()
        || evidence.guild_id() != scope.guild_id()
        || evidence.acting_user_id() != scope.acting_user_id()
        || exact.installation_id() != scope.installation_id()
    {
        return Err(indeterminate());
    }
    Ok(())
}

fn runtime_scope(
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<RuntimeDeploymentScopeV1, DeploymentStatusPortError> {
    Ok(RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse(request.scope().tenant_id().as_str())
            .map_err(|_| indeterminate())?,
        installation_id: InstallationId::parse(request.scope().installation_id().as_str())
            .map_err(|_| indeterminate())?,
        deployment_id: DeploymentId::parse(request.exact_deployment().deployment_reference())
            .map_err(|_| indeterminate())?,
    })
}

fn validate_runtime_projection(
    request: &AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    status: &RuntimeDeploymentStatusV1,
) -> Result<(), DeploymentStatusPortError> {
    let scope = request.scope();
    let exact = request.exact_deployment();
    let evidence = request.evidence();
    let identity = &status.snapshot.identity;
    let target = &status.snapshot.target;
    let maximum_lifetime =
        status_authority_lifetime(evidence.capability()).ok_or_else(indeterminate)?;
    let evidence_window_is_valid = evidence.observed_at() <= status.observed_at
        && status.observed_at < evidence.expires_at()
        && evidence
            .observed_at()
            .checked_add_signed(maximum_lifetime)
            .is_some_and(|latest| evidence.expires_at() <= latest);
    if !evidence_window_is_valid
        || identity.tenant_id.as_str() != scope.tenant_id().as_str()
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

fn project_status(
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
                failure_code: public_runtime_failure_code(failure.kind).to_string(),
            }),
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { .. },
            } => Err(indeterminate()),
            _ => Ok(DeploymentStatusProjectionV1::Pending),
        },
        DeploymentAvailabilityV1::Blocked => Ok(DeploymentStatusProjectionV1::Failed {
            retryable: false,
            failure_code: public_blocked_failure_code(status)?.to_string(),
        }),
        DeploymentAvailabilityV1::Superseded | DeploymentAvailabilityV1::Cancelled => {
            Ok(DeploymentStatusProjectionV1::Failed {
                retryable: false,
                failure_code: public_status_reason_code(status.reason_code)?.to_string(),
            })
        }
    }
}

fn public_blocked_failure_code(
    status: &RuntimeDeploymentStatusV1,
) -> Result<&'static str, DeploymentStatusPortError> {
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
) -> Result<&'static str, DeploymentStatusPortError> {
    if reason_code == "deployment_blocked" {
        return phase_failure
            .map(public_runtime_failure_code)
            .ok_or_else(indeterminate);
    }
    public_status_reason_code(reason_code)
}

fn public_runtime_failure_code(kind: RuntimeFailureKindV1) -> &'static str {
    match kind {
        RuntimeFailureKindV1::EnvironmentUnavailable => "runtime_environment_unavailable",
        RuntimeFailureKindV1::ActivationNotObservable => "activation_not_observable",
        RuntimeFailureKindV1::PanelReconciliation => "panel_reconciliation_failed",
        RuntimeFailureKindV1::GatewayStart => "gateway_start_failed",
        RuntimeFailureKindV1::GatewayReadyTimeout => "gateway_ready_timeout",
        RuntimeFailureKindV1::InvariantViolation => "runtime_invariant_violation",
    }
}

fn public_status_reason_code(value: &str) -> Result<&'static str, DeploymentStatusPortError> {
    match value {
        "deployment_blocked" => Ok("deployment_blocked"),
        "active_target_changed" => Ok("active_target_changed"),
        "binding_authority_changed" => Ok("binding_authority_changed"),
        "product_authority_inactive" => Ok("product_authority_inactive"),
        "product_authority_not_current" => Ok("product_authority_not_current"),
        "deployment_superseded" => Ok("deployment_superseded"),
        "deployment_cancelled" => Ok("deployment_cancelled"),
        _ => Err(indeterminate()),
    }
}

fn status_authority_lifetime(capability: CapabilityV1) -> Option<chrono::Duration> {
    match capability {
        CapabilityV1::Read => Some(MAX_READ_AUTHORITY_LIFETIME),
        CapabilityV1::Apply => Some(MAX_APPLY_AUTHORITY_LIFETIME),
        CapabilityV1::Promote | CapabilityV1::Approve | CapabilityV1::Reject => None,
    }
}

fn map_store_error(error: RuntimeConvergenceStoreError) -> DeploymentStatusPortError {
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

fn indeterminate() -> DeploymentStatusPortError {
    DeploymentStatusPortError::Indeterminate(
        "runtime deployment status projection is inconsistent".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use authoring_application::{CapabilityV1, DeploymentStatusPortError};
    use automation_runtime_convergence::RuntimeFailureKindV1;
    use automation_runtime_convergence_postgres::RuntimeConvergenceStoreError;

    use super::{
        blocked_failure_code, map_store_error, public_runtime_failure_code,
        public_status_reason_code, status_authority_lifetime,
    };

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
            assert_eq!(public_runtime_failure_code(kind), expected);
        }
        assert!(public_status_reason_code(&"a".repeat(64)).is_err());
        assert!(public_status_reason_code("private_internal_identifier").is_err());
        assert_eq!(
            public_status_reason_code("binding_authority_changed").unwrap(),
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
            .unwrap(),
            "product_authority_inactive"
        );
        assert_eq!(
            blocked_failure_code(
                "deployment_blocked",
                Some(RuntimeFailureKindV1::GatewayStart)
            )
            .unwrap(),
            "gateway_start_failed"
        );
        assert!(blocked_failure_code("deployment_blocked", None).is_err());
    }

    #[test]
    fn status_accepts_read_and_apply_evidence_with_distinct_freshness_bounds() {
        assert_eq!(
            status_authority_lifetime(CapabilityV1::Read),
            Some(chrono::Duration::seconds(30))
        );
        assert_eq!(
            status_authority_lifetime(CapabilityV1::Apply),
            Some(chrono::Duration::seconds(5))
        );
        assert_eq!(status_authority_lifetime(CapabilityV1::Promote), None);
        assert_eq!(status_authority_lifetime(CapabilityV1::Approve), None);
        assert_eq!(status_authority_lifetime(CapabilityV1::Reject), None);
    }

    #[test]
    fn runtime_backend_errors_are_redacted_at_the_product_boundary() {
        assert_eq!(
            map_store_error(RuntimeConvergenceStoreError::NotFound),
            DeploymentStatusPortError::NotFound
        );
        assert_eq!(
            map_store_error(RuntimeConvergenceStoreError::DatabaseFailure),
            DeploymentStatusPortError::Backend(
                "runtime deployment status backend is unavailable".to_string()
            )
        );
        assert_eq!(
            map_store_error(RuntimeConvergenceStoreError::InvalidPersistedState(
                "sensitive database detail"
            )),
            DeploymentStatusPortError::Indeterminate(
                "runtime deployment status projection is inconsistent".to_string()
            )
        );
    }
}
