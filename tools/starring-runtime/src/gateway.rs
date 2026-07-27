use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

#[cfg(test)]
use automation_runtime::GatewayCommandAckV3;
use automation_runtime::{
    shared_gateway_control_channel_with_policy_and_invalidator_v3, GatewayAdmissionPolicyV3,
    GatewayAdmissionSnapshotV3, GatewayConnectionEpochV3, GatewayConnectionObserverV3,
    GatewayConnectionStateV3, GatewayControlConfigV3, GatewayControlConfigurationErrorV3,
    GatewayControlTransitionErrorV3, GatewayDrainCauseV3, GatewayInvalidationSignalV3,
    GatewayLifecycleEventV3, GatewayPausedConnectionV3, GatewayReadyKindV3,
    GatewaySynchronousInvalidatorV3, SharedGatewayControlV3, SharedGatewayRuntimeControlV3,
};
use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayReadyAttestationV2,
    RuntimeGatewayReadyKindV2, RuntimeRecoveryIdV2,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    RuntimeAcceptedGatewayOwnerReceiptV1, RuntimeAcceptedStartupRecoveryOutcomeV2,
    RuntimeAuthorizedStartupRecoveryIterationV2, RuntimeAuthorizedStartupRecoveryObservationV2,
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimeCompletedStartupRecoveryObservationV2, RuntimeGatewayClosedLifecycleV2,
    RuntimeGatewayClosedSnapshotV2, RuntimeGatewayClosedTransitionErrorV2,
    RuntimeGatewayCoordinatorGenerationV2, RuntimeGatewayEmergencyCauseV2,
    RuntimeGatewayInvalidationCauseV2, RuntimeGatewayOwnerLeasePortV1,
    RuntimePausedGatewayObservationV2, RuntimePausedGatewaySequenceV2,
    RuntimeRegistryRecoveryEmptyObservationV2, RuntimeStartupRecoveryFixedPointProofV2,
};
#[cfg(test)]
use automation_runtime_worker::{
    RuntimeAcceptedStartupRecoveryExecutionOutcomeV2, RuntimeAuthorizedStartupRecoveryExecutionV2,
    RuntimeCompletedStartupRecoveryExecutionV2, RuntimeStartupRecoveryContinuationV2,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{sleep_until, timeout_at, Instant as TokioInstant};

use crate::closed_recovery::RuntimeClosedRecoveryTransitionAuthorityV2;
#[cfg(test)]
use crate::discord::RuntimeDiscordGatewayDriverV1;
use crate::discord::{
    prepare_twilight_runtime_discord_gateway_driver_v1, start_runtime_discord_gateway_v1,
    RuntimeDiscordControlTaskV1, RuntimeDiscordGatewayActorStartV1,
    RuntimeDiscordGatewayStartErrorV1, RuntimeDiscordGatewaySupervisorV1,
};
use crate::gateway_owner_startup_watchdog::{
    start_runtime_gateway_owner_startup_watchdog_v1,
    RuntimeGatewayOwnerClosedRecoveryCommitErrorV2, RuntimeGatewayOwnerClosedRecoverySupervisorV2,
    RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerEmergencyInvalidatorV1,
    RuntimeGatewayOwnerPreparedClosedRecoveryV2, RuntimeGatewayOwnerStartupWatchdogStartContextV1,
};
use crate::registry::RuntimeLockedRegistryEmptyEvidenceV2;
use crate::{
    GatewayResourceConfigV1, RuntimeDiscordBotTokenV1, RuntimeGatewayOwnerStartupWatchdogConfigV1,
    RuntimeGatewayOwnerStartupWatchdogHandleV1, RuntimeGatewayOwnerStartupWatchdogStartErrorV1,
    RuntimeGatewayOwnerStartupWatchdogStartFailureV1,
};

const SUPPORTED_GATEWAY_SHARD_ID: &str = "shard:0";

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
    closed_lifecycle: Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>,
}

struct SharedGatewayRuntimeHalfV3 {
    _inner: SharedGatewayRuntimeControlV3,
}

