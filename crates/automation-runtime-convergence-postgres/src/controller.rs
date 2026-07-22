use std::num::NonZeroU64;

use automation_runtime_controller::{
    GatewayShardIdV1 as ControllerGatewayShardIdV1, PanelReportDigestV1 as ControllerPanelDigestV1,
    RuntimeAttestationIdV1 as ControllerAttestationIdV1,
    RuntimeBuildRevisionV1 as ControllerBuildV1, RuntimeCertificationReceiptV1,
    RuntimeCertificationRequestV1, RuntimeClaimNextExecutionV1, RuntimeConvergenceErrorClassV1,
    RuntimeConvergenceMutationV1, RuntimeDeploymentScopeV1 as ControllerScopeV1,
    RuntimeExecutionConvergencePort, RuntimeExecutionReceiptV1, RuntimeExecutionUpdateReceiptV1,
    RuntimeLiveMetadataV1, RuntimeMutationReceiptV1, RuntimeMutationRequestV1,
    RuntimeObservePreviousServingV1, RuntimePreviousServingObservationPort,
    RuntimePreviousServingObservationReceiptV1, RuntimeRenewExecutionV1,
    RuntimeServingIdentityV1 as ControllerServingIdentityV1,
    RuntimeServingReceiptV1 as ControllerServingReceiptV1, RuntimeStaleLiveRecoveryReceiptV1,
};
use automation_runtime_convergence::{RuntimeDeploymentError, RuntimeFailureKindV1};

use crate::{
    ClaimExecutionReceiptV1, ClaimNextDeploymentV1, DeploymentMutationV1, GatewayShardIdV1,
    LiveMetadataV1, MutationReceiptV1, PanelReportDigestV1, PostgresRuntimeConvergence,
    RenewDeploymentV1, RuntimeBuildRevisionV1, RuntimeConvergenceStoreError,
    RuntimeDeploymentScopeV1, ServingLeaseReceiptV1, SubmitDeploymentMutationV1,
    SubmitLiveAttestationV1,
};

impl RuntimeExecutionConvergencePort for PostgresRuntimeConvergence {
    type Error = RuntimeConvergenceStoreError;

    async fn claim_next_execution(
        &self,
        request: RuntimeClaimNextExecutionV1,
    ) -> Result<Option<RuntimeExecutionReceiptV1>, Self::Error> {
        PostgresRuntimeConvergence::claim_next_execution(
            self,
            ClaimNextDeploymentV1 {
                controller_id: request.controller_id,
                lease_for: request.lease_for,
            },
        )
        .await
        .map(|receipt| receipt.map(execution_receipt))
    }

    async fn renew_execution(
        &self,
        request: RuntimeRenewExecutionV1,
    ) -> Result<RuntimeExecutionUpdateReceiptV1, Self::Error> {
        let receipt = PostgresRuntimeConvergence::renew_execution(
            self,
            RenewDeploymentV1 {
                scope: scope(&request.guard.scope),
                expected_revision: request.guard.expected_revision,
                controller_id: request.guard.controller_id,
                fencing_token: request.guard.fencing_token,
                convergence_attempt: request.guard.convergence_attempt,
                runtime_generation: request.guard.runtime_generation,
                lease_for: request.lease_for,
            },
        )
        .await?;
        Ok(RuntimeExecutionUpdateReceiptV1 {
            action_id: request.action_id,
            execution: execution_receipt(receipt),
        })
    }

    async fn mutate(
        &self,
        request: RuntimeMutationRequestV1,
    ) -> Result<RuntimeMutationReceiptV1, Self::Error> {
        let receipt = PostgresRuntimeConvergence::mutate(
            self,
            SubmitDeploymentMutationV1 {
                scope: scope(&request.guard.scope),
                expected_revision: request.guard.expected_revision,
                controller_id: request.guard.controller_id,
                fencing_token: request.guard.fencing_token,
                convergence_attempt: request.guard.convergence_attempt,
                runtime_generation: request.guard.runtime_generation,
                mutation: mutation(request.mutation),
            },
        )
        .await?;
        Ok(RuntimeMutationReceiptV1 {
            action_id: request.action_id,
            outcome: receipt.outcome,
            snapshot: receipt.snapshot,
            convergence_attempt: receipt.convergence_attempt,
        })
    }

