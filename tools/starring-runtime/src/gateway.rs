use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use automation_runtime::{
    shared_gateway_control_channel_with_policy_and_invalidator_v3, GatewayAdmissionPolicyV3,
    GatewayAdmissionSnapshotV3, GatewayCommandAckV3, GatewayConnectionEpochV3,
    GatewayConnectionObserverV3, GatewayConnectionStateV3, GatewayControlConfigV3,
    GatewayControlConfigurationErrorV3, GatewayControlTransitionErrorV3, GatewayDrainCauseV3,
    GatewayInvalidationSignalV3, GatewayLifecycleEventV3, GatewayPauseTokenV3,
    GatewayPausedConnectionV3, GatewayReadyKindV3, GatewaySynchronousInvalidatorV3,
    SharedGatewayControlV3, SharedGatewayRuntimeControlV3,
};
use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayReadyAttestationV2,
    RuntimeGatewayReadyKindV2, RuntimeRecoveryIdV2,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    RuntimeAcceptedGatewayOwnerReceiptV1, RuntimeAcceptedStartupRecoveryExecutionOutcomeV2,
    RuntimeAcceptedStartupRecoveryOutcomeV2, RuntimeAuthorizedStartupRecoveryExecutionV2,
    RuntimeAuthorizedStartupRecoveryIterationV2, RuntimeAuthorizedStartupRecoveryObservationV2,
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimeCompletedStartupRecoveryExecutionV2, RuntimeCompletedStartupRecoveryObservationV2,
    RuntimeGatewayClosedLifecycleV2, RuntimeGatewayClosedSnapshotV2,
    RuntimeGatewayClosedTransitionErrorV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimeGatewayEmergencyCauseV2, RuntimeGatewayInvalidationCauseV2,
    RuntimeGatewayOwnerLeasePortV1, RuntimePausedGatewayObservationV2,
    RuntimePausedGatewaySequenceV2, RuntimeRegistryRecoveryEmptyObservationV2,
    RuntimeStartupRecoveryContinuationV2, RuntimeStartupRecoveryFixedPointProofV2,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{sleep_until, timeout, timeout_at, Instant as TokioInstant};

use crate::closed_recovery::RuntimeClosedRecoveryTransitionAuthorityV2;
#[cfg(test)]
use crate::discord::RuntimeDiscordGatewayDriverV1;
use crate::discord::{
    prepare_twilight_runtime_discord_gateway_driver_v1, start_runtime_discord_gateway_v1,
    RuntimeDiscordControlTaskV1, RuntimeDiscordGatewayActorStartV1,
    RuntimeDiscordGatewayStartErrorV1, RuntimeDiscordGatewaySupervisorV1,
    RuntimeDiscordRecoveryResumeControlOutcomeV2, RuntimeDiscordRecoveryResumeEvidenceV2,
    RuntimeDiscordReservedResumeRequestV2,
};
use crate::discord_lifecycle::{
    RuntimeDiscordAdmissionReservationSnapshotV2, RuntimeDiscordPauseReservationIdentityV2,
};
use crate::gateway_owner_startup_watchdog::{
    start_runtime_gateway_owner_startup_watchdog_v1,
    RuntimeGatewayOwnerClosedRecoveryCommitErrorV2, RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerEmergencyInvalidatorV1,
    RuntimeGatewayOwnerPreparedClosedRecoveryV2, RuntimeGatewayOwnerStartupWatchdogStartContextV1,
};
use crate::registry::RuntimeLockedRegistryEmptyEvidenceV2;
use crate::shutdown::RuntimeShutdownObserverV1;
use crate::{
    GatewayResourceConfigV1, RuntimeDiscordBotTokenV1, RuntimeGatewayOwnerStartupWatchdogConfigV1,
    RuntimeGatewayOwnerStartupWatchdogHandleV1, RuntimeGatewayOwnerStartupWatchdogStartErrorV1,
    RuntimeGatewayOwnerStartupWatchdogStartFailureV1,
};

const SUPPORTED_GATEWAY_SHARD_ID: &str = "shard:0";
const DISCORD_CONTROL_OPERATION_TIMEOUT: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum RuntimeGatewayCoordinatorInterruptV2 {
    None = 0,
    TransportDisconnected = 1,
    ControlOrphaned = 2,
    OwnershipUncertain = 3,
    CapabilityNotReady = 4,
    ProtocolViolation = 5,
    Shutdown = u8::MAX,
}

impl RuntimeGatewayCoordinatorInterruptV2 {
    fn invalidation_cause(self) -> Option<RuntimeGatewayInvalidationCauseV2> {
        match self {
            Self::None | Self::Shutdown => None,
            Self::TransportDisconnected => {
                Some(RuntimeGatewayInvalidationCauseV2::TransportDisconnected)
            }
            Self::ControlOrphaned => Some(RuntimeGatewayInvalidationCauseV2::ControlOrphaned),
            Self::OwnershipUncertain => Some(RuntimeGatewayInvalidationCauseV2::OwnershipUncertain),
            Self::CapabilityNotReady => Some(RuntimeGatewayInvalidationCauseV2::CapabilityNotReady),
            Self::ProtocolViolation => Some(RuntimeGatewayInvalidationCauseV2::ProtocolViolation),
        }
    }
}

#[derive(Clone)]
struct RuntimeGatewayCoordinatorInterruptHandleV2 {
    state: Arc<Mutex<RuntimeGatewayCoordinatorArbiterStateV2>>,
}

#[derive(Clone, Copy)]
struct RuntimeGatewayCoordinatorArbiterStateV2 {
    interrupt: RuntimeGatewayCoordinatorInterruptV2,
    generation: RuntimeGatewayCoordinatorGenerationV2,
    production_generation_active: bool,
    resume_claim: Option<RuntimeGatewayCoordinatorGenerationV2>,
}

impl RuntimeGatewayCoordinatorInterruptHandleV2 {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RuntimeGatewayCoordinatorArbiterStateV2 {
                interrupt: RuntimeGatewayCoordinatorInterruptV2::None,
                generation: RuntimeGatewayCoordinatorGenerationV2::FIRST,
                production_generation_active: false,
                resume_claim: None,
            })),
        }
    }

    fn current(&self) -> RuntimeGatewayCoordinatorInterruptV2 {
        self.lock_state_v2().interrupt
    }

    fn current_generation_v2(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.lock_state_v2().generation
    }

    fn current_state_v2(
        &self,
    ) -> (
        RuntimeGatewayCoordinatorInterruptV2,
        RuntimeGatewayCoordinatorGenerationV2,
    ) {
        let state = self.lock_state_v2();
        (state.interrupt, state.generation)
    }

    fn synchronize_generation_v2(&self, generation: RuntimeGatewayCoordinatorGenerationV2) {
        let mut state = self.lock_state_v2();
        if state.production_generation_active || state.resume_claim.is_some() {
            if state.generation != generation {
                state.interrupt = RuntimeGatewayCoordinatorInterruptV2::ProtocolViolation;
            }
            return;
        }
        if generation.get() < state.generation.get() {
            state.interrupt = RuntimeGatewayCoordinatorInterruptV2::ProtocolViolation;
            return;
        }
        state.generation = generation;
    }

    fn activate_production_generation_v2(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> bool {
        let mut state = self.lock_state_v2();
        if state.interrupt != RuntimeGatewayCoordinatorInterruptV2::None
            || state.generation != expected_generation
            || state.production_generation_active
            || state.resume_claim.is_some()
        {
            return false;
        }
        state.production_generation_active = true;
        true
    }

    fn deactivate_production_generation_v2(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) {
        let mut state = self.lock_state_v2();
        if state.generation == expected_generation {
            state.production_generation_active = false;
            state.resume_claim = None;
        }
    }

    fn production_successor_generation_v2(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Option<RuntimeGatewayCoordinatorGenerationV2> {
        let state = self.lock_state_v2();
        if state.interrupt != RuntimeGatewayCoordinatorInterruptV2::None
            || state.generation != expected_generation
            || !state.production_generation_active
        {
            return None;
        }
        runtime_gateway_successor_generation_v2(expected_generation)
    }

    fn claim_recovery_resume_v2(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> bool {
        let mut state = self.lock_state_v2();
        if state.generation != expected_generation || state.resume_claim.is_some() {
            return false;
        }
        let accepted = if state.production_generation_active {
            state.interrupt == RuntimeGatewayCoordinatorInterruptV2::None
                && runtime_gateway_successor_generation_v2(expected_generation).is_some()
        } else {
            state.interrupt != RuntimeGatewayCoordinatorInterruptV2::Shutdown
        };
        if accepted {
            state.resume_claim = Some(expected_generation);
        }
        accepted
    }

    fn complete_recovery_resume_v2(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Option<RuntimeGatewayCoordinatorGenerationV2> {
        let mut state = self.lock_state_v2();
        if state.generation != expected_generation
            || state.resume_claim != Some(expected_generation)
        {
            return None;
        }
        state.resume_claim = None;
        if !state.production_generation_active {
            return Some(expected_generation);
        }
        let successor = runtime_gateway_successor_generation_v2(expected_generation)?;
        state.generation = successor;
        Some(successor)
    }

    fn cancel_recovery_resume_claim_v2(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) {
        let mut state = self.lock_state_v2();
        if state.generation == expected_generation
            && state.resume_claim == Some(expected_generation)
        {
            state.resume_claim = None;
        }
    }

    fn trip_invalidation(&self, cause: RuntimeGatewayInvalidationCauseV2) {
        let interrupt = match cause {
            RuntimeGatewayInvalidationCauseV2::TransportDisconnected => {
                RuntimeGatewayCoordinatorInterruptV2::TransportDisconnected
            }
            RuntimeGatewayInvalidationCauseV2::ControlOrphaned => {
                RuntimeGatewayCoordinatorInterruptV2::ControlOrphaned
            }
            RuntimeGatewayInvalidationCauseV2::OwnershipUncertain => {
                RuntimeGatewayCoordinatorInterruptV2::OwnershipUncertain
            }
            RuntimeGatewayInvalidationCauseV2::CapabilityNotReady => {
                RuntimeGatewayCoordinatorInterruptV2::CapabilityNotReady
            }
            RuntimeGatewayInvalidationCauseV2::ProtocolViolation => {
                RuntimeGatewayCoordinatorInterruptV2::ProtocolViolation
            }
        };
        let mut state = self.lock_state_v2();
        if state.interrupt == RuntimeGatewayCoordinatorInterruptV2::None {
            state.interrupt = interrupt;
        }
    }

    fn trip_shutdown(&self) {
        self.lock_state_v2().interrupt = RuntimeGatewayCoordinatorInterruptV2::Shutdown;
    }

    fn lock_state_v2(&self) -> std::sync::MutexGuard<'_, RuntimeGatewayCoordinatorArbiterStateV2> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Clone)]
struct RuntimeGatewayCoordinatorSnapshotObserverV2 {
    snapshot: watch::Receiver<RuntimeGatewayCoordinatorMirrorV2>,
    interrupt: RuntimeGatewayCoordinatorInterruptHandleV2,
}

impl RuntimeGatewayCoordinatorSnapshotObserverV2 {
    fn effective_snapshot(&self) -> RuntimeGatewayClosedSnapshotV2 {
        let mirror = self.snapshot.borrow();
        let (interrupt, generation) = self.interrupt.current_state_v2();
        if interrupt == mirror.applied_interrupt {
            mirror.snapshot.clone()
        } else {
            project_runtime_gateway_interrupt_v2(&mirror.snapshot, interrupt, generation)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeGatewayCoordinatorMirrorV2 {
    snapshot: RuntimeGatewayClosedSnapshotV2,
    applied_interrupt: RuntimeGatewayCoordinatorInterruptV2,
}

struct RuntimeGatewayCoordinatorOwnerV2 {
    lifecycle: RuntimeGatewayClosedLifecycleV2,
    interrupt: RuntimeGatewayCoordinatorInterruptHandleV2,
    applied_interrupt: RuntimeGatewayCoordinatorInterruptV2,
    snapshot: watch::Sender<RuntimeGatewayCoordinatorMirrorV2>,
}

impl RuntimeGatewayCoordinatorOwnerV2 {
    fn new() -> (
        Self,
        RuntimeGatewayCoordinatorInterruptHandleV2,
        RuntimeGatewayCoordinatorSnapshotObserverV2,
    ) {
        let lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
        let interrupt = RuntimeGatewayCoordinatorInterruptHandleV2::new();
        let initial = RuntimeGatewayCoordinatorMirrorV2 {
            snapshot: lifecycle.snapshot(),
            applied_interrupt: RuntimeGatewayCoordinatorInterruptV2::None,
        };
        let (snapshot, observer) = watch::channel(initial);
        (
            Self {
                lifecycle,
                interrupt: interrupt.clone(),
                applied_interrupt: RuntimeGatewayCoordinatorInterruptV2::None,
                snapshot,
            },
            interrupt.clone(),
            RuntimeGatewayCoordinatorSnapshotObserverV2 {
                snapshot: observer,
                interrupt,
            },
        )
    }

    fn lifecycle(&self) -> &RuntimeGatewayClosedLifecycleV2 {
        &self.lifecycle
    }

    fn lifecycle_mut(
        &mut self,
    ) -> Result<&mut RuntimeGatewayClosedLifecycleV2, RuntimeGatewayReadyObservationErrorV1> {
        self.reconcile_interrupt();
        if self.applied_interrupt != RuntimeGatewayCoordinatorInterruptV2::None {
            return Err(RuntimeGatewayReadyObservationErrorV1::Stopped);
        }
        Ok(&mut self.lifecycle)
    }

    fn require_uninterrupted(&self) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        if self.interrupt.current() == RuntimeGatewayCoordinatorInterruptV2::None
            && self.applied_interrupt == RuntimeGatewayCoordinatorInterruptV2::None
        {
            Ok(())
        } else {
            Err(RuntimeGatewayReadyObservationErrorV1::Stopped)
        }
    }

    fn reconcile_interrupt(&mut self) {
        let interrupt = self.interrupt.current();
        if interrupt == self.applied_interrupt {
            return;
        }
        match interrupt {
            RuntimeGatewayCoordinatorInterruptV2::None => {}
            RuntimeGatewayCoordinatorInterruptV2::Shutdown => {
                shutdown_closed_lifecycle(&mut self.lifecycle);
            }
            interrupt => {
                if let Some(cause) = interrupt.invalidation_cause() {
                    invalidate_closed_lifecycle(&mut self.lifecycle, cause);
                }
            }
        }
        self.applied_interrupt = interrupt;
        self.publish_snapshot();
    }

    fn publish_snapshot(&self) {
        let lifecycle_snapshot = self.lifecycle.snapshot();
        self.interrupt
            .synchronize_generation_v2(lifecycle_snapshot.generation());
        self.snapshot
            .send_replace(RuntimeGatewayCoordinatorMirrorV2 {
                snapshot: lifecycle_snapshot,
                applied_interrupt: self.applied_interrupt,
            });
    }
}

fn project_runtime_gateway_interrupt_v2(
    snapshot: &RuntimeGatewayClosedSnapshotV2,
    interrupt: RuntimeGatewayCoordinatorInterruptV2,
    current: RuntimeGatewayCoordinatorGenerationV2,
) -> RuntimeGatewayClosedSnapshotV2 {
    if matches!(snapshot, RuntimeGatewayClosedSnapshotV2::Shutdown { .. }) {
        return snapshot.clone();
    }
    let successor = runtime_gateway_successor_generation_v2(current);
    match (interrupt, successor) {
        (RuntimeGatewayCoordinatorInterruptV2::None, _) => snapshot.clone(),
        (RuntimeGatewayCoordinatorInterruptV2::Shutdown, Some(generation)) => {
            RuntimeGatewayClosedSnapshotV2::Shutdown { generation }
        }
        (RuntimeGatewayCoordinatorInterruptV2::Shutdown, None) | (_, None) => {
            RuntimeGatewayClosedSnapshotV2::Shutdown {
                generation: current,
            }
        }
        (interrupt, Some(generation)) => RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: interrupt
                .invalidation_cause()
                .map(Into::into)
                .unwrap_or(RuntimeGatewayEmergencyCauseV2::ProtocolViolation),
        },
    }
}

fn runtime_gateway_successor_generation_v2(
    current: RuntimeGatewayCoordinatorGenerationV2,
) -> Option<RuntimeGatewayCoordinatorGenerationV2> {
    current
        .get()
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .and_then(NonZeroU64::new)
        .map(RuntimeGatewayCoordinatorGenerationV2::new)
}

pub(crate) fn runtime_gateway_shard_id_v1() -> GatewayShardIdV1 {
    GatewayShardIdV1::parse(SUPPORTED_GATEWAY_SHARD_ID).expect("supported gateway shard identity")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayBootstrapErrorV1 {
    #[error("runtime gateway command capacity is invalid")]
    CommandCapacity,
    #[error("runtime gateway lifecycle capacity is invalid")]
    LifecycleCapacity,
}

impl RuntimeGatewayBootstrapErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::CommandCapacity => "runtime_gateway_command_capacity",
            Self::LifecycleCapacity => "runtime_gateway_lifecycle_capacity",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayReadyObservationErrorV1 {
    #[error("runtime gateway connection epoch is stale")]
    StaleConnectionEpoch,
    #[error("runtime gateway admission snapshot is stale")]
    StaleAdmissionSnapshot,
    #[error("runtime gateway control owner is unavailable")]
    ControlOrphaned,
    #[error("runtime gateway is not connected")]
    NotConnected,
    #[error("runtime gateway admission is paused")]
    AdmissionPaused,
    #[error("runtime gateway admission is not paused")]
    AdmissionNotPaused,
    #[error("runtime gateway is draining")]
    Draining,
    #[error("runtime gateway is stopped")]
    Stopped,
    #[error("runtime gateway connection epoch overflowed")]
    ConnectionEpochOverflow,
    #[error("runtime gateway admission revision overflowed")]
    AdmissionRevisionOverflow,
    #[error("runtime gateway admission sequence overflowed")]
    AdmissionSequenceOverflow,
    #[error("runtime gateway lifecycle queue overflowed")]
    LifecycleOverflow,
    #[error("runtime gateway lifecycle observer is unavailable")]
    LifecycleClosed,
    #[error("runtime gateway ready evidence is no longer current")]
    ReadyEvidenceNotCurrent,
    #[error("runtime gateway ready evidence contains a zero sequence")]
    ReadyEvidenceSequenceZero,
    #[error("runtime gateway ready evidence exceeds the persistence domain")]
    ReadyEvidenceOutOfRange,
    #[error("runtime gateway ready evidence lacks an explicit resume")]
    ReadyEvidenceNotExplicitlyResumed,
    #[error("runtime gateway ownership is uncertain")]
    OwnershipUncertain,
}

impl RuntimeGatewayReadyObservationErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::StaleConnectionEpoch => "runtime_gateway_stale_connection_epoch",
            Self::StaleAdmissionSnapshot => "runtime_gateway_stale_admission_snapshot",
            Self::ControlOrphaned => "runtime_gateway_control_orphaned",
            Self::NotConnected => "runtime_gateway_not_connected",
            Self::AdmissionPaused => "runtime_gateway_admission_paused",
            Self::AdmissionNotPaused => "runtime_gateway_admission_not_paused",
            Self::Draining => "runtime_gateway_draining",
            Self::Stopped => "runtime_gateway_stopped",
            Self::ConnectionEpochOverflow => "runtime_gateway_connection_epoch_overflow",
            Self::AdmissionRevisionOverflow => "runtime_gateway_admission_revision_overflow",
            Self::AdmissionSequenceOverflow => "runtime_gateway_admission_sequence_overflow",
            Self::LifecycleOverflow => "runtime_gateway_lifecycle_overflow",
            Self::LifecycleClosed => "runtime_gateway_lifecycle_closed",
            Self::ReadyEvidenceNotCurrent => "runtime_gateway_ready_evidence_not_current",
            Self::ReadyEvidenceSequenceZero => "runtime_gateway_ready_evidence_sequence_zero",
            Self::ReadyEvidenceOutOfRange => "runtime_gateway_ready_evidence_out_of_range",
            Self::ReadyEvidenceNotExplicitlyResumed => {
                "runtime_gateway_ready_evidence_not_explicitly_resumed"
            }
            Self::OwnershipUncertain => "runtime_gateway_ownership_uncertain",
        }
    }
}

struct SharedGatewayControlAdapterV2 {
    process_instance_id: ProcessInstanceId,
    control: Option<SharedGatewayControlV3>,
    connection_observer: GatewayConnectionObserverV3,
    discord_commands: Option<mpsc::Sender<RuntimeDiscordControlCommandV1>>,
    admission_snapshot: watch::Receiver<GatewayAdmissionSnapshotV3>,
    discord_reservation_publisher:
        Option<watch::Sender<RuntimeDiscordAdmissionReservationSnapshotV2>>,
    discord_reservation: watch::Receiver<RuntimeDiscordAdmissionReservationSnapshotV2>,
}

struct SharedGatewayRuntimeHalfV3 {
    _inner: SharedGatewayRuntimeControlV3,
}

struct RuntimePreparedDiscordGatewayStartV1 {
    runtime_handle: tokio::runtime::Handle,
    runtime: SharedGatewayRuntimeHalfV3,
    control_task: RuntimeDiscordControlTaskV1,
    lifecycle_drained: watch::Receiver<u64>,
    discord_reservation: watch::Receiver<RuntimeDiscordAdmissionReservationSnapshotV2>,
    stopped_sender: watch::Sender<bool>,
    stopped: watch::Receiver<bool>,
}

enum RuntimeDiscordControlCommandV1 {
    BeginDrain {
        response: oneshot::Sender<bool>,
    },
    #[cfg(test)]
    OpenAdmission {
        response: oneshot::Sender<bool>,
    },
}

pub struct RuntimeGatewayBootstrapV1 {
    adapter: SharedGatewayControlAdapterV2,
    coordinator: Option<RuntimeGatewayCoordinatorOwnerV2>,
    coordinator_snapshot: RuntimeGatewayCoordinatorSnapshotObserverV2,
    _runtime: Option<SharedGatewayRuntimeHalfV3>,
    owner_invalidator: Option<RuntimeGatewayOwnerInvalidationBridgeV2>,
    owner_invalidated: Arc<AtomicBool>,
    owner_discord_attachment: Arc<Mutex<Option<RuntimeGatewayOwnerDiscordAttachmentV1>>>,
}

#[derive(Clone)]
pub(crate) struct RuntimeGatewayShutdownHandleV1 {
    interrupt: RuntimeGatewayCoordinatorInterruptHandleV2,
    snapshot: RuntimeGatewayCoordinatorSnapshotObserverV2,
}

impl RuntimeGatewayShutdownHandleV1 {
    pub(crate) fn enter_shutdown(&self) -> RuntimeGatewayClosedSnapshotV2 {
        self.interrupt.trip_shutdown();
        self.snapshot.effective_snapshot()
    }
}

impl Debug for RuntimeGatewayShutdownHandleV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayShutdownHandleV1(<redacted>)")
    }
}

pub(crate) struct RuntimeGatewayAdmissionChangeWatchV1 {
    inner: watch::Receiver<GatewayAdmissionSnapshotV3>,
}

impl RuntimeGatewayAdmissionChangeWatchV1 {
    pub(crate) async fn changed(&mut self) -> bool {
        self.inner.changed().await.is_ok()
    }
}

pub(crate) struct RuntimeEmergencyGatewaySectionV2<'a> {
    gateway: &'a SharedGatewayControlAdapterV2,
    coordinator: &'a mut Option<RuntimeGatewayCoordinatorOwnerV2>,
    prepared_owner: &'a RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    owner_invalidated: &'a Arc<AtomicBool>,
    admission_snapshot: watch::Ref<'a, GatewayAdmissionSnapshotV3>,
    paused_gateway: RuntimePausedGatewayObservationV2,
    connection_epoch: GatewayConnectionEpochV3,
    pending_permit: Option<RuntimeClosedDrainRecoveryPermitV2>,
}

