use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeBindingPinV1, RuntimeConvergenceSessionV1, RuntimeDesiredTargetDigestV1,
    RuntimeDrainIntentIdV2, RuntimeExecutionGuardV1,
};
use automation_runtime_convergence::{
    FencingToken, RuntimeDeploymentPhaseV1, RuntimeProcessIdentityV1,
};
use chrono::{DateTime, TimeDelta, Utc};

use super::hydration::RuntimeHydratedCoreV2;
use super::{RuntimeAuthorityPayloadDigestV2, RuntimeConvergenceClaimKindV2};
use crate::{
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeServingSlotWorkErrorV2,
    RuntimeServingSlotWorkPermitV2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeRouteLifecycleV2 {
    Staged,
    Serving,
    DrainClaimSealed {
        intent_id: RuntimeDrainIntentIdV2,
        seal_generation: NonZeroU64,
    },
    Draining,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRouteWitnessV2 {
    pub identity: RuntimeProcessIdentityV1,
    pub controller_fencing_token: FencingToken,
    pub route_incarnation: NonZeroU64,
    pub lifecycle: RuntimeRouteLifecycleV2,
    pub active_interactions: u32,
    pub admission_generation: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeRouteStageOutcomeV2 {
    Installed,
    ExactReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeStagedRecoveryPhaseV2 {
    Drained,
    ActivationApplying,
    RuntimePendingReady,
    ReconcilingPanels,
    AwaitingGatewayReady,
}

pub struct RuntimeRouteStageRequestV2 {
    process_identity: RuntimeProcessIdentityV1,
    execution_guard: RuntimeExecutionGuardV1,
    desired_target_digest: RuntimeDesiredTargetDigestV1,
    binding_pin: RuntimeBindingPinV1,
    installation_authority_revision: NonZeroU64,
    current_authority_revision: NonZeroU64,
    installation_authority_payload_digest: RuntimeAuthorityPayloadDigestV2,
    current_authority_payload_digest: RuntimeAuthorityPayloadDigestV2,
    route_set_sequence: RuntimeRegistryGlobalObservationSequenceV2,
}

impl RuntimeRouteStageRequestV2 {
    pub fn process_identity(&self) -> &RuntimeProcessIdentityV1 {
        &self.process_identity
    }

    pub fn execution_guard(&self) -> &RuntimeExecutionGuardV1 {
        &self.execution_guard
    }

    pub fn desired_target_digest(&self) -> &RuntimeDesiredTargetDigestV1 {
        &self.desired_target_digest
    }

    pub fn binding_pin(&self) -> &RuntimeBindingPinV1 {
        &self.binding_pin
    }

    pub fn installation_authority_revision(&self) -> NonZeroU64 {
        self.installation_authority_revision
    }

    pub fn current_authority_revision(&self) -> NonZeroU64 {
        self.current_authority_revision
    }

    pub fn installation_authority_payload_digest(&self) -> &RuntimeAuthorityPayloadDigestV2 {
        &self.installation_authority_payload_digest
    }

    pub fn current_authority_payload_digest(&self) -> &RuntimeAuthorityPayloadDigestV2 {
        &self.current_authority_payload_digest
    }

    pub fn route_set_sequence(&self) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.route_set_sequence
    }
}

impl Debug for RuntimeRouteStageRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRouteStageRequestV2(<redacted>)")
    }
}

pub struct RuntimeRouteStageObservationV2<S> {
    pub outcome: RuntimeRouteStageOutcomeV2,
    pub witness: RuntimeRouteWitnessV2,
    pub staged: S,
}

impl<S> Debug for RuntimeRouteStageObservationV2<S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRouteStageObservationV2(<redacted>)")
    }
}

pub trait RuntimeStagedRoutePortV2<H> {
    type Error;
    type Staged;