    async fn certify_live(
        &self,
        request: RuntimeCertificationRequestV1,
    ) -> Result<RuntimeCertificationReceiptV1, Self::Error> {
        let metadata = live_metadata(&request.metadata)?;
        let (mutation, serving) = PostgresRuntimeConvergence::certify_live(
            self,
            SubmitLiveAttestationV1 {
                scope: scope(&request.guard.scope),
                expected_revision: request.guard.expected_revision,
                controller_id: request.guard.controller_id,
                fencing_token: request.guard.fencing_token,
                convergence_attempt: request.guard.convergence_attempt,
                runtime_generation: request.guard.runtime_generation,
                gateway_ready: request.gateway_ready,
                metadata,
                serving_lease_for: request.serving_lease_for,
            },
        )
        .await?;
        Ok(RuntimeCertificationReceiptV1 {
            action_id: request.action_id,
            outcome: mutation.outcome,
            snapshot: mutation.snapshot,
            convergence_attempt: mutation.convergence_attempt,
            metadata: request.metadata,
            serving: serving_receipt(serving, request.guard.runtime_generation)?,
        })
    }

    async fn recover_next_stale_live(
        &self,
    ) -> Result<Option<RuntimeStaleLiveRecoveryReceiptV1>, Self::Error> {
        PostgresRuntimeConvergence::recover_next_stale_live(self)
            .await
            .map(|receipt| receipt.map(stale_live_receipt))
    }

    fn classify_error(error: &Self::Error) -> RuntimeConvergenceErrorClassV1 {
        classify_error(error)
    }
}

impl RuntimePreviousServingObservationPort for PostgresRuntimeConvergence {
    async fn observe_previous_serving(
        &self,
        request: RuntimeObservePreviousServingV1,
    ) -> Result<
        RuntimePreviousServingObservationReceiptV1,
        <Self as RuntimeExecutionConvergencePort>::Error,
    > {
        self.observe_previous_serving_capability(request).await
    }
}

fn scope(value: &ControllerScopeV1) -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: value.tenant_id.clone(),
        installation_id: value.installation_id.clone(),
        deployment_id: value.deployment_id.clone(),
    }
}

fn execution_receipt(value: ClaimExecutionReceiptV1) -> RuntimeExecutionReceiptV1 {
    RuntimeExecutionReceiptV1 {
        snapshot: value.snapshot,
        controller_id: value.controller_id,
        fencing_token: value.fencing_token,
        convergence_attempt: value.convergence_attempt,
        acquired_at: value.acquired_at,
        expires_at: value.expires_at,
    }
}

fn mutation(value: RuntimeConvergenceMutationV1) -> DeploymentMutationV1 {
    match value {
        RuntimeConvergenceMutationV1::AcceptPreflight(value) => {
            DeploymentMutationV1::AcceptPreflight(value)
        }
        RuntimeConvergenceMutationV1::RequestDrain => DeploymentMutationV1::RequestDrain,
        RuntimeConvergenceMutationV1::AcceptDrain(value) => {
            DeploymentMutationV1::AcceptDrain(value)
        }
        RuntimeConvergenceMutationV1::BeginActivation => DeploymentMutationV1::BeginActivation,
        RuntimeConvergenceMutationV1::AcceptActivation(value) => {
            DeploymentMutationV1::AcceptActivation(value)
        }
        RuntimeConvergenceMutationV1::RecordRetryableFailure {
            failure_id,
            kind,
            code,
            attempt,
            retry_after,
        } => DeploymentMutationV1::RecordRetryableFailure {
            failure_id,
            kind,
            code,
            message: stable_failure_message(kind).to_string(),
            attempt,
            retry_after,
        },
        RuntimeConvergenceMutationV1::RecordBlockedFailure {
            failure_id,
            kind,
            code,
        } => DeploymentMutationV1::RecordBlockedFailure {
            failure_id,
            kind,
            code,
            message: stable_failure_message(kind).to_string(),
        },
        RuntimeConvergenceMutationV1::ResumeRuntimePending => {
            DeploymentMutationV1::ResumeRuntimePending
        }
        RuntimeConvergenceMutationV1::BeginPanelReconciliation => {
            DeploymentMutationV1::BeginPanelReconciliation
        }
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(value) => {
            DeploymentMutationV1::AcceptPanelCertificate(value)
        }
        RuntimeConvergenceMutationV1::Supersede { by, reason } => {
            DeploymentMutationV1::Supersede { by, reason }
        }
        RuntimeConvergenceMutationV1::Cancel { reason } => DeploymentMutationV1::Cancel { reason },
    }
}

