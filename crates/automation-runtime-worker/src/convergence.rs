mod hydration;
mod preflight;
mod replacement;
mod staging;

use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use automation_runtime_controller::{
    plan_runtime_action_v1, RuntimeControllerActionV1, RuntimeControllerConfigV1,
    RuntimeControllerPlanError, RuntimeConvergenceSessionError, RuntimeConvergenceSessionV1,
    RuntimeExecutionReceiptV1, RuntimeServingSlotV2,
};
use automation_runtime_convergence::{
    RuntimeDeploymentPhaseV1, RuntimePendingConditionV1, RuntimeProcessIdentityV1,
};
use chrono::{DateTime, Utc};

use crate::{RuntimeServingSlotWorkErrorV2, RuntimeServingSlotWorkPermitV2};

pub use hydration::{
    RuntimeAuthorityPayloadDigestErrorV2, RuntimeAuthorityPayloadDigestV2,
    RuntimeExactTargetEvidenceErrorV2, RuntimeExactTargetEvidenceV2,
    RuntimeExactTargetHydrationErrorV2, RuntimeExactTargetHydrationPortV2,
    RuntimeExactTargetHydrationRequestV2, RuntimeExactTargetHydrationResultV2,
    RuntimeExactTargetHydrationV2, RuntimeExactTargetObservationV2, RuntimeHydratedConvergenceV2,
    RuntimeStageReadyHydrationRefreshResultV2, RuntimeStageReadyHydrationRefreshV2,
};
pub use preflight::{
    RuntimeAcceptPreflightMutationErrorV2, RuntimeConvergenceMutationPortV2,
    RuntimeDiscordPreflightErrorV2, RuntimeDiscordPreflightEvidenceErrorV2,
    RuntimeDiscordPreflightObservationV2, RuntimeDiscordPreflightOutcomeV2,
    RuntimeDiscordPreflightPortV2, RuntimeDiscordPreflightRequestV2, RuntimeDiscordPreflightV2,
    RuntimePreflightedConvergenceV2,
};
pub use replacement::{
    RuntimeAcceptDrainFutureV2, RuntimeAcceptDrainMutationErrorV2, RuntimeAcceptDrainMutationV2,
    RuntimeAdmissionDispositionV2, RuntimeBarrierACorrelationV2, RuntimeBarrierAEvidenceV2,
    RuntimeBarrierAPauseErrorV2, RuntimeBarrierAPauseEvidenceErrorV2,
    RuntimeBarrierAPauseEvidenceV2, RuntimeBarrierAPauseFutureV2,
    RuntimeBarrierAPauseObservationV2, RuntimeBarrierAPausePortV2, RuntimeBarrierAPauseRequestV2,
    RuntimeBarrierAPauseV2, RuntimeBarrierAPausedConvergenceV2, RuntimeBarrierAResumeErrorV2,
    RuntimeBarrierAResumeEvidenceErrorV2, RuntimeBarrierAResumeEvidenceV2,
    RuntimeBarrierAResumeFutureV2, RuntimeBarrierAResumeObservationV2, RuntimeBarrierAResumePortV2,
    RuntimeBarrierAResumeRequestStateV2, RuntimeBarrierAResumeRequestV2,
    RuntimeBarrierAResumedConvergenceV2, RuntimeDrainRequestedConvergenceV2,
    RuntimeDrainedConvergenceHandoffV2, RuntimeDrainedConvergenceV2,
    RuntimeExactPreviousServingErrorV2, RuntimeExactPreviousServingEvidenceErrorV2,
    RuntimeExactPreviousServingObservationPortV2, RuntimeExactPreviousServingV2,
    RuntimeObservedPreviousServingConvergenceV2, RuntimePredecessorRemovedConvergenceV2,
    RuntimePredecessorRetirementErrorV2, RuntimePredecessorRetirementEvidenceErrorV2,
    RuntimePredecessorRetirementFutureV2, RuntimePredecessorRetirementObservationV2,
    RuntimePredecessorRetirementPortV2, RuntimePredecessorRetirementReadyConvergenceV2,
    RuntimePredecessorRetirementRequestV2, RuntimePredecessorRetirementV2,
    RuntimePredecessorTransitionResultV2, RuntimePreviousServingDisconnectErrorV2,
    RuntimePreviousServingDisconnectFutureV2, RuntimePreviousServingDisconnectOutcomeV2,
    RuntimePreviousServingDisconnectPortV2, RuntimePreviousServingDisconnectV2,
    RuntimeReplacementExecutionErrorV2, RuntimeReplacementFutureV2, RuntimeReplacementResultV2,
    RuntimeRequestDrainMutationErrorV2, RuntimeRequestDrainMutationV2,
    RuntimeRoutePredecessorDrainingConvergenceV2, RuntimeRoutePredecessorRemovalObservationV2,
    RuntimeRoutePredecessorTransitionErrorV2, RuntimeRoutePredecessorTransitionEvidenceErrorV2,
    RuntimeRoutePredecessorTransitionEvidenceV2, RuntimeRoutePredecessorTransitionObservationV2,
    RuntimeRoutePredecessorTransitionPortV2, RuntimeRoutePredecessorTransitionRequestV2,
    RuntimeRoutePredecessorTransitionV2,
};
pub use staging::{
    RuntimeRefreshedStageReadyConvergenceV2, RuntimeRouteLifecycleV2, RuntimeRouteStageErrorV2,
    RuntimeRouteStageEvidenceErrorV2, RuntimeRouteStageObservationV2, RuntimeRouteStageOutcomeV2,
    RuntimeRouteStageRequestV2, RuntimeRouteWitnessV2, RuntimeStageReadyConvergenceV2,
    RuntimeStagedConvergenceV2, RuntimeStagedRecoveryHandoffV2, RuntimeStagedRecoveryPhaseV2,
    RuntimeStagedRecoveryRouteHandoffV2, RuntimeStagedRecoveryRouteV2, RuntimeStagedRoutePortV2,
};