    fn install_staged(
        &self,
        request: &RuntimeRouteStageRequestV2,
        hydrated: &H,
    ) -> Result<RuntimeRouteStageObservationV2<Self::Staged>, Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRouteStageEvidenceErrorV2 {
    #[error("runtime staged route process identity does not match")]
    ProcessIdentityMismatch,
    #[error("runtime staged route fencing token does not match")]
    FencingTokenMismatch,
    #[error("runtime staged route lifecycle is not Staged")]
    LifecycleMismatch,
    #[error("runtime staged route has active interactions")]
    ActiveInteractionsPresent,
    #[error("runtime staged route observation sequence regressed")]
    ObservationSequenceRegression,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeRouteStageErrorV2<E> {
    #[error("runtime staged route slot work authority is not active")]
    SlotWork(RuntimeServingSlotWorkErrorV2),
    #[error("runtime staged route completion time is invalid")]
    InvalidCompletionTime,
    #[error("runtime staged route session guard is unavailable")]
    SessionGuardUnavailable,
    #[error("runtime staged route port failed")]
    Port(E),
    #[error("runtime staged route evidence is invalid")]
    Evidence(RuntimeRouteStageEvidenceErrorV2),
}

pub struct RuntimeStageReadyConvergenceV2<H> {
    pub(super) core: RuntimeHydratedCoreV2<H>,
    pub(super) ready_at: DateTime<Utc>,
}

impl<H> RuntimeStageReadyConvergenceV2<H> {
    pub(super) fn from_preflight(core: RuntimeHydratedCoreV2<H>, ready_at: DateTime<Utc>) -> Self {
        Self { core, ready_at }
    }

    pub fn evidence(&self) -> &super::RuntimeExactTargetEvidenceV2 {
        &self.core.evidence
    }

    pub fn hydrated(&self) -> &H {
        &self.core.hydrated
    }
}

impl<H> Debug for RuntimeStageReadyConvergenceV2<H> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStageReadyConvergenceV2(<redacted>)")
    }
}

pub struct RuntimeRefreshedStageReadyConvergenceV2<H> {
    core: RuntimeHydratedCoreV2<H>,
    refreshed_at: DateTime<Utc>,
}

impl<H> RuntimeRefreshedStageReadyConvergenceV2<H> {
    pub(super) fn from_refresh(
        core: RuntimeHydratedCoreV2<H>,
        refreshed_at: DateTime<Utc>,
    ) -> Self {
        Self { core, refreshed_at }
    }

    pub fn evidence(&self) -> &super::RuntimeExactTargetEvidenceV2 {
        &self.core.evidence
    }

    pub fn hydrated(&self) -> &H {
        &self.core.hydrated
    }

    pub fn stage<P>(
        self,
        port: &P,
        completed_at: DateTime<Utc>,
    ) -> Result<RuntimeStagedConvergenceV2<H, P::Staged>, RuntimeRouteStageErrorV2<P::Error>>
    where
        P: RuntimeStagedRoutePortV2<H>,
    {
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeRouteStageErrorV2::SlotWork)?;
        if !valid_stage_completion_time(
            self.refreshed_at,
            self.core.claimed.session.expires_at(),
            self.core.claimed.config.controller_renew_before,
            completed_at,
        ) {
            return Err(RuntimeRouteStageErrorV2::InvalidCompletionTime);
        }
        let execution_guard = self
            .core
            .claimed
            .session
            .execution_guard()
            .map_err(|_| RuntimeRouteStageErrorV2::SessionGuardUnavailable)?;
        let request = RuntimeRouteStageRequestV2 {
            process_identity: self.core.claimed.process_identity.clone(),
            execution_guard,
            desired_target_digest: self.core.evidence.persisted_desired_target_digest().clone(),
            binding_pin: self.core.evidence.binding_pin().clone(),
            installation_authority_revision: self.core.evidence.installation_authority_revision(),
            current_authority_revision: self.core.evidence.current_authority_revision(),
            installation_authority_payload_digest: self
                .core
                .evidence
                .installation_authority_payload_digest()
                .clone(),
            current_authority_payload_digest: self
                .core
                .evidence
                .current_authority_payload_digest()
                .clone(),
            route_set_sequence: self.core.claimed.permit.route_set_sequence(),
        };
        let observation = port
            .install_staged(&request, &self.core.hydrated)
            .map_err(RuntimeRouteStageErrorV2::Port)?;
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeRouteStageErrorV2::SlotWork)?;
        validate_stage_observation(&request, &observation.witness)
            .map_err(RuntimeRouteStageErrorV2::Evidence)?;
        self.core
            .claimed
            .ensure_active()
            .map_err(RuntimeRouteStageErrorV2::SlotWork)?;
        Ok(RuntimeStagedConvergenceV2 {
            staged: observation.staged,
            witness: observation.witness,
            core: self.core,
        })
    }
}

pub(super) fn valid_stage_completion_time(
    refreshed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    renew_before: std::time::Duration,
    completed_at: DateTime<Utc>,
) -> bool {
    let Ok(renew_before) = TimeDelta::from_std(renew_before) else {
        return false;
    };
    let Some(renew_at) = expires_at.checked_sub_signed(renew_before) else {
        return false;
    };
    completed_at >= refreshed_at && completed_at < renew_at
}

