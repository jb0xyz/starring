use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use automation_runtime::{
    GatewayCommandAckV3, GatewayDisconnectKindV3, GatewayReadyKindV3,
    GatewayRuntimeCommandOutcomeV3, SharedGatewayRuntimeControlV3,
};
use paused_discord_gateway::error::ReceiveMessageErrorType;
use paused_discord_gateway::{
    CloseFrame, Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt,
};
use tokio::sync::{oneshot, watch};
use tokio::task::{AbortHandle, JoinHandle};
use tokio::time::{sleep_until, timeout_at, Instant as TokioInstant};

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
}

pub(crate) struct RuntimeDiscordGatewayActorStartV1 {
    pub(crate) control: SharedGatewayRuntimeControlV3,
    pub(crate) operation_cutoff: Instant,
    pub(crate) shutdown_deadline: Instant,
    pub(crate) lifecycle_drained: watch::Receiver<u64>,
    pub(crate) runtime: tokio::runtime::Handle,
    pub(crate) control_task: RuntimeDiscordControlTaskV1,
    pub(crate) stopped_sender: watch::Sender<bool>,
    pub(crate) stopped: watch::Receiver<bool>,
}

pub(crate) struct RuntimeDiscordControlTaskV1 {
    task: Option<JoinHandle<()>>,
}

impl RuntimeDiscordControlTaskV1 {
    pub(crate) fn new(task: JoinHandle<()>) -> Self {
        Self { task: Some(task) }
    }

    fn into_inner(mut self) -> JoinHandle<()> {
        self.task.take().expect("runtime Discord control task")
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
        mut self,
        begin_drain: F,
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
        runtime,
        control_task,
        stopped_sender,
        stopped,
    } = start;
    let control_task = control_task.into_inner();
    let (terminal_sender, terminal) = watch::channel(None);
    let (start, start_receiver) = oneshot::channel();
    let publisher = RuntimeDiscordGatewayTerminalPublisherV1::new(terminal_sender);
    let actor_task = runtime.spawn(async move {
        let mut publisher = publisher;
        let terminal = if start_receiver.await.is_ok() {
            run_runtime_discord_gateway_v1(
                driver,
                control,
                operation_cutoff,
                shutdown_deadline,
                lifecycle_drained,
            )
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

async fn run_runtime_discord_gateway_v1<D>(
    mut driver: D,
    mut control: SharedGatewayRuntimeControlV3,
    operation_cutoff: Instant,
    shutdown_deadline: Instant,
    mut lifecycle_drained: watch::Receiver<u64>,
) -> RuntimeDiscordGatewayTerminalV1
where
    D: RuntimeDiscordGatewayDriverV1,
{
    if Instant::now() >= operation_cutoff {
        return finish_runtime_discord_gateway_without_transport_v1(
            &mut control,
            RuntimeDiscordGatewayExitV1::StartDeadlineElapsed,
            shutdown_deadline,
            &mut lifecycle_drained,
        )
        .await;
    }
    loop {
        let lifecycle_sequence = *lifecycle_drained.borrow();
        tokio::select! {
            biased;
            _ = sleep_until(TokioInstant::from_std(operation_cutoff)) => {
                return finish_runtime_discord_gateway_without_transport_v1(
                    &mut control,
                    RuntimeDiscordGatewayExitV1::StartDeadlineElapsed,
                    shutdown_deadline,
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
                            shutdown_deadline,
                        )
                        .await
                        {
                            return finish_runtime_discord_gateway_if_connected_v1(
                                &mut driver,
                                &mut control,
                                RuntimeDiscordGatewayExitV1::RuntimeFailure,
                                shutdown_deadline,
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
                            shutdown_deadline,
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
                            shutdown_deadline,
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
                            shutdown_deadline,
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    GatewayRuntimeCommandOutcomeV3::Rejected(_) => {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::RuntimeFailure,
                            shutdown_deadline,
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    GatewayRuntimeCommandOutcomeV3::ControlOrphaned => {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::ControlOrphaned,
                            shutdown_deadline,
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
                            shutdown_deadline,
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    RuntimeDiscordGatewaySignalV1::StreamEnded => {
                        return finish_runtime_discord_gateway_without_transport_v1(
                            &mut control,
                            RuntimeDiscordGatewayExitV1::StreamEnded,
                            shutdown_deadline,
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
                            shutdown_deadline,
                        )
                        .await =>
                    {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::RuntimeFailure,
                            shutdown_deadline,
                            &mut lifecycle_drained,
                        )
                        .await;
                    }
                    Err(_) => {
                        return finish_runtime_discord_gateway_if_connected_v1(
                            &mut driver,
                            &mut control,
                            RuntimeDiscordGatewayExitV1::RuntimeFailure,
                            shutdown_deadline,
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
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use automation_runtime::{
        shared_gateway_control_channel_with_policy_v3, GatewayAdmissionPolicyV3,
        GatewayControlConfigV3,
    };
    use automation_runtime_controller::RuntimeGatewayReadyKindV2;
    use automation_runtime_convergence::ProcessInstanceId;
    use tokio::sync::{mpsc, oneshot, watch};

    use crate::gateway::{
        compose_runtime_gateway_section_test_bootstrap_v2,
        compose_runtime_gateway_section_test_bootstrap_with_capacity_v2,
        RuntimeGatewayReadyObservationErrorV1,
    };

    use super::{
        RuntimeDiscordGatewayCloseOutcomeV1, RuntimeDiscordGatewayDriverV1,
        RuntimeDiscordGatewayExitV1, RuntimeDiscordGatewayShutdownErrorV1,
        RuntimeDiscordGatewaySignalV1,
    };

    struct TestDiscordGatewayDriverV1 {
        signals: mpsc::UnboundedReceiver<RuntimeDiscordGatewaySignalV1>,
        transport_state: Arc<Mutex<super::RuntimeDiscordGatewayTransportStateV1>>,
        polls: Arc<AtomicUsize>,
        closes: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
        close_acknowledgement: Option<oneshot::Receiver<()>>,
    }

    impl RuntimeDiscordGatewayDriverV1 for TestDiscordGatewayDriverV1 {
        fn transport_state(&self) -> super::RuntimeDiscordGatewayTransportStateV1 {
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
                    if *state != super::RuntimeDiscordGatewayTransportStateV1::Active {
                        *state = super::RuntimeDiscordGatewayTransportStateV1::Connecting;
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
                        *state = super::RuntimeDiscordGatewayTransportStateV1::Active;
                    }
                    RuntimeDiscordGatewaySignalV1::Close
                    | RuntimeDiscordGatewaySignalV1::Reconnect
                    | RuntimeDiscordGatewaySignalV1::SessionInvalidated
                    | RuntimeDiscordGatewaySignalV1::ReceiveError
                    | RuntimeDiscordGatewaySignalV1::StreamEnded => {
                        *state = super::RuntimeDiscordGatewayTransportStateV1::Disconnected;
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

    fn driver() -> (
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
        let transport_state = Arc::new(Mutex::new(
            super::RuntimeDiscordGatewayTransportStateV1::Unstarted,
        ));
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

    fn delayed_close_driver() -> (
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
