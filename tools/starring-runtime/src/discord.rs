use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::pin::Pin;
use std::time::{Duration, Instant};

use automation_runtime::{
    GatewayAdmissionSnapshotV3, GatewayCommandAckV3, GatewayConnectionStateV3,
    GatewayDisconnectKindV3, GatewayReadyKindV3, GatewayReadyLeaseV3,
    GatewayRuntimeCommandOutcomeV3, SharedGatewayRuntimeControlV3,
};
use automation_runtime_worker::RuntimeGatewayCoordinatorGenerationV2;
use paused_discord_gateway::error::ReceiveMessageErrorType;
use paused_discord_gateway::{
    CloseFrame, Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{AbortHandle, JoinHandle};
use tokio::time::{sleep_until, timeout_at, Instant as TokioInstant};

use crate::discord_lifecycle::{
    RuntimeDiscordActorModeV2, RuntimeDiscordAdmissionReservationSnapshotV2,
    RuntimeDiscordPauseReservationIdentityV2,
};

const DISCORD_SHUTDOWN_ABORT_RESERVE: Duration = Duration::from_millis(25);
const DISCORD_ACTOR_TERMINATION_RESERVE: Duration = Duration::from_millis(100);
const DISCORD_GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const DISCORD_LIFECYCLE_DRAIN_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordGatewaySignalV1 {
    Ready,
    Resumed,
    Close,
    Reconnect,
    SessionInvalidated,
    ReceiveError,
    FatalReceiveError,
    StreamEnded,
    Unrelated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordGatewayTransportStateV1 {
    Unstarted,
    Connecting,
    Active,
    Disconnected,
}

pub(crate) trait RuntimeDiscordGatewayDriverV1: Send + 'static {
    fn transport_state(&self) -> RuntimeDiscordGatewayTransportStateV1;

    fn next_signal(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = RuntimeDiscordGatewaySignalV1> + Send + '_>>;

    fn close_until(&mut self, deadline: Instant)
        -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}

struct TwilightRuntimeDiscordGatewayDriverV1 {
    shard: Shard,
    transport_state: RuntimeDiscordGatewayTransportStateV1,
}

impl TwilightRuntimeDiscordGatewayDriverV1 {
    fn new(token: String) -> Self {
        Self {
            shard: Shard::new(ShardId::ONE, token, Intents::empty()),
            transport_state: RuntimeDiscordGatewayTransportStateV1::Unstarted,
        }
    }
}

impl RuntimeDiscordGatewayDriverV1 for TwilightRuntimeDiscordGatewayDriverV1 {
    fn transport_state(&self) -> RuntimeDiscordGatewayTransportStateV1 {
        self.transport_state
    }

    fn next_signal(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = RuntimeDiscordGatewaySignalV1> + Send + '_>> {
        Box::pin(async move {
            if self.transport_state != RuntimeDiscordGatewayTransportStateV1::Active {
                self.transport_state = RuntimeDiscordGatewayTransportStateV1::Connecting;
            }
            let event_types = EventTypeFlags::READY
                | EventTypeFlags::RESUMED
                | EventTypeFlags::GATEWAY_RECONNECT
                | EventTypeFlags::GATEWAY_INVALIDATE_SESSION;
            let signal = match self.shard.next_event(event_types).await {
                Some(Ok(Event::Ready(_))) => RuntimeDiscordGatewaySignalV1::Ready,
                Some(Ok(Event::Resumed)) => RuntimeDiscordGatewaySignalV1::Resumed,
                Some(Ok(Event::GatewayClose(_))) => RuntimeDiscordGatewaySignalV1::Close,
                Some(Ok(Event::GatewayReconnect)) => RuntimeDiscordGatewaySignalV1::Reconnect,
                Some(Ok(Event::GatewayInvalidateSession(_))) => {
                    RuntimeDiscordGatewaySignalV1::SessionInvalidated
                }
                Some(Ok(_)) => RuntimeDiscordGatewaySignalV1::Unrelated,
                Some(Err(error)) if matches!(error.kind(), ReceiveMessageErrorType::Reconnect) => {
                    RuntimeDiscordGatewaySignalV1::ReceiveError
                }
                Some(Err(_)) => RuntimeDiscordGatewaySignalV1::FatalReceiveError,
                None => RuntimeDiscordGatewaySignalV1::StreamEnded,
            };
            match signal {
                RuntimeDiscordGatewaySignalV1::Ready | RuntimeDiscordGatewaySignalV1::Resumed => {
                    self.transport_state = RuntimeDiscordGatewayTransportStateV1::Active;
                }
                RuntimeDiscordGatewaySignalV1::Close
                | RuntimeDiscordGatewaySignalV1::Reconnect
                | RuntimeDiscordGatewaySignalV1::SessionInvalidated
                | RuntimeDiscordGatewaySignalV1::ReceiveError
                | RuntimeDiscordGatewaySignalV1::StreamEnded => {
                    self.transport_state = RuntimeDiscordGatewayTransportStateV1::Disconnected;
                }
                RuntimeDiscordGatewaySignalV1::FatalReceiveError
                | RuntimeDiscordGatewaySignalV1::Unrelated => {}
            }
            signal
        })
    }

    fn close_until(
        &mut self,
        deadline: Instant,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            self.shard.close(CloseFrame::NORMAL);
            if Instant::now() >= deadline {
                return false;
            }
            loop {
                tokio::select! {
                    biased;
                    _ = sleep_until(TokioInstant::from_std(deadline)) => return false,
                    event = self.shard.next_event(EventTypeFlags::empty()) => {
                        match event {
                            Some(Ok(Event::GatewayClose(_))) | None => {
                                return Instant::now() < deadline;
                            }
                            Some(Ok(_)) => {}
                            Some(Err(_)) => return false,
                        }
                    }
                }
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordGatewayExitV1 {
    Commanded,
    ControlOrphaned,
    StreamEnded,
    RuntimeFailure,
    AdmissionOpened,
    StartDeadlineElapsed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordGatewayCloseOutcomeV1 {
    Confirmed,
    DeadlineElapsed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeDiscordGatewayTerminalV1 {
    exit: RuntimeDiscordGatewayExitV1,
    close: RuntimeDiscordGatewayCloseOutcomeV1,
    control_stopped: bool,
}

impl RuntimeDiscordGatewayTerminalV1 {
    pub(crate) fn exit(self) -> RuntimeDiscordGatewayExitV1 {
        self.exit
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordGatewayStartErrorV1 {
    RuntimeUnavailable,
    RuntimeHalfUnavailable,
    OwnerInvalidated,
    OperationDeadlineElapsed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordGatewayShutdownErrorV1 {
    DeadlineElapsed,
    TaskStopped,
    CloseDeadlineElapsed,
    UnexpectedExit(RuntimeDiscordGatewayTerminalV1),
}

pub(crate) struct RuntimeDiscordGatewaySupervisorV1 {
    terminal: watch::Receiver<Option<RuntimeDiscordGatewayTerminalV1>>,
    stopped: watch::Receiver<bool>,
    start: Option<oneshot::Sender<()>>,
    actor_abort: AbortHandle,
    control_abort: AbortHandle,
    join_task: Option<JoinHandle<bool>>,
    startup_operation_cutoff: Instant,
    process_handoff: Option<oneshot::Sender<RuntimeDiscordProcessHandoffCommandV2>>,
    process_handoff_state: RuntimeDiscordProcessHandoffStateV2,
    drain: Option<oneshot::Sender<RuntimeDiscordDrainCommandV2>>,
    recovery_resume: mpsc::Sender<RuntimeDiscordRecoveryResumeCommandV2>,
    discord_reservation: watch::Receiver<RuntimeDiscordAdmissionReservationSnapshotV2>,
    recovery_resume_observation: watch::Receiver<Option<RuntimeDiscordRecoveryResumeEvidenceV2>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeDiscordProcessHandoffStateV2 {
    NotStarted,
    InFlight,
    Process,
    NotApplied(RuntimeDiscordProcessHandoffFailureV2),
    Indeterminate(RuntimeDiscordProcessHandoffFailureV2),
}

pub(crate) struct RuntimeDiscordProcessSupervisorV2 {
    inner: RuntimeDiscordGatewaySupervisorV1,
    recovery_resume_state: RuntimeDiscordRecoveryResumeStateV2,
    recovery_resume_acknowledgement:
        Option<oneshot::Receiver<RuntimeDiscordRecoveryResumeActorOutcomeV2>>,
}

pub(crate) struct RuntimeDiscordShutdownOnlySupervisorV2 {
    inner: RuntimeDiscordGatewaySupervisorV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordProcessHandoffFailureV2 {
    CommandUnavailable,
    ActorRejected,
    AcknowledgementLost,
    ActorTerminal,
    DeadlineElapsed,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeDiscordProcessHandoffV2 {
    Process(RuntimeDiscordProcessSupervisorV2),
    NotApplied {
        supervisor: RuntimeDiscordGatewaySupervisorV1,
        failure: RuntimeDiscordProcessHandoffFailureV2,
    },
    Indeterminate {
        supervisor: RuntimeDiscordShutdownOnlySupervisorV2,
        failure: RuntimeDiscordProcessHandoffFailureV2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeDiscordActorAcknowledgementV2 {
    Accepted,
    Rejected,
    Lost,
    Terminal,
    DeadlineElapsed,
}

struct RuntimeDiscordProcessHandoffCommandV2 {
    process_generation: NonZeroU64,
    respond: bool,
    response: oneshot::Sender<bool>,
}

struct RuntimeDiscordDrainCommandV2 {
    shutdown_generation: NonZeroU64,
    deadline: Instant,
    response: oneshot::Sender<bool>,
}

struct RuntimeDiscordRecoveryResumeCommandV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    expected: RuntimeDiscordPauseReservationIdentityV2,
    respond: bool,
    response: oneshot::Sender<RuntimeDiscordRecoveryResumeActorOutcomeV2>,
}

pub(crate) struct RuntimeDiscordReservedResumeRequestV2 {
    pub(crate) coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    pub(crate) expected: RuntimeDiscordPauseReservationIdentityV2,
    pub(crate) observation: watch::Sender<Option<RuntimeDiscordRecoveryResumeEvidenceV2>>,
    pub(crate) response: oneshot::Sender<RuntimeDiscordRecoveryResumeControlOutcomeV2>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordRecoveryResumeFailureV2 {
    CommandUnavailable,
    ActorRejected,
    AcknowledgementLost,
    ActorTerminal,
    DeadlineElapsed,
}

#[cfg(test)]
pub(crate) enum RuntimeDiscordRecoveryResumeOwnershipV2 {
    Process(RuntimeDiscordProcessSupervisorV2),
    ShutdownOnly(RuntimeDiscordShutdownOnlySupervisorV2),
}

#[cfg(test)]
pub(crate) enum RuntimeDiscordRecoveryResumeV2 {
    Applied {
        supervisor: RuntimeDiscordProcessSupervisorV2,
        evidence: RuntimeDiscordRecoveryResumeEvidenceV2,
    },
    DefinitelyNotApplied {
        ownership: RuntimeDiscordRecoveryResumeOwnershipV2,
        failure: RuntimeDiscordRecoveryResumeFailureV2,
    },
    Indeterminate {
        supervisor: RuntimeDiscordShutdownOnlySupervisorV2,
        failure: RuntimeDiscordRecoveryResumeFailureV2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordRecoveryResumeAttemptV2 {
    Applied(RuntimeDiscordRecoveryResumeEvidenceV2),
    DefinitelyNotApplied(RuntimeDiscordRecoveryResumeFailureV2),
    Indeterminate(RuntimeDiscordRecoveryResumeFailureV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeDiscordRecoveryResumeStateV2 {
    Idle,
    InFlight {
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
    },
    Applied(RuntimeDiscordRecoveryResumeEvidenceV2),
    DefinitelyNotApplied(RuntimeDiscordRecoveryResumeFailureV2),
    Indeterminate(RuntimeDiscordRecoveryResumeFailureV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeDiscordRecoveryResumeActorOutcomeV2 {
    Applied(RuntimeDiscordRecoveryResumeEvidenceV2),
    DefinitelyNotApplied,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeDiscordRecoveryResumeControlOutcomeV2 {
    Applied(RuntimeDiscordRecoveryResumeEvidenceV2),
    DefinitelyNotApplied,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeDiscordRecoveryResumeEvidenceV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    expected: RuntimeDiscordPauseReservationIdentityV2,
    admission: GatewayAdmissionSnapshotV3,
    ready: GatewayReadyLeaseV3,
}

impl RuntimeDiscordRecoveryResumeEvidenceV2 {
    pub(crate) fn from_exact_snapshot_v2(
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
        admission: GatewayAdmissionSnapshotV3,
        ready: GatewayReadyLeaseV3,
    ) -> Option<Self> {
        let GatewayConnectionStateV3::Connected { epoch, kind } = admission.connection() else {
            return None;
        };
        if epoch != expected.epoch()
            || kind != ready.kind()
            || admission.admission_revision() != ready.admission_revision()
            || admission.admission_revision() != expected.admission_revision()
            || admission.connected_event_sequence() != Some(ready.connected_event_sequence())
            || admission.resume_sequence() != Some(ready.resume_sequence())
            || admission.transition_sequence() != ready.resume_sequence()
            || admission.transition_sequence() <= expected.transition_sequence()
            || ready.epoch() != expected.epoch()
            || !ready.was_explicitly_resumed()
        {
            return None;
        }
        Some(Self {
            coordinator_generation,
            expected,
            admission,
            ready,
        })
    }

    pub(crate) fn coordinator_generation_v2(self) -> RuntimeGatewayCoordinatorGenerationV2 {
        self.coordinator_generation
    }

    pub(crate) fn expected_v2(self) -> RuntimeDiscordPauseReservationIdentityV2 {
        self.expected
    }

    #[cfg(test)]
    pub(crate) fn admission_v2(self) -> GatewayAdmissionSnapshotV3 {
        self.admission
    }

    #[cfg(test)]
    pub(crate) fn ready_v2(self) -> GatewayReadyLeaseV3 {
        self.ready
    }

    fn matches_current_v2(
        self,
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
        current: RuntimeDiscordAdmissionReservationSnapshotV2,
    ) -> bool {
        self.coordinator_generation == coordinator_generation
            && self.expected == expected
            && current.reservation().is_none()
            && current.admission() == self.admission
            && Self::from_exact_snapshot_v2(
                coordinator_generation,
                expected,
                self.admission,
                self.ready,
            ) == Some(self)
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeDiscordGatewayObservationV1 {
    terminal: watch::Receiver<Option<RuntimeDiscordGatewayTerminalV1>>,
    stopped: watch::Receiver<bool>,
}

impl RuntimeDiscordGatewayObservationV1 {
    pub(crate) fn terminal_status(&self) -> Option<RuntimeDiscordGatewayTerminalV1> {
        *self.terminal.borrow()
    }

    pub(crate) fn is_finished(&self) -> bool {
        *self.stopped.borrow()
    }

    pub(crate) async fn wait_terminal(&mut self) -> RuntimeDiscordGatewayTerminalV1 {
        loop {
            if let Some(terminal) = *self.terminal.borrow_and_update() {
                return terminal;
            }
            if self.terminal.changed().await.is_err() {
                return RuntimeDiscordGatewayTerminalV1 {
                    exit: RuntimeDiscordGatewayExitV1::RuntimeFailure,
                    close: RuntimeDiscordGatewayCloseOutcomeV1::DeadlineElapsed,
                    control_stopped: false,
                };
            }
        }
    }
}

async fn wait_for_runtime_discord_acknowledgement_v2(
    mut acknowledgement: oneshot::Receiver<bool>,
    terminal: &mut watch::Receiver<Option<RuntimeDiscordGatewayTerminalV1>>,
    deadline: Instant,
) -> RuntimeDiscordActorAcknowledgementV2 {
    if terminal.borrow().is_some() {
        return RuntimeDiscordActorAcknowledgementV2::Terminal;
    }
    if Instant::now() >= deadline {
        return RuntimeDiscordActorAcknowledgementV2::DeadlineElapsed;
    }
    tokio::select! {
        biased;
        acknowledgement = &mut acknowledgement => match acknowledgement {
            Ok(true) => RuntimeDiscordActorAcknowledgementV2::Accepted,
            Ok(false) => RuntimeDiscordActorAcknowledgementV2::Rejected,
            Err(_) if terminal.borrow().is_some() => {
                RuntimeDiscordActorAcknowledgementV2::Terminal
            }
            Err(_) => RuntimeDiscordActorAcknowledgementV2::Lost,
        },
        changed = terminal.changed() => {
            let _changed = changed;
            RuntimeDiscordActorAcknowledgementV2::Terminal
        }
        _ = sleep_until(TokioInstant::from_std(deadline)) => {
            RuntimeDiscordActorAcknowledgementV2::DeadlineElapsed
        },
    }
}

async fn wait_for_runtime_discord_recovery_resume_v2(
    acknowledgement: &mut oneshot::Receiver<RuntimeDiscordRecoveryResumeActorOutcomeV2>,
    terminal: &mut watch::Receiver<Option<RuntimeDiscordGatewayTerminalV1>>,
    deadline: Instant,
) -> Result<RuntimeDiscordRecoveryResumeActorOutcomeV2, RuntimeDiscordActorAcknowledgementV2> {
    if terminal.borrow().is_some() {
        return Err(RuntimeDiscordActorAcknowledgementV2::Terminal);
    }
    if Instant::now() >= deadline {
        return Err(RuntimeDiscordActorAcknowledgementV2::DeadlineElapsed);
    }
    tokio::select! {
        biased;
        changed = terminal.changed() => {
            let _changed = changed;
            match acknowledgement.try_recv() {
                Ok(outcome) => Ok(outcome),
                Err(oneshot::error::TryRecvError::Empty)
                | Err(oneshot::error::TryRecvError::Closed) => {
                    Err(RuntimeDiscordActorAcknowledgementV2::Terminal)
                }
            }
        }
        _ = sleep_until(TokioInstant::from_std(deadline)) => {
            match acknowledgement.try_recv() {
                Ok(outcome) => Ok(outcome),
                Err(oneshot::error::TryRecvError::Empty) => {
                    Err(RuntimeDiscordActorAcknowledgementV2::DeadlineElapsed)
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    Err(RuntimeDiscordActorAcknowledgementV2::Lost)
                }
            }
        }
        acknowledgement = &mut *acknowledgement => match acknowledgement {
            Ok(outcome) => Ok(outcome),
            Err(_) if terminal.borrow().is_some() => {
                Err(RuntimeDiscordActorAcknowledgementV2::Terminal)
            }
            Err(_) => Err(RuntimeDiscordActorAcknowledgementV2::Lost),
        },
    }
}

impl Debug for RuntimeDiscordGatewayObservationV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordGatewayObservationV1(<redacted>)")
    }
}

pub(crate) struct RuntimeDiscordGatewayActorStartV1 {
    pub(crate) control: SharedGatewayRuntimeControlV3,
    pub(crate) operation_cutoff: Instant,
    pub(crate) shutdown_deadline: Instant,
    pub(crate) lifecycle_drained: watch::Receiver<u64>,
    pub(crate) discord_reservation: watch::Receiver<RuntimeDiscordAdmissionReservationSnapshotV2>,
    pub(crate) runtime: tokio::runtime::Handle,
    pub(crate) control_task: RuntimeDiscordControlTaskV1,
    pub(crate) stopped_sender: watch::Sender<bool>,
    pub(crate) stopped: watch::Receiver<bool>,
}

pub(crate) struct RuntimeDiscordControlTaskV1 {
    task: Option<JoinHandle<()>>,
    reserved_resume: Option<mpsc::Sender<RuntimeDiscordReservedResumeRequestV2>>,
}

impl RuntimeDiscordControlTaskV1 {
    pub(crate) fn new(
        task: JoinHandle<()>,
        reserved_resume: mpsc::Sender<RuntimeDiscordReservedResumeRequestV2>,
    ) -> Self {
        Self {
            task: Some(task),
            reserved_resume: Some(reserved_resume),
        }
    }

    fn into_parts(
        mut self,
    ) -> (
        JoinHandle<()>,
        mpsc::Sender<RuntimeDiscordReservedResumeRequestV2>,
    ) {
        (
            self.task.take().expect("runtime Discord control task"),
            self.reserved_resume
                .take()
                .expect("runtime Discord reserved resume"),
        )
    }
}

impl Drop for RuntimeDiscordControlTaskV1 {
    fn drop(&mut self) {
        if let Some(task) = self.task.as_ref() {
            task.abort();
        }
    }
}

impl RuntimeDiscordGatewaySupervisorV1 {
    pub(crate) fn observation_v1(&self) -> RuntimeDiscordGatewayObservationV1 {
        RuntimeDiscordGatewayObservationV1 {
            terminal: self.terminal.clone(),
            stopped: self.stopped.clone(),
        }
    }

    pub(crate) fn terminal_status(&self) -> Option<RuntimeDiscordGatewayTerminalV1> {
        *self.terminal.borrow()
    }

    pub(crate) fn is_finished(&self) -> bool {
        *self.stopped.borrow() || self.join_task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub(crate) fn abort_handles(&self) -> Option<(AbortHandle, AbortHandle)> {
        Some((self.actor_abort.clone(), self.control_abort.clone()))
    }

    pub(crate) fn release_start_v1(&mut self) -> bool {
        self.start
            .take()
            .is_some_and(|start| start.send(()).is_ok())
    }

    #[cfg(test)]
    pub(crate) fn stopped_watch(&self) -> watch::Receiver<bool> {
        self.stopped.clone()
    }

    pub(crate) async fn wait_terminal(&mut self) -> RuntimeDiscordGatewayTerminalV1 {
        loop {
            if let Some(exit) = *self.terminal.borrow() {
                return exit;
            }
            if self.terminal.changed().await.is_err() {
                return RuntimeDiscordGatewayTerminalV1 {
                    exit: RuntimeDiscordGatewayExitV1::RuntimeFailure,
                    close: RuntimeDiscordGatewayCloseOutcomeV1::DeadlineElapsed,
                    control_stopped: false,
                };
            }
        }
    }

    pub(crate) async fn shutdown_until<F>(
        self,
        begin_drain: F,
        cleanup_deadline: Instant,
    ) -> Result<RuntimeDiscordGatewayTerminalV1, RuntimeDiscordGatewayShutdownErrorV1>
    where
        F: Future<Output = bool>,
    {
        self.shutdown_until_with_generation_v2(begin_drain, NonZeroU64::MIN, cleanup_deadline)
            .await
    }

    async fn shutdown_until_with_generation_v2<F>(
        mut self,
        begin_drain: F,
        shutdown_generation: NonZeroU64,
        cleanup_deadline: Instant,
    ) -> Result<RuntimeDiscordGatewayTerminalV1, RuntimeDiscordGatewayShutdownErrorV1>
    where
        F: Future<Output = bool>,
    {
        if Instant::now() >= cleanup_deadline {
            self.abort_tasks();
            return Err(RuntimeDiscordGatewayShutdownErrorV1::DeadlineElapsed);
        }
        let shutdown_cutoff = cleanup_deadline
            .checked_sub(DISCORD_SHUTDOWN_ABORT_RESERVE)
            .unwrap_or(cleanup_deadline);
        let terminal = if let Some(exit) = self.terminal_status() {
            Some(exit)
        } else {
            if !self
                .enter_draining_v2(shutdown_generation, cleanup_deadline)
                .await
            {
                self.abort_tasks();
                let _joined = self.join_task_until(cleanup_deadline).await;
                return Err(RuntimeDiscordGatewayShutdownErrorV1::DeadlineElapsed);
            }
            let drain = begin_drain;
            tokio::pin!(drain);
            tokio::select! {
                biased;
                _ = sleep_until(TokioInstant::from_std(shutdown_cutoff)) => None,
                exit = self.wait_terminal() => Some(exit),
                _acknowledged = &mut drain => {
                    self.wait_terminal_until(shutdown_cutoff).await
                }
            }
        };
        let Some(terminal) = terminal else {
            self.abort_tasks();
            let _joined = self.join_task_until(cleanup_deadline).await;
            return Err(RuntimeDiscordGatewayShutdownErrorV1::DeadlineElapsed);
        };
        let joined = self.join_task_until(cleanup_deadline).await;
        let Some(joined) = joined else {
            self.abort_tasks();
            return Err(RuntimeDiscordGatewayShutdownErrorV1::DeadlineElapsed);
        };
        if !joined {
            return Err(RuntimeDiscordGatewayShutdownErrorV1::TaskStopped);
        }
        if terminal.exit != RuntimeDiscordGatewayExitV1::Commanded {
            return Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(
                terminal,
            ));
        }
        if !terminal.control_stopped {
            return Err(RuntimeDiscordGatewayShutdownErrorV1::TaskStopped);
        }
        if terminal.close == RuntimeDiscordGatewayCloseOutcomeV1::DeadlineElapsed {
            return Err(RuntimeDiscordGatewayShutdownErrorV1::CloseDeadlineElapsed);
        }
        Ok(terminal)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn handoff_to_process_v2(
        self,
        process_generation: NonZeroU64,
    ) -> RuntimeDiscordProcessHandoffV2 {
        self.handoff_to_process_with_response_v2(process_generation, true)
            .await
    }

    #[cfg(test)]
    async fn handoff_to_process_losing_acknowledgement_for_test_v2(
        self,
        process_generation: NonZeroU64,
    ) -> RuntimeDiscordProcessHandoffV2 {
        self.handoff_to_process_with_response_v2(process_generation, false)
            .await
    }

    async fn handoff_to_process_with_response_v2(
        mut self,
        process_generation: NonZeroU64,
        respond: bool,
    ) -> RuntimeDiscordProcessHandoffV2 {
        self.handoff_to_process_in_place_with_response_v2(process_generation, respond)
            .await;
        self.into_process_handoff_v2()
    }

    pub(crate) async fn handoff_to_process_in_place_v2(&mut self, process_generation: NonZeroU64) {
        self.handoff_to_process_in_place_with_response_v2(process_generation, true)
            .await;
    }

    async fn handoff_to_process_in_place_with_response_v2(
        &mut self,
        process_generation: NonZeroU64,
        respond: bool,
    ) {
        if self.process_handoff_state != RuntimeDiscordProcessHandoffStateV2::NotStarted {
            self.process_handoff_state = RuntimeDiscordProcessHandoffStateV2::Indeterminate(
                RuntimeDiscordProcessHandoffFailureV2::ActorRejected,
            );
            return;
        }
        let Some(process_handoff) = self.process_handoff.take() else {
            self.process_handoff_state = RuntimeDiscordProcessHandoffStateV2::NotApplied(
                RuntimeDiscordProcessHandoffFailureV2::CommandUnavailable,
            );
            return;
        };
        let (response, acknowledgement) = oneshot::channel();
        if process_handoff
            .send(RuntimeDiscordProcessHandoffCommandV2 {
                process_generation,
                respond,
                response,
            })
            .is_err()
        {
            self.process_handoff_state = RuntimeDiscordProcessHandoffStateV2::NotApplied(
                RuntimeDiscordProcessHandoffFailureV2::CommandUnavailable,
            );
            return;
        }
        self.process_handoff_state = RuntimeDiscordProcessHandoffStateV2::InFlight;
        let mut terminal = self.terminal.clone();
        let acknowledgement = wait_for_runtime_discord_acknowledgement_v2(
            acknowledgement,
            &mut terminal,
            self.startup_operation_cutoff,
        )
        .await;
        self.process_handoff_state = match acknowledgement {
            RuntimeDiscordActorAcknowledgementV2::Accepted => {
                RuntimeDiscordProcessHandoffStateV2::Process
            }
            RuntimeDiscordActorAcknowledgementV2::Rejected => {
                RuntimeDiscordProcessHandoffStateV2::NotApplied(
                    RuntimeDiscordProcessHandoffFailureV2::ActorRejected,
                )
            }
            RuntimeDiscordActorAcknowledgementV2::Lost
            | RuntimeDiscordActorAcknowledgementV2::Terminal
            | RuntimeDiscordActorAcknowledgementV2::DeadlineElapsed => {
                let failure = match acknowledgement {
                    RuntimeDiscordActorAcknowledgementV2::Lost => {
                        RuntimeDiscordProcessHandoffFailureV2::AcknowledgementLost
                    }
                    RuntimeDiscordActorAcknowledgementV2::Terminal => {
                        RuntimeDiscordProcessHandoffFailureV2::ActorTerminal
                    }
                    RuntimeDiscordActorAcknowledgementV2::DeadlineElapsed => {
                        RuntimeDiscordProcessHandoffFailureV2::DeadlineElapsed
                    }
                    RuntimeDiscordActorAcknowledgementV2::Accepted
                    | RuntimeDiscordActorAcknowledgementV2::Rejected => unreachable!(),
                };
                RuntimeDiscordProcessHandoffStateV2::Indeterminate(failure)
            }
        };
    }

    pub(crate) fn into_process_handoff_v2(self) -> RuntimeDiscordProcessHandoffV2 {
        match self.process_handoff_state {
            RuntimeDiscordProcessHandoffStateV2::Process => {
                RuntimeDiscordProcessHandoffV2::Process(RuntimeDiscordProcessSupervisorV2 {
                    inner: self,
                    recovery_resume_state: RuntimeDiscordRecoveryResumeStateV2::Idle,
                    recovery_resume_acknowledgement: None,
                })
            }
            RuntimeDiscordProcessHandoffStateV2::NotApplied(failure) => {
                RuntimeDiscordProcessHandoffV2::NotApplied {
                    supervisor: self,
                    failure,
                }
            }
            RuntimeDiscordProcessHandoffStateV2::NotStarted => {
                RuntimeDiscordProcessHandoffV2::NotApplied {
                    supervisor: self,
                    failure: RuntimeDiscordProcessHandoffFailureV2::CommandUnavailable,
                }
            }
            RuntimeDiscordProcessHandoffStateV2::InFlight => {
                RuntimeDiscordProcessHandoffV2::Indeterminate {
                    supervisor: RuntimeDiscordShutdownOnlySupervisorV2 { inner: self },
                    failure: RuntimeDiscordProcessHandoffFailureV2::AcknowledgementLost,
                }
            }
            RuntimeDiscordProcessHandoffStateV2::Indeterminate(failure) => {
                RuntimeDiscordProcessHandoffV2::Indeterminate {
                    supervisor: RuntimeDiscordShutdownOnlySupervisorV2 { inner: self },
                    failure,
                }
            }
        }
    }

    async fn enter_draining_v2(
        &mut self,
        shutdown_generation: NonZeroU64,
        deadline: Instant,
    ) -> bool {
        let Some(drain) = self.drain.take() else {
            return self.terminal_status().is_some();
        };
        let (response, acknowledgement) = oneshot::channel();
        if drain
            .send(RuntimeDiscordDrainCommandV2 {
                shutdown_generation,
                deadline,
                response,
            })
            .is_err()
        {
            return self.terminal_status().is_some();
        }
        if Instant::now() >= deadline {
            return false;
        }
        timeout_at(TokioInstant::from_std(deadline), acknowledgement)
            .await
            .is_ok_and(|acknowledgement| acknowledgement == Ok(true))
    }

    async fn wait_terminal_until(
        &mut self,
        deadline: Instant,
    ) -> Option<RuntimeDiscordGatewayTerminalV1> {
        if Instant::now() >= deadline {
            return None;
        }
        timeout_at(TokioInstant::from_std(deadline), self.wait_terminal())
            .await
            .ok()
    }

    async fn join_task_until(&mut self, deadline: Instant) -> Option<bool> {
        if Instant::now() >= deadline {
            return None;
        }
        let mut task = self.join_task.take()?;
        match timeout_at(TokioInstant::from_std(deadline), &mut task).await {
            Ok(result) => Some(result.unwrap_or(false)),
            Err(_) => {
                self.join_task = Some(task);
                None
            }
        }
    }

    fn abort_tasks(&self) {
        self.actor_abort.abort();
        self.control_abort.abort();
    }
}

impl RuntimeDiscordProcessSupervisorV2 {
    pub(crate) fn terminal_status_v2(&self) -> Option<RuntimeDiscordGatewayTerminalV1> {
        self.inner.terminal_status()
    }

    pub(crate) fn is_finished_v2(&self) -> bool {
        self.inner.is_finished()
    }

    pub(crate) fn observation_v2(&self) -> RuntimeDiscordGatewayObservationV1 {
        self.inner.observation_v1()
    }

    #[cfg(test)]
    pub(crate) async fn resume_reserved_admission_v2(
        self,
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
        deadline: Instant,
    ) -> RuntimeDiscordRecoveryResumeV2 {
        self.resume_reserved_admission_with_response_v2(
            coordinator_generation,
            expected,
            deadline,
            true,
        )
        .await
    }

    #[cfg(test)]
    async fn resume_reserved_admission_losing_acknowledgement_for_test_v2(
        self,
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
        deadline: Instant,
    ) -> RuntimeDiscordRecoveryResumeV2 {
        self.resume_reserved_admission_with_response_v2(
            coordinator_generation,
            expected,
            deadline,
            false,
        )
        .await
    }

    #[cfg(test)]
    async fn resume_reserved_admission_with_response_v2(
        self,
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
        deadline: Instant,
        respond: bool,
    ) -> RuntimeDiscordRecoveryResumeV2 {
        let mut this = self;
        let attempt = this
            .resume_reserved_admission_in_place_with_response_v2(
                coordinator_generation,
                expected,
                deadline,
                respond,
            )
            .await;
        match attempt {
            RuntimeDiscordRecoveryResumeAttemptV2::Applied(evidence) => {
                RuntimeDiscordRecoveryResumeV2::Applied {
                    supervisor: this,
                    evidence,
                }
            }
            RuntimeDiscordRecoveryResumeAttemptV2::DefinitelyNotApplied(failure)
                if this.recovery_resume_state == RuntimeDiscordRecoveryResumeStateV2::Idle =>
            {
                RuntimeDiscordRecoveryResumeV2::DefinitelyNotApplied {
                    ownership: RuntimeDiscordRecoveryResumeOwnershipV2::Process(this),
                    failure,
                }
            }
            RuntimeDiscordRecoveryResumeAttemptV2::DefinitelyNotApplied(failure) => {
                RuntimeDiscordRecoveryResumeV2::DefinitelyNotApplied {
                    ownership: RuntimeDiscordRecoveryResumeOwnershipV2::ShutdownOnly(
                        RuntimeDiscordShutdownOnlySupervisorV2 { inner: this.inner },
                    ),
                    failure,
                }
            }
            RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(failure) => {
                RuntimeDiscordRecoveryResumeV2::Indeterminate {
                    supervisor: RuntimeDiscordShutdownOnlySupervisorV2 { inner: this.inner },
                    failure,
                }
            }
        }
    }

    pub(crate) async fn resume_reserved_admission_in_place_v2(
        &mut self,
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
        deadline: Instant,
    ) -> RuntimeDiscordRecoveryResumeAttemptV2 {
        self.resume_reserved_admission_in_place_with_response_v2(
            coordinator_generation,
            expected,
            deadline,
            true,
        )
        .await
    }

    #[cfg(test)]
    async fn resume_reserved_admission_in_place_losing_acknowledgement_for_test_v2(
        &mut self,
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
        deadline: Instant,
    ) -> RuntimeDiscordRecoveryResumeAttemptV2 {
        self.resume_reserved_admission_in_place_with_response_v2(
            coordinator_generation,
            expected,
            deadline,
            false,
        )
        .await
    }

    async fn resume_reserved_admission_in_place_with_response_v2(
        &mut self,
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
        deadline: Instant,
        respond: bool,
    ) -> RuntimeDiscordRecoveryResumeAttemptV2 {
        match self.recovery_resume_state {
            RuntimeDiscordRecoveryResumeStateV2::Applied(evidence) => {
                if evidence.coordinator_generation_v2() == coordinator_generation
                    && evidence.expected_v2() == expected
                    && self.exact_recovery_resume_evidence_v2(coordinator_generation, expected)
                        == Some(evidence)
                {
                    return RuntimeDiscordRecoveryResumeAttemptV2::Applied(evidence);
                }
                let current = *self.inner.discord_reservation.borrow();
                if evidence.coordinator_generation_v2() == coordinator_generation
                    && evidence.expected_v2().epoch() != expected.epoch()
                    && current.reservation() == Some(expected)
                    && current.admission().connection().current_epoch() == Some(expected.epoch())
                {
                    self.recovery_resume_state = RuntimeDiscordRecoveryResumeStateV2::Idle;
                    self.recovery_resume_acknowledgement = None;
                } else {
                    let failure = RuntimeDiscordRecoveryResumeFailureV2::AcknowledgementLost;
                    self.recovery_resume_state =
                        RuntimeDiscordRecoveryResumeStateV2::Indeterminate(failure);
                    return RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(failure);
                }
            }
            RuntimeDiscordRecoveryResumeStateV2::DefinitelyNotApplied(failure) => {
                return RuntimeDiscordRecoveryResumeAttemptV2::DefinitelyNotApplied(failure);
            }
            RuntimeDiscordRecoveryResumeStateV2::Indeterminate(failure) => {
                return RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(failure);
            }
            RuntimeDiscordRecoveryResumeStateV2::InFlight {
                coordinator_generation: current_generation,
                expected: current_expected,
            } if current_generation != coordinator_generation || current_expected != expected => {
                let failure = RuntimeDiscordRecoveryResumeFailureV2::ActorRejected;
                self.recovery_resume_state =
                    RuntimeDiscordRecoveryResumeStateV2::Indeterminate(failure);
                self.recovery_resume_acknowledgement = None;
                return RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(failure);
            }
            RuntimeDiscordRecoveryResumeStateV2::Idle
            | RuntimeDiscordRecoveryResumeStateV2::InFlight { .. } => {}
        }
        if self.recovery_resume_state == RuntimeDiscordRecoveryResumeStateV2::Idle {
            if Instant::now() >= deadline {
                return RuntimeDiscordRecoveryResumeAttemptV2::DefinitelyNotApplied(
                    RuntimeDiscordRecoveryResumeFailureV2::DeadlineElapsed,
                );
            }
            let (response, acknowledgement) = oneshot::channel();
            let sent = timeout_at(
                TokioInstant::from_std(deadline),
                self.inner
                    .recovery_resume
                    .send(RuntimeDiscordRecoveryResumeCommandV2 {
                        coordinator_generation,
                        expected,
                        respond,
                        response,
                    }),
            )
            .await;
            match sent {
                Ok(Ok(())) => {
                    self.recovery_resume_state = RuntimeDiscordRecoveryResumeStateV2::InFlight {
                        coordinator_generation,
                        expected,
                    };
                    self.recovery_resume_acknowledgement = Some(acknowledgement);
                }
                Err(_) => {
                    return RuntimeDiscordRecoveryResumeAttemptV2::DefinitelyNotApplied(
                        RuntimeDiscordRecoveryResumeFailureV2::DeadlineElapsed,
                    );
                }
                Ok(Err(_)) => {
                    let failure = RuntimeDiscordRecoveryResumeFailureV2::CommandUnavailable;
                    self.recovery_resume_state =
                        RuntimeDiscordRecoveryResumeStateV2::DefinitelyNotApplied(failure);
                    return RuntimeDiscordRecoveryResumeAttemptV2::DefinitelyNotApplied(failure);
                }
            }
        }
        if let Some(evidence) =
            self.exact_recovery_resume_evidence_v2(coordinator_generation, expected)
        {
            self.recovery_resume_state = RuntimeDiscordRecoveryResumeStateV2::Applied(evidence);
            self.recovery_resume_acknowledgement = None;
            return RuntimeDiscordRecoveryResumeAttemptV2::Applied(evidence);
        }
        let Some(acknowledgement) = self.recovery_resume_acknowledgement.as_mut() else {
            let failure = RuntimeDiscordRecoveryResumeFailureV2::AcknowledgementLost;
            self.recovery_resume_state =
                RuntimeDiscordRecoveryResumeStateV2::Indeterminate(failure);
            return RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(failure);
        };
        let mut terminal = self.inner.terminal.clone();
        let acknowledgement =
            wait_for_runtime_discord_recovery_resume_v2(acknowledgement, &mut terminal, deadline)
                .await;
        self.recovery_resume_acknowledgement = None;
        let exact = self.exact_recovery_resume_evidence_v2(coordinator_generation, expected);
        let attempt = match acknowledgement {
            Ok(RuntimeDiscordRecoveryResumeActorOutcomeV2::Applied(acknowledged))
                if exact == Some(acknowledged) =>
            {
                RuntimeDiscordRecoveryResumeAttemptV2::Applied(acknowledged)
            }
            Err(
                RuntimeDiscordActorAcknowledgementV2::Lost
                | RuntimeDiscordActorAcknowledgementV2::DeadlineElapsed,
            ) if exact.is_some() => RuntimeDiscordRecoveryResumeAttemptV2::Applied(
                exact.expect("exact Discord recovery resume evidence"),
            ),
            Ok(RuntimeDiscordRecoveryResumeActorOutcomeV2::DefinitelyNotApplied) => {
                RuntimeDiscordRecoveryResumeAttemptV2::DefinitelyNotApplied(
                    RuntimeDiscordRecoveryResumeFailureV2::ActorRejected,
                )
            }
            Ok(
                RuntimeDiscordRecoveryResumeActorOutcomeV2::Applied(_)
                | RuntimeDiscordRecoveryResumeActorOutcomeV2::Indeterminate,
            ) => RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(
                RuntimeDiscordRecoveryResumeFailureV2::AcknowledgementLost,
            ),
            Err(RuntimeDiscordActorAcknowledgementV2::Terminal) => {
                RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(
                    RuntimeDiscordRecoveryResumeFailureV2::ActorTerminal,
                )
            }
            Err(RuntimeDiscordActorAcknowledgementV2::DeadlineElapsed) => {
                RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(
                    RuntimeDiscordRecoveryResumeFailureV2::DeadlineElapsed,
                )
            }
            Err(RuntimeDiscordActorAcknowledgementV2::Lost) => {
                RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(
                    RuntimeDiscordRecoveryResumeFailureV2::AcknowledgementLost,
                )
            }
            Err(
                RuntimeDiscordActorAcknowledgementV2::Accepted
                | RuntimeDiscordActorAcknowledgementV2::Rejected,
            ) => unreachable!(),
        };
        self.recovery_resume_state = match attempt {
            RuntimeDiscordRecoveryResumeAttemptV2::Applied(evidence) => {
                RuntimeDiscordRecoveryResumeStateV2::Applied(evidence)
            }
            RuntimeDiscordRecoveryResumeAttemptV2::DefinitelyNotApplied(failure) => {
                RuntimeDiscordRecoveryResumeStateV2::DefinitelyNotApplied(failure)
            }
            RuntimeDiscordRecoveryResumeAttemptV2::Indeterminate(failure) => {
                RuntimeDiscordRecoveryResumeStateV2::Indeterminate(failure)
            }
        };
        attempt
    }

    fn exact_recovery_resume_evidence_v2(
        &self,
        coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
        expected: RuntimeDiscordPauseReservationIdentityV2,
    ) -> Option<RuntimeDiscordRecoveryResumeEvidenceV2> {
        if self.terminal_status_v2().is_some() || self.is_finished_v2() {
            return None;
        }
        let first_evidence = *self.inner.recovery_resume_observation.borrow();
        let first_reservation = *self.inner.discord_reservation.borrow();
        let evidence = first_evidence?;
        if !evidence.matches_current_v2(coordinator_generation, expected, first_reservation) {
            return None;
        }
        let second_evidence = *self.inner.recovery_resume_observation.borrow();
        let second_reservation = *self.inner.discord_reservation.borrow();
        if first_evidence != second_evidence
            || first_reservation != second_reservation
            || self.terminal_status_v2().is_some()
            || self.is_finished_v2()
        {
            return None;
        }
        Some(evidence)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn shutdown_until<F>(
        self,
        begin_drain: F,
        shutdown_generation: NonZeroU64,
        cleanup_deadline: Instant,
    ) -> Result<RuntimeDiscordGatewayTerminalV1, RuntimeDiscordGatewayShutdownErrorV1>
    where
        F: Future<Output = bool>,
    {
        self.inner
            .shutdown_until_with_generation_v2(begin_drain, shutdown_generation, cleanup_deadline)
            .await
    }
}

impl RuntimeDiscordShutdownOnlySupervisorV2 {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn shutdown_until<F>(
        self,
        begin_drain: F,
        shutdown_generation: NonZeroU64,
        cleanup_deadline: Instant,
    ) -> Result<RuntimeDiscordGatewayTerminalV1, RuntimeDiscordGatewayShutdownErrorV1>
    where
        F: Future<Output = bool>,
    {
        self.inner
            .shutdown_until_with_generation_v2(begin_drain, shutdown_generation, cleanup_deadline)
            .await
    }
}

impl Debug for RuntimeDiscordProcessSupervisorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordProcessSupervisorV2(<redacted>)")
    }
}

impl Debug for RuntimeDiscordShutdownOnlySupervisorV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordShutdownOnlySupervisorV2(<redacted>)")
    }
}

impl Drop for RuntimeDiscordGatewaySupervisorV1 {
    fn drop(&mut self) {
        self.abort_tasks();
    }
}

impl Debug for RuntimeDiscordGatewaySupervisorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDiscordGatewaySupervisorV1(<redacted>)")
    }
}

pub(crate) fn prepare_twilight_runtime_discord_gateway_driver_v1(
    token: String,
) -> impl RuntimeDiscordGatewayDriverV1 {
    TwilightRuntimeDiscordGatewayDriverV1::new(token)
}

pub(crate) fn start_runtime_discord_gateway_v1<D>(
    driver: D,
    start: RuntimeDiscordGatewayActorStartV1,
) -> RuntimeDiscordGatewaySupervisorV1
where
    D: RuntimeDiscordGatewayDriverV1,
{
    let RuntimeDiscordGatewayActorStartV1 {
        control,
        operation_cutoff,
        shutdown_deadline,
        lifecycle_drained,
        discord_reservation,
        runtime,
        control_task,
        stopped_sender,
        stopped,
    } = start;
    let (control_task, reserved_resume) = control_task.into_parts();
    let (terminal_sender, terminal) = watch::channel(None);
    let (start, start_receiver) = oneshot::channel();
    let (process_handoff, process_handoff_receiver) = oneshot::channel();
    let (drain, drain_receiver) = oneshot::channel();
    let (recovery_resume, recovery_resume_receiver) = mpsc::channel(1);
    let (recovery_resume_observation_sender, recovery_resume_observation) = watch::channel(None);
    let supervisor_discord_reservation = discord_reservation.clone();
    let publisher = RuntimeDiscordGatewayTerminalPublisherV1::new(terminal_sender);
    let actor_task = runtime.spawn(async move {
        let mut publisher = publisher;
        let terminal = if start_receiver.await.is_ok() {
            run_runtime_discord_gateway_v1(RuntimeDiscordGatewayActorV2 {
                driver,
                control,
                mode: RuntimeDiscordActorModeV2::StartupPaused { operation_cutoff },
                failure_deadline: shutdown_deadline,
                lifecycle_drained,
                discord_reservation,
                process_handoff: process_handoff_receiver,
                drain: drain_receiver,
                recovery_resume: recovery_resume_receiver,
                recovery_resume_observation: recovery_resume_observation_sender,
                reserved_resume,
            })
            .await
        } else {
            RuntimeDiscordGatewayTerminalV1 {
                exit: RuntimeDiscordGatewayExitV1::RuntimeFailure,
                close: RuntimeDiscordGatewayCloseOutcomeV1::DeadlineElapsed,
                control_stopped: false,
            }
        };
        publisher.publish(terminal);
    });
    let actor_abort = actor_task.abort_handle();
    let control_abort = control_task.abort_handle();
    let coordinator_control_abort = control_abort.clone();
    let join_task = runtime.spawn(async move {
        let actor_joined = actor_task.await.is_ok();
        coordinator_control_abort.abort();
        let _control_result = control_task.await;
        let _stopped = stopped_sender.send(true);
        actor_joined
    });
    RuntimeDiscordGatewaySupervisorV1 {
        terminal,
        stopped,
        start: Some(start),
        actor_abort,
        control_abort,
        join_task: Some(join_task),
        startup_operation_cutoff: operation_cutoff,
        process_handoff: Some(process_handoff),
        process_handoff_state: RuntimeDiscordProcessHandoffStateV2::NotStarted,
        drain: Some(drain),
        recovery_resume,
        discord_reservation: supervisor_discord_reservation,
        recovery_resume_observation,
    }
}

struct RuntimeDiscordGatewayTerminalPublisherV1 {
    terminal: watch::Sender<Option<RuntimeDiscordGatewayTerminalV1>>,
    published: bool,
}

impl RuntimeDiscordGatewayTerminalPublisherV1 {
    fn new(terminal: watch::Sender<Option<RuntimeDiscordGatewayTerminalV1>>) -> Self {
        Self {
            terminal,
            published: false,
        }
    }

    fn publish(&mut self, terminal: RuntimeDiscordGatewayTerminalV1) {
        let _terminal = self.terminal.send(Some(terminal));
        self.published = true;
    }
}

impl Drop for RuntimeDiscordGatewayTerminalPublisherV1 {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        let _terminal = self.terminal.send(Some(RuntimeDiscordGatewayTerminalV1 {
            exit: RuntimeDiscordGatewayExitV1::RuntimeFailure,
            close: RuntimeDiscordGatewayCloseOutcomeV1::DeadlineElapsed,
            control_stopped: false,
        }));
    }
}

struct RuntimeDiscordGatewayActorV2<D> {
    driver: D,
    control: SharedGatewayRuntimeControlV3,
    mode: RuntimeDiscordActorModeV2,
    failure_deadline: Instant,
    lifecycle_drained: watch::Receiver<u64>,
    discord_reservation: watch::Receiver<RuntimeDiscordAdmissionReservationSnapshotV2>,
    process_handoff: oneshot::Receiver<RuntimeDiscordProcessHandoffCommandV2>,
    drain: oneshot::Receiver<RuntimeDiscordDrainCommandV2>,
    recovery_resume: mpsc::Receiver<RuntimeDiscordRecoveryResumeCommandV2>,
    recovery_resume_observation: watch::Sender<Option<RuntimeDiscordRecoveryResumeEvidenceV2>>,
    reserved_resume: mpsc::Sender<RuntimeDiscordReservedResumeRequestV2>,
}

async fn run_runtime_discord_gateway_v1<D>(
    actor: RuntimeDiscordGatewayActorV2<D>,
) -> RuntimeDiscordGatewayTerminalV1
where
    D: RuntimeDiscordGatewayDriverV1,
{
    let RuntimeDiscordGatewayActorV2 {
        mut driver,
        mut control,
        mut mode,
        failure_deadline,
        mut lifecycle_drained,
        mut discord_reservation,
        mut process_handoff,
        mut drain,
        mut recovery_resume,
        recovery_resume_observation,
        reserved_resume,
    } = actor;
    let RuntimeDiscordActorModeV2::StartupPaused { operation_cutoff } = mode else {
        return runtime_discord_gateway_failure_terminal_v1();
    };
    if Instant::now() >= operation_cutoff {
        return finish_runtime_discord_gateway_without_transport_v1(
            &mut control,
            RuntimeDiscordGatewayExitV1::StartDeadlineElapsed,
            failure_deadline,
            &mut lifecycle_drained,
        )
        .await;
    }
    let mut _suppressed_handoff_response = None;
    let mut _suppressed_recovery_resume_response = None;
    loop {
        let lifecycle_sequence = *lifecycle_drained.borrow();
        tokio::select! {
            biased;
            handoff = &mut process_handoff, if matches!(mode, RuntimeDiscordActorModeV2::StartupPaused { .. }) => {
                let Ok(handoff) = handoff else {
                    return finish_runtime_discord_gateway_if_connected_v1(
                        &mut driver,
                        &mut control,
                        RuntimeDiscordGatewayExitV1::RuntimeFailure,
                        failure_deadline,
                        &mut lifecycle_drained,
                    )
                    .await;
                };
                let accepted = match mode {
                    RuntimeDiscordActorModeV2::StartupPaused { operation_cutoff }
                        if Instant::now() < operation_cutoff =>
                    {
                        mode = RuntimeDiscordActorModeV2::ProcessSupervised {
                            process_generation: handoff.process_generation,
                        };
                        true
                    }
                    RuntimeDiscordActorModeV2::StartupPaused { .. }
                    | RuntimeDiscordActorModeV2::ProcessSupervised { .. }
                    | RuntimeDiscordActorModeV2::Draining { .. } => false,
                };
                if handoff.respond {
                    let _response = handoff.response.send(accepted);
                } else {
                    _suppressed_handoff_response = Some(handoff.response);
                }
            }
            drain_command = &mut drain, if !matches!(mode, RuntimeDiscordActorModeV2::Draining { .. }) => {
                let Ok(drain_command) = drain_command else {
                    return finish_runtime_discord_gateway_if_connected_v1(
                        &mut driver,
                        &mut control,
                        RuntimeDiscordGatewayExitV1::RuntimeFailure,
                        failure_deadline,
                        &mut lifecycle_drained,
                    )
                    .await;
                };
                let accepted = Instant::now() < drain_command.deadline;
                if accepted {
                    mode = RuntimeDiscordActorModeV2::Draining {
                        shutdown_generation: drain_command.shutdown_generation,
                        deadline: drain_command.deadline,
                    };
                }
                let _response = drain_command.response.send(accepted);
                if !accepted {
                    return finish_runtime_discord_gateway_if_connected_v1(
                        &mut driver,
                        &mut control,
                        RuntimeDiscordGatewayExitV1::RuntimeFailure,
                        failure_deadline,
                        &mut lifecycle_drained,
                    )
                    .await;
                }
            }
            recovery = recovery_resume.recv(), if mode.process_generation().is_some() => {
                let Some(recovery) = recovery else {
                    return finish_runtime_discord_gateway_if_connected_v1(
                        &mut driver,
                        &mut control,
                        RuntimeDiscordGatewayExitV1::RuntimeFailure,
                        failure_deadline,
                        &mut lifecycle_drained,
                    )
                    .await;
                };
                let resumed = resume_reserved_runtime_discord_admission_v2(
                    RuntimeDiscordRecoveryResumeActorContextV2 {
                        control: &mut control,
                        lifecycle_drained: &mut lifecycle_drained,
                        discord_reservation: &mut discord_reservation,
                        recovery_resume_observation: &recovery_resume_observation,
                        reserved_resume: &reserved_resume,
                    },
                    recovery.coordinator_generation,
                    recovery.expected,
                    runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                )
                .await;
                if recovery.respond {
                    let _response = recovery.response.send(resumed);
                } else {
                    _suppressed_recovery_resume_response = Some(recovery.response);
                }
                if !matches!(resumed, RuntimeDiscordRecoveryResumeActorOutcomeV2::Applied(_)) {
                    return finish_runtime_discord_gateway_if_connected_v1(
                        &mut driver,
                        &mut control,
                        RuntimeDiscordGatewayExitV1::AdmissionOpened,
                        failure_deadline,
                        &mut lifecycle_drained,
                    )
                    .await;
                }
            }
            _ = sleep_until(TokioInstant::from_std(
                mode.deadline().unwrap_or(failure_deadline)
            )), if mode.deadline().is_some() => {
                let reason = match mode {
                    RuntimeDiscordActorModeV2::StartupPaused { .. } => {
                        RuntimeDiscordGatewayExitV1::StartDeadlineElapsed
                    }
                    RuntimeDiscordActorModeV2::Draining { .. } => {
                        RuntimeDiscordGatewayExitV1::RuntimeFailure
                    }
                    RuntimeDiscordActorModeV2::ProcessSupervised { .. } => {
                        RuntimeDiscordGatewayExitV1::RuntimeFailure
                    }
                };
                return finish_runtime_discord_gateway_without_transport_v1(
                    &mut control,
                    reason,
                    runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                    &mut lifecycle_drained,
                )
                .await;
            }
            command = control.process_next_command() => {
                match command {
                    GatewayRuntimeCommandOutcomeV3::Applied(
                        GatewayCommandAckV3::Paused { .. }
                    ) => {
                        if !wait_for_lifecycle_drain_v1(
                            &mut lifecycle_drained,
                            lifecycle_sequence,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                        )
                        .await
                        {
                            return finish_runtime_discord_gateway_if_connected_v1(
                                &mut driver,
                                &mut control,
                                RuntimeDiscordGatewayExitV1::RuntimeFailure,
                                runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                                &mut lifecycle_drained,
                            )
                            .await;
                        }
                    }
                    GatewayRuntimeCommandOutcomeV3::Applied(
                        GatewayCommandAckV3::AdmissionResumed { .. }
                    ) => {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::AdmissionOpened,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    GatewayRuntimeCommandOutcomeV3::Applied(
                        GatewayCommandAckV3::Draining { .. }
                    ) => {
                        let lifecycle_was_drained = wait_for_lifecycle_drain_v1(
                            &mut lifecycle_drained,
                            lifecycle_sequence,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                        )
                        .await;
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            if lifecycle_was_drained {
                                RuntimeDiscordGatewayExitV1::Commanded
                            } else {
                                RuntimeDiscordGatewayExitV1::RuntimeFailure
                            },
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    GatewayRuntimeCommandOutcomeV3::Rejected(_) => {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::RuntimeFailure,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    GatewayRuntimeCommandOutcomeV3::ControlOrphaned => {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::ControlOrphaned,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                }
            }
            signal = driver.next_signal() => {
                let transition = match signal {
                    RuntimeDiscordGatewaySignalV1::Ready => {
                        control.mark_connected(GatewayReadyKindV3::Ready).map(|_| true)
                    }
                    RuntimeDiscordGatewaySignalV1::Resumed => {
                        control.mark_connected(GatewayReadyKindV3::Resumed).map(|_| true)
                    }
                    RuntimeDiscordGatewaySignalV1::Close => {
                        control.mark_disconnected(GatewayDisconnectKindV3::Close).map(|_| true)
                    }
                    RuntimeDiscordGatewaySignalV1::Reconnect => {
                        control.mark_disconnected(GatewayDisconnectKindV3::Reconnect).map(|_| true)
                    }
                    RuntimeDiscordGatewaySignalV1::SessionInvalidated => {
                        control
                            .mark_disconnected(GatewayDisconnectKindV3::SessionInvalidated)
                            .map(|_| true)
                    }
                    RuntimeDiscordGatewaySignalV1::ReceiveError => {
                        control
                            .mark_disconnected(GatewayDisconnectKindV3::ReceiveError)
                            .map(|_| true)
                    }
                    RuntimeDiscordGatewaySignalV1::FatalReceiveError => {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::RuntimeFailure,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    RuntimeDiscordGatewaySignalV1::StreamEnded => {
                        return finish_runtime_discord_gateway_without_transport_v1(
                            &mut control,
                            RuntimeDiscordGatewayExitV1::StreamEnded,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    RuntimeDiscordGatewaySignalV1::Unrelated => Ok(false),
                };
                match transition {
                    Ok(true)
                        if !wait_for_lifecycle_drain_v1(
                            &mut lifecycle_drained,
                            lifecycle_sequence,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                        )
                        .await =>
                    {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::RuntimeFailure,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    Err(_) => {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::RuntimeFailure,
                            runtime_discord_cleanup_deadline_v2(mode, failure_deadline),
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    Ok(_) => {}
                }
            }
        }
    }
}

fn runtime_discord_gateway_failure_terminal_v1() -> RuntimeDiscordGatewayTerminalV1 {
    RuntimeDiscordGatewayTerminalV1 {
        exit: RuntimeDiscordGatewayExitV1::RuntimeFailure,
        close: RuntimeDiscordGatewayCloseOutcomeV1::DeadlineElapsed,
        control_stopped: false,
    }
}

fn runtime_discord_cleanup_deadline_v2(
    mode: RuntimeDiscordActorModeV2,
    startup_cleanup_deadline: Instant,
) -> Instant {
    match mode {
        RuntimeDiscordActorModeV2::StartupPaused { .. } => startup_cleanup_deadline,
        RuntimeDiscordActorModeV2::ProcessSupervised { .. } => Instant::now()
            .checked_add(DISCORD_GRACEFUL_CLOSE_TIMEOUT)
            .unwrap_or(startup_cleanup_deadline),
        RuntimeDiscordActorModeV2::Draining { deadline, .. } => deadline,
    }
}

struct RuntimeDiscordRecoveryResumeActorContextV2<'a> {
    control: &'a mut SharedGatewayRuntimeControlV3,
    lifecycle_drained: &'a mut watch::Receiver<u64>,
    discord_reservation: &'a mut watch::Receiver<RuntimeDiscordAdmissionReservationSnapshotV2>,
    recovery_resume_observation: &'a watch::Sender<Option<RuntimeDiscordRecoveryResumeEvidenceV2>>,
    reserved_resume: &'a mpsc::Sender<RuntimeDiscordReservedResumeRequestV2>,
}

async fn resume_reserved_runtime_discord_admission_v2(
    context: RuntimeDiscordRecoveryResumeActorContextV2<'_>,
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    expected: RuntimeDiscordPauseReservationIdentityV2,
    deadline: Instant,
) -> RuntimeDiscordRecoveryResumeActorOutcomeV2 {
    let RuntimeDiscordRecoveryResumeActorContextV2 {
        control,
        lifecycle_drained,
        discord_reservation,
        recovery_resume_observation,
        reserved_resume,
    } = context;
    let current = *discord_reservation.borrow();
    if current.reservation() != Some(expected)
        || current.admission().connection().current_epoch() != Some(expected.epoch())
    {
        return RuntimeDiscordRecoveryResumeActorOutcomeV2::DefinitelyNotApplied;
    }
    recovery_resume_observation.send_replace(None);
    let lifecycle_sequence = *lifecycle_drained.borrow();
    let (response, mut acknowledgement) = oneshot::channel();
    if Instant::now() >= deadline {
        return RuntimeDiscordRecoveryResumeActorOutcomeV2::DefinitelyNotApplied;
    }
    match timeout_at(
        TokioInstant::from_std(deadline),
        reserved_resume.send(RuntimeDiscordReservedResumeRequestV2 {
            coordinator_generation,
            expected,
            observation: recovery_resume_observation.clone(),
            response,
        }),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(_)) => return RuntimeDiscordRecoveryResumeActorOutcomeV2::DefinitelyNotApplied,
        Err(_) => return RuntimeDiscordRecoveryResumeActorOutcomeV2::DefinitelyNotApplied,
    }
    let outcome = tokio::select! {
        biased;
        acknowledgement = &mut acknowledgement => {
            return match acknowledgement {
                Ok(RuntimeDiscordRecoveryResumeControlOutcomeV2::Applied(evidence))
                    if exact_runtime_discord_resume_evidence_v2(
                        discord_reservation,
                        recovery_resume_observation,
                        coordinator_generation,
                        expected,
                    ) == Some(evidence) =>
                {
                    RuntimeDiscordRecoveryResumeActorOutcomeV2::Applied(evidence)
                }
                Ok(RuntimeDiscordRecoveryResumeControlOutcomeV2::DefinitelyNotApplied) => {
                    RuntimeDiscordRecoveryResumeActorOutcomeV2::DefinitelyNotApplied
                }
                Ok(RuntimeDiscordRecoveryResumeControlOutcomeV2::Applied(_))
                | Ok(RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate)
                | Err(_) => RuntimeDiscordRecoveryResumeActorOutcomeV2::Indeterminate,
            };
        }
        outcome = control.process_next_command() => outcome,
    };
    if !matches!(
        outcome,
        GatewayRuntimeCommandOutcomeV3::Applied(GatewayCommandAckV3::AdmissionResumed { epoch })
            if epoch == expected.epoch()
    ) {
        return match outcome {
            GatewayRuntimeCommandOutcomeV3::Rejected(_) => {
                RuntimeDiscordRecoveryResumeActorOutcomeV2::DefinitelyNotApplied
            }
            GatewayRuntimeCommandOutcomeV3::Applied(_)
            | GatewayRuntimeCommandOutcomeV3::ControlOrphaned => {
                RuntimeDiscordRecoveryResumeActorOutcomeV2::Indeterminate
            }
        };
    }
    if !wait_for_lifecycle_drain_v1(lifecycle_drained, lifecycle_sequence, deadline).await {
        return RuntimeDiscordRecoveryResumeActorOutcomeV2::Indeterminate;
    }
    let acknowledged = if Instant::now() >= deadline {
        None
    } else {
        timeout_at(TokioInstant::from_std(deadline), &mut acknowledgement)
            .await
            .ok()
            .and_then(Result::ok)
    };
    let exact = exact_runtime_discord_resume_evidence_v2(
        discord_reservation,
        recovery_resume_observation,
        coordinator_generation,
        expected,
    );
    match (acknowledged, exact) {
        (
            Some(RuntimeDiscordRecoveryResumeControlOutcomeV2::Applied(acknowledged)),
            Some(exact),
        ) if acknowledged == exact => RuntimeDiscordRecoveryResumeActorOutcomeV2::Applied(exact),
        (Some(RuntimeDiscordRecoveryResumeControlOutcomeV2::DefinitelyNotApplied), None) => {
            RuntimeDiscordRecoveryResumeActorOutcomeV2::DefinitelyNotApplied
        }
        (None, Some(exact)) => RuntimeDiscordRecoveryResumeActorOutcomeV2::Applied(exact),
        (
            Some(
                RuntimeDiscordRecoveryResumeControlOutcomeV2::Applied(_)
                | RuntimeDiscordRecoveryResumeControlOutcomeV2::Indeterminate,
            ),
            _,
        )
        | (Some(RuntimeDiscordRecoveryResumeControlOutcomeV2::DefinitelyNotApplied), Some(_))
        | (None, None) => RuntimeDiscordRecoveryResumeActorOutcomeV2::Indeterminate,
    }
}

fn exact_runtime_discord_resume_evidence_v2(
    discord_reservation: &watch::Receiver<RuntimeDiscordAdmissionReservationSnapshotV2>,
    recovery_resume_observation: &watch::Sender<Option<RuntimeDiscordRecoveryResumeEvidenceV2>>,
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    expected: RuntimeDiscordPauseReservationIdentityV2,
) -> Option<RuntimeDiscordRecoveryResumeEvidenceV2> {
    let first_reservation = *discord_reservation.borrow();
    let first_evidence = *recovery_resume_observation.borrow();
    let evidence = first_evidence?;
    if !evidence.matches_current_v2(coordinator_generation, expected, first_reservation) {
        return None;
    }
    let second_reservation = *discord_reservation.borrow();
    let second_evidence = *recovery_resume_observation.borrow();
    if first_reservation != second_reservation || first_evidence != second_evidence {
        return None;
    }
    Some(evidence)
}

async fn finish_runtime_discord_gateway_if_connected_v1<D>(
    driver: &mut D,
    control: &mut SharedGatewayRuntimeControlV3,
    reason: RuntimeDiscordGatewayExitV1,
    shutdown_deadline: Instant,
    lifecycle_drained: &mut watch::Receiver<u64>,
) -> RuntimeDiscordGatewayTerminalV1
where
    D: RuntimeDiscordGatewayDriverV1,
{
    if driver.transport_state() == RuntimeDiscordGatewayTransportStateV1::Active {
        finish_runtime_discord_gateway_v1(
            driver,
            control,
            reason,
            shutdown_deadline,
            lifecycle_drained,
        )
        .await
    } else {
        finish_runtime_discord_gateway_without_transport_v1(
            control,
            reason,
            shutdown_deadline,
            lifecycle_drained,
        )
        .await
    }
}

async fn finish_runtime_discord_gateway_without_transport_v1(
    control: &mut SharedGatewayRuntimeControlV3,
    reason: RuntimeDiscordGatewayExitV1,
    shutdown_deadline: Instant,
    lifecycle_drained: &mut watch::Receiver<u64>,
) -> RuntimeDiscordGatewayTerminalV1 {
    let lifecycle_sequence = *lifecycle_drained.borrow();
    let control_stopped = control.mark_stopped().is_ok()
        && wait_for_lifecycle_drain_v1(lifecycle_drained, lifecycle_sequence, shutdown_deadline)
            .await;
    RuntimeDiscordGatewayTerminalV1 {
        exit: reason,
        close: RuntimeDiscordGatewayCloseOutcomeV1::Confirmed,
        control_stopped,
    }
}

async fn finish_runtime_discord_gateway_v1<D>(
    driver: &mut D,
    control: &mut SharedGatewayRuntimeControlV3,
    reason: RuntimeDiscordGatewayExitV1,
    shutdown_deadline: Instant,
    lifecycle_drained: &mut watch::Receiver<u64>,
) -> RuntimeDiscordGatewayTerminalV1
where
    D: RuntimeDiscordGatewayDriverV1,
{
    let absolute_close_deadline = shutdown_deadline
        .checked_sub(DISCORD_ACTOR_TERMINATION_RESERVE)
        .unwrap_or(shutdown_deadline);
    let local_close_deadline = Instant::now()
        .checked_add(DISCORD_GRACEFUL_CLOSE_TIMEOUT)
        .unwrap_or(absolute_close_deadline);
    let close_deadline = absolute_close_deadline.min(local_close_deadline);
    let lifecycle_sequence = *lifecycle_drained.borrow();
    let control_stopped = control.mark_stopped().is_ok()
        && wait_for_lifecycle_drain_v1(lifecycle_drained, lifecycle_sequence, shutdown_deadline)
            .await;
    let close = if driver.close_until(close_deadline).await {
        RuntimeDiscordGatewayCloseOutcomeV1::Confirmed
    } else {
        RuntimeDiscordGatewayCloseOutcomeV1::DeadlineElapsed
    };
    RuntimeDiscordGatewayTerminalV1 {
        exit: reason,
        close,
        control_stopped,
    }
}

async fn wait_for_lifecycle_drain_v1(
    lifecycle_drained: &mut watch::Receiver<u64>,
    previous: u64,
    absolute_deadline: Instant,
) -> bool {
    let local_deadline = Instant::now()
        .checked_add(DISCORD_LIFECYCLE_DRAIN_TIMEOUT)
        .unwrap_or(absolute_deadline);
    let deadline = absolute_deadline.min(local_deadline);
    loop {
        if *lifecycle_drained.borrow() > previous {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::select! {
            biased;
            _ = sleep_until(TokioInstant::from_std(deadline)) => return false,
            changed = lifecycle_drained.changed() => {
                if changed.is_err() {
                    return false;
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use tokio::sync::{mpsc, oneshot};

    use super::{
        RuntimeDiscordGatewayDriverV1, RuntimeDiscordGatewaySignalV1,
        RuntimeDiscordGatewayTransportStateV1,
    };

    pub(crate) struct TestDiscordGatewayDriverV1 {
        signals: mpsc::UnboundedReceiver<RuntimeDiscordGatewaySignalV1>,
        transport_state: Arc<Mutex<RuntimeDiscordGatewayTransportStateV1>>,
        polls: Arc<AtomicUsize>,
        closes: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
        close_acknowledgement: Option<oneshot::Receiver<()>>,
    }

    impl RuntimeDiscordGatewayDriverV1 for TestDiscordGatewayDriverV1 {
        fn transport_state(&self) -> RuntimeDiscordGatewayTransportStateV1 {
            *self.transport_state.lock().unwrap()
        }

        fn next_signal(
            &mut self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = RuntimeDiscordGatewaySignalV1> + Send + '_>,
        > {
            Box::pin(async move {
                {
                    let mut state = self.transport_state.lock().unwrap();
                    if *state != RuntimeDiscordGatewayTransportStateV1::Active {
                        *state = RuntimeDiscordGatewayTransportStateV1::Connecting;
                    }
                }
                self.polls.fetch_add(1, Ordering::AcqRel);
                let signal = self
                    .signals
                    .recv()
                    .await
                    .unwrap_or(RuntimeDiscordGatewaySignalV1::StreamEnded);
                let mut state = self.transport_state.lock().unwrap();
                match signal {
                    RuntimeDiscordGatewaySignalV1::Ready
                    | RuntimeDiscordGatewaySignalV1::Resumed => {
                        *state = RuntimeDiscordGatewayTransportStateV1::Active;
                    }
                    RuntimeDiscordGatewaySignalV1::Close
                    | RuntimeDiscordGatewaySignalV1::Reconnect
                    | RuntimeDiscordGatewaySignalV1::SessionInvalidated
                    | RuntimeDiscordGatewaySignalV1::ReceiveError
                    | RuntimeDiscordGatewaySignalV1::StreamEnded => {
                        *state = RuntimeDiscordGatewayTransportStateV1::Disconnected;
                    }
                    RuntimeDiscordGatewaySignalV1::FatalReceiveError
                    | RuntimeDiscordGatewaySignalV1::Unrelated => {}
                }
                drop(state);
                signal
            })
        }

        fn close_until(
            &mut self,
            deadline: Instant,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
            self.closes.fetch_add(1, Ordering::AcqRel);
            let acknowledgement = self.close_acknowledgement.take();
            Box::pin(async move {
                let Some(acknowledgement) = acknowledgement else {
                    return true;
                };
                if Instant::now() >= deadline {
                    return false;
                }
                tokio::select! {
                    biased;
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => false,
                    acknowledged = acknowledgement => acknowledged.is_ok() && Instant::now() < deadline,
                }
            })
        }
    }

    impl Drop for TestDiscordGatewayDriverV1 {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::AcqRel);
        }
    }

    pub(crate) fn driver() -> (
        mpsc::UnboundedSender<RuntimeDiscordGatewaySignalV1>,
        TestDiscordGatewayDriverV1,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    ) {
        let (sender, signals) = mpsc::unbounded_channel();
        let polls = Arc::new(AtomicUsize::new(0));
        let closes = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let transport_state =
            Arc::new(Mutex::new(RuntimeDiscordGatewayTransportStateV1::Unstarted));
        (
            sender,
            TestDiscordGatewayDriverV1 {
                signals,
                transport_state,
                polls: polls.clone(),
                closes: closes.clone(),
                drops: drops.clone(),
                close_acknowledgement: None,
            },
            polls,
            closes,
            drops,
        )
    }

    pub(crate) fn delayed_close_driver() -> (
        mpsc::UnboundedSender<RuntimeDiscordGatewaySignalV1>,
        TestDiscordGatewayDriverV1,
        oneshot::Sender<()>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    ) {
        let (signals, mut driver, _polls, closes, drops) = driver();
        let (acknowledgement, close_acknowledgement) = oneshot::channel();
        driver.close_acknowledgement = Some(close_acknowledgement);
        (signals, driver, acknowledgement, closes, drops)
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU64, NonZeroUsize};
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use automation_runtime::{
        shared_gateway_control_channel_with_policy_v3, GatewayAdmissionPolicyV3,
        GatewayControlConfigV3,
    };
    use automation_runtime_controller::RuntimeGatewayReadyKindV2;
    use automation_runtime_convergence::ProcessInstanceId;
    use automation_runtime_worker::RuntimeGatewayCoordinatorGenerationV2;
    use tokio::sync::watch;

    use crate::gateway::{
        compose_runtime_gateway_section_test_bootstrap_v2,
        compose_runtime_gateway_section_test_bootstrap_with_capacity_v2,
        RuntimeGatewayReadyObservationErrorV1,
    };

    use super::test_support::{delayed_close_driver, driver};
    use super::{
        RuntimeDiscordGatewayCloseOutcomeV1, RuntimeDiscordGatewayExitV1,
        RuntimeDiscordGatewayShutdownErrorV1, RuntimeDiscordGatewaySignalV1,
        RuntimeDiscordProcessHandoffFailureV2, RuntimeDiscordProcessHandoffV2,
        RuntimeDiscordRecoveryResumeOwnershipV2, RuntimeDiscordRecoveryResumeV2,
    };

    fn gateway() -> crate::RuntimeGatewayBootstrapV1 {
        compose_runtime_gateway_section_test_bootstrap_v2(
            ProcessInstanceId::parse("runtime-process:discord-test").unwrap(),
        )
    }

    fn gateway_with_lifecycle_capacity(
        lifecycle_capacity: usize,
    ) -> crate::RuntimeGatewayBootstrapV1 {
        compose_runtime_gateway_section_test_bootstrap_with_capacity_v2(
            ProcessInstanceId::parse("runtime-process:discord-capacity-test").unwrap(),
            NonZeroUsize::new(lifecycle_capacity).unwrap(),
        )
    }

    async fn wait_for_epoch(
        gateway: &crate::RuntimeGatewayBootstrapV1,
        expected_epoch: u64,
    ) -> automation_runtime_worker::RuntimePausedGatewayObservationV2 {
        let mut changes = gateway.admission_change_watch_v1();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Ok(observation) = gateway.observe_paused_connected_gateway_v2() {
                    if observation.connection_epoch().get() == expected_epoch {
                        return observation;
                    }
                }
                assert!(changes.changed().await);
            }
        })
        .await
        .unwrap()
    }

    fn expect_process_handoff(
        outcome: RuntimeDiscordProcessHandoffV2,
    ) -> super::RuntimeDiscordProcessSupervisorV2 {
        match outcome {
            RuntimeDiscordProcessHandoffV2::Process(process) => process,
            RuntimeDiscordProcessHandoffV2::NotApplied {
                supervisor,
                failure,
            } => {
                drop(supervisor);
                panic!("Discord process handoff was not applied: {failure:?}")
            }
            RuntimeDiscordProcessHandoffV2::Indeterminate {
                supervisor,
                failure,
            } => {
                drop(supervisor);
                panic!("Discord process handoff was indeterminate: {failure:?}")
            }
        }
    }

    fn expect_applied_resume(
        outcome: RuntimeDiscordRecoveryResumeV2,
    ) -> (
        super::RuntimeDiscordProcessSupervisorV2,
        super::RuntimeDiscordRecoveryResumeEvidenceV2,
    ) {
        match outcome {
            RuntimeDiscordRecoveryResumeV2::Applied {
                supervisor,
                evidence,
            } => (supervisor, evidence),
            RuntimeDiscordRecoveryResumeV2::DefinitelyNotApplied { ownership, failure } => {
                drop(ownership);
                panic!("Discord recovery resume was not applied: {failure:?}")
            }
            RuntimeDiscordRecoveryResumeV2::Indeterminate {
                supervisor,
                failure,
            } => {
                drop(supervisor);
                panic!("Discord recovery resume was indeterminate: {failure:?}")
            }
        }
    }

    #[tokio::test]
    async fn ready_and_resumed_epochs_remain_exactly_paused() {
        let mut gateway = gateway();
        let (signals, driver, _polls, closes, drops) = driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();

        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let ready = wait_for_epoch(&gateway, 1).await;
        assert_eq!(ready.kind(), RuntimeGatewayReadyKindV2::Ready);
        assert!(ready.last_resume_sequence().is_none());
        assert_eq!(
            gateway.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );

        signals
            .send(RuntimeDiscordGatewaySignalV1::Reconnect)
            .unwrap();
        signals
            .send(RuntimeDiscordGatewaySignalV1::Resumed)
            .unwrap();
        let resumed = wait_for_epoch(&gateway, 2).await;
        assert_eq!(resumed.kind(), RuntimeGatewayReadyKindV2::Resumed);
        assert!(resumed.last_resume_sequence().is_none());
        assert_eq!(
            gateway.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        );

        let shutdown = supervisor
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(shutdown.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn lifecycle_capacity_one_supports_ready_reconnect_resume_and_shutdown() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, _polls, closes, drops) = driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();

        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let ready = wait_for_epoch(&gateway, 1).await;
        assert_eq!(ready.kind(), RuntimeGatewayReadyKindV2::Ready);
        signals
            .send(RuntimeDiscordGatewaySignalV1::Reconnect)
            .unwrap();
        signals
            .send(RuntimeDiscordGatewaySignalV1::Resumed)
            .unwrap();
        let resumed = wait_for_epoch(&gateway, 2).await;
        assert_eq!(resumed.kind(), RuntimeGatewayReadyKindV2::Resumed);

        let terminal = supervisor
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn stopped_state_precedes_close_ack_and_terminal_evidence_follows_it() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, close_acknowledgement, closes, drops) = delayed_close_driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        let stopped = supervisor.stopped_watch();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;

        let shutdown = tokio::spawn(supervisor.shutdown_until(
            gateway.begin_discord_drain_v1(),
            Instant::now() + Duration::from_secs(2),
        ));
        tokio::time::timeout(Duration::from_secs(1), async {
            while closes.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            gateway.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::Stopped)
        );
        assert!(!*stopped.borrow());

        close_acknowledgement.send(()).unwrap();
        let terminal = shutdown.await.unwrap().unwrap();
        assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert!(*stopped.borrow());
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn unacknowledged_close_is_bounded_and_reported_separately() {
        let mut gateway = gateway();
        let (signals, driver, _close_acknowledgement, closes, drops) = delayed_close_driver();
        let shutdown_deadline = Instant::now() + Duration::from_millis(500);
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_millis(300),
                shutdown_deadline,
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;

        assert_eq!(
            supervisor
                .shutdown_until(gateway.begin_discord_drain_v1(), shutdown_deadline)
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::CloseDeadlineElapsed)
        );
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn close_timeout_never_erases_the_primary_admission_failure() {
        let (mut control, mut runtime) = shared_gateway_control_channel_with_policy_v3(
            GatewayControlConfigV3::new(NonZeroUsize::MIN, NonZeroUsize::MIN).unwrap(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        );
        assert!(control.next_lifecycle().await.is_some());
        let (lifecycle_drained_sender, mut lifecycle_drained) = watch::channel(1u64);
        let lifecycle_task = tokio::spawn(async move {
            if control.next_lifecycle().await.is_some() {
                lifecycle_drained_sender.send_replace(2);
            }
        });
        let (_signals, mut driver, _close_acknowledgement, _closes, _drops) =
            delayed_close_driver();

        let terminal = super::finish_runtime_discord_gateway_v1(
            &mut driver,
            &mut runtime,
            RuntimeDiscordGatewayExitV1::AdmissionOpened,
            Instant::now() + Duration::from_millis(300),
            &mut lifecycle_drained,
        )
        .await;

        assert_eq!(
            terminal.exit(),
            RuntimeDiscordGatewayExitV1::AdmissionOpened
        );
        assert_eq!(
            terminal.close,
            RuntimeDiscordGatewayCloseOutcomeV1::DeadlineElapsed
        );
        assert!(terminal.control_stopped);
        lifecycle_task.await.unwrap();
    }

    #[tokio::test]
    async fn an_actual_admission_resume_attempt_terminates_the_paused_actor() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, close_acknowledgement, closes, drops) = delayed_close_driver();
        let mut supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;

        assert!(gateway.open_discord_admission_for_test_v1().await);
        tokio::time::timeout(Duration::from_secs(1), async {
            while closes.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            gateway.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::Stopped)
        );

        close_acknowledgement.send(()).unwrap();
        assert_eq!(
            supervisor.wait_terminal().await.exit(),
            RuntimeDiscordGatewayExitV1::AdmissionOpened
        );
        assert!(matches!(
            supervisor
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal))
                if terminal.exit() == RuntimeDiscordGatewayExitV1::AdmissionOpened
        ));
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn lost_handoff_ack_is_bounded_and_returns_shutdown_only_ownership() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (_signals, driver, _polls, closes, drops) = driver();
        let operation_cutoff = Instant::now() + Duration::from_millis(50);
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                operation_cutoff,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();

        let shutdown_only = match supervisor
            .handoff_to_process_losing_acknowledgement_for_test_v2(NonZeroU64::MIN)
            .await
        {
            RuntimeDiscordProcessHandoffV2::Indeterminate {
                supervisor,
                failure: RuntimeDiscordProcessHandoffFailureV2::DeadlineElapsed,
            } => supervisor,
            outcome => {
                drop(outcome);
                panic!("lost Discord handoff acknowledgement was not indeterminate")
            }
        };
        assert!(Instant::now() >= operation_cutoff);
        let terminal = shutdown_only
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                NonZeroU64::MIN,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert_eq!(closes.load(Ordering::Acquire), 0);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn canceled_in_place_handoff_retains_shutdown_only_join_authority() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (_signals, driver, _polls, closes, drops) = driver();
        let mut supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(1),
                Instant::now() + Duration::from_secs(2),
            )
            .await
            .unwrap();
        let mut handoff = Box::pin(
            supervisor.handoff_to_process_in_place_with_response_v2(NonZeroU64::MIN, false),
        );
        tokio::select! {
            biased;
            () = &mut handoff => panic!("in-place handoff unexpectedly completed"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        drop(handoff);
        let shutdown_only = match supervisor.into_process_handoff_v2() {
            RuntimeDiscordProcessHandoffV2::Indeterminate {
                supervisor,
                failure: RuntimeDiscordProcessHandoffFailureV2::AcknowledgementLost,
            } => supervisor,
            outcome => {
                drop(outcome);
                panic!("canceled Discord handoff returned retryable ownership")
            }
        };
        let terminal = shutdown_only
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                NonZeroU64::MIN,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert_eq!(closes.load(Ordering::Acquire), 0);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn process_handoff_survives_startup_cutoff_and_rearms_resume_per_epoch() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, _polls, closes, drops) = driver();
        let operation_cutoff = Instant::now() + Duration::from_secs(1);
        let startup_cleanup_deadline = operation_cutoff + Duration::from_millis(50);
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                operation_cutoff,
                startup_cleanup_deadline,
            )
            .await
            .unwrap();
        let observation = supervisor.observation_v1();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;
        let first_reservation = gateway
            .discord_pause_reservation_for_test_v2()
            .expect("first Discord pause reservation");
        let process =
            expect_process_handoff(supervisor.handoff_to_process_v2(NonZeroU64::MIN).await);

        tokio::time::sleep_until(tokio::time::Instant::from_std(
            operation_cutoff + Duration::from_millis(25),
        ))
        .await;
        assert_eq!(observation.terminal_status(), None);
        assert!(!observation.is_finished());
        let (process, first_evidence) = expect_applied_resume(
            process
                .resume_reserved_admission_v2(
                    RuntimeGatewayCoordinatorGenerationV2::FIRST,
                    first_reservation,
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
        );
        assert_eq!(
            first_evidence.coordinator_generation_v2(),
            RuntimeGatewayCoordinatorGenerationV2::FIRST
        );
        assert_eq!(first_evidence.expected_v2(), first_reservation);
        assert_eq!(
            first_evidence.admission_v2().connection().current_epoch(),
            Some(first_reservation.epoch())
        );
        assert_eq!(first_evidence.ready_v2().epoch(), first_reservation.epoch());
        assert!(gateway.observe_current_ready_attestation().is_ok());

        signals
            .send(RuntimeDiscordGatewaySignalV1::ReceiveError)
            .unwrap();
        signals
            .send(RuntimeDiscordGatewaySignalV1::Resumed)
            .unwrap();
        let _resumed = wait_for_epoch(&gateway, 2).await;
        let second_reservation = gateway
            .discord_pause_reservation_for_test_v2()
            .expect("second Discord pause reservation");
        assert_ne!(first_reservation, second_reservation);
        let (process, second_evidence) = expect_applied_resume(
            process
                .resume_reserved_admission_v2(
                    RuntimeGatewayCoordinatorGenerationV2::FIRST,
                    second_reservation,
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
        );
        assert_eq!(second_evidence.expected_v2(), second_reservation);
        assert!(gateway.observe_current_ready_attestation().is_ok());

        let terminal = process
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                NonZeroU64::MIN,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn lost_recovery_resume_ack_exact_observes_without_competing_resend() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, _polls, closes, drops) = driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;
        let reservation = gateway
            .discord_pause_reservation_for_test_v2()
            .expect("Discord pause reservation");
        let process =
            expect_process_handoff(supervisor.handoff_to_process_v2(NonZeroU64::MIN).await);
        let (process, evidence) = expect_applied_resume(
            process
                .resume_reserved_admission_losing_acknowledgement_for_test_v2(
                    RuntimeGatewayCoordinatorGenerationV2::FIRST,
                    reservation,
                    Instant::now() + Duration::from_millis(100),
                )
                .await,
        );
        assert_eq!(evidence.expected_v2(), reservation);
        assert!(gateway.observe_current_ready_attestation().is_ok());
        assert_eq!(
            gateway
                .observe_current_ready_attestation()
                .unwrap()
                .admission_revision
                .get(),
            evidence.ready_v2().admission_revision().get()
        );
        let terminal = process
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                NonZeroU64::MIN,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn deadline_before_resume_enqueue_is_definitely_not_applied() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, _polls, closes, drops) = driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;
        let reservation = gateway
            .discord_pause_reservation_for_test_v2()
            .expect("Discord pause reservation");
        let process =
            expect_process_handoff(supervisor.handoff_to_process_v2(NonZeroU64::MIN).await);
        let process = match process
            .resume_reserved_admission_v2(
                RuntimeGatewayCoordinatorGenerationV2::FIRST,
                reservation,
                Instant::now(),
            )
            .await
        {
            RuntimeDiscordRecoveryResumeV2::DefinitelyNotApplied {
                ownership: RuntimeDiscordRecoveryResumeOwnershipV2::Process(supervisor),
                failure: super::RuntimeDiscordRecoveryResumeFailureV2::DeadlineElapsed,
            } => supervisor,
            outcome => {
                drop(outcome);
                panic!("expired pre-enqueue resume was not definitely unapplied")
            }
        };
        assert!(matches!(
            gateway.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        ));
        assert_eq!(
            gateway.discord_pause_reservation_for_test_v2(),
            Some(reservation)
        );
        let (process, _) = expect_applied_resume(
            process
                .resume_reserved_admission_v2(
                    RuntimeGatewayCoordinatorGenerationV2::FIRST,
                    reservation,
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
        );
        let terminal = process
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                NonZeroU64::MIN,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn reconnect_invalidates_lost_ack_evidence_without_resend() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, _polls, closes, drops) = driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;
        let reservation = gateway
            .discord_pause_reservation_for_test_v2()
            .expect("Discord pause reservation");
        let process =
            expect_process_handoff(supervisor.handoff_to_process_v2(NonZeroU64::MIN).await);
        let resume = tokio::spawn(
            process.resume_reserved_admission_losing_acknowledgement_for_test_v2(
                RuntimeGatewayCoordinatorGenerationV2::FIRST,
                reservation,
                Instant::now() + Duration::from_millis(250),
            ),
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if gateway.observe_current_ready_attestation().is_ok() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        signals
            .send(RuntimeDiscordGatewaySignalV1::ReceiveError)
            .unwrap();
        signals
            .send(RuntimeDiscordGatewaySignalV1::Resumed)
            .unwrap();
        let _resumed = wait_for_epoch(&gateway, 2).await;
        let shutdown_only = match resume.await.unwrap() {
            RuntimeDiscordRecoveryResumeV2::Indeterminate {
                supervisor,
                failure: super::RuntimeDiscordRecoveryResumeFailureV2::DeadlineElapsed,
            } => supervisor,
            outcome => {
                drop(outcome);
                panic!("reconnected lost-ack resume was not indeterminate")
            }
        };
        assert!(matches!(
            gateway.observe_current_ready_attestation(),
            Err(RuntimeGatewayReadyObservationErrorV1::AdmissionPaused)
        ));
        let terminal = shutdown_only
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                NonZeroU64::MIN,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn canceled_in_place_resume_retains_same_actor_without_resend() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, _polls, closes, drops) = driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        let observation = supervisor.observation_v1();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;
        let reservation = gateway
            .discord_pause_reservation_for_test_v2()
            .expect("Discord pause reservation");
        let mut process =
            expect_process_handoff(supervisor.handoff_to_process_v2(NonZeroU64::MIN).await);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut resume = Box::pin(
            process.resume_reserved_admission_in_place_losing_acknowledgement_for_test_v2(
                RuntimeGatewayCoordinatorGenerationV2::FIRST,
                reservation,
                deadline,
            ),
        );
        tokio::select! {
            biased;
            _ = &mut resume => panic!("suppressed resume acknowledgement completed early"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        drop(resume);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if gateway.observe_current_ready_attestation().is_ok() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let attempt = process
            .resume_reserved_admission_in_place_v2(
                RuntimeGatewayCoordinatorGenerationV2::FIRST,
                reservation,
                deadline,
            )
            .await;
        assert!(matches!(
            attempt,
            super::RuntimeDiscordRecoveryResumeAttemptV2::Applied(evidence)
                if evidence.expected_v2() == reservation
        ));
        assert!(!observation.is_finished());
        let terminal = process
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                NonZeroU64::MIN,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn stale_process_resume_is_terminal_and_retains_shutdown_only_ownership() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, _polls, closes, drops) = driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;
        let stale = gateway
            .discord_pause_reservation_for_test_v2()
            .expect("initial Discord pause reservation");
        let process =
            expect_process_handoff(supervisor.handoff_to_process_v2(NonZeroU64::MIN).await);
        signals
            .send(RuntimeDiscordGatewaySignalV1::ReceiveError)
            .unwrap();
        signals
            .send(RuntimeDiscordGatewaySignalV1::Resumed)
            .unwrap();
        let _resumed = wait_for_epoch(&gateway, 2).await;

        let shutdown_only = match process
            .resume_reserved_admission_v2(
                RuntimeGatewayCoordinatorGenerationV2::FIRST,
                stale,
                Instant::now() + Duration::from_secs(1),
            )
            .await
        {
            RuntimeDiscordRecoveryResumeV2::DefinitelyNotApplied {
                ownership: RuntimeDiscordRecoveryResumeOwnershipV2::ShutdownOnly(supervisor),
                ..
            } => supervisor,
            outcome => {
                drop(outcome);
                panic!("stale reserved resume did not fail closed")
            }
        };
        assert!(matches!(
            shutdown_only
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    NonZeroU64::MIN,
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal))
                if terminal.exit() == RuntimeDiscordGatewayExitV1::AdmissionOpened
        ));
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn unauthorized_process_resume_remains_terminal() {
        let mut gateway = gateway_with_lifecycle_capacity(1);
        let (signals, driver, _polls, closes, drops) = driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;
        let process =
            expect_process_handoff(supervisor.handoff_to_process_v2(NonZeroU64::MIN).await);

        assert!(gateway.open_discord_admission_for_test_v1().await);
        assert!(matches!(
            process
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    NonZeroU64::MIN,
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal))
                if terminal.exit() == RuntimeDiscordGatewayExitV1::AdmissionOpened
        ));
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn operation_cutoff_prevents_the_driver_from_being_polled() {
        let mut gateway = gateway();
        let (_signals, driver, polls, closes, drops) = driver();
        let result = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now(),
                Instant::now() + Duration::from_secs(1),
            )
            .await;

        assert!(matches!(
            result,
            Err(super::RuntimeDiscordGatewayStartErrorV1::OperationDeadlineElapsed)
        ));
        assert_eq!(drops.load(Ordering::Acquire), 1);
        assert_eq!(polls.load(Ordering::Acquire), 0);
        assert_eq!(closes.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn elapsed_cutoff_behind_start_gate_never_touches_the_driver() {
        let mut gateway = gateway();
        let (_signals, driver, polls, closes, drops) = driver();
        let operation_cutoff = Instant::now() + Duration::from_millis(25);
        let mut supervisor = gateway
            .start_discord_gateway_with_driver_before_release_v1(
                driver,
                operation_cutoff,
                Instant::now() + Duration::from_secs(1),
                |_| std::thread::sleep(Duration::from_millis(50)),
            )
            .await
            .unwrap();

        assert_eq!(
            supervisor.wait_terminal().await.exit(),
            RuntimeDiscordGatewayExitV1::StartDeadlineElapsed
        );
        assert_eq!(polls.load(Ordering::Acquire), 0);
        assert_eq!(closes.load(Ordering::Acquire), 0);
        assert_eq!(
            gateway.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::Stopped)
        );
        assert!(matches!(
            supervisor
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal))
                if terminal.exit() == RuntimeDiscordGatewayExitV1::StartDeadlineElapsed
        ));
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn cutoff_after_a_receive_error_never_starts_a_reconnect_during_close() {
        let mut gateway = gateway();
        let (signals, driver, polls, closes, drops) = driver();
        let operation_cutoff = Instant::now() + Duration::from_millis(150);
        let mut supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                operation_cutoff,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;
        let mut changes = gateway.admission_change_watch_v1();

        signals
            .send(RuntimeDiscordGatewaySignalV1::ReceiveError)
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), changes.changed())
            .await
            .unwrap();
        assert_eq!(
            supervisor.wait_terminal().await.exit(),
            RuntimeDiscordGatewayExitV1::StartDeadlineElapsed
        );
        assert!(polls.load(Ordering::Acquire) >= 2);
        assert_eq!(closes.load(Ordering::Acquire), 0);
        assert!(matches!(
            supervisor
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal))
                if terminal.exit() == RuntimeDiscordGatewayExitV1::StartDeadlineElapsed
        ));
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn cutoff_after_ready_drops_the_active_transport_without_close_io() {
        let mut gateway = gateway();
        let (signals, driver, _polls, closes, drops) = driver();
        let operation_cutoff = Instant::now() + Duration::from_millis(100);
        let mut supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                operation_cutoff,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;

        assert_eq!(
            supervisor.wait_terminal().await.exit(),
            RuntimeDiscordGatewayExitV1::StartDeadlineElapsed
        );
        assert_eq!(closes.load(Ordering::Acquire), 0);
        assert!(matches!(
            supervisor
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal))
                if terminal.exit() == RuntimeDiscordGatewayExitV1::StartDeadlineElapsed
        ));
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn cutoff_drops_a_pending_handshake_without_polling_close() {
        let mut gateway = gateway();
        let (_signals, driver, polls, closes, drops) = driver();
        let operation_cutoff = Instant::now() + Duration::from_millis(100);
        let mut supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                operation_cutoff,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while polls.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        assert_eq!(
            supervisor.wait_terminal().await.exit(),
            RuntimeDiscordGatewayExitV1::StartDeadlineElapsed
        );
        assert_eq!(closes.load(Ordering::Acquire), 0);
        assert!(matches!(
            supervisor
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal))
                if terminal.exit() == RuntimeDiscordGatewayExitV1::StartDeadlineElapsed
        ));
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn fatal_parse_after_ready_closes_the_active_transport_once() {
        let mut gateway = gateway();
        let (signals, driver, _polls, closes, drops) = driver();
        let mut supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(1),
                Instant::now() + Duration::from_secs(2),
            )
            .await
            .unwrap();
        signals.send(RuntimeDiscordGatewaySignalV1::Ready).unwrap();
        let _ready = wait_for_epoch(&gateway, 1).await;
        signals
            .send(RuntimeDiscordGatewaySignalV1::FatalReceiveError)
            .unwrap();

        assert_eq!(
            supervisor.wait_terminal().await.exit(),
            RuntimeDiscordGatewayExitV1::RuntimeFailure
        );
        assert_eq!(closes.load(Ordering::Acquire), 1);
        assert!(matches!(
            supervisor
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal))
                if terminal.exit() == RuntimeDiscordGatewayExitV1::RuntimeFailure
        ));
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn stream_end_is_terminal_and_a_second_runtime_start_is_rejected() {
        let mut gateway = gateway();
        let (signals, first_driver, _polls, closes, drops) = driver();
        let mut supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                first_driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        let (_second_signals, second, _second_polls, _second_closes, _second_drops) = driver();

        assert!(matches!(
            gateway
                .start_discord_gateway_with_driver_v1(
                    second,
                    Instant::now() + Duration::from_secs(2),
                    Instant::now() + Duration::from_secs(3),
                )
                .await,
            Err(super::RuntimeDiscordGatewayStartErrorV1::RuntimeHalfUnavailable)
        ));
        drop(signals);
        assert_eq!(
            supervisor.wait_terminal().await.exit(),
            RuntimeDiscordGatewayExitV1::StreamEnded
        );
        assert!(matches!(
            supervisor
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::UnexpectedExit(terminal))
                if terminal.exit() == RuntimeDiscordGatewayExitV1::StreamEnded
        ));
        assert_eq!(closes.load(Ordering::Acquire), 0);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn owner_invalidation_aborts_the_attached_discord_task() {
        let mut gateway = gateway();
        let (_signals, driver, _polls, _closes, _drops) = driver();
        let mut supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        let mut stopped = supervisor.stopped_watch();

        gateway.invalidate_owner_for_discord_test_v1();
        assert_eq!(
            supervisor.wait_terminal().await.exit(),
            RuntimeDiscordGatewayExitV1::RuntimeFailure
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while !*stopped.borrow() {
                assert!(stopped.changed().await.is_ok());
            }
        })
        .await
        .unwrap();
        assert!(*stopped.borrow());
        assert_eq!(
            gateway.observe_paused_connected_gateway_v2(),
            Err(RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
        );
        assert_eq!(
            supervisor
                .shutdown_until(
                    gateway.begin_discord_drain_v1(),
                    Instant::now() + Duration::from_secs(1),
                )
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::TaskStopped)
        );
    }

    #[tokio::test]
    async fn owner_invalidation_before_start_gate_prevents_driver_polling() {
        let mut gateway = gateway();
        let (_signals, driver, polls, _closes, drops) = driver();

        assert!(matches!(
            gateway
                .start_discord_gateway_with_driver_before_release_v1(
                    driver,
                    Instant::now() + Duration::from_secs(2),
                    Instant::now() + Duration::from_secs(3),
                    |gateway| gateway.invalidate_owner_for_discord_test_v1(),
                )
                .await,
            Err(super::RuntimeDiscordGatewayStartErrorV1::OwnerInvalidated)
        ));
        tokio::time::timeout(Duration::from_secs(1), async {
            while drops.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(polls.load(Ordering::Acquire), 0);
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn elapsed_shutdown_aborts_joins_and_drops_the_driver() {
        let mut gateway = gateway();
        let (_signals, driver, _polls, _closes, drops) = driver();
        let supervisor = gateway
            .start_discord_gateway_with_driver_v1(
                driver,
                Instant::now() + Duration::from_secs(2),
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap();
        let mut stopped = supervisor.stopped_watch();

        assert_eq!(
            supervisor
                .shutdown_until(std::future::pending::<bool>(), Instant::now())
                .await,
            Err(RuntimeDiscordGatewayShutdownErrorV1::DeadlineElapsed)
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while !*stopped.borrow() {
                assert!(stopped.changed().await.is_ok());
            }
        })
        .await
        .unwrap();
        assert!(*stopped.borrow());
        assert_eq!(drops.load(Ordering::Acquire), 1);
    }
}