impl<H> Debug for RuntimeRefreshedStageReadyConvergenceV2<H> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRefreshedStageReadyConvergenceV2(<redacted>)")
    }
}

pub struct RuntimeStagedConvergenceV2<H, S> {
    pub(super) staged: S,
    pub(super) witness: RuntimeRouteWitnessV2,
    pub(super) core: RuntimeHydratedCoreV2<H>,
}

impl<H, S> RuntimeStagedConvergenceV2<H, S> {
    pub fn witness(&self) -> &RuntimeRouteWitnessV2 {
        &self.witness
    }

    pub fn hydrated(&self) -> &H {
        &self.core.hydrated
    }

    pub fn staged(&self) -> &S {
        &self.staged
    }

    pub fn ensure_active(&self) -> Result<(), RuntimeServingSlotWorkErrorV2> {
        self.core.claimed.ensure_active()
    }

    pub fn into_handoff(
        self,
    ) -> (
        automation_runtime_controller::RuntimeConvergenceSessionV1,
        crate::RuntimeServingSlotWorkPermitV2,
        super::RuntimeExactTargetEvidenceV2,
        H,
        RuntimeRouteWitnessV2,
        S,
    ) {
        (
            self.core.claimed.session,
            self.core.claimed.permit,
            self.core.evidence,
            self.core.hydrated,
            self.witness,
            self.staged,
        )
    }

    pub fn into_staged_recovery(
        self,
    ) -> Result<RuntimeStagedRecoveryHandoffV2<H, S>, Box<RuntimeStagedConvergenceV2<H, S>>> {
        let RuntimeConvergenceClaimKindV2::StagedRecovery(phase) = self.core.claimed.claim_kind
        else {
            return Err(Box::new(self));
        };
        if !phase_matches_snapshot(phase, &self.core.claimed.session.snapshot().phase) {
            return Err(Box::new(self));
        }
        let route = RuntimeStagedRecoveryRouteV2 {
            staged: self.staged,
            witness: self.witness,
            core: self.core,
        };
        Ok(match phase {
            RuntimeStagedRecoveryPhaseV2::Drained => RuntimeStagedRecoveryHandoffV2::Drained(route),
            RuntimeStagedRecoveryPhaseV2::ActivationApplying => {
                RuntimeStagedRecoveryHandoffV2::ActivationApplying(route)
            }
            RuntimeStagedRecoveryPhaseV2::RuntimePendingReady => {
                RuntimeStagedRecoveryHandoffV2::RuntimePendingReady(route)
            }
            RuntimeStagedRecoveryPhaseV2::ReconcilingPanels => {
                RuntimeStagedRecoveryHandoffV2::ReconcilingPanels(route)
            }
            RuntimeStagedRecoveryPhaseV2::AwaitingGatewayReady => {
                RuntimeStagedRecoveryHandoffV2::AwaitingGatewayReady(route)
            }
        })
    }
}

impl<H, S> Debug for RuntimeStagedConvergenceV2<H, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStagedConvergenceV2(<redacted>)")
    }
}

pub type RuntimeStagedRecoveryRouteHandoffV2<H, S> = (
    S,
    RuntimeConvergenceSessionV1,
    RuntimeServingSlotWorkPermitV2,
    super::RuntimeExactTargetEvidenceV2,
    H,
    RuntimeRouteWitnessV2,
);

pub struct RuntimeStagedRecoveryRouteV2<H, S> {
    staged: S,
    witness: RuntimeRouteWitnessV2,
    core: RuntimeHydratedCoreV2<H>,
}

impl<H, S> RuntimeStagedRecoveryRouteV2<H, S> {
    pub fn session(&self) -> &RuntimeConvergenceSessionV1 {
        &self.core.claimed.session
    }

    pub fn staged(&self) -> &S {
        &self.staged
    }

    pub fn hydrated(&self) -> &H {
        &self.core.hydrated
    }

    pub fn witness(&self) -> &RuntimeRouteWitnessV2 {
        &self.witness
    }

    pub fn evidence(&self) -> &super::RuntimeExactTargetEvidenceV2 {
        &self.core.evidence
    }

    pub fn ensure_active(&self) -> Result<(), RuntimeServingSlotWorkErrorV2> {
        self.core.claimed.ensure_active()
    }

    pub fn into_handoff(self) -> RuntimeStagedRecoveryRouteHandoffV2<H, S> {
        (
            self.staged,
            self.core.claimed.session,
            self.core.claimed.permit,
            self.core.evidence,
            self.core.hydrated,
            self.witness,
        )
    }
}