fn live_metadata(
    value: &RuntimeLiveMetadataV1,
) -> Result<LiveMetadataV1, RuntimeConvergenceStoreError> {
    Ok(LiveMetadataV1 {
        runtime_build_revision: RuntimeBuildRevisionV1::parse(ControllerBuildV1::as_str(
            &value.runtime_build_revision,
        ))?,
        panel_report_digest: PanelReportDigestV1::parse(ControllerPanelDigestV1::as_str(
            &value.panel_report_digest,
        ))?,
        gateway_shard_id: GatewayShardIdV1::parse(ControllerGatewayShardIdV1::as_str(
            &value.gateway_shard_id,
        ))?,
    })
}

fn serving_receipt(
    value: ServingLeaseReceiptV1,
    runtime_generation: automation_runtime_convergence::RuntimeGeneration,
) -> Result<ControllerServingReceiptV1, RuntimeConvergenceStoreError> {
    let lease_epoch = NonZeroU64::new(value.identity.lease_epoch).ok_or(
        RuntimeConvergenceStoreError::InvalidPersistedState("runtime serving lease epoch"),
    )?;
    let expected_revision = NonZeroU64::new(value.identity.expected_revision).ok_or(
        RuntimeConvergenceStoreError::InvalidPersistedState("runtime serving lease revision"),
    )?;
    let attestation_id = ControllerAttestationIdV1::parse(value.identity.attestation_id.as_str())
        .map_err(|_| {
        RuntimeConvergenceStoreError::InvalidPersistedState("runtime serving attestation")
    })?;
    Ok(ControllerServingReceiptV1 {
        identity: ControllerServingIdentityV1 {
            scope: ControllerScopeV1 {
                tenant_id: value.identity.scope.tenant_id,
                installation_id: value.identity.scope.installation_id,
                deployment_id: value.identity.scope.deployment_id,
            },
            attestation_id,
            process_instance_id: value.identity.process_instance_id,
            runtime_generation,
            lease_epoch,
            expected_revision,
        },
        runtime_generation,
        acquired_at: value.acquired_at,
        last_heartbeat_at: value.last_heartbeat_at,
        expires_at: value.expires_at,
        connected: value.connected,
        serving: value.serving,
    })
}

fn stale_live_receipt(value: MutationReceiptV1) -> RuntimeStaleLiveRecoveryReceiptV1 {
    RuntimeStaleLiveRecoveryReceiptV1 {
        outcome: value.outcome,
        snapshot: value.snapshot,
    }
}

fn stable_failure_message(kind: RuntimeFailureKindV1) -> &'static str {
    match kind {
        RuntimeFailureKindV1::EnvironmentUnavailable => "runtime environment unavailable",
        RuntimeFailureKindV1::ActivationNotObservable => "activation not observable",
        RuntimeFailureKindV1::PanelReconciliation => "panel reconciliation failed",
        RuntimeFailureKindV1::GatewayStart => "gateway start failed",
        RuntimeFailureKindV1::GatewayReadyTimeout => "gateway Ready timed out",
        RuntimeFailureKindV1::InvariantViolation => "runtime invariant rejected",
    }
}

