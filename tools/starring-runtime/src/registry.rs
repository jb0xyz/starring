use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::sync::{Arc, Mutex, MutexGuard};

use automation_runtime_controller::RuntimeServingSlotV2;
use automation_runtime_convergence::{FencingToken, ProcessInstanceId, RuntimeProcessIdentityV1};
use automation_runtime_registry::{
    ExactServingRouteV1, RegistryEmptyRecoveryCursorV2, RegistryRecoveryObservationGuardV2,
    RegistryRecoveryObservationV2, SealedEmptyRecoveryDrainClaimV2, ServingSlotKeyV1,
    ServingSlotRegistryConfigV1, ServingSlotRegistryError, ServingSlotRegistryV1,
    SlotActivationOutcomeV1, SlotAdmissionStateV2, SlotAtomicObservationV2, SlotDrainOutcomeV1,
    SlotInstallOutcomeV1, SlotLifecycleV1, SlotMutationTokenV1, SlotRemovalOutcomeV1,
    SlotRouteWitnessV1, SlotSealKeyV2,
};
use automation_runtime_worker::{
    accept_runtime_registry_recovery_empty_observation_v2, accept_runtime_route_set_observation_v2,
    RuntimeDurablyAcknowledgedPendingDrainSuccessionV3, RuntimeDurablyAcknowledgedPendingDrainV2,
    RuntimePendingDrainCandidateV2, RuntimePendingDrainCompoundErrorV2,
    RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    RuntimePendingDrainRegistrySealWitnessInputV2, RuntimePendingDrainRegistrySealWitnessV2,
    RuntimePendingDrainRegistryUnsealWitnessV2, RuntimePendingDrainSlotObservationV2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryEmptyObservationV2,
    RuntimeRegistryRecoveryObservationErrorV2, RuntimeRegistryRecoveryObservationInputV2,
    RuntimeRouteSetEpochV2, RuntimeRouteSetObservationErrorV2, RuntimeRouteSetObservationInputV2,
    RuntimeRouteSetObservationV2,
};

use crate::closed_recovery::RuntimeClosedRecoveryTransitionAuthorityV2;
use crate::gateway::{
    RuntimeDiscordCertificationBarrierBActivatedV2, RuntimeDiscordCertificationBarrierBPausedV2,
    RuntimeEmergencyGatewaySectionV2, RuntimeRecoveryPendingGatewaySectionV2,
};
use crate::GatewayResourceConfigV1;

const REGISTRY_MAX_SLOTS: NonZeroU32 = NonZeroU32::new(4_096).unwrap();
const REGISTRY_MAX_RETIRED_ROUTES_PER_SLOT: NonZeroU32 = NonZeroU32::new(8).unwrap();

#[allow(dead_code)]
pub(crate) fn runtime_registry_max_slots_v2() -> NonZeroUsize {
    NonZeroUsize::new(
        usize::try_from(REGISTRY_MAX_SLOTS.get())
            .expect("runtime registry maximum slots must fit usize"),
    )
    .expect("runtime registry maximum slots must remain nonzero")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRegistryBootstrapErrorV1 {
    #[error("runtime registry active interaction capacity is outside its supported domain")]
    ActiveInteractionCapacity,
}

impl RuntimeRegistryBootstrapErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ActiveInteractionCapacity => "runtime_registry_active_interaction_capacity",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRegistryRecoveryObservationErrorV1 {
    #[error("runtime registry is unavailable")]
    RegistryUnavailable,
    #[error("runtime registry recovery observation is invalid")]
    ObservationInvalid,
    #[error("runtime registry recovery observation overflowed")]
    ObservationOverflow,
    #[error("runtime registry recovery observation is failed closed")]
    FailedClosed,
    #[error("runtime registry recovery observation is not empty")]
    NotEmpty,
    #[error("runtime registry recovery retained counts are inconsistent")]
    InconsistentRetainedCounts,
    #[error("runtime registry recovery observation sequence is outside the persistence domain")]
    ObservationSequenceOutOfRange,
    #[error("runtime registry empty recovery binding is stale")]
    StaleEmptyBinding,
    #[error("runtime registry recovery protocol was violated")]
    ProtocolViolation,
}

impl RuntimeRegistryRecoveryObservationErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::RegistryUnavailable => "runtime_registry_unavailable",
            Self::ObservationInvalid => "runtime_registry_observation_invalid",
            Self::ObservationOverflow => "runtime_registry_observation_overflow",
            Self::FailedClosed => "runtime_registry_failed_closed",
            Self::NotEmpty => "runtime_registry_not_empty",
            Self::InconsistentRetainedCounts => "runtime_registry_retained_counts_inconsistent",
            Self::ObservationSequenceOutOfRange => {
                "runtime_registry_observation_sequence_out_of_range"
            }
            Self::StaleEmptyBinding => "runtime_registry_empty_binding_stale",
            Self::ProtocolViolation => "runtime_registry_protocol_violation",
        }
    }
}

pub struct RuntimeRegistryBootstrapV1 {
    process_instance_id: ProcessInstanceId,
    registry: ServingSlotRegistryV1,
}

#[allow(dead_code)]
pub(crate) struct RuntimeInteractionDispatchRegistryV1 {
    registry: ServingSlotRegistryV1,
}

impl RuntimeInteractionDispatchRegistryV1 {
    pub(crate) fn into_registry_v1(self) -> ServingSlotRegistryV1 {
        self.registry
    }
}

impl Debug for RuntimeInteractionDispatchRegistryV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionDispatchRegistryV1(<redacted>)")
    }
}

impl RuntimeRegistryBootstrapV1 {
    #[allow(dead_code)]
    pub(crate) fn interaction_dispatch_registry_v1(&self) -> RuntimeInteractionDispatchRegistryV1 {
        RuntimeInteractionDispatchRegistryV1 {
            registry: self.registry.clone(),
        }
    }

    pub fn observe_recovery_empty_projection_v2(
        &self,
    ) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        self.recovery_observation_guard_unordered_v2()?
            .empty_projection_v2()
    }

    pub(crate) fn observe_shutdown_route_set_v2(
        &self,
    ) -> Result<RuntimeRouteSetObservationV2, RuntimeRegistryRecoveryObservationErrorV1> {
        let observation = self
            .registry
            .recovery_observation_guard_v2()
            .map_err(map_registry_observation_error)?
            .observation();
        project_route_set_observation_v2(&self.process_instance_id, observation)
    }

    pub(crate) fn recovery_observation_guard_v2(
        &self,
        _authority: &RuntimeClosedRecoveryTransitionAuthorityV2,
        _section: &RuntimeEmergencyGatewaySectionV2<'_>,
    ) -> Result<RuntimeRegistryRecoveryGuardV1<'_>, RuntimeRegistryRecoveryObservationErrorV1> {
        self.recovery_observation_guard_unordered_v2()
    }

    fn recovery_observation_guard_unordered_v2(
        &self,
    ) -> Result<RuntimeRegistryRecoveryGuardV1<'_>, RuntimeRegistryRecoveryObservationErrorV1> {
        let guard = self
            .registry
            .recovery_observation_guard_v2()
            .map_err(map_registry_observation_error)?;
        Ok(RuntimeRegistryRecoveryGuardV1 {
            bootstrap: self,
            guard,
        })
    }
}

impl Debug for RuntimeRegistryBootstrapV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBootstrapV1(<redacted>)")
    }
}

pub(crate) struct RuntimeRegistryRecoveryGuardV1<'a> {
    bootstrap: &'a RuntimeRegistryBootstrapV1,
    guard: RegistryRecoveryObservationGuardV2<'a>,
}

impl<'a> RuntimeRegistryRecoveryGuardV1<'a> {
    fn empty_projection_v2(
        &self,
    ) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        project_empty_observation_v2(
            &self.bootstrap.process_instance_id,
            self.guard.observation(),
        )
    }

    pub(crate) fn locked_empty_evidence_v2<'evidence>(
        &'evidence self,
    ) -> Result<
        RuntimeLockedRegistryEmptyEvidenceV2<'evidence, 'a>,
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        let observation = project_empty_observation_v2(
            &self.bootstrap.process_instance_id,
            self.guard.observation(),
        )?;
        Ok(RuntimeLockedRegistryEmptyEvidenceV2 {
            observation,
            _guard: self,
        })
    }

    pub(crate) fn into_empty_binding_v2(
        self,
    ) -> Result<RuntimeRegistryEmptyRecoveryBindingV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        let cursor = self
            .guard
            .into_empty_cursor()
            .map_err(map_registry_observation_error)?;
        Ok(RuntimeRegistryEmptyRecoveryBindingV2 {
            process_instance_id: self.bootstrap.process_instance_id.clone(),
            registry: self.bootstrap.registry.clone(),
            cursor,
        })
    }
}

pub(crate) struct RuntimeLockedRegistryEmptyEvidenceV2<'evidence, 'registry> {
    observation: RuntimeRegistryRecoveryEmptyObservationV2,
    _guard: &'evidence RuntimeRegistryRecoveryGuardV1<'registry>,
}

impl RuntimeLockedRegistryEmptyEvidenceV2<'_, '_> {
    pub(crate) fn into_observation_v2(self) -> RuntimeRegistryRecoveryEmptyObservationV2 {
        self.observation
    }
}

impl Debug for RuntimeLockedRegistryEmptyEvidenceV2<'_, '_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLockedRegistryEmptyEvidenceV2(<redacted>)")
    }
}

pub(crate) struct RuntimeRegistryEmptyRecoveryBindingV2 {
    process_instance_id: ProcessInstanceId,
    registry: ServingSlotRegistryV1,
    cursor: RegistryEmptyRecoveryCursorV2,
}

pub(crate) struct RuntimeRegistryPreparedServingTransitionV2 {
    binding: RuntimeRegistryEmptyRecoveryBindingV2,
    initial_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    initial_retained_slot_count: u64,
    initial_retained_empty_tombstone_count: u64,
}

pub(crate) struct RuntimeRegistryServingBindingV2 {
    process_instance_id: ProcessInstanceId,
    registry: ServingSlotRegistryV1,
    initial_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    initial_retained_slot_count: u64,
    initial_retained_empty_tombstone_count: u64,
}

#[derive(Clone)]
pub(crate) struct RuntimeRegistryStagingPortV2 {
    process_instance_id: ProcessInstanceId,
    registry: ServingSlotRegistryV1,
}

#[derive(Clone)]
pub(crate) struct RuntimeRegistryEmergencyTriggerV2 {
    trip: Arc<dyn Fn() + Send + Sync>,
}

impl RuntimeRegistryEmergencyTriggerV2 {
    pub(crate) fn new(trip: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            trip: Arc::new(trip),
        }
    }

    fn trip_v2(&self) {
        (self.trip)();
    }
}