impl<H, S> Debug for RuntimeStagedRecoveryRouteV2<H, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStagedRecoveryRouteV2(<redacted>)")
    }
}

pub enum RuntimeStagedRecoveryHandoffV2<H, S> {
    Drained(RuntimeStagedRecoveryRouteV2<H, S>),
    ActivationApplying(RuntimeStagedRecoveryRouteV2<H, S>),
    RuntimePendingReady(RuntimeStagedRecoveryRouteV2<H, S>),
    ReconcilingPanels(RuntimeStagedRecoveryRouteV2<H, S>),
    AwaitingGatewayReady(RuntimeStagedRecoveryRouteV2<H, S>),
}

impl<H, S> RuntimeStagedRecoveryHandoffV2<H, S> {
    pub fn phase(&self) -> RuntimeStagedRecoveryPhaseV2 {
        match self {
            Self::Drained(_) => RuntimeStagedRecoveryPhaseV2::Drained,
            Self::ActivationApplying(_) => RuntimeStagedRecoveryPhaseV2::ActivationApplying,
            Self::RuntimePendingReady(_) => RuntimeStagedRecoveryPhaseV2::RuntimePendingReady,
            Self::ReconcilingPanels(_) => RuntimeStagedRecoveryPhaseV2::ReconcilingPanels,
            Self::AwaitingGatewayReady(_) => RuntimeStagedRecoveryPhaseV2::AwaitingGatewayReady,
        }
    }

    pub fn route(&self) -> &RuntimeStagedRecoveryRouteV2<H, S> {
        match self {
            Self::Drained(route)
            | Self::ActivationApplying(route)
            | Self::RuntimePendingReady(route)
            | Self::ReconcilingPanels(route)
            | Self::AwaitingGatewayReady(route) => route,
        }
    }

    pub fn into_route(self) -> RuntimeStagedRecoveryRouteV2<H, S> {
        match self {
            Self::Drained(route)
            | Self::ActivationApplying(route)
            | Self::RuntimePendingReady(route)
            | Self::ReconcilingPanels(route)
            | Self::AwaitingGatewayReady(route) => route,
        }
    }
}

impl<H, S> Debug for RuntimeStagedRecoveryHandoffV2<H, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeStagedRecoveryHandoffV2(<redacted>)")
    }
}

fn phase_matches_snapshot(
    phase: RuntimeStagedRecoveryPhaseV2,
    snapshot_phase: &RuntimeDeploymentPhaseV1,
) -> bool {
    matches!(
        (phase, snapshot_phase),
        (
            RuntimeStagedRecoveryPhaseV2::Drained,
            RuntimeDeploymentPhaseV1::Drained
        ) | (
            RuntimeStagedRecoveryPhaseV2::ActivationApplying,
            RuntimeDeploymentPhaseV1::ActivationApplying
        ) | (
            RuntimeStagedRecoveryPhaseV2::RuntimePendingReady,
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: automation_runtime_convergence::RuntimePendingConditionV1::Ready
            }
        ) | (
            RuntimeStagedRecoveryPhaseV2::ReconcilingPanels,
            RuntimeDeploymentPhaseV1::ReconcilingPanels
        ) | (
            RuntimeStagedRecoveryPhaseV2::AwaitingGatewayReady,
            RuntimeDeploymentPhaseV1::AwaitingGatewayReady
        )
    )
}

fn validate_stage_observation(
    request: &RuntimeRouteStageRequestV2,
    witness: &RuntimeRouteWitnessV2,
) -> Result<(), RuntimeRouteStageEvidenceErrorV2> {
    if witness.identity != request.process_identity {
        return Err(RuntimeRouteStageEvidenceErrorV2::ProcessIdentityMismatch);
    }
    if witness.controller_fencing_token != request.execution_guard.fencing_token {
        return Err(RuntimeRouteStageEvidenceErrorV2::FencingTokenMismatch);
    }
    if !matches!(witness.lifecycle, RuntimeRouteLifecycleV2::Staged) {
        return Err(RuntimeRouteStageEvidenceErrorV2::LifecycleMismatch);
    }
    if witness.active_interactions != 0 {
        return Err(RuntimeRouteStageEvidenceErrorV2::ActiveInteractionsPresent);
    }
    if witness.registry_observation_sequence.get() < request.route_set_sequence.get() {
        return Err(RuntimeRouteStageEvidenceErrorV2::ObservationSequenceRegression);
    }
    Ok(())
}