fn classify_error(error: &RuntimeConvergenceStoreError) -> RuntimeConvergenceErrorClassV1 {
    match error {
        RuntimeConvergenceStoreError::RetryNotReady => {
            RuntimeConvergenceErrorClassV1::RetryNotReady
        }
        RuntimeConvergenceStoreError::DatabaseTimeout
        | RuntimeConvergenceStoreError::DatabaseConcurrency
        | RuntimeConvergenceStoreError::DatabaseUnavailable => {
            RuntimeConvergenceErrorClassV1::Retryable
        }
        RuntimeConvergenceStoreError::ActiveTargetMismatch => {
            RuntimeConvergenceErrorClassV1::Superseded
        }
        RuntimeConvergenceStoreError::BindingAuthorityMismatch
        | RuntimeConvergenceStoreError::ProductAuthorityInactive
        | RuntimeConvergenceStoreError::OperatorActionRequired => {
            RuntimeConvergenceErrorClassV1::AuthorityBlocked
        }
        RuntimeConvergenceStoreError::NotFound
        | RuntimeConvergenceStoreError::ScopeMismatch
        | RuntimeConvergenceStoreError::RevisionConflict
        | RuntimeConvergenceStoreError::ServingLeaseConflict
        | RuntimeConvergenceStoreError::ConvergenceAttemptConflict
        | RuntimeConvergenceStoreError::ExecutionClaimStale
        | RuntimeConvergenceStoreError::Domain(
            RuntimeDeploymentError::RevisionConflict { .. }
            | RuntimeDeploymentError::LeaseHeld { .. }
            | RuntimeDeploymentError::LeaseRequired
            | RuntimeDeploymentError::LeaseExpired { .. }
            | RuntimeDeploymentError::ControllerMismatch
            | RuntimeDeploymentError::FencingTokenConflict { .. }
            | RuntimeDeploymentError::FencingTokenNotMonotonic,
        ) => RuntimeConvergenceErrorClassV1::OwnershipLost,
        RuntimeConvergenceStoreError::IdempotencyConflict
        | RuntimeConvergenceStoreError::AttestationConflict
        | RuntimeConvergenceStoreError::ConvergenceAttemptOverflow
        | RuntimeConvergenceStoreError::InvalidInput(_)
        | RuntimeConvergenceStoreError::InvalidPersistedState(_)
        | RuntimeConvergenceStoreError::DatabaseFailure
        | RuntimeConvergenceStoreError::DatabaseAuthorityMismatch
        | RuntimeConvergenceStoreError::Domain(_) => RuntimeConvergenceErrorClassV1::InvalidState,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_errors_map_to_closed_controller_classes() {
        let cases = [
            (
                RuntimeConvergenceStoreError::DatabaseUnavailable,
                RuntimeConvergenceErrorClassV1::Retryable,
            ),
            (
                RuntimeConvergenceStoreError::RetryNotReady,
                RuntimeConvergenceErrorClassV1::RetryNotReady,
            ),
            (
                RuntimeConvergenceStoreError::RevisionConflict,
                RuntimeConvergenceErrorClassV1::OwnershipLost,
            ),
            (
                RuntimeConvergenceStoreError::ActiveTargetMismatch,
                RuntimeConvergenceErrorClassV1::Superseded,
            ),
            (
                RuntimeConvergenceStoreError::BindingAuthorityMismatch,
                RuntimeConvergenceErrorClassV1::AuthorityBlocked,
            ),
            (
                RuntimeConvergenceStoreError::InvalidPersistedState("test"),
                RuntimeConvergenceErrorClassV1::InvalidState,
            ),
            (
                RuntimeConvergenceStoreError::DatabaseAuthorityMismatch,
                RuntimeConvergenceErrorClassV1::InvalidState,
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(classify_error(&error), expected);
            assert_eq!(
                <PostgresRuntimeConvergence as RuntimeExecutionConvergencePort>::classify_error(
                    &error,
                ),
                expected
            );
        }
    }

    #[test]
    fn every_failure_kind_has_a_bounded_stable_message() {
        for kind in [
            RuntimeFailureKindV1::EnvironmentUnavailable,
            RuntimeFailureKindV1::ActivationNotObservable,
            RuntimeFailureKindV1::PanelReconciliation,
            RuntimeFailureKindV1::GatewayStart,
            RuntimeFailureKindV1::GatewayReadyTimeout,
            RuntimeFailureKindV1::InvariantViolation,
        ] {
            let message = stable_failure_message(kind);
            assert!(!message.is_empty());
            assert!(message.len() <= 64);
        }
    }
}