struct RuntimePreparedDiscordGatewayStartV1 {
    runtime_handle: tokio::runtime::Handle,
    runtime: SharedGatewayRuntimeHalfV3,
    control_task: RuntimeDiscordControlTaskV1,
    lifecycle_drained: watch::Receiver<u64>,
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
    _runtime: Option<SharedGatewayRuntimeHalfV3>,
    owner_invalidator: Option<RuntimeGatewayOwnerInvalidationBridgeV2>,
    owner_invalidated: Arc<AtomicBool>,
    owner_discord_attachment: Arc<Mutex<Option<RuntimeGatewayOwnerDiscordAttachmentV1>>>,
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
    prepared_owner: &'a RuntimeGatewayOwnerPreparedClosedRecoveryV2,
    owner_invalidated: &'a Arc<AtomicBool>,
    coordinator: MutexGuard<'a, RuntimeGatewayClosedLifecycleV2>,
    admission_snapshot: watch::Ref<'a, GatewayAdmissionSnapshotV3>,
    paused_gateway: RuntimePausedGatewayObservationV2,
    connection_epoch: GatewayConnectionEpochV3,
    pending_permit: Option<RuntimeClosedDrainRecoveryPermitV2>,
}

pub(crate) struct RuntimeRecoveryPendingGatewayBindingV2 {
    process_instance_id: ProcessInstanceId,
    observer: GatewayConnectionObserverV3,
    admission_snapshot: watch::Receiver<GatewayAdmissionSnapshotV3>,
    closed_lifecycle: Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>,
    owner_invalidated: Arc<AtomicBool>,
    permit: RuntimeClosedDrainRecoveryPermitV2,
}

pub(crate) struct RuntimeRecoveryPendingGatewaySectionV2<'a> {
    binding: &'a RuntimeRecoveryPendingGatewayBindingV2,
    owner: RuntimeGatewayOwnerRecoveryEvidenceV2<'a>,
    coordinator: MutexGuard<'a, RuntimeGatewayClosedLifecycleV2>,
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

struct RuntimeGatewayInvalidationBridgeV2 {
    closed_lifecycle: Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>,
}

struct RuntimeGatewayOwnerInvalidationBridgeV2 {
    closed_lifecycle: Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>,
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
        invalidate_gateway_owner_state(&self.closed_lifecycle, &self.invalidated);
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
        let mut lifecycle = self
            .closed_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match signal {
            GatewayInvalidationSignalV3::AdmissionPaused => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::CapabilityNotReady,
            ),
            GatewayInvalidationSignalV3::Disconnected(_) => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::TransportDisconnected,
            ),
            GatewayInvalidationSignalV3::Draining(GatewayDrainCauseV3::Commanded)
            | GatewayInvalidationSignalV3::Stopped(_) => shutdown_closed_lifecycle(&mut lifecycle),
            GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::ControlOrphaned | GatewayDrainCauseV3::LifecycleClosed,
            )
            | GatewayInvalidationSignalV3::ControlOrphaned => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::ControlOrphaned,
            ),
            GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::ConnectionEpochOverflow
                | GatewayDrainCauseV3::AdmissionRevisionOverflow
                | GatewayDrainCauseV3::AdmissionSequenceOverflow,
            ) => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            ),
            GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::LifecycleOverflow | GatewayDrainCauseV3::RuntimeFailure,
            ) => invalidate_closed_lifecycle(
                &mut lifecycle,
                RuntimeGatewayInvalidationCauseV2::CapabilityNotReady,
            ),
        }
    }
}