pub(crate) struct RuntimeRecoveryPendingGatewayBindingV2 {
    process_instance_id: ProcessInstanceId,
    observer: GatewayConnectionObserverV3,
    admission_snapshot: watch::Receiver<GatewayAdmissionSnapshotV3>,
    discord_reservation: watch::Receiver<RuntimeDiscordAdmissionReservationSnapshotV2>,
    coordinator: Option<RuntimeGatewayCoordinatorOwnerV2>,
    owner_invalidated: Arc<AtomicBool>,
    permit: Option<RuntimeClosedDrainRecoveryPermitV2>,
}

pub(crate) struct RuntimeRecoveryPendingGatewaySectionV2<'a> {
    binding: &'a RuntimeRecoveryPendingGatewayBindingV2,
    owner: RuntimeGatewayOwnerRecoveryEvidenceV2<'a>,
    admission_snapshot: watch::Ref<'a, GatewayAdmissionSnapshotV3>,
}

enum RuntimeGatewayOwnerRecoveryEvidenceV2<'a> {
    Prepared(&'a RuntimeGatewayOwnerPreparedClosedRecoveryV2),
    Committed(&'a RuntimeGatewayOwnerClosedRecoverySupervisorV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeGatewayRecoverySectionErrorV2 {
    #[error("runtime gateway recovery section observation failed")]
    Gateway(RuntimeGatewayReadyObservationErrorV1),
    #[error("runtime gateway recovery coordinator transition failed")]
    Coordinator(RuntimeGatewayClosedTransitionErrorV2),
    #[error("runtime gateway recovery section protocol was violated")]
    ProtocolViolation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeGatewayRecoveryOwnerCommitErrorV2 {
    #[error("runtime gateway recovery owner commit deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime gateway recovery owner commit precondition failed")]
    Section(RuntimeGatewayRecoverySectionErrorV2),
    #[error("runtime gateway recovery owner commit failed")]
    Owner(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeGatewayProductionInterruptV2 {
    Invalidation(RuntimeGatewayInvalidationCauseV2),
    Shutdown,
}

pub(crate) struct RuntimeGatewayProductionCoordinatorV2 {
    process_instance_id: ProcessInstanceId,
    observer: GatewayConnectionObserverV3,
    admission_snapshot: watch::Receiver<GatewayAdmissionSnapshotV3>,
    discord_reservation: watch::Receiver<RuntimeDiscordAdmissionReservationSnapshotV2>,
    fixed_point_admission_snapshot: GatewayAdmissionSnapshotV3,
    interrupt: RuntimeGatewayCoordinatorInterruptHandleV2,
    applied_interrupt: RuntimeGatewayCoordinatorInterruptV2,
    snapshot: watch::Sender<RuntimeGatewayCoordinatorMirrorV2>,
}

impl RuntimeGatewayProductionCoordinatorV2 {
    pub(crate) fn coordinator_generation_v2(&self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.interrupt.current_generation_v2()
    }

    pub(crate) fn recovery_resume_successor_generation_v2(
        &self,
        expected_predecessor: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Result<RuntimeGatewayCoordinatorGenerationV2, RuntimeGatewayReadyObservationErrorV1> {
        self.require_resume_observation_generation_v2(expected_predecessor)?;
        self.interrupt
            .production_successor_generation_v2(expected_predecessor)
            .ok_or(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceOutOfRange)
    }

    pub(crate) fn current_interrupt_v2(&self) -> Option<RuntimeGatewayProductionInterruptV2> {
        let interrupt = self.interrupt.current();
        if interrupt == self.applied_interrupt
            || interrupt == RuntimeGatewayCoordinatorInterruptV2::None
        {
            return None;
        }
        match interrupt {
            RuntimeGatewayCoordinatorInterruptV2::Shutdown => {
                Some(RuntimeGatewayProductionInterruptV2::Shutdown)
            }
            interrupt => interrupt
                .invalidation_cause()
                .map(RuntimeGatewayProductionInterruptV2::Invalidation),
        }
    }

    pub(crate) fn closed_snapshot_v2(&self) -> RuntimeGatewayClosedSnapshotV2 {
        let mirror = self.snapshot.borrow();
        let (interrupt, generation) = self.interrupt.current_state_v2();
        project_runtime_gateway_interrupt_v2(&mirror.snapshot, interrupt, generation)
    }

    pub(crate) fn observe_exact_pause_reservation_v2(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Result<RuntimeDiscordPauseReservationIdentityV2, RuntimeGatewayReadyObservationErrorV1>
    {
        self.require_resume_observation_generation_v2(expected_generation)?;
        let first_admission = self.observer.current_admission_snapshot();
        let first_watch = *self.admission_snapshot.borrow();
        let first_reservation = *self.discord_reservation.borrow();
        if first_admission != first_watch || first_reservation.admission() != first_admission {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        let identity = first_reservation
            .reservation()
            .ok_or(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot)?;
        if identity.epoch()
            != first_admission
                .connection()
                .current_epoch()
                .ok_or(RuntimeGatewayReadyObservationErrorV1::NotConnected)?
            || identity.admission_revision() != first_admission.admission_revision()
            || identity.transition_sequence() != first_admission.transition_sequence()
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        let (_, epoch) = map_paused_connected_observation_v2(
            &self.process_instance_id,
            expected_generation,
            first_admission,
        )?;
        require_healthy_paused_observer_v2(&self.observer, epoch)?;
        self.require_resume_observation_generation_v2(expected_generation)?;
        let second_admission = self.observer.current_admission_snapshot();
        let second_watch = *self.admission_snapshot.borrow();
        let second_reservation = *self.discord_reservation.borrow();
        if first_admission != second_admission
            || first_watch != second_watch
            || first_reservation != second_reservation
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        self.require_resume_observation_generation_v2(expected_generation)?;
        Ok(identity)
    }

    pub(crate) fn observe_exact_current_ready_attestation_v2(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Result<RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyObservationErrorV1> {
        self.require_resume_observation_generation_v2(expected_generation)?;
        let first_admission = self.observer.current_admission_snapshot();
        let first_watch = *self.admission_snapshot.borrow();
        let first_reservation = *self.discord_reservation.borrow();
        if first_admission != first_watch
            || first_reservation.admission() != first_admission
            || first_reservation.reservation().is_some()
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        let attestation =
            observe_current_ready_attestation_v2(&self.process_instance_id, &self.observer)?;
        self.require_resume_observation_generation_v2(expected_generation)?;
        let second_admission = self.observer.current_admission_snapshot();
        let second_watch = *self.admission_snapshot.borrow();
        let second_reservation = *self.discord_reservation.borrow();
        if first_admission != second_admission
            || first_watch != second_watch
            || first_reservation != second_reservation
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        if attestation.connection_epoch.get()
            != first_admission
                .connection()
                .current_epoch()
                .ok_or(RuntimeGatewayReadyObservationErrorV1::NotConnected)?
                .get()
            || attestation.admission_revision.get() != first_admission.admission_revision().get()
            || attestation.connected_event_sequence.get()
                != first_admission
                    .connected_event_sequence()
                    .ok_or(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceSequenceZero)?
                    .get()
            || attestation.resume_sequence.get()
                != first_admission
                    .resume_sequence()
                    .ok_or(
                        RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotExplicitlyResumed,
                    )?
                    .get()
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotCurrent);
        }
        self.require_resume_observation_generation_v2(expected_generation)?;
        Ok(attestation)
    }

    fn require_resume_observation_generation_v2(
        &self,
        expected_generation: RuntimeGatewayCoordinatorGenerationV2,
    ) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        if self.coordinator_generation_v2() != expected_generation {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        match (self.applied_interrupt, self.interrupt.current()) {
            (
                RuntimeGatewayCoordinatorInterruptV2::None,
                RuntimeGatewayCoordinatorInterruptV2::None,
            ) => {}
            (_, RuntimeGatewayCoordinatorInterruptV2::Shutdown) => {
                return Err(RuntimeGatewayReadyObservationErrorV1::Stopped);
            }
            _ => return Err(RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain),
        }
        if matches!(
            self.closed_snapshot_v2(),
            RuntimeGatewayClosedSnapshotV2::Emergency { .. }
                | RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ) {
            return Err(RuntimeGatewayReadyObservationErrorV1::Stopped);
        }
        Ok(())
    }

    pub(crate) fn observe_exact_admission_snapshot_v2(
        &self,
    ) -> Result<GatewayAdmissionSnapshotV3, RuntimeGatewayReadyObservationErrorV1> {
        let first = self.observer.current_admission_snapshot();
        if *self.admission_snapshot.borrow() != first {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        let (_, connection_epoch) = map_paused_connected_observation_v2(
            &self.process_instance_id,
            self.coordinator_generation_v2(),
            first,
        )?;
        require_healthy_paused_observer_v2(&self.observer, connection_epoch)?;
        if self.observer.current_admission_snapshot() != first
            || *self.admission_snapshot.borrow() != first
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        Ok(first)
    }

    pub(crate) fn revalidate_fixed_point_admission_v2(
        &self,
    ) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        if self.observe_exact_admission_snapshot_v2()? == self.fixed_point_admission_snapshot {
            Ok(())
        } else {
            Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot)
        }
    }
}

impl Debug for RuntimeGatewayProductionCoordinatorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayProductionCoordinatorV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeGatewayFixedPointAcceptanceErrorV2 {
    Gateway(RuntimeGatewayRecoverySectionErrorV2),
    Worker(automation_runtime_worker::RuntimeProductionLifecycleErrorV2),
}

pub(crate) struct RuntimeGatewayFixedPointAcceptanceFailureV2 {
    binding: Box<RuntimeRecoveryPendingGatewayBindingV2>,
    proof: Box<RuntimeStartupRecoveryFixedPointProofV2>,
    error: RuntimeGatewayFixedPointAcceptanceErrorV2,
}

impl RuntimeGatewayFixedPointAcceptanceFailureV2 {
    pub(crate) fn into_parts(
        self,
    ) -> (
        RuntimeRecoveryPendingGatewayBindingV2,
        RuntimeStartupRecoveryFixedPointProofV2,
        RuntimeGatewayFixedPointAcceptanceErrorV2,
    ) {
        (*self.binding, *self.proof, self.error)
    }
}

impl Debug for RuntimeGatewayFixedPointAcceptanceFailureV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeGatewayFixedPointAcceptanceFailureV2(<redacted>)")
    }
}

struct RuntimeGatewayInvalidationBridgeV2 {
    interrupt: RuntimeGatewayCoordinatorInterruptHandleV2,
}

struct RuntimeGatewayOwnerInvalidationBridgeV2 {
    interrupt: RuntimeGatewayCoordinatorInterruptHandleV2,
    invalidated: Arc<AtomicBool>,
    discord_attachment: Arc<Mutex<Option<RuntimeGatewayOwnerDiscordAttachmentV1>>>,
}

struct RuntimeGatewayOwnerDiscordAttachmentV1 {
    discord_abort_handle: Option<tokio::task::AbortHandle>,
    control_abort_handle: Option<tokio::task::AbortHandle>,
    stopped: watch::Receiver<bool>,
}

#[cfg(test)]
struct RuntimeGatewaySnapshotTestInvalidatorV3;

#[cfg(test)]
impl GatewaySynchronousInvalidatorV3 for RuntimeGatewaySnapshotTestInvalidatorV3 {
    fn invalidate(&self, _signal: GatewayInvalidationSignalV3) {}
}

impl RuntimeGatewayOwnerEmergencyInvalidatorV1 for RuntimeGatewayOwnerInvalidationBridgeV2 {
    fn invalidate_gateway_ownership(&self) {
        invalidate_gateway_owner_state(&self.interrupt, &self.invalidated);
        let attachment = self
            .discord_attachment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(attachment) = attachment.as_ref() {
            if let Some(abort_handle) = attachment.discord_abort_handle.as_ref() {
                abort_handle.abort();
            }
            if let Some(abort_handle) = attachment.control_abort_handle.as_ref() {
                abort_handle.abort();
            }
        }
    }

    fn gateway_shutdown_watch(&self) -> Option<watch::Receiver<bool>> {
        self.discord_attachment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|attachment| attachment.stopped.clone())
    }
}

impl GatewaySynchronousInvalidatorV3 for RuntimeGatewayInvalidationBridgeV2 {
    fn invalidate(&self, signal: GatewayInvalidationSignalV3) {
        match signal {
            GatewayInvalidationSignalV3::AdmissionPaused => self
                .interrupt
                .trip_invalidation(RuntimeGatewayInvalidationCauseV2::CapabilityNotReady),
            GatewayInvalidationSignalV3::Disconnected(_) => self
                .interrupt
                .trip_invalidation(RuntimeGatewayInvalidationCauseV2::TransportDisconnected),
            GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::Commanded)
            | GatewayInvalidationSignalV3::Stopped(_) => self.interrupt.trip_shutdown(),
            GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::ControlOrphaned | GatewayDrainCauseV3::LifecycleClosed,
            )
            | GatewayInvalidationSignalV3::ControlOrphaned => self
                .interrupt
                .trip_invalidation(RuntimeGatewayInvalidationCauseV2::ControlOrphaned),
            GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::ConnectionEpochOverflow
                | GatewayDrainCauseV3::AdmissionRevisionOverflow
                | GatewayDrainCauseV3::AdmissionSequenceOverflow,
            ) => self
                .interrupt
                .trip_invalidation(RuntimeGatewayInvalidationCauseV2::ProtocolViolation),
            GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::LifecycleOverflow | GatewayDrainCauseV3::RuntimeFailure,
            ) => self
                .interrupt
                .trip_invalidation(RuntimeGatewayInvalidationCauseV2::CapabilityNotReady),
        }
    }
}