impl Debug for RuntimeRegistryEmergencyTriggerV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryEmergencyTriggerV2(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeRegistryStagingErrorV2 {
    #[error("runtime staged route belongs to another process")]
    ProcessMismatch,
    #[error("runtime exact route is not exclusively staged")]
    UnexpectedLifecycle,
    #[error("runtime staged route evidence is inconsistent")]
    EvidenceMismatch,
    #[error("runtime staged route registry operation failed")]
    Registry(ServingSlotRegistryError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeRegistryStagedInstallOutcomeV2 {
    Installed,
    ExactReplay,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct RuntimeRegistryStagedInstallEvidenceV2 {
    route: SlotRouteWitnessV1,
    active_interactions: u32,
    admission_generation: NonZeroU64,
    slot_observation_sequence: NonZeroU64,
    registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
}

pub(crate) struct RuntimeRegistryStagedRouteV2 {
    registry: ServingSlotRegistryV1,
    identity: RuntimeProcessIdentityV1,
    token: Option<SlotMutationTokenV1>,
    emergency: RuntimeRegistryEmergencyTriggerV2,
}

pub(crate) struct RuntimeRegistryReplacementRouteV2 {
    identity: RuntimeProcessIdentityV1,
    state: Mutex<RuntimeRegistryReplacementStateV2>,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeRegistryBarrierBActivationV2 {
    evidence: RuntimeRegistryBarrierBActivationEvidenceV2,
    authority: RuntimeRegistryBarrierBServingAuthorityV2,
}

#[must_use]
#[allow(dead_code)]
pub(crate) struct RuntimeRegistryCertificationBarrierBActivationFailureV2 {
    paused: Box<RuntimeDiscordCertificationBarrierBPausedV2>,
    source: RuntimeRegistryBarrierBActivationErrorV2,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeRegistryBarrierBServingAuthorityV2 {
    registry: ServingSlotRegistryV1,
    token: SlotMutationTokenV1,
    route: SlotRouteWitnessV1,
    activation_sequence: NonZeroU64,
    emergency: RuntimeRegistryEmergencyTriggerV2,
    armed: bool,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeRegistryBarrierBServingMonitorAuthorityV2 {
    registry: ServingSlotRegistryV1,
    token: SlotMutationTokenV1,
    route: SlotRouteWitnessV1,
    activation_sequence: NonZeroU64,
    emergency: RuntimeRegistryEmergencyTriggerV2,
    completion_liveness: Arc<Mutex<bool>>,
    armed: bool,
}

#[must_use]
#[allow(dead_code)]
pub(crate) struct RuntimeRegistryBarrierBServingCompletionWitnessV2 {
    registry: ServingSlotRegistryV1,
    token: SlotMutationTokenV1,
    route: SlotRouteWitnessV1,
    activation_sequence: NonZeroU64,
    emergency: RuntimeRegistryEmergencyTriggerV2,
    completion_liveness: Arc<Mutex<bool>>,
}

#[must_use]
#[allow(dead_code)]
pub(crate) struct RuntimeRegistryBarrierBServingCompletionGuardV2<'a> {
    _liveness: MutexGuard<'a, bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeRegistryBarrierBActivationOutcomeV2 {
    Activated,
    AlreadyServing,
}

#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeRegistryBarrierBActivationEvidenceV2 {
    outcome: RuntimeRegistryBarrierBActivationOutcomeV2,
    route: SlotRouteWitnessV1,
    activation_sequence: NonZeroU64,
    active_interactions: u32,
    admission_generation: NonZeroU64,
    slot_observation_sequence: NonZeroU64,
}

#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeRegistryBarrierBExactServingObservationV2 {
    route: SlotRouteWitnessV1,
    activation_sequence: NonZeroU64,
    active_interactions: u32,
    admission_generation: NonZeroU64,
    slot_observation_sequence: NonZeroU64,
}

#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeRegistryBarrierBRemovalEvidenceV2 {
    route: SlotRouteWitnessV1,
    activation_sequence: NonZeroU64,
    draining_admission_generation: NonZeroU64,
    draining_slot_observation_sequence: NonZeroU64,
    removed_admission_generation: NonZeroU64,
    removed_slot_observation_sequence: NonZeroU64,
}

struct RuntimeRegistryReplacementStateV2 {
    staged: RuntimeRegistryStagedRouteV2,
    predecessor: RuntimeRegistryPredecessorStateV2,
}

enum RuntimeRegistryPredecessorStateV2 {
    Unverified,
    Absent,
    Draining {
        token: SlotMutationTokenV1,
        witness: SlotRouteWitnessV1,
        initial_active_interactions: u32,
    },
    Removed {
        witness: Option<SlotRouteWitnessV1>,
        initial_active_interactions: u32,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct RuntimeRegistryPredecessorTransitionObservationV2 {
    predecessor: Option<SlotRouteWitnessV1>,
    successor: RuntimeRegistryStagedInstallEvidenceV2,
    initial_active_interactions: u32,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct RuntimeRegistryPredecessorRemovalObservationV2 {
    removed_predecessor: Option<SlotRouteWitnessV1>,
    successor: RuntimeRegistryStagedInstallEvidenceV2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeRegistryPredecessorDrainObservationV2 {
    active_interactions: u32,
    drained: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeRegistryPredecessorReplacementErrorV2 {
    #[error("runtime registry expected predecessor is absent")]
    ExpectedPredecessorAbsent,
    #[error("runtime registry contains an unexpected predecessor")]
    UnexpectedPredecessorPresent,
    #[error("runtime registry predecessor identity does not exactly match")]
    PredecessorIdentityMismatch,
    #[error("runtime registry predecessor still has active interactions")]
    ActiveInteractionsRemain { active: u32 },
    #[error("runtime registry predecessor transition has not been verified")]
    PredecessorNotVerified,
    #[error("runtime registry predecessor operation has an unexpected outcome")]
    UnexpectedOutcome,
    #[error("runtime staged route authority is invalid")]
    StagedAuthorityInvalid,
    #[error("runtime predecessor replacement state is unavailable")]
    StateUnavailable,
    #[error("runtime predecessor registry operation failed")]
    Registry(ServingSlotRegistryError),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeRegistryBarrierBActivationErrorV2 {
    #[error("runtime registry predecessor replacement is not final")]
    PredecessorNotFinal,
    #[error("runtime Barrier B staged route authority is invalid")]
    StagedAuthorityInvalid,
    #[error("runtime Barrier B activation evidence is inconsistent")]
    EvidenceMismatch,
    #[error("runtime Barrier B replacement state is unavailable")]
    StateUnavailable,
    #[error("runtime Barrier B registry operation failed")]
    Registry(ServingSlotRegistryError),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeRegistryBarrierBServingErrorV2 {
    #[error("runtime Barrier B serving authority no longer identifies the exact serving route")]
    ExactServingLost,
    #[error("runtime Barrier B serving route still has active interactions")]
    ActiveInteractionsRemain { active: u32 },
    #[error("runtime Barrier B serving transition evidence is inconsistent")]
    EvidenceMismatch,
    #[error("runtime Barrier B serving registry operation failed")]
    Registry(ServingSlotRegistryError),
}

pub(crate) struct RuntimeRegistryStagedInstallV2 {
    outcome: RuntimeRegistryStagedInstallOutcomeV2,
    evidence: RuntimeRegistryStagedInstallEvidenceV2,
    authority: RuntimeRegistryStagedRouteV2,
}

impl RuntimeRegistryStagedInstallEvidenceV2 {
    pub(crate) fn identity_v2(&self) -> &RuntimeProcessIdentityV1 {
        &self.route.identity
    }

    pub(crate) fn fencing_token_v2(&self) -> FencingToken {
        self.route.fencing_token
    }

    pub(crate) fn route_incarnation_v2(&self) -> NonZeroU64 {
        self.route.incarnation
    }

    pub(crate) fn active_interactions_v2(&self) -> u32 {
        self.active_interactions
    }

    pub(crate) fn admission_generation_v2(&self) -> NonZeroU64 {
        self.admission_generation
    }

    pub(crate) fn registry_observation_sequence_v2(
        &self,
    ) -> RuntimeRegistryGlobalObservationSequenceV2 {
        self.registry_observation_sequence
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeRegistryBarrierBActivationEvidenceV2 {
    pub(crate) fn outcome_v2(&self) -> RuntimeRegistryBarrierBActivationOutcomeV2 {
        self.outcome
    }

    pub(crate) fn identity_v2(&self) -> &RuntimeProcessIdentityV1 {
        &self.route.identity
    }

    pub(crate) fn fencing_token_v2(&self) -> FencingToken {
        self.route.fencing_token
    }

    pub(crate) fn route_incarnation_v2(&self) -> NonZeroU64 {
        self.route.incarnation
    }

    pub(crate) fn activation_sequence_v2(&self) -> NonZeroU64 {
        self.activation_sequence
    }

    pub(crate) fn active_interactions_v2(&self) -> u32 {
        self.active_interactions
    }

    pub(crate) fn admission_generation_v2(&self) -> NonZeroU64 {
        self.admission_generation
    }

    pub(crate) fn slot_observation_sequence_v2(&self) -> NonZeroU64 {
        self.slot_observation_sequence
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeRegistryBarrierBExactServingObservationV2 {
    pub(crate) fn identity_v2(&self) -> &RuntimeProcessIdentityV1 {
        &self.route.identity
    }

    pub(crate) fn fencing_token_v2(&self) -> FencingToken {
        self.route.fencing_token
    }

    pub(crate) fn route_incarnation_v2(&self) -> NonZeroU64 {
        self.route.incarnation
    }

    pub(crate) fn activation_sequence_v2(&self) -> NonZeroU64 {
        self.activation_sequence
    }

    pub(crate) fn active_interactions_v2(&self) -> u32 {
        self.active_interactions
    }

    pub(crate) fn admission_generation_v2(&self) -> NonZeroU64 {
        self.admission_generation
    }

    pub(crate) fn slot_observation_sequence_v2(&self) -> NonZeroU64 {
        self.slot_observation_sequence
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeRegistryBarrierBRemovalEvidenceV2 {
    pub(crate) fn identity_v2(&self) -> &RuntimeProcessIdentityV1 {
        &self.route.identity
    }

    pub(crate) fn fencing_token_v2(&self) -> FencingToken {
        self.route.fencing_token
    }

    pub(crate) fn route_incarnation_v2(&self) -> NonZeroU64 {
        self.route.incarnation
    }

    pub(crate) fn activation_sequence_v2(&self) -> NonZeroU64 {
        self.activation_sequence
    }

    pub(crate) fn draining_admission_generation_v2(&self) -> NonZeroU64 {
        self.draining_admission_generation
    }

    pub(crate) fn draining_slot_observation_sequence_v2(&self) -> NonZeroU64 {
        self.draining_slot_observation_sequence
    }

    pub(crate) fn removed_admission_generation_v2(&self) -> NonZeroU64 {
        self.removed_admission_generation
    }

    pub(crate) fn removed_slot_observation_sequence_v2(&self) -> NonZeroU64 {
        self.removed_slot_observation_sequence
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeRegistryBarrierBActivationV2 {
    pub(crate) fn evidence_v2(&self) -> &RuntimeRegistryBarrierBActivationEvidenceV2 {
        &self.evidence
    }

    pub(crate) fn serving_authority_v2(&self) -> &RuntimeRegistryBarrierBServingAuthorityV2 {
        &self.authority
    }

    pub(crate) fn into_parts_v2(
        self,
    ) -> (
        RuntimeRegistryBarrierBActivationEvidenceV2,
        RuntimeRegistryBarrierBServingAuthorityV2,
    ) {
        (self.evidence, self.authority)
    }
}

#[allow(dead_code)]
impl RuntimeRegistryCertificationBarrierBActivationFailureV2 {
    pub(crate) fn paused_v2(&self) -> &RuntimeDiscordCertificationBarrierBPausedV2 {
        self.paused.as_ref()
    }

    pub(crate) fn source_v2(&self) -> &RuntimeRegistryBarrierBActivationErrorV2 {
        &self.source
    }

    pub(crate) fn into_parts_v2(
        self,
    ) -> (
        RuntimeDiscordCertificationBarrierBPausedV2,
        RuntimeRegistryBarrierBActivationErrorV2,
    ) {
        (*self.paused, self.source)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeRegistryBarrierBServingAuthorityV2 {
    pub(crate) fn identity_v2(&self) -> &RuntimeProcessIdentityV1 {
        &self.route.identity
    }

    pub(crate) fn fencing_token_v2(&self) -> FencingToken {
        self.route.fencing_token
    }

    pub(crate) fn route_incarnation_v2(&self) -> NonZeroU64 {
        self.route.incarnation
    }

    pub(crate) fn activation_sequence_v2(&self) -> NonZeroU64 {
        self.activation_sequence
    }

    pub(crate) fn ensure_exact_serving_v2(
        &self,
    ) -> Result<(), RuntimeRegistryBarrierBServingErrorV2> {
        observe_exact_barrier_b_serving_v2(
            &self.registry,
            &self.token,
            &self.route,
            self.activation_sequence,
        )
        .map(|_| ())
    }

    pub(crate) fn remove_exact_serving_v2(
        mut self,
    ) -> Result<RuntimeRegistryBarrierBRemovalEvidenceV2, RuntimeRegistryBarrierBServingErrorV2>
    {
        let result = remove_exact_barrier_b_serving_v2(
            &self.registry,
            &self.token,
            &self.route,
            self.activation_sequence,
        );
        if result.is_ok() {
            self.armed = false;
        }
        result
    }

    pub(crate) fn into_serving_monitor_v2(
        self,
    ) -> Result<
        RuntimeRegistryBarrierBServingMonitorAuthorityV2,
        RuntimeRegistryBarrierBServingErrorV2,
    > {
        let (monitor, _completion) = self.into_serving_monitor_with_completion_v2()?;
        Ok(monitor)
    }

    pub(crate) fn into_serving_monitor_with_completion_v2(
        mut self,
    ) -> Result<
        (
            RuntimeRegistryBarrierBServingMonitorAuthorityV2,
            RuntimeRegistryBarrierBServingCompletionWitnessV2,
        ),
        RuntimeRegistryBarrierBServingErrorV2,
    > {
        self.ensure_exact_serving_v2()?;
        let completion_liveness = Arc::new(Mutex::new(true));
        let monitor = RuntimeRegistryBarrierBServingMonitorAuthorityV2 {
            registry: self.registry.clone(),
            token: self.token.clone(),
            route: self.route.clone(),
            activation_sequence: self.activation_sequence,
            emergency: self.emergency.clone(),
            completion_liveness: completion_liveness.clone(),
            armed: true,
        };
        let completion = RuntimeRegistryBarrierBServingCompletionWitnessV2 {
            registry: self.registry.clone(),
            token: self.token.clone(),
            route: self.route.clone(),
            activation_sequence: self.activation_sequence,
            emergency: self.emergency.clone(),
            completion_liveness,
        };
        self.armed = false;
        Ok((monitor, completion))
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeRegistryBarrierBServingMonitorAuthorityV2 {
    pub(crate) fn identity_v2(&self) -> &RuntimeProcessIdentityV1 {
        &self.route.identity
    }

    pub(crate) fn fencing_token_v2(&self) -> FencingToken {
        self.route.fencing_token
    }

    pub(crate) fn route_incarnation_v2(&self) -> NonZeroU64 {
        self.route.incarnation
    }

    pub(crate) fn activation_sequence_v2(&self) -> NonZeroU64 {
        self.activation_sequence
    }

    pub(crate) fn observe_exact_serving_v2(
        &self,
    ) -> Result<
        RuntimeRegistryBarrierBExactServingObservationV2,
        RuntimeRegistryBarrierBServingErrorV2,
    > {
        observe_exact_barrier_b_serving_v2(
            &self.registry,
            &self.token,
            &self.route,
            self.activation_sequence,
        )
    }

    pub(crate) fn remove_exact_serving_v2(
        mut self,
    ) -> Result<RuntimeRegistryBarrierBRemovalEvidenceV2, RuntimeRegistryBarrierBServingErrorV2>
    {
        close_barrier_b_serving_completion_v2(&self.completion_liveness, &self.emergency);
        let result = remove_exact_barrier_b_serving_v2(
            &self.registry,
            &self.token,
            &self.route,
            self.activation_sequence,
        );
        if result.is_ok() {
            self.armed = false;
        }
        result
    }
}

#[allow(dead_code)]
impl RuntimeRegistryBarrierBServingCompletionWitnessV2 {
    pub(crate) fn lock_exact_serving_v2(
        &self,
    ) -> Result<
        RuntimeRegistryBarrierBServingCompletionGuardV2<'_>,
        RuntimeRegistryBarrierBServingErrorV2,
    > {
        let liveness = match self.completion_liveness.lock() {
            Ok(liveness) => liveness,
            Err(poisoned) => {
                let mut liveness = poisoned.into_inner();
                *liveness = false;
                self.completion_liveness.clear_poison();
                self.emergency.trip_v2();
                return Err(RuntimeRegistryBarrierBServingErrorV2::ExactServingLost);
            }
        };
        if !*liveness {
            return Err(RuntimeRegistryBarrierBServingErrorV2::ExactServingLost);
        }
        observe_exact_barrier_b_serving_v2(
            &self.registry,
            &self.token,
            &self.route,
            self.activation_sequence,
        )?;
        Ok(RuntimeRegistryBarrierBServingCompletionGuardV2 {
            _liveness: liveness,
        })
    }
}

fn close_barrier_b_serving_completion_v2(
    completion_liveness: &Mutex<bool>,
    emergency: &RuntimeRegistryEmergencyTriggerV2,
) {
    match completion_liveness.lock() {
        Ok(mut liveness) => {
            *liveness = false;
        }
        Err(poisoned) => {
            let mut liveness = poisoned.into_inner();
            *liveness = false;
            completion_liveness.clear_poison();
            emergency.trip_v2();
        }
    }
}

fn map_barrier_b_serving_observation_error_v2(
    error: ServingSlotRegistryError,
) -> RuntimeRegistryBarrierBServingErrorV2 {
    match error {
        ServingSlotRegistryError::StaleMutationToken
        | ServingSlotRegistryError::ActivationTargetMismatch => {
            RuntimeRegistryBarrierBServingErrorV2::ExactServingLost
        }
        error => RuntimeRegistryBarrierBServingErrorV2::Registry(error),
    }
}

fn observe_exact_barrier_b_serving_v2(
    registry: &ServingSlotRegistryV1,
    token: &SlotMutationTokenV1,
    expected_route: &SlotRouteWitnessV1,
    expected_activation_sequence: NonZeroU64,
) -> Result<RuntimeRegistryBarrierBExactServingObservationV2, RuntimeRegistryBarrierBServingErrorV2>
{
    let route = registry
        .route_witness(token)
        .map_err(map_barrier_b_serving_observation_error_v2)?;
    let atomic = registry
        .atomic_observation_v2(token.key())
        .map_err(map_barrier_b_serving_observation_error_v2)?
        .ok_or(RuntimeRegistryBarrierBServingErrorV2::ExactServingLost)?;
    if route != *expected_route
        || route.lifecycle != SlotLifecycleV1::Serving
        || atomic.route.as_ref() != Some(expected_route)
        || atomic.admission_state != SlotAdmissionStateV2::Serving
    {
        return Err(RuntimeRegistryBarrierBServingErrorV2::ExactServingLost);
    }
    let activation = registry
        .activate_with_sequence_v2(token, &expected_route.identity)
        .map_err(map_barrier_b_serving_observation_error_v2)?;
    if activation.outcome() != SlotActivationOutcomeV1::AlreadyServing
        || activation.route() != expected_route
        || activation.activation_sequence() != expected_activation_sequence
        || activation.observation() != &atomic
    {
        return Err(RuntimeRegistryBarrierBServingErrorV2::ExactServingLost);
    }
    Ok(RuntimeRegistryBarrierBExactServingObservationV2 {
        route,
        activation_sequence: expected_activation_sequence,
        active_interactions: atomic.active_interactions,
        admission_generation: atomic.admission_generation,
        slot_observation_sequence: atomic.observation_sequence,
    })
}

fn remove_exact_barrier_b_serving_v2(
    registry: &ServingSlotRegistryV1,
    token: &SlotMutationTokenV1,
    expected_route: &SlotRouteWitnessV1,
    expected_activation_sequence: NonZeroU64,
) -> Result<RuntimeRegistryBarrierBRemovalEvidenceV2, RuntimeRegistryBarrierBServingErrorV2> {
    observe_exact_barrier_b_serving_v2(
        registry,
        token,
        expected_route,
        expected_activation_sequence,
    )?;
    let drain = registry
        .begin_drain(token)
        .map_err(map_barrier_b_serving_observation_error_v2)?;
    let active = match drain {
        SlotDrainOutcomeV1::DrainStarted {
            active_interactions,
        } => active_interactions,
        SlotDrainOutcomeV1::AlreadyDraining { .. } => {
            return Err(RuntimeRegistryBarrierBServingErrorV2::ExactServingLost);
        }
    };
    if active != 0 {
        return Err(RuntimeRegistryBarrierBServingErrorV2::ActiveInteractionsRemain { active });
    }
    let mut expected_draining_route = expected_route.clone();
    expected_draining_route.lifecycle = SlotLifecycleV1::Draining;
    let draining_route = registry
        .route_witness(token)
        .map_err(map_barrier_b_serving_observation_error_v2)?;
    let draining_atomic = registry
        .atomic_observation_v2(token.key())
        .map_err(map_barrier_b_serving_observation_error_v2)?
        .ok_or(RuntimeRegistryBarrierBServingErrorV2::EvidenceMismatch)?;
    if draining_route != expected_draining_route
        || draining_atomic.route.as_ref() != Some(&expected_draining_route)
        || draining_atomic.admission_state != SlotAdmissionStateV2::Draining
        || draining_atomic.active_interactions != 0
    {
        return Err(RuntimeRegistryBarrierBServingErrorV2::EvidenceMismatch);
    }
    let removal = registry
        .remove(token)
        .map_err(map_barrier_b_serving_observation_error_v2)?;
    if removal != SlotRemovalOutcomeV1::RemovedDraining {
        return Err(RuntimeRegistryBarrierBServingErrorV2::EvidenceMismatch);
    }
    let removed_atomic = registry
        .atomic_observation_v2(token.key())
        .map_err(map_barrier_b_serving_observation_error_v2)?
        .ok_or(RuntimeRegistryBarrierBServingErrorV2::EvidenceMismatch)?;
    if removed_atomic.route.is_some()
        || removed_atomic.admission_state != SlotAdmissionStateV2::Empty
        || removed_atomic.active_interactions != 0
    {
        return Err(RuntimeRegistryBarrierBServingErrorV2::EvidenceMismatch);
    }
    Ok(RuntimeRegistryBarrierBRemovalEvidenceV2 {
        route: expected_route.clone(),
        activation_sequence: expected_activation_sequence,
        draining_admission_generation: draining_atomic.admission_generation,
        draining_slot_observation_sequence: draining_atomic.observation_sequence,
        removed_admission_generation: removed_atomic.admission_generation,
        removed_slot_observation_sequence: removed_atomic.observation_sequence,
    })
}

impl RuntimeRegistryStagedInstallV2 {
    pub(crate) fn outcome_v2(&self) -> RuntimeRegistryStagedInstallOutcomeV2 {
        self.outcome
    }

    pub(crate) fn evidence_v2(&self) -> &RuntimeRegistryStagedInstallEvidenceV2 {
        &self.evidence
    }

    pub(crate) fn into_parts_v2(
        self,
    ) -> (
        RuntimeRegistryStagedInstallOutcomeV2,
        RuntimeRegistryStagedInstallEvidenceV2,
        RuntimeRegistryStagedRouteV2,
    ) {
        (self.outcome, self.evidence, self.authority)
    }
}

impl RuntimeRegistryStagedRouteV2 {
    #[cfg(test)]
    pub(crate) fn identity_v2(&self) -> &RuntimeProcessIdentityV1 {
        &self.identity
    }

    pub(crate) fn fencing_token_v2(&self) -> FencingToken {
        self.token
            .as_ref()
            .expect("live staged route authority must retain its token")
            .fencing_token()
    }

    pub(crate) fn ensure_staged_v2(&self) -> Result<(), RuntimeRegistryStagingErrorV2> {
        let token = self
            .token
            .as_ref()
            .ok_or(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)?;
        let witness = self
            .registry
            .route_witness(token)
            .map_err(RuntimeRegistryStagingErrorV2::Registry)?;
        if witness.identity != self.identity
            || witness.fencing_token != token.fencing_token()
            || witness.lifecycle != SlotLifecycleV1::Staged
        {
            return Err(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle);
        }
        Ok(())
    }

    pub(crate) fn advance_authority_v2(
        &mut self,
        next_fencing_token: FencingToken,
    ) -> Result<RuntimeRegistryStagedInstallEvidenceV2, RuntimeRegistryStagingErrorV2> {
        self.ensure_staged_v2()?;
        let token = self
            .token
            .as_ref()
            .ok_or(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)?;
        let successor = self
            .registry
            .advance_authority(token, &self.identity, next_fencing_token)
            .map_err(RuntimeRegistryStagingErrorV2::Registry)?;
        self.token = Some(successor);
        self.observe_staged_evidence_v2()
    }

    pub(crate) fn into_replacement_v2(self) -> RuntimeRegistryReplacementRouteV2 {
        RuntimeRegistryReplacementRouteV2 {
            identity: self.identity.clone(),
            state: Mutex::new(RuntimeRegistryReplacementStateV2 {
                staged: self,
                predecessor: RuntimeRegistryPredecessorStateV2::Unverified,
            }),
        }
    }

    fn ensure_exclusively_staged_v2(
        &self,
    ) -> Result<(), RuntimeRegistryPredecessorReplacementErrorV2> {
        self.ensure_staged_v2()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
        let token = self
            .token
            .as_ref()
            .ok_or(RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
        let witness = self
            .registry
            .route_witness(token)
            .map_err(RuntimeRegistryPredecessorReplacementErrorV2::Registry)?;
        let atomic = self
            .registry
            .atomic_observation_v2(token.key())
            .map_err(RuntimeRegistryPredecessorReplacementErrorV2::Registry)?
            .ok_or(RuntimeRegistryPredecessorReplacementErrorV2::UnexpectedPredecessorPresent)?;
        if atomic.route.as_ref() != Some(&witness)
            || atomic.admission_state != SlotAdmissionStateV2::Staged
            || atomic.active_interactions != 0
        {
            return Err(RuntimeRegistryPredecessorReplacementErrorV2::UnexpectedPredecessorPresent);
        }
        self.ensure_staged_v2()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)
    }

    pub(crate) fn remove_v2(mut self) -> Result<(), RuntimeRegistryStagingErrorV2> {
        let result = self.remove_inner_v2();
        if result.is_err() {
            self.emergency.trip_v2();
        }
        result
    }

    fn remove_inner_v2(&mut self) -> Result<(), RuntimeRegistryStagingErrorV2> {
        let token = self
            .token
            .take()
            .ok_or(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)?;
        match self
            .registry
            .remove(&token)
            .map_err(RuntimeRegistryStagingErrorV2::Registry)?
        {
            SlotRemovalOutcomeV1::RemovedStaged => Ok(()),
            SlotRemovalOutcomeV1::RemovedDraining => {
                Err(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)
            }
        }
    }

    fn observe_staged_evidence_v2(
        &self,
    ) -> Result<RuntimeRegistryStagedInstallEvidenceV2, RuntimeRegistryStagingErrorV2> {
        let token = self
            .token
            .as_ref()
            .ok_or(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)?;
        let witness = self
            .registry
            .route_witness(token)
            .map_err(RuntimeRegistryStagingErrorV2::Registry)?;
        let atomic = self
            .registry
            .atomic_observation_v2(token.key())
            .map_err(RuntimeRegistryStagingErrorV2::Registry)?
            .ok_or(RuntimeRegistryStagingErrorV2::EvidenceMismatch)?;
        let registry_observation = self
            .registry
            .recovery_observation_v2()
            .map_err(RuntimeRegistryStagingErrorV2::Registry)?;
        validate_staged_evidence_v2(self, witness, atomic, registry_observation)
    }
}

impl RuntimeRegistryReplacementRouteV2 {
    pub(crate) fn identity_v2(&self) -> &RuntimeProcessIdentityV1 {
        &self.identity
    }

    pub(crate) fn ensure_staged_v2(
        &self,
    ) -> Result<(), RuntimeRegistryPredecessorReplacementErrorV2> {
        let state = self.lock_state_v2()?;
        state
            .staged
            .ensure_staged_v2()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)
    }

    pub(crate) fn fencing_token_v2(
        &self,
    ) -> Result<FencingToken, RuntimeRegistryPredecessorReplacementErrorV2> {
        let state = self.lock_state_v2()?;
        state
            .staged
            .ensure_staged_v2()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
        Ok(state.staged.fencing_token_v2())
    }

    pub(crate) fn advance_authority_v2(
        &self,
        next_fencing_token: FencingToken,
    ) -> Result<RuntimeRegistryStagedInstallEvidenceV2, RuntimeRegistryStagingErrorV2> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)?;
        state.staged.advance_authority_v2(next_fencing_token)
    }

    pub(crate) fn transition_predecessor_to_draining_v2(
        &self,
        expected_predecessor: Option<&RuntimeProcessIdentityV1>,
    ) -> Result<
        RuntimeRegistryPredecessorTransitionObservationV2,
        RuntimeRegistryPredecessorReplacementErrorV2,
    > {
        let mut state = self.lock_state_v2()?;
        state
            .staged
            .ensure_staged_v2()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
        match &state.predecessor {
            RuntimeRegistryPredecessorStateV2::Unverified => {}
            RuntimeRegistryPredecessorStateV2::Absent if expected_predecessor.is_none() => {
                return replacement_transition_observation_v2(&state);
            }
            RuntimeRegistryPredecessorStateV2::Draining { witness, .. }
            | RuntimeRegistryPredecessorStateV2::Removed {
                witness: Some(witness),
                ..
            } if expected_predecessor == Some(&witness.identity) => {
                return replacement_transition_observation_v2(&state);
            }
            RuntimeRegistryPredecessorStateV2::Removed { witness: None, .. }
                if expected_predecessor.is_none() =>
            {
                return replacement_transition_observation_v2(&state);
            }
            RuntimeRegistryPredecessorStateV2::Absent
            | RuntimeRegistryPredecessorStateV2::Draining { .. }
            | RuntimeRegistryPredecessorStateV2::Removed { .. } => {
                return Err(
                    RuntimeRegistryPredecessorReplacementErrorV2::PredecessorIdentityMismatch,
                );
            }
        }
        let staged_token = state
            .staged
            .token
            .as_ref()
            .ok_or(RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
        let serving = state
            .staged
            .registry
            .serving_snapshot(staged_token.key())
            .map_err(RuntimeRegistryPredecessorReplacementErrorV2::Registry)?;
        match (expected_predecessor, serving) {
            (None, None) => {
                state.staged.ensure_exclusively_staged_v2()?;
                state.predecessor = RuntimeRegistryPredecessorStateV2::Absent;
            }
            (None, Some(_)) => {
                return Err(
                    RuntimeRegistryPredecessorReplacementErrorV2::UnexpectedPredecessorPresent,
                );
            }
            (Some(_), None) => {
                return Err(
                    RuntimeRegistryPredecessorReplacementErrorV2::ExpectedPredecessorAbsent,
                );
            }
            (Some(expected), Some(serving)) => {
                if serving.identity() != expected {
                    return Err(
                        RuntimeRegistryPredecessorReplacementErrorV2::PredecessorIdentityMismatch,
                    );
                }
                let predecessor = serving.token().clone();
                let outcome = state
                    .staged
                    .registry
                    .begin_drain_with_authority(staged_token, &predecessor)
                    .map_err(RuntimeRegistryPredecessorReplacementErrorV2::Registry)?;
                let initial_active_interactions = match outcome {
                    SlotDrainOutcomeV1::DrainStarted {
                        active_interactions,
                    }
                    | SlotDrainOutcomeV1::AlreadyDraining {
                        active_interactions,
                    } => active_interactions,
                };
                let witness = state
                    .staged
                    .registry
                    .route_witness(&predecessor)
                    .map_err(RuntimeRegistryPredecessorReplacementErrorV2::Registry)?;
                if witness.identity != *expected || witness.lifecycle != SlotLifecycleV1::Draining {
                    return Err(
                        RuntimeRegistryPredecessorReplacementErrorV2::PredecessorIdentityMismatch,
                    );
                }
                state.predecessor = RuntimeRegistryPredecessorStateV2::Draining {
                    token: predecessor,
                    witness,
                    initial_active_interactions,
                };
            }
        }
        state
            .staged
            .ensure_staged_v2()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
        replacement_transition_observation_v2(&state)
    }

    pub(crate) fn observe_predecessor_drain_v2(
        &self,
    ) -> Result<
        RuntimeRegistryPredecessorDrainObservationV2,
        RuntimeRegistryPredecessorReplacementErrorV2,
    > {
        let state = self.lock_state_v2()?;
        state
            .staged
            .ensure_staged_v2()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
        match &state.predecessor {
            RuntimeRegistryPredecessorStateV2::Unverified => {
                Err(RuntimeRegistryPredecessorReplacementErrorV2::PredecessorNotVerified)
            }
            RuntimeRegistryPredecessorStateV2::Absent
            | RuntimeRegistryPredecessorStateV2::Removed { .. } => {
                Ok(RuntimeRegistryPredecessorDrainObservationV2 {
                    active_interactions: 0,
                    drained: true,
                })
            }
            RuntimeRegistryPredecessorStateV2::Draining { token, witness, .. } => {
                if token.identity() != &witness.identity {
                    return Err(
                        RuntimeRegistryPredecessorReplacementErrorV2::PredecessorIdentityMismatch,
                    );
                }
                let observation = state
                    .staged
                    .registry
                    .observe_drain(token)
                    .map_err(RuntimeRegistryPredecessorReplacementErrorV2::Registry)?;
                Ok(RuntimeRegistryPredecessorDrainObservationV2 {
                    active_interactions: observation.active_interactions,
                    drained: observation.drained,
                })
            }
        }
    }

    pub(crate) fn remove_drained_predecessor_v2(
        &self,
    ) -> Result<
        RuntimeRegistryPredecessorRemovalObservationV2,
        RuntimeRegistryPredecessorReplacementErrorV2,
    > {
        let mut state = self.lock_state_v2()?;
        state
            .staged
            .ensure_staged_v2()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
        match &state.predecessor {
            RuntimeRegistryPredecessorStateV2::Unverified => {
                return Err(RuntimeRegistryPredecessorReplacementErrorV2::PredecessorNotVerified);
            }
            RuntimeRegistryPredecessorStateV2::Absent => {
                state.predecessor = RuntimeRegistryPredecessorStateV2::Removed {
                    witness: None,
                    initial_active_interactions: 0,
                };
            }
            RuntimeRegistryPredecessorStateV2::Removed { .. } => {}
            RuntimeRegistryPredecessorStateV2::Draining {
                token,
                witness,
                initial_active_interactions,
            } => {
                let observation = state
                    .staged
                    .registry
                    .observe_drain(token)
                    .map_err(RuntimeRegistryPredecessorReplacementErrorV2::Registry)?;
                if !observation.drained {
                    return Err(
                        RuntimeRegistryPredecessorReplacementErrorV2::ActiveInteractionsRemain {
                            active: observation.active_interactions,
                        },
                    );
                }
                let staged_token =
                    state.staged.token.as_ref().ok_or(
                        RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid,
                    )?;
                let outcome = state
                    .staged
                    .registry
                    .remove_with_authority(staged_token, token)
                    .map_err(RuntimeRegistryPredecessorReplacementErrorV2::Registry)?;
                if outcome != SlotRemovalOutcomeV1::RemovedDraining {
                    return Err(RuntimeRegistryPredecessorReplacementErrorV2::UnexpectedOutcome);
                }
                state.predecessor = RuntimeRegistryPredecessorStateV2::Removed {
                    witness: Some(witness.clone()),
                    initial_active_interactions: *initial_active_interactions,
                };
            }
        }
        let successor = state
            .staged
            .observe_staged_evidence_v2()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
        let removed_predecessor = match &state.predecessor {
            RuntimeRegistryPredecessorStateV2::Removed { witness, .. } => witness.clone(),
            _ => {
                return Err(RuntimeRegistryPredecessorReplacementErrorV2::UnexpectedOutcome);
            }
        };
        Ok(RuntimeRegistryPredecessorRemovalObservationV2 {
            removed_predecessor,
            successor,
        })
    }

    #[allow(dead_code)]
    pub(crate) fn activate_certification_barrier_b_v2(
        self,
        paused: RuntimeDiscordCertificationBarrierBPausedV2,
    ) -> Result<
        RuntimeDiscordCertificationBarrierBActivatedV2,
        RuntimeRegistryCertificationBarrierBActivationFailureV2,
    > {
        match self.activate_barrier_b_v2() {
            Ok(activation) => Ok(paused.bind_registry_activation_v2(activation)),
            Err(source) => Err(RuntimeRegistryCertificationBarrierBActivationFailureV2 {
                paused: Box::new(paused),
                source,
            }),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn activate_barrier_b_v2(
        self,
    ) -> Result<RuntimeRegistryBarrierBActivationV2, RuntimeRegistryBarrierBActivationErrorV2> {
        let expected_identity = self.identity;
        let state = self
            .state
            .into_inner()
            .map_err(|_| RuntimeRegistryBarrierBActivationErrorV2::StateUnavailable)?;
        if !matches!(
            &state.predecessor,
            RuntimeRegistryPredecessorStateV2::Removed { .. }
        ) {
            return Err(RuntimeRegistryBarrierBActivationErrorV2::PredecessorNotFinal);
        }
        if state.staged.identity != expected_identity {
            return Err(RuntimeRegistryBarrierBActivationErrorV2::StagedAuthorityInvalid);
        }
        activate_barrier_b_staged_route_v2(state.staged)
    }

    pub(crate) fn remove_v2(self) -> Result<(), RuntimeRegistryStagingErrorV2> {
        self.state
            .into_inner()
            .map_err(|_| RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)?
            .staged
            .remove_v2()
    }

    fn lock_state_v2(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, RuntimeRegistryReplacementStateV2>,
        RuntimeRegistryPredecessorReplacementErrorV2,
    > {
        self.state
            .lock()
            .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StateUnavailable)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn activate_barrier_b_staged_route_v2(
    mut staged: RuntimeRegistryStagedRouteV2,
) -> Result<RuntimeRegistryBarrierBActivationV2, RuntimeRegistryBarrierBActivationErrorV2> {
    let token = staged
        .token
        .as_ref()
        .ok_or(RuntimeRegistryBarrierBActivationErrorV2::StagedAuthorityInvalid)?;
    let staged_route = staged
        .registry
        .route_witness(token)
        .map_err(RuntimeRegistryBarrierBActivationErrorV2::Registry)?;
    let staged_atomic = staged
        .registry
        .atomic_observation_v2(token.key())
        .map_err(RuntimeRegistryBarrierBActivationErrorV2::Registry)?
        .ok_or(RuntimeRegistryBarrierBActivationErrorV2::StagedAuthorityInvalid)?;
    validate_barrier_b_activation_candidate_v2(&staged, &staged_route, &staged_atomic)?;
    let activation = staged
        .registry
        .activate_with_sequence_v2(token, &staged.identity)
        .map_err(RuntimeRegistryBarrierBActivationErrorV2::Registry)?;
    let outcome = match activation.outcome() {
        SlotActivationOutcomeV1::Activated => RuntimeRegistryBarrierBActivationOutcomeV2::Activated,
        SlotActivationOutcomeV1::AlreadyServing => {
            RuntimeRegistryBarrierBActivationOutcomeV2::AlreadyServing
        }
    };
    if staged_route.lifecycle == SlotLifecycleV1::Serving
        && outcome != RuntimeRegistryBarrierBActivationOutcomeV2::AlreadyServing
    {
        return Err(RuntimeRegistryBarrierBActivationErrorV2::EvidenceMismatch);
    }
    let route = activation.route();
    let atomic = activation.observation();
    if route.identity != staged_route.identity
        || route.fencing_token != staged_route.fencing_token
        || route.incarnation != staged_route.incarnation
        || route.lifecycle != SlotLifecycleV1::Serving
        || atomic.route.as_ref() != Some(route)
        || atomic.admission_state != SlotAdmissionStateV2::Serving
        || atomic.active_interactions != 0
    {
        return Err(RuntimeRegistryBarrierBActivationErrorV2::EvidenceMismatch);
    }
    let exact_route = staged
        .registry
        .route_witness(token)
        .map_err(RuntimeRegistryBarrierBActivationErrorV2::Registry)?;
    let exact_atomic = staged
        .registry
        .atomic_observation_v2(token.key())
        .map_err(RuntimeRegistryBarrierBActivationErrorV2::Registry)?
        .ok_or(RuntimeRegistryBarrierBActivationErrorV2::EvidenceMismatch)?;
    if exact_route != *route || exact_atomic != *atomic {
        return Err(RuntimeRegistryBarrierBActivationErrorV2::EvidenceMismatch);
    }
    let evidence = RuntimeRegistryBarrierBActivationEvidenceV2 {
        outcome,
        route: route.clone(),
        activation_sequence: activation.activation_sequence(),
        active_interactions: atomic.active_interactions,
        admission_generation: atomic.admission_generation,
        slot_observation_sequence: atomic.observation_sequence,
    };
    let registry = staged.registry.clone();
    let route = evidence.route.clone();
    let activation_sequence = evidence.activation_sequence;
    let emergency = staged.emergency.clone();
    let token = staged
        .token
        .take()
        .ok_or(RuntimeRegistryBarrierBActivationErrorV2::StagedAuthorityInvalid)?;
    let authority = RuntimeRegistryBarrierBServingAuthorityV2 {
        registry,
        token,
        route,
        activation_sequence,
        emergency,
        armed: true,
    };
    Ok(RuntimeRegistryBarrierBActivationV2 {
        evidence,
        authority,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn validate_barrier_b_activation_candidate_v2(
    staged: &RuntimeRegistryStagedRouteV2,
    route: &SlotRouteWitnessV1,
    atomic: &SlotAtomicObservationV2,
) -> Result<(), RuntimeRegistryBarrierBActivationErrorV2> {
    let token = staged
        .token
        .as_ref()
        .ok_or(RuntimeRegistryBarrierBActivationErrorV2::StagedAuthorityInvalid)?;
    let expected_admission = match route.lifecycle {
        SlotLifecycleV1::Staged => SlotAdmissionStateV2::Staged,
        SlotLifecycleV1::Serving => SlotAdmissionStateV2::Serving,
        SlotLifecycleV1::Draining => {
            return Err(RuntimeRegistryBarrierBActivationErrorV2::StagedAuthorityInvalid);
        }
    };
    if route.identity != staged.identity
        || token.identity() != &staged.identity
        || route.fencing_token != token.fencing_token()
        || atomic.route.as_ref() != Some(route)
        || atomic.admission_state != expected_admission
        || atomic.active_interactions != 0
    {
        return Err(RuntimeRegistryBarrierBActivationErrorV2::StagedAuthorityInvalid);
    }
    Ok(())
}

fn replacement_transition_observation_v2(
    state: &RuntimeRegistryReplacementStateV2,
) -> Result<
    RuntimeRegistryPredecessorTransitionObservationV2,
    RuntimeRegistryPredecessorReplacementErrorV2,
> {
    let successor = state
        .staged
        .observe_staged_evidence_v2()
        .map_err(|_| RuntimeRegistryPredecessorReplacementErrorV2::StagedAuthorityInvalid)?;
    let (predecessor, initial_active_interactions) = match &state.predecessor {
        RuntimeRegistryPredecessorStateV2::Absent => (None, 0),
        RuntimeRegistryPredecessorStateV2::Draining {
            witness,
            initial_active_interactions,
            ..
        }
        | RuntimeRegistryPredecessorStateV2::Removed {
            witness: Some(witness),
            initial_active_interactions,
        } => (Some(witness.clone()), *initial_active_interactions),
        RuntimeRegistryPredecessorStateV2::Removed {
            witness: None,
            initial_active_interactions,
        } => (None, *initial_active_interactions),
        RuntimeRegistryPredecessorStateV2::Unverified => {
            return Err(RuntimeRegistryPredecessorReplacementErrorV2::PredecessorNotVerified);
        }
    };
    Ok(RuntimeRegistryPredecessorTransitionObservationV2 {
        predecessor,
        successor,
        initial_active_interactions,
    })
}

impl RuntimeRegistryPredecessorDrainObservationV2 {
    pub(crate) fn active_interactions_v2(self) -> u32 {
        self.active_interactions
    }

    pub(crate) fn drained_v2(self) -> bool {
        self.drained
    }
}

impl RuntimeRegistryPredecessorTransitionObservationV2 {
    pub(crate) fn predecessor_v2(&self) -> Option<&SlotRouteWitnessV1> {
        self.predecessor.as_ref()
    }

    pub(crate) fn successor_v2(&self) -> &RuntimeRegistryStagedInstallEvidenceV2 {
        &self.successor
    }

    pub(crate) fn initial_active_interactions_v2(&self) -> u32 {
        self.initial_active_interactions
    }
}

impl RuntimeRegistryPredecessorRemovalObservationV2 {
    pub(crate) fn removed_predecessor_v2(&self) -> Option<&SlotRouteWitnessV1> {
        self.removed_predecessor.as_ref()
    }

    pub(crate) fn successor_v2(&self) -> &RuntimeRegistryStagedInstallEvidenceV2 {
        &self.successor
    }
}

impl Drop for RuntimeRegistryStagedRouteV2 {
    fn drop(&mut self) {
        if self.token.is_some() && self.remove_inner_v2().is_err() {
            self.emergency.trip_v2();
        }
    }
}

impl Drop for RuntimeRegistryBarrierBServingAuthorityV2 {
    fn drop(&mut self) {
        if self.armed {
            self.emergency.trip_v2();
        }
    }
}

impl Drop for RuntimeRegistryBarrierBServingMonitorAuthorityV2 {
    fn drop(&mut self) {
        if self.armed {
            close_barrier_b_serving_completion_v2(&self.completion_liveness, &self.emergency);
            if remove_exact_barrier_b_serving_v2(
                &self.registry,
                &self.token,
                &self.route,
                self.activation_sequence,
            )
            .is_err()
            {
                self.emergency.trip_v2();
            }
        }
    }
}

impl Debug for RuntimeRegistryReplacementRouteV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryReplacementRouteV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryBarrierBActivationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBarrierBActivationV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryCertificationBarrierBActivationFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryCertificationBarrierBActivationFailureV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryBarrierBServingAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBarrierBServingAuthorityV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryBarrierBServingMonitorAuthorityV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBarrierBServingMonitorAuthorityV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryBarrierBServingCompletionWitnessV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBarrierBServingCompletionWitnessV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryBarrierBServingCompletionGuardV2<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBarrierBServingCompletionGuardV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryBarrierBActivationEvidenceV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBarrierBActivationEvidenceV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryBarrierBExactServingObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBarrierBExactServingObservationV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryBarrierBRemovalEvidenceV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryBarrierBRemovalEvidenceV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryPredecessorTransitionObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryPredecessorTransitionObservationV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryPredecessorRemovalObservationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryPredecessorRemovalObservationV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryStagedRouteV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryStagedRouteV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryStagedInstallEvidenceV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryStagedInstallEvidenceV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryStagedInstallV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryStagedInstallV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryStagingPortV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryStagingPortV2(<redacted>)")
    }
}

pub(crate) struct RuntimeRegistryServingTransitionFailureV2 {
    binding: RuntimeRegistryEmptyRecoveryBindingV2,
    error: RuntimeRegistryRecoveryObservationErrorV1,
}

impl RuntimeRegistryEmptyRecoveryBindingV2 {
    pub(crate) fn revalidate_empty_projection_v2(
        &self,
        _section: &RuntimeRecoveryPendingGatewaySectionV2<'_>,
    ) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        self.revalidate_empty_projection_unordered_v2()
    }

    fn revalidate_empty_projection_unordered_v2(
        &self,
    ) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        let observation = self
            .registry
            .revalidate_empty_recovery_cursor_v2(&self.cursor)
            .map_err(map_registry_observation_error)?;
        project_empty_observation_v2(&self.process_instance_id, observation)
    }

    pub(crate) fn revalidate_production_empty_projection_v2(
        &self,
    ) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1>
    {
        self.revalidate_empty_projection_unordered_v2()
    }

    pub(crate) fn prepare_serving_transition_v2(
        self,
        route_set_epoch: &RuntimeRouteSetEpochV2,
    ) -> Result<RuntimeRegistryPreparedServingTransitionV2, RuntimeRegistryServingTransitionFailureV2>
    {
        let observation = match self.revalidate_empty_projection_unordered_v2() {
            Ok(observation) => observation,
            Err(error) => {
                return Err(RuntimeRegistryServingTransitionFailureV2 {
                    binding: self,
                    error,
                });
            }
        };
        if route_set_epoch.process_instance_id() != &self.process_instance_id
            || route_set_epoch.initial_registry_observation_sequence()
                != observation.observation_sequence()
            || route_set_epoch.initial_retained_slot_count() != observation.retained_slot_count()
            || route_set_epoch.initial_retained_empty_tombstone_count()
                != observation.retained_empty_tombstone_count()
        {
            return Err(RuntimeRegistryServingTransitionFailureV2 {
                binding: self,
                error: RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation,
            });
        }
        Ok(RuntimeRegistryPreparedServingTransitionV2 {
            binding: self,
            initial_registry_observation_sequence: observation.observation_sequence(),
            initial_retained_slot_count: observation.retained_slot_count(),
            initial_retained_empty_tombstone_count: observation.retained_empty_tombstone_count(),
        })
    }

    pub(crate) fn into_pending_drain_seal_binding_v2(
        self,
        candidate: &RuntimePendingDrainCandidateV2,
    ) -> Result<
        (
            RuntimeRegistryPendingDrainSealBindingV2,
            RuntimePendingDrainRegistrySealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        self.into_pending_drain_seal_binding_common_v2(
            ServingSlotKeyV1::new(
                candidate.slot().guild_id,
                candidate.slot().ruleset_key.clone(),
            ),
            SlotSealKeyV2::try_from(candidate.intent_id().canonical_bytes().as_slice())
                .map_err(|_| RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)?,
            candidate.slot(),
        )
    }

    pub(crate) fn into_pending_drain_succession_seal_binding_v3(
        self,
        candidate: &RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
    ) -> Result<
        (
            RuntimeRegistryPendingDrainSuccessionSealBindingV3,
            RuntimePendingDrainRegistrySealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        let (binding, witness) = self.into_pending_drain_seal_binding_common_v2(
            ServingSlotKeyV1::new(
                candidate.slot().guild_id,
                candidate.slot().ruleset_key.clone(),
            ),
            SlotSealKeyV2::try_from(candidate.intent_id().canonical_bytes().as_slice())
                .map_err(|_| RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)?,
            candidate.slot(),
        )?;
        Ok((
            RuntimeRegistryPendingDrainSuccessionSealBindingV3 { binding },
            witness,
        ))
    }

    fn into_pending_drain_seal_binding_common_v2(
        self,
        key: ServingSlotKeyV1,
        seal_key: SlotSealKeyV2,
        slot: &RuntimeServingSlotV2,
    ) -> Result<
        (
            RuntimeRegistryPendingDrainSealBindingV2,
            RuntimePendingDrainRegistrySealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        let source_empty_observation = self.revalidate_empty_projection_unordered_v2()?;
        let sealed = self
            .registry
            .seal_empty_recovery_drain_claim_v2(self.cursor, &key, seal_key)
            .map_err(map_registry_observation_error)?;
        validate_pending_drain_seal_v2(&key, seal_key, &source_empty_observation, &sealed)?;
        let witness = pending_drain_seal_witness_v2(
            &self.process_instance_id,
            slot,
            &source_empty_observation,
            &sealed,
        )?;
        let binding = RuntimeRegistryPendingDrainSealBindingV2 {
            process_instance_id: self.process_instance_id,
            registry: self.registry,
            sealed,
            source_empty_observation,
            witness: witness.clone(),
        };
        Ok((binding, witness))
    }
}

impl RuntimeRegistryPreparedServingTransitionV2 {
    pub(crate) fn observe_route_set_v2(
        &self,
        route_set_epoch: &RuntimeRouteSetEpochV2,
    ) -> Result<RuntimeRouteSetObservationV2, RuntimeRegistryRecoveryObservationErrorV1> {
        validate_route_set_epoch_v2(
            &self.binding.process_instance_id,
            self.initial_registry_observation_sequence,
            self.initial_retained_slot_count,
            self.initial_retained_empty_tombstone_count,
            route_set_epoch,
        )?;
        let observation = self
            .binding
            .registry
            .revalidate_empty_recovery_cursor_v2(&self.binding.cursor)
            .map_err(map_registry_observation_error)?;
        if RuntimeRegistryGlobalObservationSequenceV2::new(
            observation.observation_sequence().as_non_zero(),
        ) != self.initial_registry_observation_sequence
        {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding);
        }
        project_route_set_observation_v2(&self.binding.process_instance_id, observation)
    }

    pub(crate) fn commit_v2(
        self,
        route_set_epoch: &RuntimeRouteSetEpochV2,
    ) -> Result<
        (
            RuntimeRegistryServingBindingV2,
            RuntimeRouteSetObservationV2,
        ),
        RuntimeRegistryServingTransitionFailureV2,
    > {
        let observation = match self.observe_route_set_v2(route_set_epoch) {
            Ok(observation) => observation,
            Err(error) => {
                return Err(RuntimeRegistryServingTransitionFailureV2 {
                    binding: self.binding,
                    error,
                });
            }
        };
        let Self {
            binding,
            initial_registry_observation_sequence,
            initial_retained_slot_count,
            initial_retained_empty_tombstone_count,
        } = self;
        let RuntimeRegistryEmptyRecoveryBindingV2 {
            process_instance_id,
            registry,
            cursor,
        } = binding;
        drop(cursor);
        Ok((
            RuntimeRegistryServingBindingV2 {
                process_instance_id,
                registry,
                initial_registry_observation_sequence,
                initial_retained_slot_count,
                initial_retained_empty_tombstone_count,
            },
            observation,
        ))
    }

    pub(crate) fn cancel_v2(self) -> RuntimeRegistryEmptyRecoveryBindingV2 {
        self.binding
    }
}

impl RuntimeRegistryServingBindingV2 {
    pub(crate) fn staging_port_v2(&self) -> RuntimeRegistryStagingPortV2 {
        RuntimeRegistryStagingPortV2 {
            process_instance_id: self.process_instance_id.clone(),
            registry: self.registry.clone(),
        }
    }

    pub(crate) fn observe_route_set_v2(
        &self,
        route_set_epoch: &RuntimeRouteSetEpochV2,
    ) -> Result<RuntimeRouteSetObservationV2, RuntimeRegistryRecoveryObservationErrorV1> {
        validate_route_set_epoch_v2(
            &self.process_instance_id,
            self.initial_registry_observation_sequence,
            self.initial_retained_slot_count,
            self.initial_retained_empty_tombstone_count,
            route_set_epoch,
        )?;
        let observation = self
            .registry
            .recovery_observation_guard_v2()
            .map_err(map_registry_observation_error)?
            .observation();
        project_route_set_observation_v2(&self.process_instance_id, observation)
    }

    pub(crate) fn observe_shutdown_route_set_v2(
        &self,
    ) -> Result<RuntimeRouteSetObservationV2, RuntimeRegistryRecoveryObservationErrorV1> {
        let observation = self
            .registry
            .recovery_observation_guard_v2()
            .map_err(map_registry_observation_error)?
            .observation();
        project_route_set_observation_v2(&self.process_instance_id, observation)
    }
}

impl RuntimeRegistryStagingPortV2 {
    pub(crate) fn install_staged_route_v2(
        &self,
        route: ExactServingRouteV1,
        fencing_token: FencingToken,
        emergency: RuntimeRegistryEmergencyTriggerV2,
    ) -> Result<RuntimeRegistryStagedInstallV2, RuntimeRegistryStagingErrorV2> {
        if route.identity().process_instance_id != self.process_instance_id {
            return Err(RuntimeRegistryStagingErrorV2::ProcessMismatch);
        }
        let key = route.slot_key();
        let receipt = self
            .registry
            .install(key, route, fencing_token)
            .map_err(RuntimeRegistryStagingErrorV2::Registry)?;
        let outcome = match receipt.outcome {
            SlotInstallOutcomeV1::Staged => RuntimeRegistryStagedInstallOutcomeV2::Installed,
            SlotInstallOutcomeV1::AlreadyStaged => {
                RuntimeRegistryStagedInstallOutcomeV2::ExactReplay
            }
            SlotInstallOutcomeV1::AlreadyServing | SlotInstallOutcomeV1::AlreadyDraining => {
                return Err(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle);
            }
        };
        let staged = RuntimeRegistryStagedRouteV2 {
            registry: self.registry.clone(),
            identity: receipt.token.identity().clone(),
            token: Some(receipt.token),
            emergency,
        };
        let token = staged
            .token
            .as_ref()
            .ok_or(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)?;
        let witness = self.registry.route_witness(token);
        let atomic = self.registry.atomic_observation_v2(token.key());
        let registry_observation = self.registry.recovery_observation_v2();
        complete_staged_install_v2(outcome, staged, witness, atomic, registry_observation)
    }
}

fn complete_staged_install_v2(
    outcome: RuntimeRegistryStagedInstallOutcomeV2,
    staged: RuntimeRegistryStagedRouteV2,
    witness: Result<SlotRouteWitnessV1, ServingSlotRegistryError>,
    atomic: Result<Option<SlotAtomicObservationV2>, ServingSlotRegistryError>,
    registry_observation: Result<RegistryRecoveryObservationV2, ServingSlotRegistryError>,
) -> Result<RuntimeRegistryStagedInstallV2, RuntimeRegistryStagingErrorV2> {
    let witness = witness.map_err(RuntimeRegistryStagingErrorV2::Registry)?;
    let atomic = atomic
        .map_err(RuntimeRegistryStagingErrorV2::Registry)?
        .ok_or(RuntimeRegistryStagingErrorV2::EvidenceMismatch)?;
    let registry_observation =
        registry_observation.map_err(RuntimeRegistryStagingErrorV2::Registry)?;
    let evidence = validate_staged_evidence_v2(&staged, witness, atomic, registry_observation)?;
    Ok(RuntimeRegistryStagedInstallV2 {
        outcome,
        evidence,
        authority: staged,
    })
}

fn validate_staged_evidence_v2(
    staged: &RuntimeRegistryStagedRouteV2,
    witness: SlotRouteWitnessV1,
    atomic: SlotAtomicObservationV2,
    registry_observation: RegistryRecoveryObservationV2,
) -> Result<RuntimeRegistryStagedInstallEvidenceV2, RuntimeRegistryStagingErrorV2> {
    let token = staged
        .token
        .as_ref()
        .ok_or(RuntimeRegistryStagingErrorV2::UnexpectedLifecycle)?;
    let selected_route_is_valid = match atomic.route.as_ref() {
        Some(selected) if selected == &witness => {
            atomic.admission_state == SlotAdmissionStateV2::Staged
                && atomic.active_interactions == 0
        }
        Some(selected) => match selected.lifecycle {
            SlotLifecycleV1::Serving => atomic.admission_state == SlotAdmissionStateV2::Serving,
            SlotLifecycleV1::Draining => atomic.admission_state == SlotAdmissionStateV2::Draining,
            SlotLifecycleV1::Staged => false,
        },
        None => false,
    };
    if witness.identity != staged.identity
        || witness.fencing_token != token.fencing_token()
        || witness.lifecycle != SlotLifecycleV1::Staged
        || !selected_route_is_valid
        || registry_observation.registry_failed_closed()
        || registry_observation.failed_closed_slot_count() != 0
        || registry_observation.staged_route_count() == 0
        || registry_observation.observation_sequence().get() < atomic.observation_sequence.get()
    {
        return Err(RuntimeRegistryStagingErrorV2::EvidenceMismatch);
    }
    staged.ensure_staged_v2()?;
    Ok(RuntimeRegistryStagedInstallEvidenceV2 {
        route: witness,
        active_interactions: 0,
        admission_generation: atomic.admission_generation,
        slot_observation_sequence: atomic.observation_sequence,
        registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
            registry_observation.observation_sequence().as_non_zero(),
        ),
    })
}

impl RuntimeRegistryServingTransitionFailureV2 {
    pub(crate) fn error_v2(&self) -> RuntimeRegistryRecoveryObservationErrorV1 {
        self.error
    }

    pub(crate) fn into_binding_v2(self) -> RuntimeRegistryEmptyRecoveryBindingV2 {
        self.binding
    }
}

impl Debug for RuntimeRegistryPreparedServingTransitionV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryPreparedServingTransitionV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryServingBindingV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryServingBindingV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryServingTransitionFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryServingTransitionFailureV2(<redacted>)")
    }
}

impl Debug for RuntimeRegistryEmptyRecoveryBindingV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryEmptyRecoveryBindingV2(<redacted>)")
    }
}

pub(crate) struct RuntimeRegistryPendingDrainSealBindingV2 {
    process_instance_id: ProcessInstanceId,
    registry: ServingSlotRegistryV1,
    sealed: SealedEmptyRecoveryDrainClaimV2,
    source_empty_observation: RuntimeRegistryRecoveryEmptyObservationV2,
    witness: RuntimePendingDrainRegistrySealWitnessV2,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeRegistryPendingDrainSealBindingV2 {
    pub(crate) fn seal_key_bytes_v2(&self) -> [u8; 16] {
        *self.sealed.seal().seal_key().as_bytes()
    }

    pub(crate) fn seal_generation_v2(&self) -> NonZeroU64 {
        self.sealed.seal().seal_generation()
    }

    pub(crate) fn source_empty_observation_v2(&self) -> &RuntimeRegistryRecoveryEmptyObservationV2 {
        &self.source_empty_observation
    }

    pub(crate) fn source_slot_is_present_v2(&self) -> bool {
        self.sealed.source_slot_observation().is_some()
    }

    pub(crate) fn source_slot_admission_generation_v2(&self) -> Option<NonZeroU64> {
        self.sealed
            .source_slot_observation()
            .map(|observation| observation.admission_generation)
    }

    pub(crate) fn source_slot_observation_sequence_v2(&self) -> Option<NonZeroU64> {
        self.sealed
            .source_slot_observation()
            .map(|observation| observation.observation_sequence)
    }

    pub(crate) fn post_seal_admission_generation_v2(&self) -> NonZeroU64 {
        self.sealed.slot_observation().admission_generation
    }

    pub(crate) fn post_seal_slot_observation_sequence_v2(&self) -> NonZeroU64 {
        self.sealed.slot_observation().observation_sequence
    }

    pub(crate) fn post_seal_global_observation_sequence_v2(
        &self,
    ) -> RuntimeRegistryGlobalObservationSequenceV2 {
        RuntimeRegistryGlobalObservationSequenceV2::new(
            self.sealed
                .registry_observation()
                .observation_sequence()
                .as_non_zero(),
        )
    }

    pub(crate) fn post_seal_retained_slot_count_v2(&self) -> u64 {
        self.sealed.registry_observation().retained_slot_count()
    }

    pub(crate) fn post_seal_retained_empty_tombstone_count_v2(&self) -> u64 {
        self.sealed
            .registry_observation()
            .retained_empty_tombstone_count()
    }

    pub(crate) fn post_seal_staged_route_count_v2(&self) -> u64 {
        self.sealed.registry_observation().staged_route_count()
    }

    pub(crate) fn post_seal_serving_route_count_v2(&self) -> u64 {
        self.sealed.registry_observation().serving_route_count()
    }

    pub(crate) fn post_seal_draining_route_count_v2(&self) -> u64 {
        self.sealed.registry_observation().draining_route_count()
    }

    pub(crate) fn post_seal_sealed_slot_count_v2(&self) -> u64 {
        self.sealed.registry_observation().sealed_slot_count()
    }

    pub(crate) fn post_seal_active_interaction_count_v2(&self) -> u64 {
        self.sealed
            .registry_observation()
            .active_interaction_count()
    }

    pub(crate) fn post_seal_failed_closed_slot_count_v2(&self) -> u64 {
        self.sealed
            .registry_observation()
            .failed_closed_slot_count()
    }

    pub(crate) fn post_seal_registry_failed_closed_v2(&self) -> bool {
        self.sealed.registry_observation().registry_failed_closed()
    }

    pub(crate) fn revalidate_sealed_v2(
        &self,
    ) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
        let first_registry_observation = self
            .registry
            .recovery_observation_v2()
            .map_err(map_registry_observation_error)?;
        if first_registry_observation != self.sealed.registry_observation() {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let slot_observation = self
            .registry
            .atomic_observation_v2(self.sealed.seal().key())
            .map_err(map_registry_observation_error)?;
        if slot_observation.as_ref() != Some(self.sealed.slot_observation()) {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let second_registry_observation = self
            .registry
            .recovery_observation_v2()
            .map_err(map_registry_observation_error)?;
        if second_registry_observation != self.sealed.registry_observation() {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        validate_pending_drain_seal_v2(
            self.sealed.seal().key(),
            self.sealed.seal().seal_key(),
            &self.source_empty_observation,
            &self.sealed,
        )
    }

    pub(crate) fn into_empty_binding_after_durable_ack_v2(
        self,
        durable: &RuntimeDurablyAcknowledgedPendingDrainV2,
    ) -> Result<
        (
            RuntimeRegistryEmptyRecoveryBindingV2,
            RuntimePendingDrainRegistryUnsealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        self.into_empty_binding_after_durable_seal_common_v2(durable.seal_witness())
    }

    fn into_empty_binding_after_durable_seal_common_v2(
        self,
        durable_seal: &RuntimePendingDrainRegistrySealWitnessV2,
    ) -> Result<
        (
            RuntimeRegistryEmptyRecoveryBindingV2,
            RuntimePendingDrainRegistryUnsealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        require_pending_drain_durable_seal_match_v2(&self.witness, durable_seal)?;
        self.revalidate_sealed_v2()?;
        let expected_slot_observation_sequence = successor_persistence_non_zero_u64_v2(
            self.sealed.slot_observation().observation_sequence,
        )?;
        let expected_admission_generation = successor_persistence_non_zero_u64_v2(
            self.sealed.slot_observation().admission_generation,
        )?;
        let expected_registry_observation_sequence = successor_persistence_non_zero_u64_v2(
            self.sealed
                .registry_observation()
                .observation_sequence()
                .as_non_zero(),
        )?;
        let expected_retained_slot_count = self.sealed.registry_observation().retained_slot_count();
        let expected_retained_empty_tombstone_count = self
            .sealed
            .registry_observation()
            .retained_empty_tombstone_count()
            .checked_add(1)
            .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow)?;
        let expected_empty_observation = accept_runtime_registry_recovery_empty_observation_v2(
            self.process_instance_id.clone(),
            RuntimeRegistryRecoveryObservationInputV2 {
                observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                    expected_registry_observation_sequence,
                ),
                retained_slot_count: expected_retained_slot_count,
                retained_empty_tombstone_count: expected_retained_empty_tombstone_count,
                staged_route_count: 0,
                serving_route_count: 0,
                draining_route_count: 0,
                sealed_slot_count: 0,
                active_interaction_count: 0,
                failed_closed_slot_count: 0,
                registry_failed_closed: false,
            },
        )
        .map_err(map_worker_observation_error)?;
        let preflight_witness = RuntimePendingDrainRegistryUnsealWitnessV2::new(
            self.process_instance_id.clone(),
            durable_seal.slot().clone(),
            expected_admission_generation,
            expected_slot_observation_sequence,
            expected_empty_observation,
        )
        .map_err(map_pending_drain_compound_error)?;
        drop(preflight_witness);
        let Self {
            process_instance_id,
            registry,
            sealed,
            ..
        } = self;
        let unsealed = registry
            .unseal_empty_recovery_drain_claim_v2(sealed)
            .map_err(map_registry_observation_error)?;
        let slot_observation = unsealed.slot_observation();
        if slot_observation.route.is_some()
            || slot_observation.admission_state != SlotAdmissionStateV2::Empty
            || slot_observation.active_interactions != 0
            || slot_observation.admission_generation != expected_admission_generation
            || slot_observation.observation_sequence != expected_slot_observation_sequence
        {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let registry_observation = unsealed.registry_observation();
        if registry_observation.observation_sequence().as_non_zero()
            != expected_registry_observation_sequence
            || registry_observation.retained_slot_count() != expected_retained_slot_count
            || registry_observation.retained_empty_tombstone_count()
                != expected_retained_empty_tombstone_count
        {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let projected_empty_observation =
            project_empty_observation_v2(&process_instance_id, registry_observation)?;
        let binding = RuntimeRegistryEmptyRecoveryBindingV2 {
            process_instance_id: process_instance_id.clone(),
            registry,
            cursor: unsealed.into_cursor(),
        };
        if binding.revalidate_empty_projection_unordered_v2()? != projected_empty_observation {
            return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
        }
        let witness = RuntimePendingDrainRegistryUnsealWitnessV2::new(
            process_instance_id,
            durable_seal.slot().clone(),
            expected_admission_generation,
            expected_slot_observation_sequence,
            projected_empty_observation,
        )
        .map_err(map_pending_drain_compound_error)?;
        Ok((binding, witness))
    }
}

fn require_pending_drain_durable_seal_match_v2(
    actual: &RuntimePendingDrainRegistrySealWitnessV2,
    authorized: &RuntimePendingDrainRegistrySealWitnessV2,
) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
    if actual != authorized {
        Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)
    } else {
        Ok(())
    }
}

impl Debug for RuntimeRegistryPendingDrainSealBindingV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryPendingDrainSealBindingV2(<redacted>)")
    }
}

pub(crate) struct RuntimeRegistryPendingDrainSuccessionSealBindingV3 {
    binding: RuntimeRegistryPendingDrainSealBindingV2,
}

impl RuntimeRegistryPendingDrainSuccessionSealBindingV3 {
    pub(crate) fn revalidate_sealed_v3(
        &self,
    ) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
        self.binding.revalidate_sealed_v2()
    }

    pub(crate) fn into_empty_binding_after_durable_succession_v3(
        self,
        durable: &RuntimeDurablyAcknowledgedPendingDrainSuccessionV3,
    ) -> Result<
        (
            RuntimeRegistryEmptyRecoveryBindingV2,
            RuntimePendingDrainRegistryUnsealWitnessV2,
        ),
        RuntimeRegistryRecoveryObservationErrorV1,
    > {
        self.binding
            .into_empty_binding_after_durable_seal_common_v2(durable.seal_witness())
    }
}

impl Debug for RuntimeRegistryPendingDrainSuccessionSealBindingV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegistryPendingDrainSuccessionSealBindingV3(<redacted>)")
    }
}

fn validate_pending_drain_seal_v2(
    key: &ServingSlotKeyV1,
    seal_key: SlotSealKeyV2,
    source_empty_observation: &RuntimeRegistryRecoveryEmptyObservationV2,
    sealed: &SealedEmptyRecoveryDrainClaimV2,
) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
    let seal = sealed.seal();
    let source_slot_observation = sealed.source_slot_observation();
    let slot_observation = sealed.slot_observation();
    let registry_observation = sealed.registry_observation();
    if seal.key() != key
        || seal.seal_key() != seal_key
        || seal.route().is_some()
        || slot_observation.route.is_some()
        || slot_observation.active_interactions != 0
        || slot_observation.admission_state
            != (SlotAdmissionStateV2::DrainClaimSealed {
                seal_key,
                seal_generation: seal.seal_generation(),
            })
        || registry_observation.registry_failed_closed()
        || registry_observation.staged_route_count() != 0
        || registry_observation.serving_route_count() != 0
        || registry_observation.draining_route_count() != 0
        || registry_observation.sealed_slot_count() != 1
        || registry_observation.active_interaction_count() != 0
        || registry_observation.failed_closed_slot_count() != 0
    {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
    }
    let expected_registry_sequence = successor_non_zero_u64_v2(
        source_empty_observation
            .observation_sequence()
            .as_non_zero(),
    )?;
    if registry_observation.observation_sequence().as_non_zero() != expected_registry_sequence {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
    }
    let source_retained_slot_count = source_empty_observation.retained_slot_count();
    let source_retained_empty_tombstone_count =
        source_empty_observation.retained_empty_tombstone_count();
    if source_retained_slot_count != source_retained_empty_tombstone_count {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
    }
    let (expected_retained_slot_count, expected_retained_empty_tombstone_count) =
        if let Some(source_slot_observation) = source_slot_observation {
            if source_slot_observation.route.is_some()
                || source_slot_observation.admission_state != SlotAdmissionStateV2::Empty
                || source_slot_observation.active_interactions != 0
                || slot_observation.admission_generation
                    != successor_non_zero_u64_v2(source_slot_observation.admission_generation)?
                || slot_observation.observation_sequence
                    != successor_non_zero_u64_v2(source_slot_observation.observation_sequence)?
            {
                return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
            }
            (
                source_retained_slot_count,
                source_retained_empty_tombstone_count
                    .checked_sub(1)
                    .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)?,
            )
        } else {
            if slot_observation.admission_generation != NonZeroU64::MIN
                || slot_observation.observation_sequence != NonZeroU64::MIN
            {
                return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
            }
            (
                source_retained_slot_count
                    .checked_add(1)
                    .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow)?,
                source_retained_empty_tombstone_count,
            )
        };
    if registry_observation.retained_slot_count() != expected_retained_slot_count
        || registry_observation.retained_empty_tombstone_count()
            != expected_retained_empty_tombstone_count
        || registry_observation.retained_slot_count()
            != registry_observation
                .retained_empty_tombstone_count()
                .checked_add(1)
                .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow)?
    {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation);
    }
    Ok(())
}

fn pending_drain_seal_witness_v2(
    process_instance_id: &ProcessInstanceId,
    slot: &RuntimeServingSlotV2,
    source_empty_observation: &RuntimeRegistryRecoveryEmptyObservationV2,
    sealed: &SealedEmptyRecoveryDrainClaimV2,
) -> Result<RuntimePendingDrainRegistrySealWitnessV2, RuntimeRegistryRecoveryObservationErrorV1> {
    let source_slot_observation =
        sealed
            .source_slot_observation()
            .map(|observation| RuntimePendingDrainSlotObservationV2 {
                admission_generation: observation.admission_generation,
                observation_sequence: observation.observation_sequence,
            });
    let registry_observation = sealed.registry_observation();
    RuntimePendingDrainRegistrySealWitnessV2::new(RuntimePendingDrainRegistrySealWitnessInputV2 {
        process_instance_id: process_instance_id.clone(),
        slot: slot.clone(),
        pre_slot_observation: source_slot_observation,
        seal_key: *sealed.seal().seal_key().as_bytes(),
        seal_generation: sealed.seal().seal_generation(),
        post_slot_admission_generation: sealed.slot_observation().admission_generation,
        post_slot_observation_sequence: sealed.slot_observation().observation_sequence,
        pre_registry_observation_sequence: source_empty_observation.observation_sequence(),
        pre_registry_retained_slot_count: source_empty_observation.retained_slot_count(),
        pre_registry_retained_empty_tombstone_count: source_empty_observation
            .retained_empty_tombstone_count(),
        post_registry_observation: RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                registry_observation.observation_sequence().as_non_zero(),
            ),
            retained_slot_count: registry_observation.retained_slot_count(),
            retained_empty_tombstone_count: registry_observation.retained_empty_tombstone_count(),
            staged_route_count: registry_observation.staged_route_count(),
            serving_route_count: registry_observation.serving_route_count(),
            draining_route_count: registry_observation.draining_route_count(),
            sealed_slot_count: registry_observation.sealed_slot_count(),
            active_interaction_count: registry_observation.active_interaction_count(),
            failed_closed_slot_count: registry_observation.failed_closed_slot_count(),
            registry_failed_closed: registry_observation.registry_failed_closed(),
        },
    })
    .map_err(map_pending_drain_compound_error)
}

fn map_pending_drain_compound_error(
    _error: RuntimePendingDrainCompoundErrorV2,
) -> RuntimeRegistryRecoveryObservationErrorV1 {
    RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation
}

fn successor_non_zero_u64_v2(
    value: NonZeroU64,
) -> Result<NonZeroU64, RuntimeRegistryRecoveryObservationErrorV1> {
    value
        .get()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow)
}

fn successor_persistence_non_zero_u64_v2(
    value: NonZeroU64,
) -> Result<NonZeroU64, RuntimeRegistryRecoveryObservationErrorV1> {
    let successor = successor_non_zero_u64_v2(value)?;
    if successor.get() > i64::MAX as u64 {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::ObservationSequenceOutOfRange);
    }
    Ok(successor)
}

pub fn compose_runtime_registry_bootstrap_v1(
    process_instance_id: ProcessInstanceId,
    gateway: GatewayResourceConfigV1,
) -> Result<RuntimeRegistryBootstrapV1, RuntimeRegistryBootstrapErrorV1> {
    let max_active_interactions_per_slot =
        registry_active_interaction_capacity(gateway.global_admission_capacity())?;
    let registry = ServingSlotRegistryV1::new(ServingSlotRegistryConfigV1 {
        max_slots: REGISTRY_MAX_SLOTS,
        max_active_interactions_per_slot,
        max_retired_routes_per_slot: REGISTRY_MAX_RETIRED_ROUTES_PER_SLOT,
    });
    Ok(RuntimeRegistryBootstrapV1 {
        process_instance_id,
        registry,
    })
}

fn validate_route_set_epoch_v2(
    process_instance_id: &ProcessInstanceId,
    initial_registry_observation_sequence: RuntimeRegistryGlobalObservationSequenceV2,
    initial_retained_slot_count: u64,
    initial_retained_empty_tombstone_count: u64,
    route_set_epoch: &RuntimeRouteSetEpochV2,
) -> Result<(), RuntimeRegistryRecoveryObservationErrorV1> {
    if route_set_epoch.process_instance_id() != process_instance_id
        || route_set_epoch.initial_registry_observation_sequence()
            != initial_registry_observation_sequence
        || route_set_epoch.initial_retained_slot_count() != initial_retained_slot_count
        || route_set_epoch.initial_retained_empty_tombstone_count()
            != initial_retained_empty_tombstone_count
    {
        Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)
    } else {
        Ok(())
    }
}

fn project_route_set_observation_v2(
    process_instance_id: &ProcessInstanceId,
    observation: RegistryRecoveryObservationV2,
) -> Result<RuntimeRouteSetObservationV2, RuntimeRegistryRecoveryObservationErrorV1> {
    if observation.registry_failed_closed() || observation.failed_closed_slot_count() != 0 {
        return Err(RuntimeRegistryRecoveryObservationErrorV1::FailedClosed);
    }
    accept_projected_route_set_observation_v2(
        process_instance_id.clone(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                observation.observation_sequence().as_non_zero(),
            ),
            retained_slot_count: observation.retained_slot_count(),
            retained_empty_tombstone_count: observation.retained_empty_tombstone_count(),
            staged_route_count: observation.staged_route_count(),
            serving_route_count: observation.serving_route_count(),
            draining_route_count: observation.draining_route_count(),
            sealed_slot_count: observation.sealed_slot_count(),
            active_interaction_count: observation.active_interaction_count(),
            failed_closed_slot_count: observation.failed_closed_slot_count(),
            registry_failed_closed: observation.registry_failed_closed(),
        },
    )
}

fn accept_projected_route_set_observation_v2(
    process_instance_id: ProcessInstanceId,
    registry: RuntimeRegistryRecoveryObservationInputV2,
) -> Result<RuntimeRouteSetObservationV2, RuntimeRegistryRecoveryObservationErrorV1> {
    accept_runtime_route_set_observation_v2(RuntimeRouteSetObservationInputV2 {
        process_instance_id,
        registry,
    })
    .map_err(map_route_set_observation_error)
}

fn map_route_set_observation_error(
    error: RuntimeRouteSetObservationErrorV2,
) -> RuntimeRegistryRecoveryObservationErrorV1 {
    match error {
        RuntimeRouteSetObservationErrorV2::ObservationSequenceOutOfRange => {
            RuntimeRegistryRecoveryObservationErrorV1::ObservationSequenceOutOfRange
        }
        RuntimeRouteSetObservationErrorV2::FailedClosed => {
            RuntimeRegistryRecoveryObservationErrorV1::FailedClosed
        }
        RuntimeRouteSetObservationErrorV2::InconsistentRetainedCounts => {
            RuntimeRegistryRecoveryObservationErrorV1::InconsistentRetainedCounts
        }
    }
}

fn registry_active_interaction_capacity(
    capacity: NonZeroUsize,
) -> Result<NonZeroU32, RuntimeRegistryBootstrapErrorV1> {
    u32::try_from(capacity.get())
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or(RuntimeRegistryBootstrapErrorV1::ActiveInteractionCapacity)
}

fn project_empty_observation_v2(
    process_instance_id: &ProcessInstanceId,
    observation: RegistryRecoveryObservationV2,
) -> Result<RuntimeRegistryRecoveryEmptyObservationV2, RuntimeRegistryRecoveryObservationErrorV1> {
    let observation_sequence = NonZeroU64::new(observation.observation_sequence().get())
        .ok_or(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)?;
    accept_runtime_registry_recovery_empty_observation_v2(
        process_instance_id.clone(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                observation_sequence,
            ),
            retained_slot_count: observation.retained_slot_count(),
            retained_empty_tombstone_count: observation.retained_empty_tombstone_count(),
            staged_route_count: observation.staged_route_count(),
            serving_route_count: observation.serving_route_count(),
            draining_route_count: observation.draining_route_count(),
            sealed_slot_count: observation.sealed_slot_count(),
            active_interaction_count: observation.active_interaction_count(),
            failed_closed_slot_count: observation.failed_closed_slot_count(),
            registry_failed_closed: observation.registry_failed_closed(),
        },
    )
    .map_err(map_worker_observation_error)
}

fn map_registry_observation_error(
    error: ServingSlotRegistryError,
) -> RuntimeRegistryRecoveryObservationErrorV1 {
    match error {
        ServingSlotRegistryError::RegistryPoisoned => {
            RuntimeRegistryRecoveryObservationErrorV1::RegistryUnavailable
        }
        ServingSlotRegistryError::RegistryObservationInvalid => {
            RuntimeRegistryRecoveryObservationErrorV1::ObservationInvalid
        }
        ServingSlotRegistryError::RegistryObservationOverflow => {
            RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow
        }
        ServingSlotRegistryError::RegistryRecoveryNotEmpty => {
            RuntimeRegistryRecoveryObservationErrorV1::NotEmpty
        }
        ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor => {
            RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding
        }
        ServingSlotRegistryError::TargetSlotMismatch
        | ServingSlotRegistryError::StaleFencingToken { .. }
        | ServingSlotRegistryError::StaleRuntimeGeneration { .. }
        | ServingSlotRegistryError::RuntimeGenerationIdentityConflict
        | ServingSlotRegistryError::AuthorityTargetMismatch
        | ServingSlotRegistryError::NonSuccessorFencingToken { .. }
        | ServingSlotRegistryError::FencingTokenExhausted
        | ServingSlotRegistryError::StaleMutationToken
        | ServingSlotRegistryError::ActivationTargetMismatch
        | ServingSlotRegistryError::NotServing
        | ServingSlotRegistryError::ActiveInteractionCapacityExceeded
        | ServingSlotRegistryError::NotDraining
        | ServingSlotRegistryError::ActiveInteractionsRemain { .. }
        | ServingSlotRegistryError::RetiredRouteCapacityExceeded
        | ServingSlotRegistryError::SlotCapacityExceeded
        | ServingSlotRegistryError::IncarnationExhausted
        | ServingSlotRegistryError::SlotSequenceExhausted
        | ServingSlotRegistryError::RegistrySequenceExhausted
        | ServingSlotRegistryError::AdmissionGenerationMismatch { .. }
        | ServingSlotRegistryError::StaleSlotObservation
        | ServingSlotRegistryError::SlotSealed
        | ServingSlotRegistryError::StaleSlotSeal
        | ServingSlotRegistryError::V4RegistryMismatch
        | ServingSlotRegistryError::V4CapabilityStale
        | ServingSlotRegistryError::V4RouteMismatch
        | ServingSlotRegistryError::V4LifecycleMismatch
        | ServingSlotRegistryError::V4FenceMismatch
        | ServingSlotRegistryError::V4GuardMismatch
        | ServingSlotRegistryError::V4ReceiptMismatch
        | ServingSlotRegistryError::V4ObservationMismatch
        | ServingSlotRegistryError::V4EmptySuccessionMismatch => {
            RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation
        }
    }
}

fn map_worker_observation_error(
    error: RuntimeRegistryRecoveryObservationErrorV2,
) -> RuntimeRegistryRecoveryObservationErrorV1 {
    match error {
        RuntimeRegistryRecoveryObservationErrorV2::FailedClosed => {
            RuntimeRegistryRecoveryObservationErrorV1::FailedClosed
        }
        RuntimeRegistryRecoveryObservationErrorV2::ObservationSequenceOutOfRange => {
            RuntimeRegistryRecoveryObservationErrorV1::ObservationSequenceOutOfRange
        }
        RuntimeRegistryRecoveryObservationErrorV2::NotEmpty => {
            RuntimeRegistryRecoveryObservationErrorV1::NotEmpty
        }
        RuntimeRegistryRecoveryObservationErrorV2::InconsistentRetainedCounts => {
            RuntimeRegistryRecoveryObservationErrorV1::InconsistentRetainedCounts
        }
    }
}

#[cfg(test)]
#[path = "registry_succession_tests.rs"]
pub(crate) mod succession_tests;

#[cfg(test)]
#[path = "registry_staging_tests.rs"]
mod staging_tests;

#[cfg(test)]
pub(crate) use tests::{
    barrier_b_activation_for_gateway_test_v2, nonfinal_barrier_b_replacement_for_gateway_test_v2,
};

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU64, NonZeroUsize};

    use automation_runtime_controller::{RuntimeDrainIntentIdV2, RuntimeServingSlotV2};
    use automation_runtime_convergence::{
        FencingToken, ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1,
    };
    use automation_runtime_registry::{
        ExactServingRouteV1, ServingSlotKeyV1, ServingSlotRegistryError, ServingSlotRegistryV1,
        SlotAdmissionStateV2, SlotLifecycleV1, SlotSealKeyV2,
    };
    use automation_runtime_worker::{
        RuntimePendingDrainCandidateV2, RuntimePendingDrainStateDigestV2,
        RuntimeRegistryRecoveryObservationErrorV2,
    };
    use serde_json::json;

    use super::{
        compose_runtime_registry_bootstrap_v1, map_registry_observation_error,
        map_worker_observation_error, registry_active_interaction_capacity,
        RuntimeRegistryBootstrapErrorV1, RuntimeRegistryEmergencyTriggerV2,
        RuntimeRegistryEmptyRecoveryBindingV2, RuntimeRegistryPredecessorReplacementErrorV2,
        RuntimeRegistryRecoveryObservationErrorV1, RuntimeRegistryReplacementRouteV2,
        RuntimeRegistryStagingPortV2,
    };
    use crate::GatewayResourceConfigV1;

    fn target() -> RuntimeDeploymentTargetV1 {
        serde_json::from_value(json!({
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "2".repeat(64),
            "binding_revision": 1,
            "binding_fingerprint": "3".repeat(64)
        }))
        .unwrap()
    }

    fn slot_key() -> ServingSlotKeyV1 {
        ServingSlotKeyV1::from_target(&target())
    }

    fn candidate() -> RuntimePendingDrainCandidateV2 {
        let target = target();
        RuntimePendingDrainCandidateV2::new(
            RuntimeDrainIntentIdV2::parse("07".repeat(16)).unwrap(),
            RuntimeServingSlotV2::from_target(&target),
            target,
            NonZeroU64::new(1).unwrap(),
            RuntimePendingDrainStateDigestV2::new([8; 32]).unwrap(),
        )
        .unwrap()
    }

    fn seal_key() -> SlotSealKeyV2 {
        SlotSealKeyV2::try_from([7_u8; 16].as_slice()).unwrap()
    }

    fn replacement_route(
        process_instance_id: &str,
        runtime_generation: u64,
        version: u64,
    ) -> ExactServingRouteV1 {
        let content_hash = "9f2bbed3d90d3439ebe5bb07a69f8ff179c29e8c71500b6890a7d24653a65ff6";
        let binding_fingerprint =
            "a44fd4f629a1183147a25a8afb93b026de7e3f92efe737637da222617df0c655";
        let identity: RuntimeProcessIdentityV1 = serde_json::from_value(json!({
            "target": {
                "guild_id": "42",
                "ruleset_key": "studyroom",
                "version": version,
                "content_hash": content_hash,
                "binding_revision": 1,
                "binding_fingerprint": binding_fingerprint
            },
            "runtime_generation": runtime_generation,
            "process_instance_id": process_instance_id
        }))
        .unwrap();
        let ruleset = serde_json::from_value(json!({
            "guild_id": "42",
            "ruleset_key": "studyroom",
            "version": version,
            "schema_version": 1,
            "definition": {
                "version": 1,
                "panels": [],
                "modals": [],
                "rules": []
            },
            "content_hash": content_hash,
            "created_by": "9"
        }))
        .unwrap();
        ExactServingRouteV1::new(identity, ruleset, Default::default()).unwrap()
    }

    fn replacement_registry(
        process_instance_id: &str,
    ) -> (RuntimeRegistryStagingPortV2, ServingSlotRegistryV1) {
        let registry = ServingSlotRegistryV1::new(Default::default());
        (
            RuntimeRegistryStagingPortV2 {
                process_instance_id: ProcessInstanceId::parse(process_instance_id).unwrap(),
                registry: registry.clone(),
            },
            registry,
        )
    }

    fn install_replacement(
        port: &RuntimeRegistryStagingPortV2,
        route: ExactServingRouteV1,
        fencing_token: u64,
    ) -> RuntimeRegistryReplacementRouteV2 {
        port.install_staged_route_v2(
            route,
            FencingToken::new(fencing_token).unwrap(),
            RuntimeRegistryEmergencyTriggerV2::new(|| {}),
        )
        .unwrap()
        .into_parts_v2()
        .2
        .into_replacement_v2()
    }

    pub(crate) fn nonfinal_barrier_b_replacement_for_gateway_test_v2(
        process_instance_id: &str,
    ) -> RuntimeRegistryReplacementRouteV2 {
        let (port, _) = replacement_registry(process_instance_id);
        let replacement =
            install_replacement(&port, replacement_route(process_instance_id, 1, 1), 1);
        replacement
            .transition_predecessor_to_draining_v2(None)
            .unwrap();
        replacement
    }

    pub(crate) fn barrier_b_activation_for_gateway_test_v2(
        process_instance_id: &str,
    ) -> super::RuntimeRegistryBarrierBActivationV2 {
        let (port, _) = replacement_registry(process_instance_id);
        let replacement =
            install_replacement(&port, replacement_route(process_instance_id, 1, 1), 1);
        replacement
            .transition_predecessor_to_draining_v2(None)
            .unwrap();
        replacement.remove_drained_predecessor_v2().unwrap();
        replacement.activate_barrier_b_v2().unwrap()
    }

    impl super::RuntimeRegistryBootstrapV1 {
        pub(crate) fn advance_empty_sequence_for_test_v2(&self) {
            let key = slot_key();
            let expected = self.registry.atomic_observation_v2(&key).unwrap();
            let (seal, _) = self
                .registry
                .seal_drain_claim_v2(&key, seal_key(), expected.as_ref())
                .unwrap();
            self.registry.unseal_drain_claim_v2(seal).unwrap();
        }
    }

    #[test]
    fn composes_exact_empty_projection_without_exposing_registry_authority() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();

        let projection = bootstrap.observe_recovery_empty_projection_v2().unwrap();

        assert_eq!(
            projection.process_instance_id().as_str(),
            "runtime-process:1"
        );
        assert_eq!(projection.observation_sequence().get(), 1);
        assert_eq!(projection.retained_slot_count(), 0);
        assert_eq!(projection.retained_empty_tombstone_count(), 0);
        assert_eq!(
            format!("{bootstrap:?}"),
            "RuntimeRegistryBootstrapV1(<redacted>)"
        );
    }

    #[test]
    fn private_empty_binding_revalidates_the_exact_bootstrap_and_sequence() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let guard = bootstrap.recovery_observation_guard_unordered_v2().unwrap();
        let projected = guard.empty_projection_v2().unwrap();
        let binding = guard.into_empty_binding_v2().unwrap();

        assert_eq!(
            binding.revalidate_empty_projection_unordered_v2().unwrap(),
            projected
        );
        assert_eq!(
            format!("{binding:?}"),
            "RuntimeRegistryEmptyRecoveryBindingV2(<redacted>)"
        );
    }

    #[test]
    fn private_empty_binding_rejects_sequence_aba_and_foreign_registry() {
        let process_instance_id = ProcessInstanceId::parse("runtime-process:1").unwrap();
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            process_instance_id.clone(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let key = slot_key();
        let (seal, _) = bootstrap
            .registry
            .seal_drain_claim_v2(&key, seal_key(), None)
            .unwrap();
        bootstrap.registry.unseal_drain_claim_v2(seal).unwrap();
        let stale = bootstrap
            .recovery_observation_guard_unordered_v2()
            .unwrap()
            .into_empty_binding_v2()
            .unwrap();
        let before = bootstrap.observe_recovery_empty_projection_v2().unwrap();
        let expected = bootstrap.registry.atomic_observation_v2(&key).unwrap();
        let (seal, _) = bootstrap
            .registry
            .seal_drain_claim_v2(&key, seal_key(), expected.as_ref())
            .unwrap();
        bootstrap.registry.unseal_drain_claim_v2(seal).unwrap();
        let after = bootstrap.observe_recovery_empty_projection_v2().unwrap();

        assert_eq!(before.retained_slot_count(), after.retained_slot_count());
        assert_eq!(
            before.retained_empty_tombstone_count(),
            after.retained_empty_tombstone_count()
        );
        assert_ne!(before.observation_sequence(), after.observation_sequence());

        assert_eq!(
            stale.revalidate_empty_projection_unordered_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding)
        );

        let source = compose_runtime_registry_bootstrap_v1(
            process_instance_id.clone(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let foreign = compose_runtime_registry_bootstrap_v1(
            process_instance_id,
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let cursor = source
            .registry
            .recovery_observation_guard_v2()
            .unwrap()
            .into_empty_cursor()
            .unwrap();
        let foreign_binding = RuntimeRegistryEmptyRecoveryBindingV2 {
            process_instance_id: foreign.process_instance_id.clone(),
            registry: foreign.registry.clone(),
            cursor,
        };

        assert_eq!(
            foreign_binding.revalidate_empty_projection_unordered_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding)
        );
    }

    #[test]
    fn pending_drain_seal_binding_tracks_absent_slot_s0_s1_exactly() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let guard = bootstrap.recovery_observation_guard_unordered_v2().unwrap();
        let source = guard.empty_projection_v2().unwrap();
        let binding = guard.into_empty_binding_v2().unwrap();

        let (sealed, witness) = binding
            .into_pending_drain_seal_binding_v2(&candidate())
            .unwrap();

        assert_eq!(sealed.source_empty_observation_v2(), &source);
        assert_eq!(&witness, &sealed.witness);
        assert_eq!(sealed.seal_key_bytes_v2(), [7_u8; 16]);
        assert_eq!(sealed.seal_generation_v2().get(), 1);
        assert!(!sealed.source_slot_is_present_v2());
        assert_eq!(sealed.source_slot_admission_generation_v2(), None);
        assert_eq!(sealed.source_slot_observation_sequence_v2(), None);
        assert_eq!(sealed.post_seal_admission_generation_v2().get(), 1);
        assert_eq!(sealed.post_seal_slot_observation_sequence_v2().get(), 1);
        assert_eq!(
            sealed.post_seal_global_observation_sequence_v2().get(),
            source.observation_sequence().get() + 1
        );
        assert_eq!(sealed.post_seal_retained_slot_count_v2(), 1);
        assert_eq!(sealed.post_seal_retained_empty_tombstone_count_v2(), 0);
        assert_eq!(sealed.post_seal_staged_route_count_v2(), 0);
        assert_eq!(sealed.post_seal_serving_route_count_v2(), 0);
        assert_eq!(sealed.post_seal_draining_route_count_v2(), 0);
        assert_eq!(sealed.post_seal_sealed_slot_count_v2(), 1);
        assert_eq!(sealed.post_seal_active_interaction_count_v2(), 0);
        assert_eq!(sealed.post_seal_failed_closed_slot_count_v2(), 0);
        assert!(!sealed.post_seal_registry_failed_closed_v2());
        assert_eq!(
            format!("{sealed:?}"),
            "RuntimeRegistryPendingDrainSealBindingV2(<redacted>)"
        );
        sealed.revalidate_sealed_v2().unwrap();
    }

    #[test]
    fn pending_drain_seal_binding_preserves_tombstone_local_generation() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        bootstrap.advance_empty_sequence_for_test_v2();
        let guard = bootstrap.recovery_observation_guard_unordered_v2().unwrap();
        let source = guard.empty_projection_v2().unwrap();
        let binding = guard.into_empty_binding_v2().unwrap();

        let (sealed, _) = binding
            .into_pending_drain_seal_binding_v2(&candidate())
            .unwrap();

        assert!(sealed.source_slot_is_present_v2());
        assert_eq!(
            sealed.source_slot_admission_generation_v2().unwrap().get(),
            2
        );
        assert_eq!(
            sealed.source_slot_observation_sequence_v2().unwrap().get(),
            2
        );
        assert_eq!(sealed.seal_generation_v2().get(), 2);
        assert_eq!(sealed.post_seal_admission_generation_v2().get(), 3);
        assert_eq!(sealed.post_seal_slot_observation_sequence_v2().get(), 3);
        assert_eq!(sealed.post_seal_retained_slot_count_v2(), 1);
        assert_eq!(sealed.post_seal_retained_empty_tombstone_count_v2(), 0);
        assert_eq!(
            sealed.post_seal_global_observation_sequence_v2().get(),
            source.observation_sequence().get() + 1
        );
    }

    #[test]
    fn pending_drain_seal_rejects_stale_and_foreign_s0_bindings() {
        let process_instance_id = ProcessInstanceId::parse("runtime-process:1").unwrap();
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            process_instance_id.clone(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let stale = bootstrap
            .recovery_observation_guard_unordered_v2()
            .unwrap()
            .into_empty_binding_v2()
            .unwrap();
        bootstrap.advance_empty_sequence_for_test_v2();

        assert_eq!(
            stale
                .into_pending_drain_seal_binding_v2(&candidate())
                .err()
                .unwrap(),
            RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding
        );

        let source = compose_runtime_registry_bootstrap_v1(
            process_instance_id.clone(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let foreign = compose_runtime_registry_bootstrap_v1(
            process_instance_id,
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let cursor = source
            .registry
            .recovery_observation_guard_v2()
            .unwrap()
            .into_empty_cursor()
            .unwrap();
        let foreign_binding = RuntimeRegistryEmptyRecoveryBindingV2 {
            process_instance_id: foreign.process_instance_id.clone(),
            registry: foreign.registry.clone(),
            cursor,
        };

        assert_eq!(
            foreign_binding
                .into_pending_drain_seal_binding_v2(&candidate())
                .err()
                .unwrap(),
            RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding
        );
    }

    #[test]
    fn pending_drain_seal_exposes_no_mutable_capability_and_drop_stays_sealed() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let binding = bootstrap
            .recovery_observation_guard_unordered_v2()
            .unwrap()
            .into_empty_binding_v2()
            .unwrap();
        let (sealed, _) = binding
            .into_pending_drain_seal_binding_v2(&candidate())
            .unwrap();
        let mut exposed_key = sealed.seal_key_bytes_v2();
        exposed_key[0] = 99;

        assert_eq!(exposed_key[0], 99);
        assert_eq!(sealed.seal_key_bytes_v2(), [7_u8; 16]);
        sealed.revalidate_sealed_v2().unwrap();
        drop(sealed);
        assert_eq!(
            bootstrap.observe_recovery_empty_projection_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
        );
        let observation = bootstrap
            .registry
            .atomic_observation_v2(&slot_key())
            .unwrap()
            .unwrap();
        assert!(matches!(
            observation.admission_state,
            automation_runtime_registry::SlotAdmissionStateV2::DrainClaimSealed { .. }
        ));
    }

    #[test]
    fn pending_drain_durable_token_seal_mismatch_precedes_registry_mutation() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let binding = bootstrap
            .recovery_observation_guard_unordered_v2()
            .unwrap()
            .into_empty_binding_v2()
            .unwrap();
        let (sealed, witness) = binding
            .into_pending_drain_seal_binding_v2(&candidate())
            .unwrap();
        let mismatched = automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessV2::new(
            automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessInputV2 {
                process_instance_id: witness.process_instance_id().clone(),
                slot: witness.slot().clone(),
                pre_slot_observation: witness.pre_slot_observation(),
                seal_key: [9; 16],
                seal_generation: witness.seal_generation(),
                post_slot_admission_generation: witness.post_slot_admission_generation(),
                post_slot_observation_sequence: witness.post_slot_observation_sequence(),
                pre_registry_observation_sequence: witness.pre_registry_observation_sequence(),
                pre_registry_retained_slot_count: witness.pre_registry_retained_slot_count(),
                pre_registry_retained_empty_tombstone_count: witness
                    .pre_registry_retained_empty_tombstone_count(),
                post_registry_observation: witness.post_registry_observation(),
            },
        )
        .unwrap();

        assert_eq!(
            super::require_pending_drain_durable_seal_match_v2(&witness, &mismatched),
            Err(RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation)
        );
        sealed.revalidate_sealed_v2().unwrap();
        assert_eq!(
            bootstrap.observe_recovery_empty_projection_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
        );
    }

    #[test]
    fn registry_observation_errors_are_closed_and_stable() {
        for (source, expected) in [
            (
                ServingSlotRegistryError::RegistryPoisoned,
                RuntimeRegistryRecoveryObservationErrorV1::RegistryUnavailable,
            ),
            (
                ServingSlotRegistryError::RegistryObservationInvalid,
                RuntimeRegistryRecoveryObservationErrorV1::ObservationInvalid,
            ),
            (
                ServingSlotRegistryError::RegistryObservationOverflow,
                RuntimeRegistryRecoveryObservationErrorV1::ObservationOverflow,
            ),
            (
                ServingSlotRegistryError::RegistryRecoveryNotEmpty,
                RuntimeRegistryRecoveryObservationErrorV1::NotEmpty,
            ),
            (
                ServingSlotRegistryError::StaleRegistryEmptyRecoveryCursor,
                RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
            ),
            (
                ServingSlotRegistryError::RegistrySequenceExhausted,
                RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation,
            ),
        ] {
            assert_eq!(map_registry_observation_error(source), expected);
        }
    }

    #[test]
    fn sealed_empty_slot_blocks_projection_and_unseal_preserves_tombstone_evidence() {
        let bootstrap = compose_runtime_registry_bootstrap_v1(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayResourceConfigV1::default(),
        )
        .unwrap();
        let (seal, _) = bootstrap
            .registry
            .seal_drain_claim_v2(&slot_key(), seal_key(), None)
            .unwrap();

        assert_eq!(
            bootstrap.observe_recovery_empty_projection_v2(),
            Err(RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
        );

        bootstrap.registry.unseal_drain_claim_v2(seal).unwrap();
        let projection = bootstrap.observe_recovery_empty_projection_v2().unwrap();
        assert_eq!(projection.retained_slot_count(), 1);
        assert_eq!(projection.retained_empty_tombstone_count(), 1);
        assert!(projection.observation_sequence().get() > 1);
    }

    #[test]
    fn worker_observation_errors_preserve_their_failure_class() {
        for (source, expected) in [
            (
                RuntimeRegistryRecoveryObservationErrorV2::FailedClosed,
                RuntimeRegistryRecoveryObservationErrorV1::FailedClosed,
            ),
            (
                RuntimeRegistryRecoveryObservationErrorV2::ObservationSequenceOutOfRange,
                RuntimeRegistryRecoveryObservationErrorV1::ObservationSequenceOutOfRange,
            ),
            (
                RuntimeRegistryRecoveryObservationErrorV2::NotEmpty,
                RuntimeRegistryRecoveryObservationErrorV1::NotEmpty,
            ),
            (
                RuntimeRegistryRecoveryObservationErrorV2::InconsistentRetainedCounts,
                RuntimeRegistryRecoveryObservationErrorV1::InconsistentRetainedCounts,
            ),
        ] {
            assert_eq!(map_worker_observation_error(source), expected);
        }
    }

    #[test]
    fn predecessor_absence_returns_the_same_exclusively_staged_authority() {
        fn assert_replacement<T: Send + Sync>() {}
        assert_replacement::<RuntimeRegistryReplacementRouteV2>();

        let process = "runtime-process:replacement-absent";
        let (port, registry) = replacement_registry(process);
        let route = replacement_route(process, 2, 2);
        let staged = install_replacement(&port, route.clone(), 2);

        let transition = staged.transition_predecessor_to_draining_v2(None).unwrap();

        staged.ensure_staged_v2().unwrap();
        assert_eq!(staged.identity_v2(), route.identity());
        assert!(transition.predecessor_v2().is_none());
        assert_eq!(transition.successor_v2().identity_v2(), route.identity());
        assert_eq!(transition.initial_active_interactions_v2(), 0);
        assert!(registry
            .serving_snapshot(&route.slot_key())
            .unwrap()
            .is_none());
        staged.remove_v2().unwrap();
    }

    #[test]
    fn exact_predecessor_is_drained_observed_and_removed_without_losing_staged_authority() {
        let process = "runtime-process:replacement-current";
        let (port, registry) = replacement_registry(process);
        let predecessor = replacement_route("runtime-process:predecessor", 1, 1);
        let key = predecessor.slot_key();
        let predecessor_token = registry
            .install(
                key.clone(),
                predecessor.clone(),
                FencingToken::new(1).unwrap(),
            )
            .unwrap()
            .token;
        registry
            .activate(&predecessor_token, predecessor.identity())
            .unwrap();
        let interaction = registry.admit(&key).unwrap();
        let replacement = replacement_route(process, 2, 2);
        let staged = install_replacement(&port, replacement.clone(), 2);

        let transition = staged
            .transition_predecessor_to_draining_v2(Some(predecessor.identity()))
            .unwrap();
        let transition_replay = staged
            .transition_predecessor_to_draining_v2(Some(predecessor.identity()))
            .unwrap();

        assert_eq!(staged.identity_v2(), replacement.identity());
        assert_eq!(transition, transition_replay);
        assert_eq!(
            transition.predecessor_v2().unwrap().identity,
            *predecessor.identity()
        );
        assert_eq!(transition.initial_active_interactions_v2(), 1);
        assert_eq!(
            staged.fencing_token_v2().unwrap(),
            FencingToken::new(2).unwrap()
        );
        let pending = staged.observe_predecessor_drain_v2().unwrap();
        assert_eq!(pending.active_interactions_v2(), 1);
        assert!(!pending.drained_v2());
        assert_eq!(
            staged.remove_drained_predecessor_v2(),
            Err(
                RuntimeRegistryPredecessorReplacementErrorV2::ActiveInteractionsRemain {
                    active: 1
                }
            )
        );
        assert!(registry.serving_snapshot(&key).unwrap().is_none());
        drop(interaction);
        let drained = staged.observe_predecessor_drain_v2().unwrap();
        assert_eq!(drained.active_interactions_v2(), 0);
        assert!(drained.drained_v2());
        let removal = staged.remove_drained_predecessor_v2().unwrap();
        let removal_replay = staged.remove_drained_predecessor_v2().unwrap();
        assert_eq!(removal, removal_replay);
        assert_eq!(
            removal.removed_predecessor_v2().unwrap().identity,
            *predecessor.identity()
        );
        assert_eq!(removal.successor_v2().identity_v2(), replacement.identity());
        assert_eq!(
            registry.route_witness(&predecessor_token),
            Err(ServingSlotRegistryError::StaleMutationToken)
        );
        assert!(registry.serving_snapshot(&key).unwrap().is_none());
        let evidence = staged
            .advance_authority_v2(FencingToken::new(3).unwrap())
            .unwrap();
        assert_eq!(evidence.fencing_token_v2(), FencingToken::new(3).unwrap());
        staged.ensure_staged_v2().unwrap();
        staged.remove_v2().unwrap();
    }

    #[test]
    fn predecessor_identity_mismatch_never_drains_the_fresh_serving_route() {
        let process = "runtime-process:replacement-mismatch";
        let (port, registry) = replacement_registry(process);
        let serving = replacement_route("runtime-process:fresh-serving", 1, 1);
        let key = serving.slot_key();
        let serving_token = registry
            .install(key.clone(), serving.clone(), FencingToken::new(1).unwrap())
            .unwrap()
            .token;
        registry
            .activate(&serving_token, serving.identity())
            .unwrap();
        let expected = replacement_route("runtime-process:stale-expected", 1, 1);
        let replacement = replacement_route(process, 2, 2);
        let staged = install_replacement(&port, replacement, 2);

        assert!(matches!(
            staged.transition_predecessor_to_draining_v2(Some(expected.identity())),
            Err(RuntimeRegistryPredecessorReplacementErrorV2::PredecessorIdentityMismatch)
        ));

        let snapshot = registry.serving_snapshot(&key).unwrap().unwrap();
        assert_eq!(snapshot.identity(), serving.identity());
        assert_eq!(
            registry.route_witness(&serving_token).unwrap().lifecycle,
            SlotLifecycleV1::Serving
        );
        staged.remove_v2().unwrap();
    }

    #[test]
    fn expected_absence_rejects_serving_and_draining_predecessors() {
        for drain_first in [false, true] {
            let process = if drain_first {
                "runtime-process:replacement-unexpected-draining"
            } else {
                "runtime-process:replacement-unexpected-serving"
            };
            let (port, registry) = replacement_registry(process);
            let predecessor = replacement_route("runtime-process:unexpected", 1, 1);
            let key = predecessor.slot_key();
            let predecessor_token = registry
                .install(key, predecessor.clone(), FencingToken::new(1).unwrap())
                .unwrap()
                .token;
            registry
                .activate(&predecessor_token, predecessor.identity())
                .unwrap();
            let replacement = replacement_route(process, 2, 2);
            let staged = install_replacement(&port, replacement, 2);
            if drain_first {
                let state = staged.state.lock().unwrap();
                registry
                    .begin_drain_with_authority(
                        state.staged.token.as_ref().unwrap(),
                        &predecessor_token,
                    )
                    .unwrap();
            }

            assert!(matches!(
                staged.transition_predecessor_to_draining_v2(None),
                Err(RuntimeRegistryPredecessorReplacementErrorV2::UnexpectedPredecessorPresent)
            ));

            assert_eq!(
                registry
                    .route_witness(&predecessor_token)
                    .unwrap()
                    .lifecycle,
                if drain_first {
                    SlotLifecycleV1::Draining
                } else {
                    SlotLifecycleV1::Serving
                }
            );
            staged.remove_v2().unwrap();
        }
    }

    #[test]
    fn missing_expected_predecessor_fails_closed_and_retains_staged_authority() {
        let process = "runtime-process:replacement-missing";
        let (port, registry) = replacement_registry(process);
        let expected = replacement_route("runtime-process:missing", 1, 1);
        let replacement = replacement_route(process, 2, 2);
        let key = replacement.slot_key();
        let staged = install_replacement(&port, replacement, 2);

        assert!(matches!(
            staged.transition_predecessor_to_draining_v2(Some(expected.identity())),
            Err(RuntimeRegistryPredecessorReplacementErrorV2::ExpectedPredecessorAbsent)
        ));

        let atomic = registry.atomic_observation_v2(&key).unwrap().unwrap();
        assert_eq!(atomic.admission_state, SlotAdmissionStateV2::Staged);
        staged.ensure_staged_v2().unwrap();
        staged.remove_v2().unwrap();
    }

    #[test]
    fn barrier_b_activation_keeps_exact_evidence_and_serving_authority_in_one_aggregate() {
        let process = "runtime-process:barrier-b-aggregate";
        let (port, _) = replacement_registry(process);
        let route = replacement_route(process, 1, 1);
        let replacement = install_replacement(&port, route, 1);
        replacement
            .transition_predecessor_to_draining_v2(None)
            .unwrap();
        replacement.remove_drained_predecessor_v2().unwrap();

        let activation = replacement.activate_barrier_b_v2().unwrap();

        assert_eq!(
            activation.serving_authority_v2().identity_v2(),
            activation.evidence_v2().identity_v2()
        );
        assert_eq!(
            activation.serving_authority_v2().fencing_token_v2(),
            activation.evidence_v2().fencing_token_v2()
        );
        assert_eq!(
            activation.serving_authority_v2().activation_sequence_v2(),
            activation.evidence_v2().activation_sequence_v2()
        );
        activation
            .serving_authority_v2()
            .ensure_exact_serving_v2()
            .unwrap();
        let (_, authority) = activation.into_parts_v2();
        authority.remove_exact_serving_v2().unwrap();
    }

    #[test]
    fn dropped_serving_monitor_invalidates_the_affine_completion_witness() {
        let process = "runtime-process:barrier-b-completion-loss";
        let (port, _) = replacement_registry(process);
        let route = replacement_route(process, 1, 1);
        let replacement = install_replacement(&port, route, 1);
        replacement
            .transition_predecessor_to_draining_v2(None)
            .unwrap();
        replacement.remove_drained_predecessor_v2().unwrap();
        let (_, authority) = replacement.activate_barrier_b_v2().unwrap().into_parts_v2();
        let (monitor, completion) = authority.into_serving_monitor_with_completion_v2().unwrap();

        let _guard = completion.lock_exact_serving_v2().unwrap();
        drop(_guard);
        drop(monitor);

        assert!(matches!(
            completion.lock_exact_serving_v2(),
            Err(super::RuntimeRegistryBarrierBServingErrorV2::ExactServingLost)
        ));
    }

    #[test]
    fn runtime_registry_max_slots_is_exact_and_nonzero() {
        let max_slots = super::runtime_registry_max_slots_v2();

        assert_eq!(max_slots.get(), 4_096);
        assert_ne!(max_slots.get(), 0);
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(
            RuntimeRegistryBootstrapErrorV1::ActiveInteractionCapacity.code(),
            "runtime_registry_active_interaction_capacity"
        );
        assert_eq!(
            RuntimeRegistryRecoveryObservationErrorV1::ProtocolViolation.code(),
            "runtime_registry_protocol_violation"
        );
    }

    #[test]
    fn active_interaction_capacity_conversion_is_checked_at_both_boundaries() {
        assert_eq!(
            registry_active_interaction_capacity(NonZeroUsize::new(u32::MAX as usize).unwrap())
                .unwrap()
                .get(),
            u32::MAX
        );
        if usize::BITS > u32::BITS {
            assert_eq!(
                registry_active_interaction_capacity(
                    NonZeroUsize::new(u32::MAX as usize + 1).unwrap()
                ),
                Err(RuntimeRegistryBootstrapErrorV1::ActiveInteractionCapacity)
            );
        }
    }
}
