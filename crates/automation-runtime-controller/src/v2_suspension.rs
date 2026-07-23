use std::num::NonZeroU64;

use automation_runtime_convergence::{
    RuntimeDeploymentPhaseV1, RuntimeFailureV1, RuntimePendingConditionV1,
};
use chrono::{DateTime, Utc};

use crate::{
    RuntimeExactLocalRouteIdentityV2, RuntimeExecutionGuardV1,
    RuntimePreviousServingLeaseIdentityV1, RuntimeRouteMutationProvenanceV2, RuntimeServingSlotV2,
    RuntimeSessionActionIdV1, RuntimeSuspensionIdV2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeAttemptDispositionV2 {
    Retryable { retry_not_before: DateTime<Utc> },
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeResumeCheckpointV2 {
    VerifyPreflight,
    RequestDrain,
    CompleteDrain,
    BeginActivation,
    ObserveActivation,
    BeginPanels,
    ReconcilePanels,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSuspensionSourcePhaseV2 {
    Requested,
    PreflightReady,
    DrainRequested,
    Drained,
    ActivationApplying,
    RuntimePendingReady,
    ReconcilingPanels,
}

impl RuntimeSuspensionSourcePhaseV2 {
    pub fn from_deployment_phase(phase: &RuntimeDeploymentPhaseV1) -> Option<Self> {
        match phase {
            RuntimeDeploymentPhaseV1::Requested => Some(Self::Requested),
            RuntimeDeploymentPhaseV1::PreflightReady => Some(Self::PreflightReady),
            RuntimeDeploymentPhaseV1::DrainRequested => Some(Self::DrainRequested),
            RuntimeDeploymentPhaseV1::Drained => Some(Self::Drained),
            RuntimeDeploymentPhaseV1::ActivationApplying => Some(Self::ActivationApplying),
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Ready,
            } => Some(Self::RuntimePendingReady),
            RuntimeDeploymentPhaseV1::ReconcilingPanels => Some(Self::ReconcilingPanels),
            RuntimeDeploymentPhaseV1::RuntimePending { .. }
            | RuntimeDeploymentPhaseV1::AwaitingGatewayReady
            | RuntimeDeploymentPhaseV1::Live
            | RuntimeDeploymentPhaseV1::Superseded { .. }
            | RuntimeDeploymentPhaseV1::Cancelled { .. } => None,
        }
    }

    pub const fn required_checkpoint(self) -> RuntimeResumeCheckpointV2 {
        match self {
            Self::Requested => RuntimeResumeCheckpointV2::VerifyPreflight,
            Self::PreflightReady => RuntimeResumeCheckpointV2::RequestDrain,
            Self::DrainRequested => RuntimeResumeCheckpointV2::CompleteDrain,
            Self::Drained => RuntimeResumeCheckpointV2::BeginActivation,
            Self::ActivationApplying => RuntimeResumeCheckpointV2::ObserveActivation,
            Self::RuntimePendingReady => RuntimeResumeCheckpointV2::BeginPanels,
            Self::ReconcilingPanels => RuntimeResumeCheckpointV2::ReconcilePanels,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSuspendedRouteLifecycleV2 {
    Staged,
    Draining,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeDrainObligationV2 {
    None,
    ExactLocalRoute(RuntimeExactLocalRouteIdentityV2),
    PreviousServing(RuntimePreviousServingLeaseIdentityV1),
    LocalAndPrevious {
        local: RuntimeExactLocalRouteIdentityV2,
        previous: RuntimePreviousServingLeaseIdentityV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "route absence preserves the exact provenance payload without wire-shape indirection"
)]
pub enum RuntimeLocalRouteEffectV2 {
    None,
    ExactRoute {
        route: RuntimeExactLocalRouteIdentityV2,
        lifecycle: RuntimeSuspendedRouteLifecycleV2,
    },
    RouteAbsent {
        slot: RuntimeServingSlotV2,
        expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
        provenance: RuntimeRouteMutationProvenanceV2,
        observed_sequence: NonZeroU64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendAttemptRequestV2 {
    pub suspension_id: RuntimeSuspensionIdV2,
    pub action_id: RuntimeSessionActionIdV1,
    pub guard: RuntimeExecutionGuardV1,
    pub source_phase: RuntimeSuspensionSourcePhaseV2,
    pub failure: RuntimeFailureV1,
    pub disposition: RuntimeAttemptDispositionV2,
    pub checkpoint: RuntimeResumeCheckpointV2,
    pub local_effect: RuntimeLocalRouteEffectV2,
    pub drain_obligation: RuntimeDrainObligationV2,
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};

    use automation_runtime_convergence::{
        ControllerId, DeploymentId, DeploymentRevision, FencingToken, InstallationId,
        RuntimeDeploymentPhaseV1, RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1,
        RuntimeGeneration, RuntimePendingConditionV1, TenantId,
    };
    use chrono::{DateTime, Utc};

    use super::{
        RuntimeAttemptDispositionV2, RuntimeDrainObligationV2, RuntimeLocalRouteEffectV2,
        RuntimeResumeCheckpointV2, RuntimeSuspendAttemptRequestV2, RuntimeSuspensionSourcePhaseV2,
    };
    use crate::{
        RuntimeDeploymentScopeV1, RuntimeExecutionGuardV1, RuntimeSessionActionIdV1,
        RuntimeSuspensionIdV2,
    };

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn failure() -> RuntimeFailureV1 {
        RuntimeFailureV1 {
            failure_id: RuntimeFailureId::parse("failure:1").unwrap(),
            kind: RuntimeFailureKindV1::EnvironmentUnavailable,
            code: "dependency_unavailable".to_string(),
            message: "dependency unavailable".to_string(),
            recorded_at: at(20),
        }
    }

    fn guard() -> RuntimeExecutionGuardV1 {
        RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("tenant:1").unwrap(),
                installation_id: InstallationId::parse("installation:1").unwrap(),
                deployment_id: DeploymentId::parse("deployment:1").unwrap(),
            },
            expected_revision: DeploymentRevision::new(7).unwrap(),
            controller_id: ControllerId::parse("controller:1").unwrap(),
            fencing_token: FencingToken::new(8).unwrap(),
            runtime_generation: RuntimeGeneration::new(9).unwrap(),
            convergence_attempt: NonZeroU32::new(2).unwrap(),
        }
    }

    #[test]
    fn source_phase_projects_only_the_seven_resumable_phases() {
        for (phase, source, checkpoint) in [
            (
                RuntimeDeploymentPhaseV1::Requested,
                RuntimeSuspensionSourcePhaseV2::Requested,
                RuntimeResumeCheckpointV2::VerifyPreflight,
            ),
            (
                RuntimeDeploymentPhaseV1::PreflightReady,
                RuntimeSuspensionSourcePhaseV2::PreflightReady,
                RuntimeResumeCheckpointV2::RequestDrain,
            ),
            (
                RuntimeDeploymentPhaseV1::DrainRequested,
                RuntimeSuspensionSourcePhaseV2::DrainRequested,
                RuntimeResumeCheckpointV2::CompleteDrain,
            ),
            (
                RuntimeDeploymentPhaseV1::Drained,
                RuntimeSuspensionSourcePhaseV2::Drained,
                RuntimeResumeCheckpointV2::BeginActivation,
            ),
            (
                RuntimeDeploymentPhaseV1::ActivationApplying,
                RuntimeSuspensionSourcePhaseV2::ActivationApplying,
                RuntimeResumeCheckpointV2::ObserveActivation,
            ),
            (
                RuntimeDeploymentPhaseV1::RuntimePending {
                    condition: RuntimePendingConditionV1::Ready,
                },
                RuntimeSuspensionSourcePhaseV2::RuntimePendingReady,
                RuntimeResumeCheckpointV2::BeginPanels,
            ),
            (
                RuntimeDeploymentPhaseV1::ReconcilingPanels,
                RuntimeSuspensionSourcePhaseV2::ReconcilingPanels,
                RuntimeResumeCheckpointV2::ReconcilePanels,
            ),
        ] {
            let projected = RuntimeSuspensionSourcePhaseV2::from_deployment_phase(&phase).unwrap();
            assert_eq!(projected, source);
            assert_eq!(projected.required_checkpoint(), checkpoint);
        }
    }

    #[test]
    fn source_phase_rejects_legacy_pending_certification_live_and_terminal_states() {
        for phase in [
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { failure: failure() },
            },
            RuntimeDeploymentPhaseV1::AwaitingGatewayReady,
            RuntimeDeploymentPhaseV1::Live,
            RuntimeDeploymentPhaseV1::Cancelled {
                reason: "cancelled".to_string(),
                cancelled_at: at(30),
            },
        ] {
            assert_eq!(
                RuntimeSuspensionSourcePhaseV2::from_deployment_phase(&phase),
                None
            );
        }
    }

    #[test]
    fn suspend_request_preserves_the_exact_nine_inputs() {
        let request = RuntimeSuspendAttemptRequestV2 {
            suspension_id: RuntimeSuspensionIdV2::parse("00112233445566778899aabbccddeeff")
                .unwrap(),
            action_id: RuntimeSessionActionIdV1::new(non_zero(10)),
            guard: guard(),
            source_phase: RuntimeSuspensionSourcePhaseV2::Requested,
            failure: failure(),
            disposition: RuntimeAttemptDispositionV2::Retryable {
                retry_not_before: at(40),
            },
            checkpoint: RuntimeResumeCheckpointV2::VerifyPreflight,
            local_effect: RuntimeLocalRouteEffectV2::None,
            drain_obligation: RuntimeDrainObligationV2::None,
        };

        assert_eq!(
            request.suspension_id.as_str(),
            "00112233445566778899aabbccddeeff"
        );
        assert_eq!(request.action_id.get(), 10);
        assert_eq!(request.guard, guard());
        assert_eq!(
            request.source_phase.required_checkpoint(),
            request.checkpoint
        );
        assert_eq!(request.failure, failure());
        assert_eq!(
            request.disposition,
            RuntimeAttemptDispositionV2::Retryable {
                retry_not_before: at(40),
            }
        );
        assert_eq!(request.local_effect, RuntimeLocalRouteEffectV2::None);
        assert_eq!(request.drain_obligation, RuntimeDrainObligationV2::None);
    }
}