impl RuntimeGatewayBootstrapV1 {
    pub(crate) fn shutdown_handle_v1(&self) -> RuntimeGatewayShutdownHandleV1 {
        RuntimeGatewayShutdownHandleV1 {
            interrupt: self.coordinator_snapshot.interrupt.clone(),
            snapshot: self.coordinator_snapshot.clone(),
        }
    }

    pub(crate) fn admission_change_watch_v1(&self) -> RuntimeGatewayAdmissionChangeWatchV1 {
        RuntimeGatewayAdmissionChangeWatchV1 {
            inner: self.adapter.admission_snapshot.clone(),
        }
    }

    pub(crate) async fn start_discord_gateway_v1(
        &mut self,
        token: &RuntimeDiscordBotTokenV1,
        operation_cutoff: Instant,
        shutdown_deadline: Instant,
        shutdown: &mut RuntimeShutdownObserverV1,
    ) -> Result<RuntimeDiscordGatewaySupervisorV1, RuntimeDiscordGatewayStartErrorV1> {
        let driver =
            prepare_twilight_runtime_discord_gateway_driver_v1(token.expose_secret().to_owned());
        let prepared = self
            .prepare_discord_gateway_start_v1(operation_cutoff, Some(shutdown))
            .await?;
        if shutdown.observed().is_some() {
            let _stopped = prepared.stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed);
        }
        let mut supervisor = start_runtime_discord_gateway_v1(
            driver,
            RuntimeDiscordGatewayActorStartV1 {
                control: prepared.runtime._inner,
                operation_cutoff,
                shutdown_deadline,
                lifecycle_drained: prepared.lifecycle_drained,
                discord_reservation: prepared.discord_reservation,
                runtime: prepared.runtime_handle,
                control_task: prepared.control_task,
                stopped_sender: prepared.stopped_sender,
                stopped: prepared.stopped,
            },
        );
        self.attach_discord_supervisor_v1(&supervisor)?;
        if shutdown.observed().is_some() {
            return Err(RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed);
        }
        if self.owner_invalidated.load(Ordering::Acquire) {
            return Err(RuntimeDiscordGatewayStartErrorV1::OwnerInvalidated);
        }
        if !supervisor.release_start_v1() {
            return Err(RuntimeDiscordGatewayStartErrorV1::RuntimeUnavailable);
        }
        Ok(supervisor)
    }

    #[cfg(test)]
    pub(crate) async fn start_discord_gateway_with_driver_v1<D>(
        &mut self,
        driver: D,
        operation_cutoff: Instant,
        shutdown_deadline: Instant,
    ) -> Result<RuntimeDiscordGatewaySupervisorV1, RuntimeDiscordGatewayStartErrorV1>
    where
        D: RuntimeDiscordGatewayDriverV1,
    {
        self.start_discord_gateway_with_driver_before_release_v1(
            driver,
            operation_cutoff,
            shutdown_deadline,
            |_| {},
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn start_discord_gateway_with_driver_before_release_v1<D, F>(
        &mut self,
        driver: D,
        operation_cutoff: Instant,
        shutdown_deadline: Instant,
        before_release: F,
    ) -> Result<RuntimeDiscordGatewaySupervisorV1, RuntimeDiscordGatewayStartErrorV1>
    where
        D: RuntimeDiscordGatewayDriverV1,
        F: FnOnce(&Self),
    {
        let prepared = self
            .prepare_discord_gateway_start_v1(operation_cutoff, None)
            .await?;
        let mut supervisor = start_runtime_discord_gateway_v1(
            driver,
            RuntimeDiscordGatewayActorStartV1 {
                control: prepared.runtime._inner,
                operation_cutoff,
                shutdown_deadline,
                lifecycle_drained: prepared.lifecycle_drained,
                discord_reservation: prepared.discord_reservation,
                runtime: prepared.runtime_handle,
                control_task: prepared.control_task,
                stopped_sender: prepared.stopped_sender,
                stopped: prepared.stopped,
            },
        );
        self.attach_discord_supervisor_v1(&supervisor)?;
        before_release(self);
        if self.owner_invalidated.load(Ordering::Acquire) {
            return Err(RuntimeDiscordGatewayStartErrorV1::OwnerInvalidated);
        }
        if !supervisor.release_start_v1() {
            return Err(RuntimeDiscordGatewayStartErrorV1::RuntimeUnavailable);
        }
        Ok(supervisor)
    }

    #[cfg(test)]
    pub(crate) fn invalidate_owner_for_discord_test_v1(&self) {
        self.owner_invalidator
            .as_ref()
            .expect("gateway owner invalidator")
            .invalidate_gateway_ownership();
    }

    pub(crate) fn begin_discord_drain_v1(
        &self,
    ) -> impl std::future::Future<Output = bool> + Send + 'static {
        let commands = self.adapter.discord_commands.clone();
        async move {
            let Some(commands) = commands else {
                return false;
            };
            let (response, acknowledgement) = oneshot::channel();
            if commands
                .send(RuntimeDiscordControlCommandV1::BeginDrain { response })
                .await
                .is_err()
            {
                return false;
            }
            acknowledgement.await.unwrap_or(false)
        }
    }

    #[cfg(test)]
    pub(crate) async fn open_discord_admission_for_test_v1(&self) -> bool {
        let Some(commands) = self.adapter.discord_commands.as_ref() else {
            return false;
        };
        let (response, acknowledgement) = oneshot::channel();
        if commands
            .send(RuntimeDiscordControlCommandV1::OpenAdmission { response })
            .await
            .is_err()
        {
            return false;
        }
        acknowledgement.await.unwrap_or(false)
    }

    #[cfg(test)]
    pub(crate) fn discord_pause_reservation_for_test_v2(
        &self,
    ) -> Option<RuntimeDiscordPauseReservationIdentityV2> {
        self.adapter.discord_reservation.borrow().reservation()
    }

    fn attach_discord_supervisor_v1(
        &self,
        supervisor: &RuntimeDiscordGatewaySupervisorV1,
    ) -> Result<(), RuntimeDiscordGatewayStartErrorV1> {
        let Some((discord_abort_handle, control_abort_handle)) = supervisor.abort_handles() else {
            return Err(RuntimeDiscordGatewayStartErrorV1::RuntimeUnavailable);
        };
        let mut attachment = self
            .owner_discord_attachment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(attachment) = attachment.as_mut() else {
            discord_abort_handle.abort();
            control_abort_handle.abort();
            return Err(RuntimeDiscordGatewayStartErrorV1::RuntimeHalfUnavailable);
        };
        if attachment.discord_abort_handle.is_some() || attachment.control_abort_handle.is_some() {
            discord_abort_handle.abort();
            control_abort_handle.abort();
            return Err(RuntimeDiscordGatewayStartErrorV1::RuntimeHalfUnavailable);
        }
        attachment.discord_abort_handle = Some(discord_abort_handle.clone());
        attachment.control_abort_handle = Some(control_abort_handle.clone());
        if self.owner_invalidated.load(Ordering::Acquire) {
            discord_abort_handle.abort();
            control_abort_handle.abort();
        }
        Ok(())
    }

    fn reserve_discord_supervisor_v1(
        &self,
        stopped: watch::Receiver<bool>,
    ) -> Result<(), RuntimeDiscordGatewayStartErrorV1> {
        let mut attachment = self
            .owner_discord_attachment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if attachment.is_some() {
            return Err(RuntimeDiscordGatewayStartErrorV1::RuntimeHalfUnavailable);
        }
        *attachment = Some(RuntimeGatewayOwnerDiscordAttachmentV1 {
            discord_abort_handle: None,
            control_abort_handle: None,
            stopped,
        });
        Ok(())
    }

    async fn prepare_discord_gateway_start_v1(
        &mut self,
        operation_cutoff: Instant,
        mut shutdown: Option<&mut RuntimeShutdownObserverV1>,
    ) -> Result<RuntimePreparedDiscordGatewayStartV1, RuntimeDiscordGatewayStartErrorV1> {
        if shutdown
            .as_deref()
            .and_then(RuntimeShutdownObserverV1::observed)
            .is_some()
        {
            return Err(RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed);
        }
        if self.owner_invalidated.load(Ordering::Acquire) {
            return Err(RuntimeDiscordGatewayStartErrorV1::OwnerInvalidated);
        }
        let runtime_handle = tokio::runtime::Handle::try_current()
            .map_err(|_| RuntimeDiscordGatewayStartErrorV1::RuntimeUnavailable)?;
        let (stopped_sender, stopped) = watch::channel(false);
        self.reserve_discord_supervisor_v1(stopped.clone())?;
        if self.owner_invalidated.load(Ordering::Acquire) {
            let _stopped = stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::OwnerInvalidated);
        }
        if shutdown
            .as_deref()
            .and_then(RuntimeShutdownObserverV1::observed)
            .is_some()
        {
            let _stopped = stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed);
        }
        if Instant::now() >= operation_cutoff {
            let _stopped = stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed);
        }
        let (initial_lifecycle, shutdown_observed) =
            match (self.adapter.control.as_mut(), shutdown.as_deref_mut()) {
                (Some(control), Some(shutdown)) if Instant::now() < operation_cutoff => {
                    tokio::select! {
                        biased;
                        _observation = shutdown.wait() => (None, true),
                        result = timeout_at(
                            TokioInstant::from_std(operation_cutoff),
                            control.next_lifecycle(),
                        ) => (result.ok().flatten(), false),
                    }
                }
                (Some(control), None) if Instant::now() < operation_cutoff => (
                    timeout_at(
                        TokioInstant::from_std(operation_cutoff),
                        control.next_lifecycle(),
                    )
                    .await
                    .ok()
                    .flatten(),
                    false,
                ),
                (Some(_), Some(shutdown)) => (None, shutdown.observed().is_some()),
                (Some(_), None) | (None, _) => (None, false),
            };
        if shutdown_observed {
            let _stopped = stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed);
        }
        if initial_lifecycle != Some(GatewayLifecycleEventV3::Starting) {
            let _stopped = stopped_sender.send(true);
            return Err(if Instant::now() >= operation_cutoff {
                RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed
            } else {
                RuntimeDiscordGatewayStartErrorV1::RuntimeUnavailable
            });
        }
        if shutdown
            .as_deref()
            .and_then(RuntimeShutdownObserverV1::observed)
            .is_some()
        {
            let _stopped = stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed);
        }
        if self.owner_invalidated.load(Ordering::Acquire) {
            let _stopped = stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::OwnerInvalidated);
        }
        let runtime = match self._runtime.take() {
            Some(runtime) => runtime,
            None => {
                let _stopped = stopped_sender.send(true);
                return Err(RuntimeDiscordGatewayStartErrorV1::RuntimeHalfUnavailable);
            }
        };
        let Some(control) = self.adapter.control.take() else {
            let _stopped = stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::RuntimeHalfUnavailable);
        };
        let Some(discord_reservation_publisher) = self.adapter.discord_reservation_publisher.take()
        else {
            let _stopped = stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::RuntimeHalfUnavailable);
        };
        let (discord_commands, commands) = mpsc::channel(1);
        let (lifecycle_drained_sender, lifecycle_drained) = watch::channel(1);
        let (reserved_resume, reserved_resume_receiver) = mpsc::channel(1);
        let control_task = RuntimeDiscordControlTaskV1::new(
            runtime_handle.spawn(run_runtime_discord_control_v1(
                control,
                commands,
                reserved_resume_receiver,
                lifecycle_drained_sender,
                discord_reservation_publisher,
                self.coordinator_snapshot.interrupt.clone(),
            )),
            reserved_resume,
        );
        self.adapter.discord_commands = Some(discord_commands);
        Ok(RuntimePreparedDiscordGatewayStartV1 {
            runtime_handle,
            runtime,
            control_task,
            lifecycle_drained,
            discord_reservation: self.adapter.discord_reservation.clone(),
            stopped_sender,
            stopped,
        })
    }

    pub fn closed_snapshot(&self) -> RuntimeGatewayClosedSnapshotV2 {
        self.coordinator_snapshot.effective_snapshot()
    }

    pub fn observe_current_ready_attestation(
        &self,
    ) -> Result<RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyObservationErrorV1> {
        self.require_current_gateway_ownership()?;
        let attestation = self.adapter.observe_current_ready_attestation()?;
        self.require_current_gateway_ownership()?;
        Ok(attestation)
    }

    pub fn ready_attestation_is_current(
        &self,
        candidate: &RuntimeGatewayReadyAttestationV2,
    ) -> bool {
        self.observe_current_ready_attestation()
            .is_ok_and(|current| current == *candidate)
    }

    pub fn observe_paused_connected_gateway_v2(
        &self,
    ) -> Result<RuntimePausedGatewayObservationV2, RuntimeGatewayReadyObservationErrorV1> {
        self.require_current_gateway_ownership()?;
        let closed = self.closed_snapshot();
        let RuntimeGatewayClosedSnapshotV2::Emergency { generation, .. } = closed else {
            return Err(RuntimeGatewayReadyObservationErrorV1::Stopped);
        };
        let snapshot = self
            .adapter
            .connection_observer
            .current_admission_snapshot();
        let (observation, epoch) = self
            .adapter
            .map_paused_connected_observation(generation, snapshot)?;
        self.adapter
            .require_discord_pause_reservation_v2(snapshot, epoch)?;
        self.adapter.require_healthy_paused_control(epoch)?;
        if self
            .adapter
            .connection_observer
            .current_admission_snapshot()
            != snapshot
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        self.require_current_gateway_ownership()?;
        if self.closed_snapshot() != closed {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        self.adapter.require_healthy_paused_control(epoch)?;
        self.adapter
            .require_discord_pause_reservation_v2(snapshot, epoch)?;
        if self
            .adapter
            .connection_observer
            .current_admission_snapshot()
            != snapshot
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        if self.closed_snapshot() != closed {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        self.require_current_gateway_ownership()?;
        Ok(observation)
    }

    pub(crate) fn initial_emergency_gateway_section_v2<'a>(
        &'a mut self,
        prepared_owner: &'a RuntimeGatewayOwnerPreparedClosedRecoveryV2,
        expected_paused_gateway: &RuntimePausedGatewayObservationV2,
    ) -> Result<RuntimeEmergencyGatewaySectionV2<'a>, RuntimeGatewayReadyObservationErrorV1> {
        RuntimeEmergencyGatewaySectionV2::acquire(
            &self.adapter,
            &mut self.coordinator,
            &self.owner_invalidated,
            prepared_owner,
            expected_paused_gateway,
        )
    }

    pub fn start_gateway_owner_startup_watchdog_v1<P>(
        &mut self,
        port: P,
        accepted_receipt: RuntimeAcceptedGatewayOwnerReceiptV1,
        request_started_at: Instant,
        response_observed_at: Instant,
        config: RuntimeGatewayOwnerStartupWatchdogConfigV1,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogHandleV1,
        RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P>,
    >
    where
        P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
        P::Error: Send + 'static,
    {
        self.start_gateway_owner_startup_watchdog_with_cleanup_deadline_v1(
            port,
            accepted_receipt,
            request_started_at,
            response_observed_at,
            config,
            None,
        )
    }

    pub(crate) fn start_bounded_gateway_owner_startup_watchdog_v1<P>(
        &mut self,
        port: P,
        accepted_receipt: RuntimeAcceptedGatewayOwnerReceiptV1,
        request_started_at: Instant,
        response_observed_at: Instant,
        config: RuntimeGatewayOwnerStartupWatchdogConfigV1,
        cleanup_deadline: Instant,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogHandleV1,
        RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P>,
    >
    where
        P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
        P::Error: Send + 'static,
    {
        self.start_gateway_owner_startup_watchdog_with_cleanup_deadline_v1(
            port,
            accepted_receipt,
            request_started_at,
            response_observed_at,
            config,
            Some(cleanup_deadline),
        )
    }

    fn start_gateway_owner_startup_watchdog_with_cleanup_deadline_v1<P>(
        &mut self,
        port: P,
        accepted_receipt: RuntimeAcceptedGatewayOwnerReceiptV1,
        request_started_at: Instant,
        response_observed_at: Instant,
        config: RuntimeGatewayOwnerStartupWatchdogConfigV1,
        cleanup_deadline: Option<Instant>,
    ) -> Result<
        RuntimeGatewayOwnerStartupWatchdogHandleV1,
        RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P>,
    >
    where
        P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
        P::Error: Send + 'static,
    {
        let lease_id = accepted_receipt.receipt().lease_id.clone();
        let Some(invalidator) = self.owner_invalidator.take() else {
            self.coordinator_snapshot
                .interrupt
                .trip_invalidation(RuntimeGatewayInvalidationCauseV2::OwnershipUncertain);
            self.owner_invalidated.store(true, Ordering::Release);
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::AlreadyStarted,
                port,
                lease_id,
            ));
        };
        if accepted_receipt.receipt().lease_id.process_instance_id
            != self.adapter.process_instance_id
        {
            invalidator.invalidate_gateway_ownership();
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::ProcessMismatch,
                port,
                lease_id,
            ));
        }
        if accepted_receipt
            .receipt()
            .lease_id
            .gateway_shard_id
            .as_str()
            != SUPPORTED_GATEWAY_SHARD_ID
        {
            invalidator.invalidate_gateway_ownership();
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::ShardMismatch,
                port,
                lease_id,
            ));
        }
        start_runtime_gateway_owner_startup_watchdog_v1(
            port,
            invalidator,
            self.owner_invalidated.clone(),
            accepted_receipt,
            config,
            RuntimeGatewayOwnerStartupWatchdogStartContextV1::new(
                request_started_at,
                response_observed_at,
                cleanup_deadline,
            ),
        )
    }

    fn require_current_gateway_ownership(
        &self,
    ) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        if self.owner_invalidated.load(Ordering::Acquire) {
            Err(RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    pub(crate) fn connect_ready_for_gateway_section_test_v2(&mut self) {
        self._runtime
            .as_mut()
            .expect("gateway runtime half")
            ._inner
            .mark_connected(GatewayReadyKindV3::Ready)
            .unwrap();
    }

    #[cfg(test)]
    pub(crate) fn disconnect_for_gateway_section_test_v2(&mut self) {
        self._runtime
            .as_mut()
            .expect("gateway runtime half")
            ._inner
            .mark_disconnected(automation_runtime::GatewayDisconnectKindV3::Reconnect)
            .unwrap();
    }

    #[cfg(test)]
    pub(crate) async fn held_initial_section_blocks_repeated_pause_test_v2(
        &mut self,
        prepared_owner: &RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    ) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        let expected_paused_gateway = self.observe_paused_connected_gateway_v2()?;
        let Self {
            adapter,
            coordinator,
            _runtime,
            owner_invalidated,
            ..
        } = self;
        let section = RuntimeEmergencyGatewaySectionV2::acquire(
            adapter,
            coordinator,
            owner_invalidated,
            prepared_owner,
            &expected_paused_gateway,
        )?;
        let (started_sender, started_receiver) = std::sync::mpsc::sync_channel(0);
        let (completed_sender, completed_receiver) = std::sync::mpsc::sync_channel(1);
        let (dummy_control, dummy_runtime) =
            shared_gateway_control_channel_with_policy_and_invalidator_v3(
                GatewayControlConfigV3::default(),
                GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
                RuntimeGatewaySnapshotTestInvalidatorV3,
            );
        let runtime_half = std::mem::replace(
            &mut _runtime.as_mut().expect("gateway runtime half")._inner,
            dummy_runtime,
        );
        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            started_sender.send(()).unwrap();
            let mut runtime_half = runtime_half;
            let outcome = runtime.block_on(runtime_half.process_next_command());
            completed_sender.send((runtime_half, outcome)).unwrap();
        });
        started_receiver.recv().unwrap();
        let blocked = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            adapter
                .control
                .as_ref()
                .expect("gateway control half")
                .pause_admission(),
        )
        .await;
        assert!(blocked.is_err());
        assert!(!worker.is_finished());
        drop(section);
        let (runtime_half, outcome) = completed_receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        worker.join().unwrap();
        assert!(matches!(
            outcome,
            automation_runtime::GatewayRuntimeCommandOutcomeV3::Applied(
                automation_runtime::GatewayCommandAckV3::Paused { .. }
            )
        ));
        _runtime.as_mut().expect("gateway runtime half")._inner = runtime_half;
        drop(dummy_control);
        Ok(())
    }
}