impl RuntimeGatewayBootstrapV1 {
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
    ) -> Result<RuntimeDiscordGatewaySupervisorV1, RuntimeDiscordGatewayStartErrorV1> {
        let driver =
            prepare_twilight_runtime_discord_gateway_driver_v1(token.expose_secret().to_owned());
        let prepared = self
            .prepare_discord_gateway_start_v1(operation_cutoff)
            .await?;
        let mut supervisor = start_runtime_discord_gateway_v1(
            driver,
            RuntimeDiscordGatewayActorStartV1 {
                control: prepared.runtime._inner,
                operation_cutoff,
                shutdown_deadline,
                lifecycle_drained: prepared.lifecycle_drained,
                runtime: prepared.runtime_handle,
                control_task: prepared.control_task,
                stopped_sender: prepared.stopped_sender,
                stopped: prepared.stopped,
            },
        );
        self.attach_discord_supervisor_v1(&supervisor)?;
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
            .prepare_discord_gateway_start_v1(operation_cutoff)
            .await?;
        let mut supervisor = start_runtime_discord_gateway_v1(
            driver,
            RuntimeDiscordGatewayActorStartV1 {
                control: prepared.runtime._inner,
                operation_cutoff,
                shutdown_deadline,
                lifecycle_drained: prepared.lifecycle_drained,
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
    ) -> Result<RuntimePreparedDiscordGatewayStartV1, RuntimeDiscordGatewayStartErrorV1> {
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
        if Instant::now() >= operation_cutoff {
            let _stopped = stopped_sender.send(true);
            return Err(RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed);
        }
        let initial_lifecycle = match self.adapter.control.as_mut() {
            Some(control) if Instant::now() < operation_cutoff => timeout_at(
                TokioInstant::from_std(operation_cutoff),
                control.next_lifecycle(),
            )
            .await
            .ok()
            .flatten(),
            Some(_) | None => None,
        };
        if initial_lifecycle != Some(GatewayLifecycleEventV3::Starting) {
            let _stopped = stopped_sender.send(true);
            return Err(if Instant::now() >= operation_cutoff {
                RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed
            } else {
                RuntimeDiscordGatewayStartErrorV1::RuntimeUnavailable
            });
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
        let (discord_commands, commands) = mpsc::channel(1);
        let (lifecycle_drained_sender, lifecycle_drained) = watch::channel(1);
        let control_task = RuntimeDiscordControlTaskV1::new(runtime_handle.spawn(
            run_runtime_discord_control_v1(control, commands, lifecycle_drained_sender),
        ));
        self.adapter.discord_commands = Some(discord_commands);
        Ok(RuntimePreparedDiscordGatewayStartV1 {
            runtime_handle,
            runtime,
            control_task,
            lifecycle_drained,
            stopped_sender,
            stopped,
        })
    }

    pub fn closed_snapshot(&self) -> RuntimeGatewayClosedSnapshotV2 {
        self.adapter
            .closed_lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
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
        &'a self,
        prepared_owner: &'a RuntimeGatewayOwnerPreparedClosedRecoveryV2,
        expected_paused_gateway: &RuntimePausedGatewayObservationV2,
    ) -> Result<RuntimeEmergencyGatewaySectionV2<'a>, RuntimeGatewayReadyObservationErrorV1> {
        RuntimeEmergencyGatewaySectionV2::acquire(
            &self.adapter,
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
            invalidate_gateway_owner_state(&self.adapter.closed_lifecycle, &self.owner_invalidated);
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
            _runtime,
            owner_invalidated,
            ..
        } = self;
        let section = RuntimeEmergencyGatewaySectionV2::acquire(
            adapter,
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
    lifecycle_drained: watch::Sender<u64>,
) {
    let mut lifecycle_sequence = 1u64;
    loop {
        tokio::select! {
            biased;
            command = commands.recv() => {
                let Some(command) = command else {
                    return;
                };
                match command {
                    RuntimeDiscordControlCommandV1::BeginDrain { response } => {
                        let drained = if control.begin_drain().await.is_ok() {
                            match control.next_lifecycle().await {
                                Some(_) => publish_runtime_lifecycle_drain_v1(
                                    &lifecycle_drained,
                                    &mut lifecycle_sequence,
                                ),
                                None => false,
                            }
                        } else {
                            false
                        };
                        let _response = response.send(drained);
                    }
                    #[cfg(test)]
                    RuntimeDiscordControlCommandV1::OpenAdmission { response } => {
                        let opened = match control.pause_admission().await {
                            Ok(GatewayCommandAckV3::Paused { resume_token, .. }) => {
                                let paused = control.next_lifecycle().await.is_some()
                                    && publish_runtime_lifecycle_drain_v1(
                                        &lifecycle_drained,
                                        &mut lifecycle_sequence,
                                    );
                                paused
                                    && control.resume_admission(&resume_token).await.is_ok()
                                    && control.next_lifecycle().await.is_some()
                                    && publish_runtime_lifecycle_drain_v1(
                                        &lifecycle_drained,
                                        &mut lifecycle_sequence,
                                    )
                            }
                            Ok(
                                GatewayCommandAckV3::AdmissionResumed { .. }
                                | GatewayCommandAckV3::Draining { .. },
                            )
                            | Err(_) => false,
                        };
                        let _response = response.send(opened);
                    }
                }
            }
            lifecycle = control.next_lifecycle() => {
                let Some(_) = lifecycle else {
                    return;
                };
                if !publish_runtime_lifecycle_drain_v1(
                    &lifecycle_drained,
                    &mut lifecycle_sequence,
                ) {
                    return;
                }
            }
        }
    }
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
        owner_invalidated: &'a Arc<AtomicBool>,
        prepared_owner: &'a RuntimeGatewayOwnerPreparedClosedRecoveryV2,
        expected_paused_gateway: &RuntimePausedGatewayObservationV2,
    ) -> Result<Self, RuntimeGatewayReadyObservationErrorV1> {
        require_prepared_owner_lifetime_v2(owner_invalidated, prepared_owner)?;
        let coordinator = gateway
            .closed_lifecycle
            .lock()
            .map_err(|_| RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)?;
        if coordinator.snapshot()
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
            prepared_owner,
            owner_invalidated,
            coordinator,
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
        let (_, permit) = self
            .coordinator
            .begin_recovery(RuntimeGatewayCoordinatorGenerationV2::FIRST, input)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
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
        Ok(RuntimeRecoveryPendingGatewayBindingV2 {
            process_instance_id: self.gateway.process_instance_id.clone(),
            observer: self.gateway.connection_observer.clone(),
            admission_snapshot: self.gateway.admission_snapshot.clone(),
            closed_lifecycle: self.gateway.closed_lifecycle.clone(),
            owner_invalidated: self.owner_invalidated.clone(),
            permit,
        })
    }

    fn require_current_v2(&self) -> Result<(), RuntimeGatewayReadyObservationErrorV1> {
        require_prepared_owner_lifetime_v2(self.owner_invalidated, self.prepared_owner)?;
        if self.coordinator.snapshot()
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
        self.coordinator
            .validate_recovery_permit(permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        require_prepared_owner_lifetime_v2(self.owner_invalidated, self.prepared_owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        if self.prepared_owner.observation().receipt() != permit.owner_receipt()
            || self.paused_gateway != *permit.paused_gateway()
        {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        self.coordinator
            .validate_recovery_permit(permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)
    }
}

impl Drop for RuntimeEmergencyGatewaySectionV2<'_> {
    fn drop(&mut self) {
        let Some(permit) = self.pending_permit.as_ref() else {
            return;
        };
        if self.coordinator.validate_recovery_permit(permit).is_ok() {
            let _ = self.coordinator.invalidate(
                permit.coordinator_generation(),
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            );
        }
    }
}

impl RuntimeRecoveryPendingGatewayBindingV2 {
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
            result = prepared_owner.commit_closed_recovery_in_place_v2(&self.permit) => {
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
            let mut coordinator = self.closed_lifecycle.lock().map_err(|_| {
                RuntimeGatewayRecoverySectionErrorV2::Gateway(
                    RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
                )
            })?;
            let transition = coordinator.refresh_recovery_readiness(&mut self.permit, readiness);
            if transition.is_err()
                && matches!(
                    coordinator.snapshot(),
                    RuntimeGatewayClosedSnapshotV2::Emergency { .. }
                        | RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
                )
            {
                self.owner_invalidated.store(true, Ordering::Release);
            }
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
            let mut coordinator = self.closed_lifecycle.lock().map_err(|_| {
                RuntimeGatewayRecoverySectionErrorV2::Gateway(
                    RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
                )
            })?;
            coordinator.begin_startup_recovery_observation(&mut self.permit, iteration)
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
            let mut coordinator = self.closed_lifecycle.lock().map_err(|_| {
                RuntimeGatewayRecoverySectionErrorV2::Gateway(
                    RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
                )
            })?;
            let transition =
                coordinator.complete_startup_recovery_observation(&mut self.permit, completed);
            if transition.is_err()
                && matches!(
                    coordinator.snapshot(),
                    RuntimeGatewayClosedSnapshotV2::Emergency { .. }
                        | RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
                )
            {
                self.owner_invalidated.store(true, Ordering::Release);
            }
            transition
        }
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        Ok((self, transition))
    }

    #[cfg(test)]
    pub(crate) fn begin_startup_recovery_execution_for_test_v2(
        &mut self,
        committed_owner: &RuntimeGatewayOwnerClosedRecoverySupervisorV2,
        continuation: RuntimeStartupRecoveryContinuationV2,
    ) -> Result<RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeGatewayRecoverySectionErrorV2>
    {
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        let authorization = {
            let mut coordinator = self.closed_lifecycle.lock().map_err(|_| {
                RuntimeGatewayRecoverySectionErrorV2::Gateway(
                    RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
                )
            })?;
            coordinator.begin_startup_recovery_execution(&mut self.permit, continuation)
        }
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        Ok(authorization)
    }

    #[cfg(test)]
    pub(crate) fn complete_startup_recovery_execution_for_test_v2(
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
            let mut coordinator = self.closed_lifecycle.lock().map_err(|_| {
                RuntimeGatewayRecoverySectionErrorV2::Gateway(
                    RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
                )
            })?;
            let transition =
                coordinator.complete_startup_recovery_execution(&mut self.permit, completed);
            if transition.is_err()
                && matches!(
                    coordinator.snapshot(),
                    RuntimeGatewayClosedSnapshotV2::Emergency { .. }
                        | RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
                )
            {
                self.owner_invalidated.store(true, Ordering::Release);
            }
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
            let mut coordinator = self.closed_lifecycle.lock().map_err(|_| {
                RuntimeGatewayRecoverySectionErrorV2::Gateway(
                    RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
                )
            })?;
            let transition = coordinator.validate_startup_recovery_fixed_point(&self.permit, proof);
            if matches!(
                transition,
                Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryFixedPointAuthority)
            ) {
                self.owner_invalidated.store(true, Ordering::Release);
                let _ = coordinator.invalidate(
                    self.permit.coordinator_generation(),
                    RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
                );
            }
            transition
        };
        transition.map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let section = self.committed_pending_section_v2(committed_owner)?;
        drop(section);
        Ok(())
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
        let coordinator = self.closed_lifecycle.lock().map_err(|_| {
            RuntimeGatewayRecoverySectionErrorV2::Gateway(
                RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
            )
        })?;
        coordinator
            .validate_recovery_permit(&self.permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let snapshot = self.observer.current_admission_snapshot();
        let (paused_gateway, connection_epoch) = map_paused_connected_observation_v2(
            &self.process_instance_id,
            self.permit.originating_emergency_generation(),
            snapshot,
        )
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        if owner.observation().receipt() != self.permit.owner_receipt()
            || paused_gateway != *self.permit.paused_gateway()
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
            .validate_recovery_permit(&self.permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let admission_snapshot = self.admission_snapshot.borrow();
        if *admission_snapshot != snapshot {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        require_recovery_owner_lifetime_v2(&self.owner_invalidated, &owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        coordinator
            .validate_recovery_permit(&self.permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        Ok(RuntimeRecoveryPendingGatewaySectionV2 {
            binding: self,
            owner,
            coordinator,
            admission_snapshot,
        })
    }

    fn invalidate_if_current_v2(&self, cause: RuntimeGatewayInvalidationCauseV2) {
        let mut coordinator = match self.closed_lifecycle.lock() {
            Ok(coordinator) => coordinator,
            Err(poisoned) => {
                self.owner_invalidated.store(true, Ordering::Release);
                let mut coordinator = poisoned.into_inner();
                shutdown_closed_lifecycle(&mut coordinator);
                return;
            }
        };
        if coordinator.validate_recovery_permit(&self.permit).is_ok() {
            self.owner_invalidated.store(true, Ordering::Release);
            let _ = coordinator.invalidate(self.permit.coordinator_generation(), cause);
        }
    }
}

#[cfg(test)]
impl RuntimeRecoveryPendingGatewayBindingV2 {
    pub(crate) fn successor_for_stale_drop_test_v2(
        &self,
    ) -> Result<Self, RuntimeGatewayRecoverySectionErrorV2> {
        let snapshot = self.observer.current_admission_snapshot();
        let mut coordinator = self.closed_lifecycle.lock().map_err(|_| {
            RuntimeGatewayRecoverySectionErrorV2::Gateway(
                RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
            )
        })?;
        coordinator
            .validate_recovery_permit(&self.permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let emergency = coordinator
            .invalidate(
                self.permit.coordinator_generation(),
                RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
            )
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
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
        let previous_registry = self.permit.registry_evidence().empty_observation();
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
            self.permit.owner_receipt().clone(),
            self.permit.readiness().clone(),
            paused_gateway,
            RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry),
        );
        let (_, permit) = coordinator
            .begin_recovery(emergency.generation(), input)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        Ok(Self {
            process_instance_id: self.process_instance_id.clone(),
            observer: self.observer.clone(),
            admission_snapshot: self.admission_snapshot.clone(),
            closed_lifecycle: self.closed_lifecycle.clone(),
            owner_invalidated: self.owner_invalidated.clone(),
            permit,
        })
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
        if self.binding.permit.registry_evidence().empty_observation() != observation {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        self.require_current_v2()
    }

    fn require_current_v2(&self) -> Result<(), RuntimeGatewayRecoverySectionErrorV2> {
        require_recovery_owner_lifetime_v2(&self.binding.owner_invalidated, &self.owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        self.coordinator
            .validate_recovery_permit(&self.binding.permit)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Coordinator)?;
        let snapshot = *self.admission_snapshot;
        let (paused_gateway, _) = map_paused_connected_observation_v2(
            &self.binding.process_instance_id,
            self.binding.permit.originating_emergency_generation(),
            snapshot,
        )
        .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        if self.owner.observation().receipt() != self.binding.permit.owner_receipt()
            || paused_gateway != *self.binding.permit.paused_gateway()
        {
            return Err(RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation);
        }
        require_recovery_owner_lifetime_v2(&self.binding.owner_invalidated, &self.owner)
            .map_err(RuntimeGatewayRecoverySectionErrorV2::Gateway)?;
        self.coordinator
            .validate_recovery_permit(&self.binding.permit)
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
    let closed_lifecycle = Arc::new(Mutex::new(RuntimeGatewayClosedLifecycleV2::starting()));
    let owner_invalidated = Arc::new(AtomicBool::new(false));
    let owner_discord_attachment = Arc::new(Mutex::new(None));
    let (control, runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
        GatewayControlConfigV3::default(),
        GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        RuntimeGatewaySnapshotTestInvalidatorV3,
    );
    let admission_snapshot = control.admission_snapshot_watch();
    let connection_observer = control.connection_observer();
    RuntimeGatewayBootstrapV1 {
        adapter: SharedGatewayControlAdapterV2 {
            process_instance_id,
            control: Some(control),
            connection_observer,
            discord_commands: None,
            admission_snapshot,
            closed_lifecycle: closed_lifecycle.clone(),
        },
        _runtime: Some(SharedGatewayRuntimeHalfV3 { _inner: runtime }),
        owner_invalidator: Some(RuntimeGatewayOwnerInvalidationBridgeV2 {
            closed_lifecycle,
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
    let closed_lifecycle = Arc::new(Mutex::new(RuntimeGatewayClosedLifecycleV2::starting()));
    let owner_invalidated = Arc::new(AtomicBool::new(false));
    let owner_discord_attachment = Arc::new(Mutex::new(None));
    let invalidation = RuntimeGatewayInvalidationBridgeV2 {
        closed_lifecycle: closed_lifecycle.clone(),
    };
    let (control, runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
        config,
        GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        invalidation,
    );
    let admission_snapshot = control.admission_snapshot_watch();
    let connection_observer = control.connection_observer();
    RuntimeGatewayBootstrapV1 {
        adapter: SharedGatewayControlAdapterV2 {
            process_instance_id,
            control: Some(control),
            connection_observer,
            discord_commands: None,
            admission_snapshot,
            closed_lifecycle: closed_lifecycle.clone(),
        },
        _runtime: Some(SharedGatewayRuntimeHalfV3 { _inner: runtime }),
        owner_invalidator: Some(RuntimeGatewayOwnerInvalidationBridgeV2 {
            closed_lifecycle,
            invalidated: owner_invalidated.clone(),
            discord_attachment: owner_discord_attachment.clone(),
        }),
        owner_invalidated,
        owner_discord_attachment,
    }
}

fn invalidate_gateway_owner_state(
    closed_lifecycle: &Arc<Mutex<RuntimeGatewayClosedLifecycleV2>>,
    invalidated: &AtomicBool,
) {
    if invalidated.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut lifecycle = closed_lifecycle
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    invalidate_closed_lifecycle(
        &mut lifecycle,
        RuntimeGatewayInvalidationCauseV2::OwnershipUncertain,
    );
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

    fn observe_current_ready_attestation(
        &self,
    ) -> Result<RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyObservationErrorV1> {
        let snapshot = self.connection_observer.current_admission_snapshot();
        let epoch = match snapshot.connection() {
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
        let lease = self
            .connection_observer
            .issue_ready_lease(epoch)
            .map_err(map_transition_error)?;
        if !self.connection_observer.ready_lease_is_current(&lease) {
            return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotCurrent);
        }
        if !lease.was_explicitly_resumed() {
            return Err(RuntimeGatewayReadyObservationErrorV1::ReadyEvidenceNotExplicitlyResumed);
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
            process_instance_id: self.process_instance_id.clone(),
            connection_epoch,
            kind,
            admission_revision,
            connected_event_sequence,
            resume_sequence,
        })
    }
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
        GatewayInvalidationSignalV3, GatewayPauseTokenV3, GatewayReadyKindV3,
        GatewayRuntimeCommandOutcomeV3, GatewaySynchronousInvalidatorV3,
    };
    use automation_runtime_convergence::ProcessInstanceId;
    use automation_runtime_worker::{
        RuntimeGatewayClosedLifecycleV2, RuntimeGatewayClosedSnapshotV2,
        RuntimeGatewayEmergencyCauseV2,
    };

    use crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerEmergencyInvalidatorV1;

    use super::{
        compose_with_control_config, RuntimeGatewayBootstrapV1, RuntimeGatewayInvalidationBridgeV2,
        RuntimeGatewayOwnerInvalidationBridgeV2, RuntimeGatewayReadyObservationErrorV1,
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

    fn bootstrap_with_panicking_invalidator() -> RuntimeGatewayBootstrapV1 {
        let closed_lifecycle = Arc::new(Mutex::new(RuntimeGatewayClosedLifecycleV2::starting()));
        let owner_invalidated = Arc::new(AtomicBool::new(false));
        let owner_discord_attachment = Arc::new(Mutex::new(None));
        let (control, runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
            PanickingInvalidatorV3,
        );
        let admission_snapshot = control.admission_snapshot_watch();
        let connection_observer = control.connection_observer();
        RuntimeGatewayBootstrapV1 {
            adapter: SharedGatewayControlAdapterV2 {
                process_instance_id: ProcessInstanceId::parse("runtime-process:1").unwrap(),
                control: Some(control),
                connection_observer,
                discord_commands: None,
                admission_snapshot,
                closed_lifecycle: closed_lifecycle.clone(),
            },
            _runtime: Some(SharedGatewayRuntimeHalfV3 { _inner: runtime }),
            owner_invalidator: Some(RuntimeGatewayOwnerInvalidationBridgeV2 {
                closed_lifecycle,
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

    #[test]
    fn invalidation_bridge_maps_every_signal_and_shutdown_is_terminal() {
        let closed = Arc::new(Mutex::new(RuntimeGatewayClosedLifecycleV2::starting()));
        let bridge = RuntimeGatewayInvalidationBridgeV2 {
            closed_lifecycle: closed.clone(),
        };
        for (index, (signal, cause)) in [
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
        .enumerate()
        {
            bridge.invalidate(signal);
            assert_eq!(
                closed.lock().unwrap().snapshot(),
                RuntimeGatewayClosedSnapshotV2::Emergency {
                    generation:
                        automation_runtime_worker::RuntimeGatewayCoordinatorGenerationV2::new(
                            NonZeroU64::new(index as u64 + 2).unwrap(),
                        ),
                    cause,
                }
            );
        }
        bridge.invalidate(GatewayInvalidationSignalV3::Draining(
            GatewayDrainCauseV3::Commanded,
        ));
        let shutdown = closed.lock().unwrap().snapshot();
        assert!(matches!(
            shutdown,
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
        bridge.invalidate(GatewayInvalidationSignalV3::Stopped(
            GatewayDrainCauseV3::RuntimeFailure,
        ));
        assert_eq!(closed.lock().unwrap().snapshot(), shutdown);

        let stopped = Arc::new(Mutex::new(RuntimeGatewayClosedLifecycleV2::starting()));
        RuntimeGatewayInvalidationBridgeV2 {
            closed_lifecycle: stopped.clone(),
        }
        .invalidate(GatewayInvalidationSignalV3::Stopped(
            GatewayDrainCauseV3::RuntimeFailure,
        ));
        assert!(matches!(
            stopped.lock().unwrap().snapshot(),
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
            mut _runtime,
            ..
        } = bootstrap();
        let closed = adapter.closed_lifecycle.clone();
        drop(adapter);
        assert!(matches!(
            closed.lock().unwrap().snapshot(),
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
            closed.lock().unwrap().snapshot(),
            RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
        ));
    }
}