pub type RuntimeConvergenceFutureV2<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeConvergenceStartErrorV2 {
    #[error("runtime convergence slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime convergence claimed target does not match its serving slot")]
    SlotMismatch,
    #[error("runtime convergence claim cannot start a session")]
    Session(RuntimeConvergenceSessionError),
    #[error("runtime convergence claim cannot be planned")]
    Plan(RuntimeControllerPlanError),
    #[error("runtime convergence staging slice requires an active nonterminal deployment phase")]
    SupportedPhaseRequired,
    #[error("runtime convergence staging slice requires controller renewal")]
    RenewalRequired,
    #[error("runtime convergence staging slice received an unexpected plan")]
    UnexpectedPlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeConvergenceClaimKindV2 {
    Requested,
    PreflightReady,
    DrainRequested,
    StagedRecovery(RuntimeStagedRecoveryPhaseV2),
}

pub struct RuntimeClaimedConvergenceV2 {
    pub(super) session: RuntimeConvergenceSessionV1,
    pub(super) permit: RuntimeServingSlotWorkPermitV2,
    pub(super) process_identity: RuntimeProcessIdentityV1,
    pub(super) config: RuntimeControllerConfigV1,
    pub(super) preflight_timeout: Duration,
    pub(super) claim_kind: RuntimeConvergenceClaimKindV2,
}

impl RuntimeClaimedConvergenceV2 {
    pub fn from_claim(
        receipt: RuntimeExecutionReceiptV1,
        permit: RuntimeServingSlotWorkPermitV2,
        observed_at: DateTime<Utc>,
        config: RuntimeControllerConfigV1,
    ) -> Result<Self, RuntimeConvergenceStartErrorV2> {
        permit
            .ensure_active()
            .map_err(RuntimeConvergenceStartErrorV2::SlotWork)?;
        if !permit.slot().matches_target(&receipt.snapshot.target) {
            return Err(RuntimeConvergenceStartErrorV2::SlotMismatch);
        }
        let claim_kind = match &receipt.snapshot.phase {
            RuntimeDeploymentPhaseV1::Requested => RuntimeConvergenceClaimKindV2::Requested,
            RuntimeDeploymentPhaseV1::PreflightReady => {
                RuntimeConvergenceClaimKindV2::PreflightReady
            }
            RuntimeDeploymentPhaseV1::DrainRequested => {
                RuntimeConvergenceClaimKindV2::DrainRequested
            }
            RuntimeDeploymentPhaseV1::Drained => {
                RuntimeConvergenceClaimKindV2::StagedRecovery(RuntimeStagedRecoveryPhaseV2::Drained)
            }
            RuntimeDeploymentPhaseV1::ActivationApplying => {
                RuntimeConvergenceClaimKindV2::StagedRecovery(
                    RuntimeStagedRecoveryPhaseV2::ActivationApplying,
                )
            }
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Ready,
            } => RuntimeConvergenceClaimKindV2::StagedRecovery(
                RuntimeStagedRecoveryPhaseV2::RuntimePendingReady,
            ),
            RuntimeDeploymentPhaseV1::ReconcilingPanels => {
                RuntimeConvergenceClaimKindV2::StagedRecovery(
                    RuntimeStagedRecoveryPhaseV2::ReconcilingPanels,
                )
            }
            RuntimeDeploymentPhaseV1::AwaitingGatewayReady => {
                RuntimeConvergenceClaimKindV2::StagedRecovery(
                    RuntimeStagedRecoveryPhaseV2::AwaitingGatewayReady,
                )
            }
            _ => return Err(RuntimeConvergenceStartErrorV2::SupportedPhaseRequired),
        };
        let action = plan_runtime_action_v1(
            &receipt.snapshot,
            &receipt.controller_id,
            observed_at,
            &config,
        )
        .map_err(RuntimeConvergenceStartErrorV2::Plan)?;
        let preflight_timeout = match (claim_kind, action) {
            (
                RuntimeConvergenceClaimKindV2::Requested,
                RuntimeControllerActionV1::VerifyPreflight { timeout },
            ) => timeout,
            (
                RuntimeConvergenceClaimKindV2::PreflightReady,
                RuntimeControllerActionV1::RequestDrain,
            ) => config.preflight_timeout,
            (
                RuntimeConvergenceClaimKindV2::DrainRequested,
                RuntimeControllerActionV1::DrainPreviousRuntime { .. },
            ) => config.preflight_timeout,
            (
                RuntimeConvergenceClaimKindV2::StagedRecovery(
                    RuntimeStagedRecoveryPhaseV2::Drained,
                ),
                RuntimeControllerActionV1::BeginActivation,
            )
            | (
                RuntimeConvergenceClaimKindV2::StagedRecovery(
                    RuntimeStagedRecoveryPhaseV2::ActivationApplying,
                ),
                RuntimeControllerActionV1::VerifyActiveTarget { .. },
            )
            | (
                RuntimeConvergenceClaimKindV2::StagedRecovery(
                    RuntimeStagedRecoveryPhaseV2::RuntimePendingReady,
                ),
                RuntimeControllerActionV1::BeginPanelReconciliation,
            )
            | (
                RuntimeConvergenceClaimKindV2::StagedRecovery(
                    RuntimeStagedRecoveryPhaseV2::ReconcilingPanels,
                ),
                RuntimeControllerActionV1::ReconcilePanels { .. },
            )
            | (
                RuntimeConvergenceClaimKindV2::StagedRecovery(
                    RuntimeStagedRecoveryPhaseV2::AwaitingGatewayReady,
                ),
                RuntimeControllerActionV1::StartGatewayAndCertifyLive { .. },
            ) => config.preflight_timeout,
            (_, RuntimeControllerActionV1::RenewControllerLease { .. }) => {
                return Err(RuntimeConvergenceStartErrorV2::RenewalRequired);
            }
            _ => return Err(RuntimeConvergenceStartErrorV2::UnexpectedPlan),
        };
        let process_identity = RuntimeProcessIdentityV1 {
            target: receipt.snapshot.target.clone(),
            runtime_generation: receipt.snapshot.runtime_generation,
            process_instance_id: permit.process_instance_id().clone(),
        };
        let session = RuntimeConvergenceSessionV1::from_claim(receipt)
            .map_err(RuntimeConvergenceStartErrorV2::Session)?;
        permit
            .ensure_active()
            .map_err(RuntimeConvergenceStartErrorV2::SlotWork)?;
        Ok(Self {
            session,
            permit,
            process_identity,
            config,
            preflight_timeout,
            claim_kind,
        })
    }

    pub fn session(&self) -> &RuntimeConvergenceSessionV1 {
        &self.session
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        self.permit.slot()
    }

    pub fn process_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.process_identity
    }

    pub fn preflight_timeout(&self) -> Duration {
        self.preflight_timeout
    }

    pub fn claim_kind(&self) -> RuntimeConvergenceClaimKindV2 {
        self.claim_kind
    }

    pub fn ensure_active(&self) -> Result<(), RuntimeServingSlotWorkErrorV2> {
        self.permit.ensure_active()
    }
}

impl Debug for RuntimeClaimedConvergenceV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeClaimedConvergenceV2(<redacted>)")
    }
}

#[cfg(test)]
mod tests;