async fn run_runtime_discord_control_v1(
    mut control: SharedGatewayControlV3,
    mut commands: mpsc::Receiver<RuntimeDiscordControlCommandV1>,
    mut reserved_resume: mpsc::Receiver<RuntimeDiscordReservedResumeRequestV2>,
    lifecycle_drained: watch::Sender<u64>,
    discord_reservation: watch::Sender<RuntimeDiscordAdmissionReservationSnapshotV2>,
    coordinator: RuntimeGatewayCoordinatorInterruptHandleV2,
) {
    let mut lifecycle_sequence = 1u64;
    let mut pause_token: Option<GatewayPauseTokenV3> = None;
    loop {
        tokio::select! {
            biased;
            lifecycle = control.next_lifecycle() => {
                let Some(lifecycle) = lifecycle else {
                    return;
                };
                pause_token = None;
                discord_reservation.send_replace(
                    RuntimeDiscordAdmissionReservationSnapshotV2::unreserved(
                        control.current_admission_snapshot(),
                    ),
                );
                if !publish_runtime_lifecycle_drain_v1(
                    &lifecycle_drained,
                    &mut lifecycle_sequence,
                ) {
                    return;
                }
                if matches!(
                    lifecycle,
                    GatewayLifecycleEventV3::Connected { paused: true, .. }
                ) {
                    let Ok(Ok(GatewayCommandAckV3::Paused { resume_token, .. })) = timeout(
                        DISCORD_CONTROL_OPERATION_TIMEOUT,
                        control.pause_admission(),
                    )
                    .await
                    else {
                        return;
                    };
                    if !matches!(
                        timeout(
                            DISCORD_CONTROL_OPERATION_TIMEOUT,
                            control.next_lifecycle(),
                        )
                        .await,
                        Ok(Some(GatewayLifecycleEventV3::Paused { .. }))
                    ) {
                        return;
                    }
                    let Some(snapshot) =
                        RuntimeDiscordAdmissionReservationSnapshotV2::reserved(
                            control.current_admission_snapshot(),
                            &resume_token,
                        )
                    else {
                        return;
                    };
                    pause_token = Some(resume_token);
                    discord_reservation.send_replace(snapshot);
                    if !publish_runtime_lifecycle_drain_v1(
                        &lifecycle_drained,
                        &mut lifecycle_sequence,
                    ) {
                        return;
                    }
                }
            }
            request = reserved_resume.recv() => {
                let Some(request) = request else {
                    return;
                };
                let resumed = resume_reserved_discord_admission_v2(
                    &mut control,
                    &mut pause_token,
                    request.coordinator_generation,
                    request.expected,
                    &coordinator,
                    &request.observation,
                    &lifecycle_drained,
                    &mut lifecycle_sequence,
                    &discord_reservation,
                )
                .await;
                let _response = request.response.send(resumed);
            }
            command = commands.recv() => {
                let Some(command) = command else {
                    return;
                };
                match command {
                    RuntimeDiscordControlCommandV1::BeginDrain { response } => {
                        let drained = if matches!(
                            timeout(
                                DISCORD_CONTROL_OPERATION_TIMEOUT,
                                control.begin_drain(),
                            )
                            .await,
                            Ok(Ok(GatewayCommandAckV3::Draining { .. }))
                        ) {
                            match timeout(
                                DISCORD_CONTROL_OPERATION_TIMEOUT,
                                control.next_lifecycle(),
                            )
                            .await
                            {
                                Ok(Some(GatewayLifecycleEventV3::Draining { .. })) => {
                                    pause_token = None;
                                    discord_reservation.send_replace(
                                        RuntimeDiscordAdmissionReservationSnapshotV2::unreserved(
                                            control.current_admission_snapshot(),
                                        ),
                                    );
                                    publish_runtime_lifecycle_drain_v1(
                                    &lifecycle_drained,
                                    &mut lifecycle_sequence,
                                    )
                                }
                                Ok(None) | Ok(Some(_)) | Err(_) => false,
                            }
                        } else {
                            false
                        };
                        let _response = response.send(drained);
                    }
                    #[cfg(test)]
                    RuntimeDiscordControlCommandV1::OpenAdmission { response } => {
                        let opened = resume_unchecked_discord_admission_for_test_v2(
                            &mut control,
                            &mut pause_token,
                            &lifecycle_drained,
                            &mut lifecycle_sequence,
                            &discord_reservation,
                        )
                        .await;
                        let _response = response.send(opened);
                    }
                }
            }
        }
    }
}

async fn resume_reserved_discord_admission_v2(
    control: &mut SharedGatewayControlV3,
    pause_token: &mut Option<GatewayPauseTokenV3>,
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    expected: RuntimeDiscordPauseReservationIdentityV2,
    coordinator: &RuntimeGatewayCoordinatorInterruptHandleV2,
    observation: &watch::Sender<Option<RuntimeDiscordRecoveryResumeEvidenceV2>>,
    lifecycle_drained: &watch::Sender<u64>,
    lifecycle_sequence: &mut u64,
    discord_reservation: &watch::Sender<RuntimeDiscordAdmissionReservationSnapshotV2>,
) -> RuntimeDiscordRecoveryResumeControlOutcomeV2 {
    let Some(token) = pause_token.as_ref() else {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::DefinitelyNotApplied;
    };
    if RuntimeDiscordPauseReservationIdentityV2::from_token(
        &token,
        control.current_admission_snapshot(),
    ) != Some(expected)
    {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::DefinitelyNotApplied;
    }
    if !coordinator.claim_recovery_resume_v2(coordinator_generation) {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::DefinitelyNotApplied;
    }
    let Some(token) = pause_token.take() else {
        coordinator.cancel_recovery_resume_claim_v2(coordinator_generation);
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::DefinitelyNotApplied;
    };
    match timeout(
        DISCORD_CONTROL_OPERATION_TIMEOUT,
        control.resume_admission(&token),
    )
    .await
    {
        Ok(Ok(GatewayCommandAckV3::AdmissionResumed { epoch })) if epoch == expected.epoch() => {}
        Ok(Err(_)) | Ok(Ok(_)) => {
            coordinator.cancel_recovery_resume_claim_v2(coordinator_generation);
            return RuntimeDiscordRecoveryResumeControlOutcomeV2::DefinitelyNotApplied;
        }
        Err(_) => return RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate,
    }
    if coordinator
        .complete_recovery_resume_v2(coordinator_generation)
        .is_none()
    {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate;
    }
    if !matches!(
        timeout(
            DISCORD_CONTROL_OPERATION_TIMEOUT,
            control.next_lifecycle(),
        )
        .await,
        Ok(Some(GatewayLifecycleEventV3::AdmissionResumed { epoch })) if epoch == expected.epoch()
    ) {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate;
    }
    let first_admission = control.current_admission_snapshot();
    let Ok(ready) = control.issue_ready_lease(expected.epoch()) else {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate;
    };
    if !control.ready_lease_is_current(&ready) {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate;
    }
    let second_admission = control.current_admission_snapshot();
    if first_admission != second_admission || !control.ready_lease_is_current(&ready) {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate;
    }
    let Some(evidence) = RuntimeDiscordRecoveryResumeEvidenceV2::from_exact_snapshot_v2(
        coordinator_generation,
        expected,
        first_admission,
        ready,
    ) else {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate;
    };
    let reservation = RuntimeDiscordAdmissionReservationSnapshotV2::unreserved(first_admission);
    discord_reservation.send_replace(reservation);
    if *discord_reservation.borrow() != reservation
        || control.current_admission_snapshot() != first_admission
        || !control.ready_lease_is_current(&ready)
        || !publish_runtime_lifecycle_drain_v1(lifecycle_drained, lifecycle_sequence)
        || *discord_reservation.borrow() != reservation
        || control.current_admission_snapshot() != first_admission
        || !control.ready_lease_is_current(&ready)
    {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate;
    }
    observation.send_replace(Some(evidence));
    if *observation.borrow() != Some(evidence)
        || *discord_reservation.borrow() != reservation
        || control.current_admission_snapshot() != first_admission
        || !control.ready_lease_is_current(&ready)
    {
        return RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate;
    }
    RuntimeDiscordRecoveryResumeControlOutcomeV2::Applied(evidence)
}

#[cfg(test)]
async fn resume_unchecked_discord_admission_for_test_v2(
    control: &mut SharedGatewayControlV3,
    pause_token: &mut Option<GatewayPauseTokenV3>,
    lifecycle_drained: &watch::Sender<u64>,
    lifecycle_sequence: &mut u64,
    discord_reservation: &watch::Sender<RuntimeDiscordAdmissionReservationSnapshotV2>,
) -> bool {
    let Some(token) = pause_token.take() else {
        return false;
    };
    if !matches!(
        timeout(
            DISCORD_CONTROL_OPERATION_TIMEOUT,
            control.resume_admission(&token),
        )
        .await,
        Ok(Ok(GatewayCommandAckV3::AdmissionResumed { .. }))
    ) || !matches!(
        timeout(DISCORD_CONTROL_OPERATION_TIMEOUT, control.next_lifecycle(),).await,
        Ok(Some(GatewayLifecycleEventV3::AdmissionResumed { .. }))
    ) {
        return false;
    }
    discord_reservation.send_replace(RuntimeDiscordAdmissionReservationSnapshotV2::unreserved(
        control.current_admission_snapshot(),
    ));
    publish_runtime_lifecycle_drain_v1(lifecycle_drained, lifecycle_sequence)
}

fn publish_runtime_lifecycle_drain_v1(
    lifecycle_drained: &watch::Sender<u64>,
    lifecycle_sequence: &mut u64,
) -> bool {
    let Some(next) = lifecycle_sequence.checked_add(1) else {
        return false;
    };
    *lifecycle_sequence = next;
    lifecycle_drained.send_replace(next);
    true
}

impl<'a> RuntimeEmergencyGatewaySectionV2<'a> {
    fn acquire(
        gateway: &'a SharedGatewayControlAdapterV2,
        coordinator: &'a mut Option<RuntimeGatewayCoordinatorOwnerV2>,
        owner_invalidated: &'a Arc<AtomicBool>,
        prepared_owner: &'a RuntimeGatewayOwnerPreparedClosedRecoveryV2,
        expected_paused_gateway: &RuntimePausedGatewayObservationV2,
    ) -> Result<Self, RuntimeGatewayReadyObservationErrorV1> {
        require_prepared_owner_lifetime_v2(owner_invalidated, prepared_owner)?;
        let coordinator_owner = coordinator
            .as_mut()
            .ok_or(RuntimeGatewayReadyObservationErrorV1::Stopped)?;
        coordinator_owner.reconcile_interrupt();
        coordinator_owner.require_uninterrupted()?;
        if coordinator_owner.lifecycle().snapshot()
            != (RuntimeGatewayClosedSnapshotV2::Emergency {
                generation: RuntimeGatewayCoordinatorGenerationV2::FIRST,
                cause: RuntimeGatewayEmergencyCauseV2::Starting,
            })
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::Stopped);
        }
        require_prepared_owner_lifetime_v2(owner_invalidated, prepared_owner)?;
        let snapshot = gateway.connection_observer.current_admission_snapshot();
        let (paused_gateway, connection_epoch) = gateway.map_paused_connected_observation(
            RuntimeGatewayCoordinatorGenerationV2::FIRST,
            snapshot,
        )?;
        if paused_gateway != *expected_paused_gateway {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        gateway.require_healthy_paused_control(connection_epoch)?;
        if gateway.connection_observer.current_admission_snapshot() != snapshot {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        require_prepared_owner_lifetime_v2(owner_invalidated, prepared_owner)?;
        let admission_snapshot = gateway.admission_snapshot.borrow();
        if *admission_snapshot != snapshot {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        require_prepared_owner_lifetime_v2(owner_invalidated, prepared_owner)?;
        let section = Self {
            gateway,
            coordinator,
            prepared_owner,
            owner_invalidated,
            admission_snapshot,
            paused_gateway,
            connection_epoch,
            pending_permit: None,
        };
        section.require_current_v2()?;
        Ok(section)
    }

    pub(crate) fn begin_empty_recovery_v2(
        &mut self,
        _authority: &RuntimeClosedRecoveryTransitionAuthorityV2,
        recovery_id: RuntimeRecoveryIdV2,
        readiness: RuntimeCapabilityReadinessSetV2,
        registry: RuntimeLockedRegistryEmptyEvidenceV2<'_, '_>,
    ) -> Result<(), RuntimeGatewayRecoverySectionErrorV2> {
        self.require_current_v2()
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        if self.pending_permit.is_some() {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        let input = RuntimeClosedRecoveryInputV2::new(
            recovery_id,
            self.prepared_owner.observation().receipt().clone(),
            readiness,
            self.paused_gateway.clone(),
            RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry.into_observation_v2()),
        );
        let coordinator = self
            .coordinator
            .as_mut()
            .ok_or(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?;
        let (_, permit) = coordinator
            .lifecycle_mut()
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?
            .begin_recovery(RuntimeGatewayCoordinatorGenerationV2::FIRST, input)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        coordinator.publish_snapshot();
        self.pending_permit = Some(permit);
        self.require_pending_current_v2()
    }

    pub(crate) fn into_recovery_pending_binding_v2(
        mut self,
    ) -> Result<RuntimeRecoveryPendingGatewayBindingV2, RuntimeGatewayRecoverySectionErrorV2> {
        self.require_pending_current_v2()?;
        let permit = self
            .pending_permit
            .take()
            .ok_or(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?;
        let coordinator = self
            .coordinator
            .take()
            .ok_or(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?;
        Ok(RuntimeRecoveryPendingGatewayBindingV2 {
            process_instance_id: self.gateway.process_instance_id.clone(),
            observer: self.gateway.connection_observer.clone(),
            admission_snapshot: self.gateway.admission_snapshot.clone(),
            discord_reservation: self.gateway.discord_reservation.clone(),
            coordinator: Some(coordinator),
            owner_invalidated: self.owner_invalidated.clone(),
            permit: Some(permit),
        })
    }

    fn require_current_v2(&self) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        require_prepared_owner_lifetime_v2(self.owner_invalidated, self.prepared_owner)?;
        let coordinator = self
            .coordinator
            .as_ref()
            .ok_or(RuntimeGatewayReadyObservationErrorV1::Stopped)?;
        coordinator.require_uninterrupted()?;
        if coordinator.lifecycle().snapshot()
            != (RuntimeGatewayClosedSnapshotV2::Emergency {
                generation: RuntimeGatewayCoordinatorGenerationV2::FIRST,
                cause: RuntimeGatewayEmergencyCauseV2::Starting,
            })
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::Stopped);
        }
        let snapshot = *self.admission_snapshot;
        let (paused_gateway, connection_epoch) = self.gateway.map_paused_connected_observation(
            RuntimeGatewayCoordinatorGenerationV2::FIRST,
            snapshot,
        )?;
        if paused_gateway != self.paused_gateway || connection_epoch != self.connection_epoch {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        require_prepared_owner_lifetime_v2(self.owner_invalidated, self.prepared_owner)
    }

    fn require_pending_current_v2(&self) -> Result<(), RuntimeGatewayRecoverySectionErrorV2> {
        let permit = self
            .pending_permit
            .as_ref()
            .ok_or(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?;
        let coordinator = self
            .coordinator
            .as_ref()
            .ok_or(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?;
        coordinator
            .require_uninterrupted()
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        coordinator
            .lifecycle()
            .validate_recovery_permit(permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        require_prepared_owner_lifetime_v2(self.owner_invalidated, self.prepared_owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        if self.prepared_owner.observation().receipt() != permit.owner_receipt()
            || self.paused_gateway != *permit.paused_gateway()
        {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        coordinator
            .lifecycle()
            .validate_recovery_permit(permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)
    }
}

impl Drop for RuntimeEmergencyGatewaySectionV2<'_> {
    fn drop(&mut self) {
        let Some(permit) = self.pending_permit.as_ref() else {
            return;
        };
        let Some(coordinator) = self.coordinator.as_mut() else {
            return;
        };
        coordinator.reconcile_interrupt();
        if coordinator
            .lifecycle()
            .validate_recovery_permit(permit)
            .is_ok()
        {
            let _ = coordinator.lifecycle.invalidate(
                permit.coordinator_generation(),
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            );
            coordinator.publish_snapshot();
        }
    }
}

impl RuntimeRecoveryPendingGatewayBindingV2 {
    fn permit_v2(&self) -> &RuntimeClosedDrainRecoveryPermitV2 {
        self.permit.as_ref().expect("runtime recovery permit")
    }

    pub(crate) fn pending_section_v2<'a>(
        &'a self,
        prepared_owner: &'a RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    ) -> Result<RuntimeRecoveryPendingGatewaySectionV2<'a>, RuntimeGatewayRecoverySectionErrorV2>
    {
        self.pending_section_with_owner_v2(RuntimeGatewayOwnerRecoveryEvidenceV2::Prepared(
            prepared_owner,
        ))
    }

    pub(crate) fn committed_pending_section_v2<'a>(
        &'a self,
        committed_owner: &'a RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    ) -> Result<RuntimeRecoveryPendingGatewaySectionV2<'a>, RuntimeGatewayRecoverySectionErrorV2>
    {
        self.pending_section_with_owner_v2(RuntimeGatewayOwnerRecoveryEvidenceV2::Committed(
            committed_owner,
        ))
    }

    pub(crate) async fn commit_prepared_owner_in_place_v2(
        &self,
        _authority: &RuntimeClosedRecoveryTransitionAuthorityV2,
        prepared_owner: &mut RuntimeGatewayOwnerPreparedClosedRecoveryV2,
        commit_cutoff: Instant,
    ) -> Result<(), RuntimeGatewayRecoveryOwnerCommitErrorV2> {
        let section = self
            .pending_section_v2(prepared_owner)
            .map_err(RuntimeGatewayRecoveryOwnerCommitErrorV2::Section)?;
        drop(section);
        if Instant::now() >= commit_cutoff {
            return Err(RuntimeGatewayRecoveryOwnerCommitErrorV2::DeadlineElapsed);
        }
        tokio::select! {
            biased;
            _ = sleep_until(TokioInstant::from_std(commit_cutoff)) => {
                Err(RuntimeGatewayRecoveryOwnerCommitErrorV2::DeadlineElapsed)
            }
            result = prepared_owner.commit_closed_recovery_in_place_v2(self.permit_v2()) => {
                result.map_err(RuntimeGatewayRecoveryOwnerCommitErrorV2::Owner)
            }
        }
    }

    pub(crate) fn refresh_readiness_in_place_v2(
        &mut self,
        committed_owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
        readiness: RuntimeCapabilityReadinessSetV2,
    ) -> Result<RuntimeAuthorizedStartupRecoveryIterationV2, RuntimeGatewayRecoverySectionErrorV2>
    {
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        let transition = {
            let owner_invalidated = self.owner_invalidated.clone();
            let coordinator = current_runtime_gateway_coordinator_mut_v2(&mut self.coordinator)?;
            let permit = self.permit.as_mut().expect("runtime recovery permit");
            let transition = coordinator
                .lifecycle
                .refresh_recovery_readiness(permit, readiness);
            if transition.is_err()
                && matches!(
                    coordinator.lifecycle.snapshot(),
                    RuntimeGatewayClosedSnapshotV2::Emergency { .. }
                        | RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
                )
            {
                owner_invalidated.store(true, Ordering::Release);
            }
            coordinator.publish_snapshot();
            transition
        };
        let iteration = transition.map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        Ok(iteration)
    }

    pub(crate) fn begin_startup_recovery_observation_v2(
        &mut self,
        committed_owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
        iteration: RuntimeAuthorizedStartupRecoveryIterationV2,
    ) -> Result<RuntimeAuthorizedStartupRecoveryObservationV2, RuntimeGatewayRecoverySectionErrorV2>
    {
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        let authorization = {
            let coordinator = current_runtime_gateway_coordinator_mut_v2(&mut self.coordinator)?;
            let permit = self.permit.as_mut().expect("runtime recovery permit");
            let authorization = coordinator
                .lifecycle
                .begin_startup_recovery_observation(permit, iteration);
            coordinator.publish_snapshot();
            authorization
        }
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        Ok(authorization)
    }

    pub(crate) fn into_startup_recovery_observation_successor_v2(
        mut self,
        committed_owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
        completed: RuntimeCompletedStartupRecoveryObservationV2,
    ) -> Result<(Self, RuntimeAcceptedStartupRecoveryOutcomeV2), RuntimeGatewayRecoverySectionErrorV2>
    {
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        let transition = {
            let owner_invalidated = self.owner_invalidated.clone();
            let coordinator = current_runtime_gateway_coordinator_mut_v2(&mut self.coordinator)?;
            let permit = self.permit.as_mut().expect("runtime recovery permit");
            let transition = coordinator
                .lifecycle
                .complete_startup_recovery_observation(permit, completed);
            if transition.is_err()
                && matches!(
                    coordinator.lifecycle.snapshot(),
                    RuntimeGatewayClosedSnapshotV2::Emergency { .. }
                        | RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
                )
            {
                owner_invalidated.store(true, Ordering::Release);
            }
            coordinator.publish_snapshot();
            transition
        }
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        Ok((self, transition))
    }

    pub(crate) fn begin_startup_recovery_execution_v2(
        &mut self,
        committed_owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
        continuation: RuntimeStartupRecoveryContinuationV2,
    ) -> Result<RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeGatewayRecoverySectionErrorV2>
    {
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        let authorization = {
            let coordinator = current_runtime_gateway_coordinator_mut_v2(&mut self.coordinator)?;
            let permit = self.permit.as_mut().expect("runtime recovery permit");
            let authorization = coordinator
                .lifecycle
                .begin_startup_recovery_execution(permit, continuation);
            coordinator.publish_snapshot();
            authorization
        }
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        Ok(authorization)
    }

    pub(crate) fn complete_startup_recovery_execution_v2(
        &mut self,
        committed_owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
        completed: RuntimeCompletedStartupRecoveryExecutionV2,
    ) -> Result<
        RuntimeAcceptedStartupRecoveryExecutionOutcomeV2,
        RuntimeGatewayRecoverySectionErrorV2,
    > {
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        let transition = {
            let owner_invalidated = self.owner_invalidated.clone();
            let coordinator = current_runtime_gateway_coordinator_mut_v2(&mut self.coordinator)?;
            let permit = self.permit.as_mut().expect("runtime recovery permit");
            let transition = coordinator
                .lifecycle
                .complete_startup_recovery_execution(permit, completed);
            if transition.is_err()
                && matches!(
                    coordinator.lifecycle.snapshot(),
                    RuntimeGatewayClosedSnapshotV2::Emergency { .. }
                        | RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
                )
            {
                owner_invalidated.store(true, Ordering::Release);
            }
            coordinator.publish_snapshot();
            transition
        }
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        Ok(transition)
    }

    pub(crate) fn validate_startup_recovery_fixed_point_v2(
        &self,
        committed_owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
        proof: &RuntimeStartupRecoveryFixedPointProofV2,
    ) -> Result<(), RuntimeGatewayRecoverySectionErrorV2> {
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        let transition = {
            let coordinator = self.coordinator_ref_v2()?;
            let transition = coordinator
                .lifecycle()
                .validate_startup_recovery_fixed_point(self.permit_v2(), proof);
            if matches!(
                transition,
                Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryFixedPointAuthority)
            ) {
                self.owner_invalidated.store(true, Ordering::Release);
                coordinator
                    .interrupt
                    .trip_invalidation(RuntimeGatewayInvalidationCauseV2::ProtocolViolation);
            }
            transition
        };
        transition.map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        Ok(())
    }

    pub(crate) fn into_worker_fixed_point_v2(
        mut self,
        proof: RuntimeStartupRecoveryFixedPointProofV2,
    ) -> Result<
        (
            RuntimeGatewayProductionCoordinatorV2,
            automation_runtime_worker::RuntimeStartupRecoveryFixedPointProcessV2,
        ),
        RuntimeGatewayFixedPointAcceptanceFailureV2,
    > {
        let fixed_point_admission_snapshot = self.observer.current_admission_snapshot();
        if *self.admission_snapshot.borrow() != fixed_point_admission_snapshot
            || self.observer.current_admission_snapshot() != fixed_point_admission_snapshot
        {
            return Err(RuntimeGatewayFixedPointAcceptanceFailureV2 {
                binding: Box::new(self),
                proof: Box::new(proof),
                error: RuntimeGatewayFixedPointAcceptanceErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::Gateway(
                        RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot,
                    ),
                ),
            });
        }
        let paused_gateway_generation = self.permit_v2().originating_emergency_generation();
        let (paused_gateway, connection_epoch) = match map_paused_connected_observation_v2(
            &self.process_instance_id,
            paused_gateway_generation,
            fixed_point_admission_snapshot,
        ) {
            Ok(observation) => observation,
            Err(error) => {
                return Err(RuntimeGatewayFixedPointAcceptanceFailureV2 {
                    binding: Box::new(self),
                    proof: Box::new(proof),
                    error: RuntimeGatewayFixedPointAcceptanceErrorV2::Gateway(
                        RuntimeGatewayRecoverySectionErrorV2::Gateway(error),
                    ),
                });
            }
        };
        if paused_gateway != *self.permit_v2().paused_gateway()
            || require_healthy_paused_observer_v2(&self.observer, connection_epoch).is_err()
            || self.observer.current_admission_snapshot() != fixed_point_admission_snapshot
            || *self.admission_snapshot.borrow() != fixed_point_admission_snapshot
        {
            return Err(RuntimeGatewayFixedPointAcceptanceFailureV2 {
                binding: Box::new(self),
                proof: Box::new(proof),
                error: RuntimeGatewayFixedPointAcceptanceErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::Gateway(
                        RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot,
                    ),
                ),
            });
        }
        let mut coordinator = match self.coordinator.take() {
            Some(coordinator) => coordinator,
            None => {
                return Err(RuntimeGatewayFixedPointAcceptanceFailureV2 {
                    binding: Box::new(self),
                    proof: Box::new(proof),
                    error: RuntimeGatewayFixedPointAcceptanceErrorV2::Gateway(
                        RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                    ),
                });
            }
        };
        coordinator.reconcile_interrupt();
        if let Err(error) = coordinator.require_uninterrupted() {
            self.coordinator = Some(coordinator);
            return Err(RuntimeGatewayFixedPointAcceptanceFailureV2 {
                binding: Box::new(self),
                proof: Box::new(proof),
                error: RuntimeGatewayFixedPointAcceptanceErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::Gateway(error),
                ),
            });
        }
        let permit = self
            .permit
            .take()
            .expect("runtime fixed-point recovery permit");
        if coordinator.interrupt.current_generation_v2() != permit.coordinator_generation() {
            self.permit = Some(permit);
            self.coordinator = Some(coordinator);
            return Err(RuntimeGatewayFixedPointAcceptanceFailureV2 {
                binding: Box::new(self),
                proof: Box::new(proof),
                error: RuntimeGatewayFixedPointAcceptanceErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                ),
            });
        }
        if let Err(error) = coordinator
            .lifecycle()
            .validate_startup_recovery_fixed_point(&permit, &proof)
        {
            self.permit = Some(permit);
            self.coordinator = Some(coordinator);
            return Err(RuntimeGatewayFixedPointAcceptanceFailureV2 {
                binding: Box::new(self),
                proof: Box::new(proof),
                error: RuntimeGatewayFixedPointAcceptanceErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::Coordinator(error),
                ),
            });
        }
        let production_generation = permit.coordinator_generation();
        if !coordinator
            .interrupt
            .activate_production_generation_v2(production_generation)
        {
            self.permit = Some(permit);
            self.coordinator = Some(coordinator);
            return Err(RuntimeGatewayFixedPointAcceptanceFailureV2 {
                binding: Box::new(self),
                proof: Box::new(proof),
                error: RuntimeGatewayFixedPointAcceptanceErrorV2::Gateway(
                    RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
                ),
            });
        }
        let RuntimeGatewayCoordinatorOwnerV2 {
            lifecycle,
            interrupt,
            applied_interrupt,
            snapshot,
        } = coordinator;
        match lifecycle.into_production_fixed_point(permit, proof) {
            Ok(state) => Ok((
                RuntimeGatewayProductionCoordinatorV2 {
                    process_instance_id: self.process_instance_id.clone(),
                    observer: self.observer.clone(),
                    admission_snapshot: self.admission_snapshot.clone(),
                    discord_reservation: self.discord_reservation.clone(),
                    fixed_point_admission_snapshot,
                    interrupt,
                    applied_interrupt,
                    snapshot,
                },
                state,
            )),
            Err(failure) => {
                interrupt.deactivate_production_generation_v2(production_generation);
                let error = failure.error();
                let (lifecycle, permit, proof) = failure.into_parts();
                self.permit = Some(permit);
                self.coordinator = Some(RuntimeGatewayCoordinatorOwnerV2 {
                    lifecycle,
                    interrupt,
                    applied_interrupt,
                    snapshot,
                });
                Err(RuntimeGatewayFixedPointAcceptanceFailureV2 {
                    binding: Box::new(self),
                    proof: Box::new(proof),
                    error: RuntimeGatewayFixedPointAcceptanceErrorV2::Worker(error),
                })
            }
        }
    }

    pub(crate) fn invalidate_capability_not_ready_v2(&self) {
        self.invalidate_if_current_v2(RuntimeGatewayInvalidationCauseV2::CapabilityNotReady);
    }

    pub(crate) fn invalidate_protocol_violation_v2(&self) {
        self.invalidate_if_current_v2(RuntimeGatewayInvalidationCauseV2::ProtocolViolation);
    }

    fn pending_section_with_owner_v2<'a>(
        &'a self,
        owner: RuntimeGatewayOwnerRecoveryEvidenceV2<'a>,
    ) -> Result<RuntimeRecoveryPendingGatewaySectionV2<'a>, RuntimeGatewayRecoverySectionErrorV2>
    {
        require_recovery_owner_lifetime_v2(&self.owner_invalidated, &owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        let coordinator = self.coordinator_ref_v2()?;
        coordinator
            .lifecycle()
            .validate_recovery_permit(self.permit_v2())
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let snapshot = self.observer.current_admission_snapshot();
        let (paused_gateway, connection_epoch) = map_paused_connected_observation_v2(
            &self.process_instance_id,
            self.permit_v2().originating_emergency_generation(),
            snapshot,
        )
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        if owner.observation().receipt() != self.permit_v2().owner_receipt()
            || paused_gateway != *self.permit_v2().paused_gateway()
        {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        require_healthy_paused_observer_v2(&self.observer, connection_epoch)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        if self.observer.current_admission_snapshot() != snapshot {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        require_recovery_owner_lifetime_v2(&self.owner_invalidated, &owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        coordinator
            .lifecycle()
            .validate_recovery_permit(self.permit_v2())
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let admission_snapshot = self.admission_snapshot.borrow();
        if *admission_snapshot != snapshot {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        require_recovery_owner_lifetime_v2(&self.owner_invalidated, &owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        coordinator
            .lifecycle()
            .validate_recovery_permit(self.permit_v2())
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        Ok(RuntimeRecoveryPendingGatewaySectionV2 {
            binding: self,
            owner,
            admission_snapshot,
        })
    }

    fn invalidate_if_current_v2(&self, cause: RuntimeGatewayInvalidationCauseV2) {
        if self.coordinator.as_ref().is_some_and(|coordinator| {
            coordinator
                .lifecycle()
                .validate_recovery_permit(self.permit_v2())
                .is_ok()
        }) {
            self.owner_invalidated.store(true, Ordering::Release);
            if let Some(coordinator) = self.coordinator.as_ref() {
                coordinator.interrupt.trip_invalidation(cause);
            }
        }
    }

    fn coordinator_ref_v2(
        &self,
    ) -> Result<&RuntimeGatewayCoordinatorOwnerV2, RuntimeGatewayRecoverySectionErrorV2> {
        let coordinator = self
            .coordinator
            .as_ref()
            .ok_or(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?;
        coordinator
            .require_uninterrupted()
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        Ok(coordinator)
    }
}

fn current_runtime_gateway_coordinator_mut_v2(
    coordinator: &mut Option<RuntimeGatewayCoordinatorOwnerV2>,
) -> Result<&mut RuntimeGatewayCoordinatorOwnerV2, RuntimeGatewayRecoverySectionErrorV2> {
    let coordinator = coordinator
        .as_mut()
        .ok_or(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?;
    coordinator.reconcile_interrupt();
    coordinator
        .require_uninterrupted()
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
    Ok(coordinator)
}

#[cfg(test)]
impl RuntimeRecoveryPendingGatewayBindingV2 {
    pub(crate) fn successor_for_stale_drop_test_v2(
        &mut self,
    ) -> Result<(), RuntimeGatewayRecoverySectionErrorV2> {
        let snapshot = self.observer.current_admission_snapshot();
        let permit = self
            .permit
            .as_ref()
            .ok_or(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?;
        let permit_generation = permit.coordinator_generation();
        let previous_registry = permit.registry_evidence().empty_observation();
        let owner_receipt = permit.owner_receipt().clone();
        let readiness = permit.readiness().clone();
        let coordinator = current_runtime_gateway_coordinator_mut_v2(&mut self.coordinator)?;
        coordinator
            .lifecycle()
            .validate_recovery_permit(permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let emergency = coordinator
            .lifecycle
            .invalidate(
                permit_generation,
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            )
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        coordinator.publish_snapshot();
        let (paused_gateway, connection_epoch) = map_paused_connected_observation_v2(
            &self.process_instance_id,
            emergency.generation(),
            snapshot,
        )
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        require_healthy_paused_observer_v2(&self.observer, connection_epoch)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        if self.observer.current_admission_snapshot() != snapshot {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        let registry =
            automation_runtime_worker::accept_runtime_registry_recovery_empty_observation_v2(
                previous_registry.process_instance_id().clone(),
                automation_runtime_worker::RuntimeRegistryRecoveryObservationInputV2 {
                    observation_sequence: previous_registry.observation_sequence(),
                    retained_slot_count: previous_registry.retained_slot_count(),
                    retained_empty_tombstone_count: previous_registry
                        .retained_empty_tombstone_count(),
                    staged_route_count: 0,
                    serving_route_count: 0,
                    draining_route_count: 0,
                    sealed_slot_count: 0,
                    active_interaction_count: 0,
                    failed_closed_slot_count: 0,
                    registry_failed_closed: false,
                },
            )
            .map_err(|_| RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?;
        let input = RuntimeClosedRecoveryInputV2::new(
            RuntimeRecoveryIdV2::parse("fedcba9876543210fedcba9876543210")
                .map_err(|_| RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation)?,
            owner_receipt,
            readiness,
            paused_gateway,
            RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry),
        );
        let (_, permit) = coordinator
            .lifecycle
            .begin_recovery(emergency.generation(), input)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        coordinator.publish_snapshot();
        let previous = self
            .permit
            .replace(permit)
            .expect("runtime predecessor recovery permit");
        let predecessor = Self {
            process_instance_id: self.process_instance_id.clone(),
            observer: self.observer.clone(),
            admission_snapshot: self.admission_snapshot.clone(),
            discord_reservation: self.discord_reservation.clone(),
            coordinator: None,
            owner_invalidated: self.owner_invalidated.clone(),
            permit: Some(previous),
        };
        drop(predecessor);
        Ok(())
    }
}

impl Debug for RuntimeRecoveryPendingGatewayBindingV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRecoveryPendingGatewayBindingV2(<redacted>)")
    }
}

impl Drop for RuntimeRecoveryPendingGatewayBindingV2 {
    fn drop(&mut self) {
        self.invalidate_if_current_v2(RuntimeGatewayInvalidationCauseV2::ProtocolViolation);
    }
}

impl RuntimeRecoveryPendingGatewaySectionV2<'_> {
    pub(crate) fn validate_empty_registry_projection_v2(
        &self,
        observation: &RuntimeRegistryRecoveryEmptyObservationV2,
    ) -> Result<(), RuntimeGatewayRecoverySectionErrorV2> {
        if self
            .binding
            .permit_v2()
            .registry_evidence()
            .empty_observation()
            != observation
        {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        self.require_current_v2()
    }

    fn require_current_v2(&self) -> Result<(), RuntimeGatewayRecoverySectionErrorV2> {
        require_recovery_owner_lifetime_v2(&self.binding.owner_invalidated, &self.owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        let coordinator = self.binding.coordinator_ref_v2()?;
        coordinator
            .lifecycle()
            .validate_recovery_permit(self.binding.permit_v2())
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let snapshot = *self.admission_snapshot;
        let (paused_gateway, _) = map_paused_connected_observation_v2(
            &self.binding.process_instance_id,
            self.binding.permit_v2().originating_emergency_generation(),
            snapshot,
        )
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        if self.owner.observation().receipt() != self.binding.permit_v2().owner_receipt()
            || paused_gateway != *self.binding.permit_v2().paused_gateway()
        {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        require_recovery_owner_lifetime_v2(&self.binding.owner_invalidated, &self.owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        coordinator
            .lifecycle()
            .validate_recovery_permit(self.binding.permit_v2())
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)
    }
}

impl RuntimeGatewayOwnerRecoveryEvidenceV2<'_> {
    fn observation(&self) -> &RuntimeGatewayOwnerCurrentObservationV1 {
        match self {
            Self::Prepared(owner) => owner.observation(),
            Self::Committed(owner) => owner.observation(),
        }
    }

    fn is_bound_to_gateway_lifetime_v2(&self, expected: &Arc<AtomicBool>) -> bool {
        match self {
            Self::Prepared(owner) => owner.is_bound_to_gateway_lifetime_v2(expected),
            Self::Committed(owner) => owner.is_bound_to_gateway_lifetime_v2(expected),
        }
    }
}

fn require_recovery_owner_lifetime_v2(
    owner_invalidated: &Arc<AtomicBool>,
    owner: &RuntimeGatewayOwnerRecoveryEvidenceV2<'_>,
) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
    if owner_invalidated.load(Ordering::Acquire)
        || !owner.is_bound_to_gateway_lifetime_v2(owner_invalidated)
        || owner.observation().safety_deadline() <= Instant::now()
    {
        Err(RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
    } else {
        Ok(())
    }
}

fn require_prepared_owner_lifetime_v2(
    owner_invalidated: &Arc<AtomicBool>,
    prepared_owner: &RuntimeGatewayOwnerPreparedClosedRecoveryV2,
) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
    if owner_invalidated.load(Ordering::Acquire)
        || !prepared_owner.is_bound_to_gateway_lifetime_v2(owner_invalidated)
        || prepared_owner.observation().safety_deadline() <= Instant::now()
    {
        Err(RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
    } else {
        Ok(())
    }
}

pub fn compose_runtime_gateway_bootstrap_v1(
    process_instance_id: ProcessInstanceId,
    config: GatewayResourceConfigV1,
) -> Result<RuntimeGatewayBootstrapV1, RuntimeGatewayBootstrapErrorV1> {
    let control_config =
        GatewayControlConfigV3::new(config.command_capacity(), config.lifecycle_capacity())
            .map_err(map_configuration_error)?;
    Ok(compose_with_control_config(
        process_instance_id,
        control_config,
    ))
}

#[cfg(test)]
pub(crate) fn compose_runtime_gateway_section_test_bootstrap_v2(
    process_instance_id: ProcessInstanceId,
) -> RuntimeGatewayBootstrapV1 {
    let (coordinator, interrupt, coordinator_snapshot) = RuntimeGatewayCoordinatorOwnerV2::new();
    let owner_invalidated = Arc::new(AtomicBool::new(false));
    let owner_discord_attachment = Arc::new(Mutex::new(None));
    let (control, runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
        GatewayControlConfigV3::default(),
        GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        RuntimeGatewaySnapshotTestInvalidatorV3,
    );
    let admission_snapshot = control.admission_snapshot_watch();
    let (discord_reservation_publisher, discord_reservation) = watch::channel(
        RuntimeDiscordAdmissionReservationSnapshotV2::unreserved(*admission_snapshot.borrow()),
    );
    let connection_observer = control.connection_observer();
    RuntimeGatewayBootstrapV1 {
        adapter: SharedGatewayControlAdapterV2 {
            process_instance_id,
            control: Some(control),
            connection_observer,
            discord_commands: None,
            admission_snapshot,
            discord_reservation_publisher: Some(discord_reservation_publisher),
            discord_reservation,
        },
        coordinator: Some(coordinator),
        coordinator_snapshot,
        _runtime: Some(SharedGatewayRuntimeHalfV3 { _inner: runtime }),
        owner_invalidator: Some(RuntimeGatewayOwnerInvalidationBridgeV2 {
            interrupt,
            invalidated: owner_invalidated.clone(),
            discord_attachment: owner_discord_attachment.clone(),
        }),
        owner_invalidated,
        owner_discord_attachment,
    }
}

#[cfg(test)]
pub(crate) fn compose_runtime_gateway_section_test_bootstrap_with_capacity_v2(
    process_instance_id: ProcessInstanceId,
    lifecycle_capacity: std::num::NonZeroUsize,
) -> RuntimeGatewayBootstrapV1 {
    compose_with_control_config(
        process_instance_id,
        GatewayControlConfigV3::new(std::num::NonZeroUsize::MIN, lifecycle_capacity)
            .expect("bounded test gateway capacities"),
    )
}

fn compose_with_control_config(
    process_instance_id: ProcessInstanceId,
    config: GatewayControlConfigV3,
) -> RuntimeGatewayBootstrapV1 {
    let (coordinator, interrupt, coordinator_snapshot) = RuntimeGatewayCoordinatorOwnerV2::new();
    let owner_invalidated = Arc::new(AtomicBool::new(false));
    let owner_discord_attachment = Arc::new(Mutex::new(None));
    let invalidation = RuntimeGatewayInvalidationBridgeV2 {
        interrupt: interrupt.clone(),
    };
    let (control, runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
        config,
        GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        invalidation,
    );
    let admission_snapshot = control.admission_snapshot_watch();
    let (discord_reservation_publisher, discord_reservation) = watch::channel(
        RuntimeDiscordAdmissionReservationSnapshotV2::unreserved(*admission_snapshot.borrow()),
    );
    let connection_observer = control.connection_observer();
    RuntimeGatewayBootstrapV1 {
        adapter: SharedGatewayControlAdapterV2 {
            process_instance_id,
            control: Some(control),
            connection_observer,
            discord_commands: None,
            admission_snapshot,
            discord_reservation_publisher: Some(discord_reservation_publisher),
            discord_reservation,
        },
        coordinator: Some(coordinator),
        coordinator_snapshot,
        _runtime: Some(SharedGatewayRuntimeHalfV3 { _inner: runtime }),
        owner_invalidator: Some(RuntimeGatewayOwnerInvalidationBridgeV2 {
            interrupt,
            invalidated: owner_invalidated.clone(),
            discord_attachment: owner_discord_attachment.clone(),
        }),
        owner_invalidated,
        owner_discord_attachment,
    }
}

fn invalidate_gateway_owner_state(
    interrupt: &RuntimeGatewayCoordinatorInterruptHandleV2,
    invalidated: &AtomicBool,
) {
    if invalidated.swap(true, Ordering::AcqRel) {
        return;
    }
    interrupt.trip_invalidation(RuntimeGatewayInvalidationCauseV2::OwnershipUncertain);
}

fn invalidate_closed_lifecycle(
    lifecycle: &mut RuntimeGatewayClosedLifecycleV2,
    cause: RuntimeGatewayInvalidationCauseV2,
) {
    let generation = lifecycle.snapshot().generation();
    let _ = lifecycle.invalidate(generation, cause);
}

fn shutdown_closed_lifecycle(lifecycle: &mut RuntimeGatewayClosedLifecycleV2) {
    let generation = lifecycle.snapshot().generation();
    let _ = lifecycle.shutdown(generation);
}

impl SharedGatewayControlAdapterV2 {
    fn map_paused_connected_observation(
        &self,
        coordinator_generation: automation_runtime_worker::RuntimeGatewayCoordinatorGenerationV2,
        snapshot: GatewayAdmissionSnapshotV3,
    ) -> Result<
        (
            RuntimePausedGatewayObservationV2,
            automation_runtime::GatewayConnectionEpochV3,
        ),
        RuntimeGatewayReadyObservationErrorV1,
    > {
        map_paused_connected_observation_v2(
            &self.process_instance_id,
            coordinator_generation,
            snapshot,
        )
    }

    fn require_healthy_paused_control(
        &self,
        epoch: automation_runtime::GatewayConnectionEpochV3,
    ) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        match self.connection_observer.issue_ready_lease(epoch) {
            Err(GatewayControlTransitionErrorV3::AdmissionPaused) => Ok(()),
            Err(error) => Err(map_transition_error(error)),
            Ok(_) => Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot),
        }
    }

    fn require_discord_pause_reservation_v2(
        &self,
        snapshot: GatewayAdmissionSnapshotV3,
        epoch: GatewayConnectionEpochV3,
    ) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        if self.discord_commands.is_none() {
            return Ok(());
        }
        let reservation = *self.discord_reservation.borrow();
        let Some(identity) = reservation.reservation() else {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        };
        if reservation.admission() != snapshot
            || identity.epoch() != epoch
            || identity.admission_revision() != snapshot.admission_revision()
            || identity.transition_sequence() != snapshot.transition_sequence()
        {
            return Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot);
        }
        Ok(())
    }

    fn observe_current_ready_attestation(
        &self,
    ) -> Result<RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyObservationErrorV1> {
        observe_current_ready_attestation_v2(&self.process_instance_id, &self.connection_observer)
    }
}

fn observe_current_ready_attestation_v2(
    process_instance_id: &ProcessInstanceId,
    observer: &GatewayConnectionObserverV3,
) -> Result<RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyObservationErrorV1> {
    let first_snapshot = observer.current_admission_snapshot();
    let epoch = match first_snapshot.connection() {
        GatewayConnectionStateV3::Connected { epoch, .. } => epoch,
        GatewayConnectionStateV3::Starting | GatewayConnectionStateV3::Disconnected { .. } => {
            return Err(RuntimeGatewayReadyObservationErrorV1::NotConnected)
        }
        GatewayConnectionStateV3::Paused { .. } => {
            return Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        }
        GatewayConnectionStateV3::Draining { .. } => {
            return Err(RuntimeGatewayReadyObservationErrorV1::Draining)
        }
        GatewayConnectionStateV3::Stopped { .. } => {
            return Err(RuntimeGatewayReadyObservationErrorV1::Stopped)
        }
    };
    let lease = observer
        .issue_ready_lease(epoch)
        .map_err(map_transition_error)?;
    if !observer.ready_lease_is_current(&lease) {
        return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotCurrent);
    }
    if !lease.was_explicitly_resumed() {
        return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotExplicitlyResumed);
    }
    let second_snapshot = observer.current_admission_snapshot();
    if first_snapshot != second_snapshot || !observer.ready_lease_is_current(&lease) {
        return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotCurrent);
    }
    let connection_epoch = bounded_non_zero(lease.epoch().get())?;
    let admission_revision = bounded_non_zero(lease.admission_revision().get())?;
    let connected_event_sequence = bounded_sequence(lease.connected_event_sequence().get())?;
    let resume_sequence = bounded_sequence(lease.resume_sequence().get())?;
    let kind = match lease.kind() {
        GatewayReadyKindV3::Ready => RuntimeGatewayReadyKindV2::Ready,
        GatewayReadyKindV3::Resumed => RuntimeGatewayReadyKindV2::Resumed,
    };
    Ok(RuntimeGatewayReadyAttestationV2 {
        process_instance_id: process_instance_id.clone(),
        connection_epoch,
        kind,
        admission_revision,
        connected_event_sequence,
        resume_sequence,
    })
}

fn map_paused_connected_observation_v2(
    process_instance_id: &ProcessInstanceId,
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    snapshot: GatewayAdmissionSnapshotV3,
) -> Result<
    (RuntimePausedGatewayObservationV2, GatewayConnectionEpochV3),
    RuntimeGatewayReadyObservationErrorV1,
> {
    let (epoch, kind) = match snapshot.connection() {
        GatewayConnectionStateV3::Paused {
            connection: GatewayPausedConnectionV3::Connected { epoch, kind },
        } => (epoch, kind),
        GatewayConnectionStateV3::Starting
        | GatewayConnectionStateV3::Disconnected { .. }
        | GatewayConnectionStateV3::Paused {
            connection:
                GatewayPausedConnectionV3::Starting | GatewayPausedConnectionV3::Disconnected { .. },
        } => return Err(RuntimeGatewayReadyObservationErrorV1::NotConnected),
        GatewayConnectionStateV3::Connected { .. } => {
            return Err(RuntimeGatewayReadyObservationErrorV1::AdmissionNotPaused)
        }
        GatewayConnectionStateV3::Draining { .. } => {
            return Err(RuntimeGatewayReadyObservationErrorV1::Draining)
        }
        GatewayConnectionStateV3::Stopped { .. } => {
            return Err(RuntimeGatewayReadyObservationErrorV1::Stopped)
        }
    };
    let kind = match kind {
        GatewayReadyKindV3::Ready => RuntimeGatewayReadyKindV2::Ready,
        GatewayReadyKindV3::Resumed => RuntimeGatewayReadyKindV2::Resumed,
    };
    let transition_sequence = bounded_sequence(snapshot.transition_sequence().get())?;
    let connected_event_sequence = snapshot
        .connected_event_sequence()
        .ok_or(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceSequenceZero)
        .and_then(|sequence| bounded_sequence(sequence.get()))?;
    let last_resume_sequence = snapshot
        .resume_sequence()
        .map(|sequence| bounded_sequence(sequence.get()))
        .transpose()?;
    let sequence = RuntimePausedGatewaySequenceV2::new(
        transition_sequence,
        connected_event_sequence,
        last_resume_sequence,
    )
    .map_err(|_| RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot)?;
    Ok((
        RuntimePausedGatewayObservationV2::new(
            coordinator_generation,
            process_instance_id.clone(),
            bounded_non_zero(epoch.get())?,
            kind,
            bounded_non_zero(snapshot.admission_revision().get())?,
            sequence,
        ),
        epoch,
    ))
}

fn require_healthy_paused_observer_v2(
    observer: &GatewayConnectionObserverV3,
    epoch: GatewayConnectionEpochV3,
) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
    match observer.issue_ready_lease(epoch) {
        Err(GatewayControlTransitionErrorV3::AdmissionPaused) => Ok(()),
        Err(error) => Err(map_transition_error(error)),
        Ok(_) => Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot),
    }
}

fn bounded_non_zero(value: u64) -> Result<NonZeroU64, RuntimeGatewayReadyObservationErrorV1> {
    let value = NonZeroU64::new(value)
        .ok_or(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceSequenceZero)?;
    if value.get() > i64::MAX as u64 {
        return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceOutOfRange);
    }
    Ok(value)
}

fn bounded_sequence(
    value: u64,
) -> Result<RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayReadyObservationErrorV1> {
    bounded_non_zero(value).map(RuntimeGatewayAdmissionSequenceV2::new)
}

fn map_configuration_error(
    error: GatewayControlConfigurationErrorV3,
) -> RuntimeGatewayBootstrapErrorV1 {
    match error {
        GatewayControlConfigurationErrorV3::CommandCapacity => {
            RuntimeGatewayBootstrapErrorV1::CommandCapacity
        }
        GatewayControlConfigurationErrorV3::LifecycleCapacity => {
            RuntimeGatewayBootstrapErrorV1::LifecycleCapacity
        }
    }
}

fn map_transition_error(
    error: GatewayControlTransitionErrorV3,
) -> RuntimeGatewayReadyObservationErrorV1 {
    match error {
        GatewayControlTransitionErrorV3::StaleConnectionEpoch => {
            RuntimeGatewayReadyObservationErrorV1::StaleConnectionEpoch
        }
        GatewayControlTransitionErrorV3::StaleAdmissionSnapshot => {
            RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot
        }
        GatewayControlTransitionErrorV3::ControlOrphaned => {
            RuntimeGatewayReadyObservationErrorV1::ControlOrphaned
        }
        GatewayControlTransitionErrorV3::NotConnected => {
            RuntimeGatewayReadyObservationErrorV1::NotConnected
        }
        GatewayControlTransitionErrorV3::AdmissionPaused => {
            RuntimeGatewayReadyObservationErrorV1::AdmissionPaused
        }
        GatewayControlTransitionErrorV3::AdmissionNotPaused => {
            RuntimeGatewayReadyObservationErrorV1::AdmissionNotPaused
        }
        GatewayControlTransitionErrorV3::Draining => {
            RuntimeGatewayReadyObservationErrorV1::Draining
        }
        GatewayControlTransitionErrorV3::Stopped => RuntimeGatewayReadyObservationErrorV1::Stopped,
        GatewayControlTransitionErrorV3::ConnectionEpochOverflow => {
            RuntimeGatewayReadyObservationErrorV1::ConnectionEpochOverflow
        }
        GatewayControlTransitionErrorV3::AdmissionRevisionOverflow => {
            RuntimeGatewayReadyObservationErrorV1::AdmissionRevisionOverflow
        }
        GatewayControlTransitionErrorV3::AdmissionSequenceOverflow => {
            RuntimeGatewayReadyObservationErrorV1::AdmissionSequenceOverflow
        }
        GatewayControlTransitionErrorV3::LifecycleOverflow => {
            RuntimeGatewayReadyObservationErrorV1::LifecycleOverflow
        }
        GatewayControlTransitionErrorV3::LifecycleClosed => {
            RuntimeGatewayReadyObservationErrorV1::LifecycleClosed
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use automation_runtime::{
        shared_gateway_control_channel_with_policy_and_invalidator_v3, GatewayAdmissionPolicyV3,
        GatewayCommandAckV3, GatewayControlConfigV3, GatewayControlErrorV3,
        GatewayControlTransitionErrorV3, GatewayDisconnectKindV3, GatewayDrainCauseV3,
        GatewayInvalidationSignalV3, GatewayLifecycleEventV3, GatewayPauseTokenV3,
        GatewayReadyKindV3, GatewayRuntimeCommandOutcomeV3, GatewaySynchronousInvalidatorV3,
    };
    use automation_runtime_convergence::ProcessInstanceId;
    use automation_runtime_worker::{
        RuntimeClosedRecoveryAuthorityRevisionV2, RuntimeGatewayClosedSnapshotV2,
        RuntimeGatewayCoordinatorGenerationV2, RuntimeGatewayEmergencyCauseV2,
    };
    use tokio::sync::watch;

    use crate::discord_lifecycle::RuntimeDiscordAdmissionReservationSnapshotV2;
    use crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerEmergencyInvalidatorV1;

    use super::{
        compose_with_control_config, resume_reserved_discord_admission_v2,
        RuntimeGatewayBootstrapV1, RuntimeGatewayCoordinatorInterruptV2,
        RuntimeGatewayCoordinatorMirrorV2, RuntimeGatewayCoordinatorOwnerV2,
        RuntimeGatewayInvalidationBridgeV2, RuntimeGatewayOwnerInvalidationBridgeV2,
        RuntimeGatewayProductionCoordinatorV2, RuntimeGatewayReadyObservationErrorV1,
        SharedGatewayControlAdapterV2, SharedGatewayRuntimeHalfV3,
    };

    struct TestPauseTokenV1(GatewayPauseTokenV3);

    struct PanickingInvalidatorV3;

    impl GatewaySynchronousInvalidatorV3 for PanickingInvalidatorV3 {
        fn invalidate(&self, _signal: GatewayInvalidationSignalV3) {
            panic!("test invalidation failure")
        }
    }

    fn bootstrap() -> RuntimeGatewayBootstrapV1 {
        compose_with_control_config(
            ProcessInstanceId::parse("runtime-process:1").unwrap(),
            GatewayControlConfigV3::default(),
        )
    }

    fn coordinator_generation(value: u64) -> RuntimeGatewayCoordinatorGenerationV2 {
        RuntimeGatewayCoordinatorGenerationV2::new(NonZeroU64::new(value).unwrap())
    }

    fn bootstrap_with_panicking_invalidator() -> RuntimeGatewayBootstrapV1 {
        let (coordinator, interrupt, coordinator_snapshot) =
            RuntimeGatewayCoordinatorOwnerV2::new();
        let owner_invalidated = Arc::new(AtomicBool::new(false));
        let owner_discord_attachment = Arc::new(Mutex::new(None));
        let (control, runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
            PanickingInvalidatorV3,
        );
        let admission_snapshot = control.admission_snapshot_watch();
        let (discord_reservation_publisher, discord_reservation) = watch::channel(
            RuntimeDiscordAdmissionReservationSnapshotV2::unreserved(*admission_snapshot.borrow()),
        );
        let connection_observer = control.connection_observer();
        RuntimeGatewayBootstrapV1 {
            adapter: SharedGatewayControlAdapterV2 {
                process_instance_id: ProcessInstanceId::parse("runtime-process:1").unwrap(),
                control: Some(control),
                connection_observer,
                discord_commands: None,
                admission_snapshot,
                discord_reservation_publisher: Some(discord_reservation_publisher),
                discord_reservation,
            },
            coordinator: Some(coordinator),
            coordinator_snapshot,
            _runtime: Some(SharedGatewayRuntimeHalfV3 { _inner: runtime }),
            owner_invalidator: Some(RuntimeGatewayOwnerInvalidationBridgeV2 {
                interrupt,
                invalidated: owner_invalidated.clone(),
                discord_attachment: owner_discord_attachment.clone(),
            }),
            owner_invalidated,
            owner_discord_attachment,
        }
    }

    fn connect(bootstrap: &mut RuntimeGatewayBootstrapV1, kind: GatewayReadyKindV3) -> u64 {
        bootstrap
            ._runtime
            .as_mut()
            .expect("gateway runtime half")
            ._inner
            .mark_connected(kind)
            .unwrap()
            .get()
    }

    async fn pause(bootstrap: &mut RuntimeGatewayBootstrapV1) -> TestPauseTokenV1 {
        let control = bootstrap
            .adapter
            .control
            .as_ref()
            .expect("gateway control half");
        let runtime = &mut bootstrap
            ._runtime
            .as_mut()
            .expect("gateway runtime half")
            ._inner;
        let (acknowledgement, outcome) =
            tokio::join!(control.pause_admission(), runtime.process_next_command());
        let acknowledgement = acknowledgement.unwrap();
        assert_eq!(
            outcome,
            GatewayRuntimeCommandOutcomeV3::Applied(acknowledgement.clone())
        );
        match acknowledgement {
            GatewayCommandAckV3::Paused { resume_token, .. } => TestPauseTokenV1(resume_token),
            GatewayCommandAckV3::AdmissionResumed { .. } | GatewayCommandAckV3::Draining { .. } => {
                panic!("unexpected gateway acknowledgement")
            }
        }
    }

    async fn connect_and_pause_with_drained_lifecycle(
        bootstrap: &mut RuntimeGatewayBootstrapV1,
    ) -> TestPauseTokenV1 {
        assert_eq!(
            bootstrap
                .adapter
                .control
                .as_mut()
                .expect("gateway control half")
                .next_lifecycle()
                .await,
            Some(GatewayLifecycleEventV3::Starting)
        );
        connect(bootstrap, GatewayReadyKindV3::Ready);
        assert!(matches!(
            bootstrap
                .adapter
                .control
                .as_mut()
                .expect("gateway control half")
                .next_lifecycle()
                .await,
            Some(GatewayLifecycleEventV3::Connected { paused: true, .. })
        ));
        let token = pause(bootstrap).await;
        assert!(matches!(
            bootstrap
                .adapter
                .control
                .as_mut()
                .expect("gateway control half")
                .next_lifecycle()
                .await,
            Some(GatewayLifecycleEventV3::Paused { .. })
        ));
        token
    }

    async fn resume(
        bootstrap: &mut RuntimeGatewayBootstrapV1,
        token: &TestPauseTokenV1,
    ) -> Result<GatewayCommandAckV3, GatewayControlErrorV3> {
        let control = bootstrap
            .adapter
            .control
            .as_ref()
            .expect("gateway control half");
        let runtime = &mut bootstrap
            ._runtime
            .as_mut()
            .expect("gateway runtime half")
            ._inner;
        let (acknowledgement, outcome) = tokio::join!(
            control.resume_admission(&token.0),
            runtime.process_next_command()
        );
        match &acknowledgement {
            Ok(acknowledgement) => assert_eq!(
                outcome,
                GatewayRuntimeCommandOutcomeV3::Applied(acknowledgement.clone())
            ),
            Err(GatewayControlErrorV3::Transition(error)) => {
                assert_eq!(outcome, GatewayRuntimeCommandOutcomeV3::Rejected(*error))
            }
            Err(GatewayControlErrorV3::RuntimeStopped)
            | Err(GatewayControlErrorV3::AcknowledgementLost) => {
                panic!("unexpected gateway control failure")
            }
        }
        acknowledgement
    }

    fn disconnect(bootstrap: &mut RuntimeGatewayBootstrapV1) {
        bootstrap
            ._runtime
            .as_mut()
            .expect("gateway runtime half")
            ._inner
            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
            .unwrap();
    }

    #[tokio::test]
    async fn shutdown_linearized_before_resume_claim_preserves_the_pause_token() {
        let mut bootstrap = bootstrap();
        let token = connect_and_pause_with_drained_lifecycle(&mut bootstrap).await;
        let paused_admission = bootstrap
            .adapter
            .connection_observer
            .current_admission_snapshot();
        let expected =
            crate::discord_lifecycle::RuntimeDiscordPauseReservationIdentityV2::from_token(
                &token.0,
                paused_admission,
            )
            .unwrap();
        let mut pause_token = Some(token.0);
        let generation = coordinator_generation(2);
        let coordinator = bootstrap.coordinator_snapshot.interrupt.clone();
        coordinator.synchronize_generation_v2(generation);
        let shutdown = bootstrap.shutdown_handle_v1().enter_shutdown();
        assert!(matches!(
            shutdown,
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
        let (observation, _) = watch::channel(None);
        let (lifecycle_drained, _) = watch::channel(1);
        let mut lifecycle_sequence = 1;
        let reservation = RuntimeDiscordAdmissionReservationSnapshotV2::reserved(
            paused_admission,
            pause_token.as_ref().unwrap(),
        )
        .unwrap();
        let (discord_reservation, _) = watch::channel(reservation);
        let outcome = resume_reserved_discord_admission_v2(
            bootstrap
                .adapter
                .control
                .as_mut()
                .expect("gateway control half"),
            &mut pause_token,
            generation,
            expected,
            &coordinator,
            &observation,
            &lifecycle_drained,
            &mut lifecycle_sequence,
            &discord_reservation,
        )
        .await;
        assert_eq!(
            outcome,
            crate::discord::RuntimeDiscordRecoveryResumeControlOutcomeV2::DefinitelyNotApplied
        );
        assert!(pause_token.is_some());
        assert_eq!(
            bootstrap
                .adapter
                .connection_observer
                .current_admission_snapshot(),
            paused_admission
        );
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(10),
            bootstrap
                ._runtime
                .as_mut()
                .expect("gateway runtime half")
                ._inner
                .process_next_command(),
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn successful_resume_advances_one_exact_generation_before_ready_observation() {
        let mut bootstrap = bootstrap();
        let token = connect_and_pause_with_drained_lifecycle(&mut bootstrap).await;
        let paused_admission = bootstrap
            .adapter
            .connection_observer
            .current_admission_snapshot();
        let expected =
            crate::discord_lifecycle::RuntimeDiscordPauseReservationIdentityV2::from_token(
                &token.0,
                paused_admission,
            )
            .unwrap();
        let mut pause_token = Some(token.0);
        let predecessor = coordinator_generation(2);
        let successor = coordinator_generation(3);
        let coordinator = bootstrap.coordinator_snapshot.interrupt.clone();
        coordinator.synchronize_generation_v2(predecessor);
        assert!(coordinator.activate_production_generation_v2(predecessor));
        let reservation = RuntimeDiscordAdmissionReservationSnapshotV2::reserved(
            paused_admission,
            pause_token.as_ref().unwrap(),
        )
        .unwrap();
        let (discord_reservation, discord_reservation_observer) = watch::channel(reservation);
        let (snapshot, _) = watch::channel(RuntimeGatewayCoordinatorMirrorV2 {
            snapshot: RuntimeGatewayClosedSnapshotV2::RecoveryPending {
                generation: predecessor,
                recovery_id: automation_runtime_controller::RuntimeRecoveryIdV2::parse(
                    "0123456789abcdef0123456789abcdef",
                )
                .unwrap(),
                authority_revision: RuntimeClosedRecoveryAuthorityRevisionV2::FIRST,
            },
            applied_interrupt: RuntimeGatewayCoordinatorInterruptV2::None,
        });
        let production = RuntimeGatewayProductionCoordinatorV2 {
            process_instance_id: ProcessInstanceId::parse("runtime-process:1").unwrap(),
            observer: bootstrap.adapter.connection_observer.clone(),
            admission_snapshot: bootstrap.adapter.admission_snapshot.clone(),
            discord_reservation: discord_reservation_observer,
            fixed_point_admission_snapshot: paused_admission,
            interrupt: coordinator.clone(),
            applied_interrupt: RuntimeGatewayCoordinatorInterruptV2::None,
            snapshot,
        };
        assert_eq!(
            production
                .recovery_resume_successor_generation_v2(predecessor)
                .unwrap(),
            successor
        );
        let (observation, _) = watch::channel(None);
        let (lifecycle_drained, _) = watch::channel(1);
        let mut lifecycle_sequence = 1;
        let (outcome, runtime_outcome) = tokio::join!(
            resume_reserved_discord_admission_v2(
                bootstrap
                    .adapter
                    .control
                    .as_mut()
                    .expect("gateway control half"),
                &mut pause_token,
                predecessor,
                expected,
                &coordinator,
                &observation,
                &lifecycle_drained,
                &mut lifecycle_sequence,
                &discord_reservation,
            ),
            bootstrap
                ._runtime
                .as_mut()
                .expect("gateway runtime half")
                ._inner
                .process_next_command(),
        );
        let crate::discord::RuntimeDiscordRecoveryResumeControlOutcomeV2::Applied(evidence) =
            outcome
        else {
            panic!("recovery resume must be applied")
        };
        assert_eq!(evidence.coordinator_generation_v2(), predecessor);
        assert!(matches!(
            runtime_outcome,
            GatewayRuntimeCommandOutcomeV3::Applied(GatewayCommandAckV3::AdmissionResumed { .. })
        ));
        assert_eq!(coordinator.current_generation_v2(), successor);
        assert!(pause_token.is_none());
        assert_eq!(
            production.observe_exact_current_ready_attestation_v2(predecessor),
            Err(RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot)
        );
        let ready = production
            .observe_exact_current_ready_attestation_v2(successor)
            .unwrap();
        assert_eq!(ready.connection_epoch.get(), expected.epoch().get());
        assert!(ready.was_explicitly_resumed());
    }

    #[test]
    fn resume_claim_linearized_before_shutdown_can_complete_its_exact_successor() {
        let (_, coordinator, _) = RuntimeGatewayCoordinatorOwnerV2::new();
        let predecessor = coordinator_generation(2);
        let successor = coordinator_generation(3);
        coordinator.synchronize_generation_v2(predecessor);
        assert!(coordinator.activate_production_generation_v2(predecessor));
        assert!(coordinator.claim_recovery_resume_v2(predecessor));
        coordinator.trip_shutdown();
        assert_eq!(
            coordinator.complete_recovery_resume_v2(predecessor),
            Some(successor)
        );
        assert_eq!(coordinator.current_generation_v2(), successor);
        assert_eq!(
            coordinator.current(),
            RuntimeGatewayCoordinatorInterruptV2::Shutdown
        );
    }

    #[test]
    fn late_snapshot_publication_never_regresses_a_resumed_successor() {
        let (_, coordinator, _) = RuntimeGatewayCoordinatorOwnerV2::new();
        let predecessor = coordinator_generation(2);
        let successor = coordinator_generation(3);
        coordinator.synchronize_generation_v2(predecessor);
        assert!(coordinator.activate_production_generation_v2(predecessor));
        assert!(coordinator.claim_recovery_resume_v2(predecessor));
        assert_eq!(
            coordinator.complete_recovery_resume_v2(predecessor),
            Some(successor)
        );
        coordinator.synchronize_generation_v2(predecessor);
        assert_eq!(coordinator.current_generation_v2(), successor);
        assert_eq!(
            coordinator.current(),
            RuntimeGatewayCoordinatorInterruptV2::ProtocolViolation
        );
    }

    #[test]
    fn invalidation_bridge_maps_every_signal_and_shutdown_is_terminal() {
        for (signal, cause) in [
            (
                GatewayInvalidationSignalV3::AdmissionPaused,
                RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            ),
            (
                GatewayInvalidationSignalV3::Disconnected(GatewayDisconnectKindV3::Close),
                RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
            ),
            (
                GatewayInvalidationSignalV3::ControlOrphaned,
                RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::ControlOrphaned),
                RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::LifecycleClosed),
                RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::ConnectionEpochOverflow),
                RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
            ),
            (
                GatewayInvalidationSignalV3::Draining(
                    GatewayDrainCauseV3::AdmissionRevisionOverflow,
                ),
                RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
            ),
            (
                GatewayInvalidationSignalV3::Draining(
                    GatewayDrainCauseV3::AdmissionSequenceOverflow,
                ),
                RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::LifecycleOverflow),
                RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            ),
            (
                GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::RuntimeFailure),
                RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            ),
        ]
        .into_iter()
        {
            let (mut coordinator, interrupt, snapshot) = RuntimeGatewayCoordinatorOwnerV2::new();
            let bridge = RuntimeGatewayInvalidationBridgeV2 { interrupt };
            bridge.invalidate(signal);
            coordinator.reconcile_interrupt();
            assert_eq!(
                snapshot.effective_snapshot(),
                RuntimeGatewayClosedSnapshotV2::Emergency {
                    generation:
                        automation_runtime_worker::RuntimeGatewayCoordinatorGenerationV2::new(
                            NonZeroU64::new(2).unwrap(),
                        ),
                    cause,
                }
            );
        }
        let (mut coordinator, interrupt, snapshot) = RuntimeGatewayCoordinatorOwnerV2::new();
        let bridge = RuntimeGatewayInvalidationBridgeV2 {
            interrupt: interrupt.clone(),
        };
        bridge.invalidate(GatewayInvalidationSignalV3::AdmissionPaused);
        bridge.invalidate(GatewayInvalidationSignalV3::Draining(
            GatewayDrainCauseV3::Commanded,
        ));
        coordinator.reconcile_interrupt();
        let shutdown = snapshot.effective_snapshot();
        assert!(matches!(
            shutdown,
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
        bridge.invalidate(GatewayInvalidationSignalV3::Stopped(
            GatewayDrainCauseV3::RuntimeFailure,
        ));
        coordinator.reconcile_interrupt();
        assert_eq!(snapshot.effective_snapshot(), shutdown);

        let (mut stopped, interrupt, snapshot) = RuntimeGatewayCoordinatorOwnerV2::new();
        RuntimeGatewayInvalidationBridgeV2 { interrupt }.invalidate(
            GatewayInvalidationSignalV3::Stopped(GatewayDrainCauseV3::RuntimeFailure),
        );
        stopped.reconcile_interrupt();
        assert!(matches!(
            snapshot.effective_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
    }

    #[test]
    fn composition_starts_closed_and_cannot_observe_ready_evidence() {
        let bootstrap = bootstrap();
        assert!(matches!(
            bootstrap.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                cause: RuntimeGatewayEmergencyCauseV2::Starting,
                ..
            }
        ));
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );
        assert_eq!(
            bootstrap.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::NotConnected)
        );
    }

    #[test]
    fn owner_invalidation_is_one_way_and_blocks_ready_observation() {
        let mut bootstrap = bootstrap();
        let invalidator = bootstrap.owner_invalidator.take().unwrap();
        invalidator.invalidate_gateway_ownership();
        invalidator.invalidate_gateway_ownership();

        assert!(matches!(
            bootstrap.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                generation,
                cause: RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
            } if generation.get() == 2
        ));
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
        );
        assert_eq!(
            bootstrap.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
        );
    }

    #[test]
    fn connect_remains_paused_until_an_explicit_resume() {
        let mut bootstrap = bootstrap();
        assert_eq!(connect(&mut bootstrap, GatewayReadyKindV3::Ready), 1);
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );
        let paused = bootstrap.observe_paused_connected_gateway_v2().unwrap();
        assert_eq!(paused.coordinator_generation().get(), 1);
        assert_eq!(paused.process_instance_id().as_str(), "runtime-process:1");
        assert_eq!(paused.connection_epoch().get(), 1);
        assert_eq!(paused.admission_revision().get(), 1);
        assert_eq!(paused.transition_sequence().get(), 1);
        assert_eq!(paused.connected_event_sequence().get(), 1);
        assert_eq!(paused.last_resume_sequence(), None);
    }

    #[tokio::test]
    async fn repeated_pause_and_prior_resume_are_preserved_in_exact_observation() {
        let mut bootstrap = bootstrap();
        connect(&mut bootstrap, GatewayReadyKindV3::Ready);
        let first = pause(&mut bootstrap).await;
        let first_observation = bootstrap.observe_paused_connected_gateway_v2().unwrap();
        assert_eq!(first_observation.admission_revision().get(), 2);
        assert_eq!(first_observation.transition_sequence().get(), 2);
        assert_eq!(first_observation.last_resume_sequence(), None);

        resume(&mut bootstrap, &first).await.unwrap();
        assert_eq!(
            bootstrap.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionNotPaused)
        );
        let _second = pause(&mut bootstrap).await;
        let second_observation = bootstrap.observe_paused_connected_gateway_v2().unwrap();
        assert_eq!(second_observation.admission_revision().get(), 3);
        assert_eq!(second_observation.transition_sequence().get(), 4);
        assert_eq!(second_observation.last_resume_sequence().unwrap().get(), 3);
    }

    #[tokio::test]
    async fn paused_observation_rejects_an_unhealthy_invalidation_hook() {
        let mut bootstrap = bootstrap_with_panicking_invalidator();
        connect(&mut bootstrap, GatewayReadyKindV3::Ready);
        let first = pause(&mut bootstrap).await;
        resume(&mut bootstrap, &first).await.unwrap();
        let _second = pause(&mut bootstrap).await;

        assert_eq!(
            bootstrap.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::ControlOrphaned)
        );
    }

    #[tokio::test]
    async fn explicit_resume_maps_exact_ready_evidence_without_opening_worker_state() {
        let mut bootstrap = bootstrap();
        assert_eq!(connect(&mut bootstrap, GatewayReadyKindV3::Ready), 1);
        let token = pause(&mut bootstrap).await;
        assert!(matches!(
            resume(&mut bootstrap, &token).await.unwrap(),
            GatewayCommandAckV3::AdmissionResumed { .. }
        ));
        let observed = bootstrap.observe_current_ready_attestation().unwrap();
        assert_eq!(observed.process_instance_id.as_str(), "runtime-process:1");
        assert_eq!(observed.connection_epoch.get(), 1);
        assert_eq!(observed.admission_revision.get(), 2);
        assert_eq!(observed.connected_event_sequence.get(), 1);
        assert_eq!(observed.resume_sequence.get(), 3);
        assert!(observed.was_explicitly_resumed());
        assert!(bootstrap.ready_attestation_is_current(&observed));
        assert!(matches!(
            bootstrap.closed_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                cause: RuntimeGatewayEmergencyCauseV2::Starting,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn disconnect_invalidates_ready_evidence_and_requires_a_new_resume() {
        let mut bootstrap = bootstrap();
        connect(&mut bootstrap, GatewayReadyKindV3::Ready);
        let token = pause(&mut bootstrap).await;
        resume(&mut bootstrap, &token).await.unwrap();
        let previous = bootstrap.observe_current_ready_attestation().unwrap();
        let previous_revision = previous.admission_revision.get();
        disconnect(&mut bootstrap);
        assert!(!bootstrap.ready_attestation_is_current(&previous));
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );
        assert_eq!(
            bootstrap.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::NotConnected)
        );
        let closed = bootstrap.closed_snapshot();
        assert_eq!(closed.generation().get(), 2);
        assert!(matches!(
            closed,
            RuntimeGatewayClosedSnapshotV2::Emergency {
                cause: RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
                ..
            }
        ));
        assert_eq!(connect(&mut bootstrap, GatewayReadyKindV3::Ready), 2);
        assert_eq!(
            bootstrap.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );
        assert!(
            bootstrap
                .adapter
                .connection_observer
                .current_admission_snapshot()
                .admission_revision()
                .get()
                > previous_revision
        );
    }

    #[tokio::test]
    async fn resumed_gateway_kind_is_preserved_literally() {
        let mut bootstrap = bootstrap();
        connect(&mut bootstrap, GatewayReadyKindV3::Resumed);
        let token = pause(&mut bootstrap).await;
        resume(&mut bootstrap, &token).await.unwrap();
        assert_eq!(
            bootstrap.observe_current_ready_attestation().unwrap().kind,
            automation_runtime_controller::RuntimeGatewayReadyKindV2::Resumed
        );
    }

    #[tokio::test]
    async fn predecessor_pause_token_cannot_resume_a_successor_connection() {
        let mut bootstrap = bootstrap();
        connect(&mut bootstrap, GatewayReadyKindV3::Ready);
        let predecessor = pause(&mut bootstrap).await;
        resume(&mut bootstrap, &predecessor).await.unwrap();
        disconnect(&mut bootstrap);
        connect(&mut bootstrap, GatewayReadyKindV3::Ready);
        let successor = pause(&mut bootstrap).await;
        assert_eq!(
            resume(&mut bootstrap, &predecessor).await,
            Err(GatewayControlErrorV3::Transition(
                GatewayControlTransitionErrorV3::StaleConnectionEpoch
            ))
        );
        resume(&mut bootstrap, &successor).await.unwrap();
        assert_eq!(
            bootstrap
                .observe_current_ready_attestation()
                .unwrap()
                .connection_epoch
                .get(),
            2
        );
    }

    #[tokio::test]
    async fn dropping_control_owner_forces_the_runtime_half_closed() {
        let RuntimeGatewayBootstrapV1 {
            adapter,
            coordinator_snapshot,
            mut _runtime,
            ..
        } = bootstrap();
        drop(adapter);
        assert!(matches!(
            coordinator_snapshot.effective_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                cause: RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
                ..
            }
        ));
        let runtime = &mut _runtime.as_mut().expect("gateway runtime half")._inner;
        assert_eq!(
            runtime.process_next_command().await,
            GatewayRuntimeCommandOutcomeV3::ControlOrphaned
        );
        assert!(matches!(
            runtime.current_connection(),
            automation_runtime::GatewayConnectionStateV3::Draining { .. }
        ));
        drop(_runtime);
        assert!(matches!(
            coordinator_snapshot.effective_snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
    }
}
