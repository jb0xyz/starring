use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{timeout_at, Instant as TokioInstant, MissedTickBehavior};

use crate::database::{RuntimeDatabaseCompositionErrorV1, RuntimeDatabaseReadinessProbeV2};
use crate::process_supervisor::RuntimeProcessInvalidationTriggerV1;
use crate::RuntimeShutdownCauseV1;

const CAPABILITY_READINESS_CONTROL_CAPACITY_V2: usize = 1;
const CAPABILITY_READINESS_CADENCE_V2: Duration = Duration::from_secs(1);
const CAPABILITY_READINESS_VERIFY_TIMEOUT_V2: Duration = Duration::from_secs(5);
const CAPABILITY_READINESS_TRANSIENT_GRACE_V2: Duration = Duration::from_secs(5);

type RuntimeCapabilityReadinessProbeFutureV2<'a> =
    Pin<Box<dyn Future<Output = RuntimeCapabilityReadinessProbeDispositionV2> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeCapabilityReadinessProbeDispositionV2 {
    Available,
    Failed(RuntimeCapabilityReadinessProbeFailureV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeCapabilityReadinessProbeFailureV2 {
    class: RuntimeCapabilityReadinessProbeFailureClassV2,
    code: &'static str,
    context: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeCapabilityReadinessProbeFailureClassV2 {
    Retryable,
    AuthorityLost,
    ProtocolViolation,
}

pub(crate) trait RuntimeCapabilityReadinessProbePortV2:
    Clone + Send + Sync + 'static
{
    fn verify_capability_readiness_v2(&self) -> RuntimeCapabilityReadinessProbeFutureV2<'_>;
}

impl RuntimeCapabilityReadinessProbePortV2 for RuntimeDatabaseReadinessProbeV2 {
    fn verify_capability_readiness_v2(&self) -> RuntimeCapabilityReadinessProbeFutureV2<'_> {
        Box::pin(async move {
            match self.verify_v2().await {
                Ok(_) => RuntimeCapabilityReadinessProbeDispositionV2::Available,
                Err(error) => RuntimeCapabilityReadinessProbeDispositionV2::Failed(
                    capability_readiness_database_failure_v2(error),
                ),
            }
        })
    }
}

fn capability_readiness_database_failure_v2(
    error: RuntimeDatabaseCompositionErrorV1,
) -> RuntimeCapabilityReadinessProbeFailureV2 {
    let class = match error {
        RuntimeDatabaseCompositionErrorV1::Unavailable { .. }
        | RuntimeDatabaseCompositionErrorV1::ReadinessUnavailable { .. }
        | RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut => {
            RuntimeCapabilityReadinessProbeFailureClassV2::Retryable
        }
        RuntimeDatabaseCompositionErrorV1::ReadinessAuthorityMismatch { .. }
        | RuntimeDatabaseCompositionErrorV1::AuthorityMismatch => {
            RuntimeCapabilityReadinessProbeFailureClassV2::AuthorityLost
        }
        RuntimeDatabaseCompositionErrorV1::InvalidConfiguration
        | RuntimeDatabaseCompositionErrorV1::ConnectionConfiguration { .. }
        | RuntimeDatabaseCompositionErrorV1::UnsafeTransport { .. }
        | RuntimeDatabaseCompositionErrorV1::IdentityVerification
        | RuntimeDatabaseCompositionErrorV1::ReadinessRejected { .. }
        | RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut => {
            RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation
        }
    };
    RuntimeCapabilityReadinessProbeFailureV2 {
        class,
        code: error.code(),
        context: error.context(),
    }
}

fn capability_readiness_timeout_failure_v2() -> RuntimeCapabilityReadinessProbeFailureV2 {
    RuntimeCapabilityReadinessProbeFailureV2 {
        class: RuntimeCapabilityReadinessProbeFailureClassV2::Retryable,
        code: "runtime_capability_readiness_probe_timed_out",
        context: None,
    }
}

fn capability_readiness_periodic_deadline_v2(
    attempt_started_at: Instant,
    verify_timeout: Duration,
    retryable_episode_started_at: Option<Instant>,
    transient_grace: Duration,
) -> Instant {
    let verify_deadline = attempt_started_at + verify_timeout;
    retryable_episode_started_at
        .map(|started_at| started_at + transient_grace)
        .map_or(verify_deadline, |episode_deadline| {
            verify_deadline.min(episode_deadline)
        })
}

fn emit_capability_readiness_status_v2(
    status: &'static str,
    failure: RuntimeCapabilityReadinessProbeFailureV2,
    attempts: u64,
    elapsed: Duration,
) {
    let mut stderr = std::io::stderr().lock();
    if let Some(context) = failure.context {
        let _write_result = writeln!(
            stderr,
            "starring_runtime_status={status} component=capability_readiness stage=periodic class={:?} code={} context={} attempts={} elapsed_milliseconds={}",
            failure.class,
            failure.code,
            context,
            attempts,
            elapsed.as_millis(),
        );
    } else {
        let _write_result = writeln!(
            stderr,
            "starring_runtime_status={status} component=capability_readiness stage=periodic class={:?} code={} attempts={} elapsed_milliseconds={}",
            failure.class,
            failure.code,
            attempts,
            elapsed.as_millis(),
        );
    }
}

pub(crate) trait RuntimeCapabilityReadinessInvalidationPortV2:
    Clone + Send + Sync + 'static
{
    fn invalidate_readiness_v2(&self);
}

impl RuntimeCapabilityReadinessInvalidationPortV2 for RuntimeProcessInvalidationTriggerV1 {
    fn invalidate_readiness_v2(&self) {
        self.trip(RuntimeShutdownCauseV1::ReadinessLost);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RuntimeCapabilityReadinessSupervisorConfigV2 {
    cadence: Duration,
    verify_timeout: Duration,
    transient_grace: Duration,
}

impl RuntimeCapabilityReadinessSupervisorConfigV2 {
    const fn production_v2() -> Self {
        Self {
            cadence: CAPABILITY_READINESS_CADENCE_V2,
            verify_timeout: CAPABILITY_READINESS_VERIFY_TIMEOUT_V2,
            transient_grace: CAPABILITY_READINESS_TRANSIENT_GRACE_V2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeCapabilityReadinessSupervisorExitV2 {
    Commanded,
    ReadinessLost,
    ControlClosed,
    Panicked,
    DeadlineElapsed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeCapabilityReadinessActivationErrorV2 {
    #[error("runtime capability readiness activation deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime capability readiness activation was sealed")]
    Sealed,
    #[error("runtime capability readiness activation was already consumed")]
    AlreadyActivated,
    #[error("runtime capability readiness supervisor control closed")]
    ControlClosed,
    #[error("runtime capability readiness supervisor response was lost")]
    ResponseLost,
    #[error("runtime capability readiness verification failed")]
    ReadinessUnavailable,
    #[error("runtime capability readiness verification timed out")]
    ReadinessTimedOut,
}

struct RuntimeCapabilityReadinessActivationRequestV2<P> {
    probe: P,
    deadline: Instant,
    response: oneshot::Sender<Result<(), RuntimeCapabilityReadinessActivationErrorV2>>,
}

#[derive(Clone)]
pub(crate) struct RuntimeCapabilityReadinessShutdownHandleV2 {
    sender: watch::Sender<Option<Instant>>,
}

impl RuntimeCapabilityReadinessShutdownHandleV2 {
    pub(crate) fn seal_until_v2(&self, deadline: Instant) {
        self.sender.send_if_modified(|current| {
            let next = current.map_or(deadline, |observed| observed.min(deadline));
            if *current == Some(next) {
                false
            } else {
                *current = Some(next);
                true
            }
        });
    }

    #[cfg(test)]
    pub(crate) fn is_sealed_v2(&self) -> bool {
        self.sender.borrow().is_some()
    }
}

impl Debug for RuntimeCapabilityReadinessShutdownHandleV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCapabilityReadinessShutdownHandleV2(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeCapabilityReadinessTerminalObserverV2 {
    receiver: watch::Receiver<Option<RuntimeCapabilityReadinessSupervisorExitV2>>,
}

impl RuntimeCapabilityReadinessTerminalObserverV2 {
    pub(crate) async fn wait_v2(&mut self) -> RuntimeCapabilityReadinessSupervisorExitV2 {
        loop {
            if let Some(exit) = *self.receiver.borrow() {
                return exit;
            }
            if self.receiver.changed().await.is_err() {
                return RuntimeCapabilityReadinessSupervisorExitV2::Panicked;
            }
        }
    }
}

impl Debug for RuntimeCapabilityReadinessTerminalObserverV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCapabilityReadinessTerminalObserverV2(<redacted>)")
    }
}

struct RuntimeCapabilityReadinessActivationHandleV2<P> {
    sender: mpsc::Sender<RuntimeCapabilityReadinessActivationRequestV2<P>>,
    shutdown: watch::Receiver<Option<Instant>>,
}

impl<P> RuntimeCapabilityReadinessActivationHandleV2<P>
where
    P: RuntimeCapabilityReadinessProbePortV2,
{
    async fn activate_until_v2(
        self,
        probe: P,
        deadline: Instant,
    ) -> Result<(), RuntimeCapabilityReadinessActivationErrorV2> {
        if Instant::now() >= deadline {
            return Err(RuntimeCapabilityReadinessActivationErrorV2::DeadlineElapsed);
        }
        let mut shutdown = self.shutdown;
        if shutdown.borrow().is_some() {
            return Err(RuntimeCapabilityReadinessActivationErrorV2::Sealed);
        }
        let (response, result) = oneshot::channel();
        let request = RuntimeCapabilityReadinessActivationRequestV2 {
            probe,
            deadline,
            response,
        };
        let send = timeout_at(TokioInstant::from_std(deadline), self.sender.send(request));
        tokio::pin!(send);
        let send_result = tokio::select! {
            biased;
            changed = shutdown.changed() => {
                let _closed = changed.is_err();
                return Err(RuntimeCapabilityReadinessActivationErrorV2::Sealed);
            }
            result = &mut send => result,
        };
        match send_result {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                return Err(RuntimeCapabilityReadinessActivationErrorV2::ControlClosed);
            }
            Err(_) => {
                return Err(RuntimeCapabilityReadinessActivationErrorV2::DeadlineElapsed);
            }
        }
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                let _closed = changed.is_err();
                Err(RuntimeCapabilityReadinessActivationErrorV2::Sealed)
            }
            result = result => {
                result.unwrap_or(Err(RuntimeCapabilityReadinessActivationErrorV2::ResponseLost))
            }
        }
    }
}

impl<P> Debug for RuntimeCapabilityReadinessActivationHandleV2<P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCapabilityReadinessActivationHandleV2(<redacted>)")
    }
}

pub(crate) struct RuntimeCapabilityReadinessSupervisorV2<P = RuntimeDatabaseReadinessProbeV2> {
    activation: Option<RuntimeCapabilityReadinessActivationHandleV2<P>>,
    shutdown: RuntimeCapabilityReadinessShutdownHandleV2,
    terminal: RuntimeCapabilityReadinessTerminalObserverV2,
    task: Option<JoinHandle<RuntimeCapabilityReadinessSupervisorExitV2>>,
}

pub(crate) struct RuntimeCapabilityReadinessPreparedV2<P = RuntimeDatabaseReadinessProbeV2> {
    activation_sender: mpsc::Sender<RuntimeCapabilityReadinessActivationRequestV2<P>>,
    activation_receiver: mpsc::Receiver<RuntimeCapabilityReadinessActivationRequestV2<P>>,
    shutdown_sender: watch::Sender<Option<Instant>>,
    shutdown_receiver: watch::Receiver<Option<Instant>>,
    terminal_sender: watch::Sender<Option<RuntimeCapabilityReadinessSupervisorExitV2>>,
    terminal_receiver: watch::Receiver<Option<RuntimeCapabilityReadinessSupervisorExitV2>>,
    config: RuntimeCapabilityReadinessSupervisorConfigV2,
}

impl<P> RuntimeCapabilityReadinessPreparedV2<P>
where
    P: RuntimeCapabilityReadinessProbePortV2,
{
    pub(crate) fn prepare_v2() -> Self {
        Self::prepare_with_config_v2(RuntimeCapabilityReadinessSupervisorConfigV2::production_v2())
    }

    fn prepare_with_config_v2(config: RuntimeCapabilityReadinessSupervisorConfigV2) -> Self {
        let (activation_sender, activation_receiver) =
            mpsc::channel(CAPABILITY_READINESS_CONTROL_CAPACITY_V2);
        let (shutdown_sender, shutdown_receiver) = watch::channel(None);
        let (terminal_sender, terminal_receiver) = watch::channel(None);
        Self {
            activation_sender,
            activation_receiver,
            shutdown_sender,
            shutdown_receiver,
            terminal_sender,
            terminal_receiver,
            config,
        }
    }

    pub(crate) fn shutdown_handle_v2(&self) -> RuntimeCapabilityReadinessShutdownHandleV2 {
        RuntimeCapabilityReadinessShutdownHandleV2 {
            sender: self.shutdown_sender.clone(),
        }
    }

    pub(crate) fn start_v2<I>(self, invalidation: I) -> RuntimeCapabilityReadinessSupervisorV2<P>
    where
        I: RuntimeCapabilityReadinessInvalidationPortV2,
    {
        let Self {
            activation_sender,
            activation_receiver,
            shutdown_sender,
            shutdown_receiver,
            terminal_sender,
            terminal_receiver,
            config,
        } = self;
        let shutdown = RuntimeCapabilityReadinessShutdownHandleV2 {
            sender: shutdown_sender,
        };
        let task = tokio::spawn(async move {
            let exit = run_capability_readiness_supervisor_v2(
                activation_receiver,
                shutdown_receiver,
                invalidation,
                config,
            )
            .await;
            let _published = terminal_sender.send(Some(exit)).is_ok();
            exit
        });
        RuntimeCapabilityReadinessSupervisorV2 {
            activation: Some(RuntimeCapabilityReadinessActivationHandleV2 {
                sender: activation_sender,
                shutdown: shutdown.sender.subscribe(),
            }),
            shutdown,
            terminal: RuntimeCapabilityReadinessTerminalObserverV2 {
                receiver: terminal_receiver,
            },
            task: Some(task),
        }
    }
}

impl<P> Debug for RuntimeCapabilityReadinessPreparedV2<P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCapabilityReadinessPreparedV2(<redacted>)")
    }
}

impl<P> RuntimeCapabilityReadinessSupervisorV2<P>
where
    P: RuntimeCapabilityReadinessProbePortV2,
{
    #[cfg(test)]
    fn start_dormant_with_config_v2<I>(
        invalidation: I,
        config: RuntimeCapabilityReadinessSupervisorConfigV2,
    ) -> Self
    where
        I: RuntimeCapabilityReadinessInvalidationPortV2,
    {
        RuntimeCapabilityReadinessPreparedV2::prepare_with_config_v2(config).start_v2(invalidation)
    }

    #[cfg(test)]
    fn shutdown_handle_v2(&self) -> RuntimeCapabilityReadinessShutdownHandleV2 {
        self.shutdown.clone()
    }

    pub(crate) fn terminal_observer_v2(&self) -> RuntimeCapabilityReadinessTerminalObserverV2 {
        self.terminal.clone()
    }

    pub(crate) async fn activate_until_v2(
        &mut self,
        probe: P,
        deadline: Instant,
    ) -> Result<(), RuntimeCapabilityReadinessActivationErrorV2> {
        let activation = self
            .activation
            .take()
            .ok_or(RuntimeCapabilityReadinessActivationErrorV2::AlreadyActivated)?;
        activation.activate_until_v2(probe, deadline).await
    }

    pub(crate) async fn shutdown_until_v2(
        &mut self,
        deadline: Instant,
    ) -> RuntimeCapabilityReadinessSupervisorExitV2 {
        self.shutdown.seal_until_v2(deadline);
        self.activation.take();
        let Some(mut task) = self.task.take() else {
            return RuntimeCapabilityReadinessSupervisorExitV2::Commanded;
        };
        if Instant::now() >= deadline {
            task.abort();
            let _joined = task.await;
            return RuntimeCapabilityReadinessSupervisorExitV2::DeadlineElapsed;
        }
        match timeout_at(TokioInstant::from_std(deadline), &mut task).await {
            Ok(Ok(exit)) => exit,
            Ok(Err(_)) => RuntimeCapabilityReadinessSupervisorExitV2::Panicked,
            Err(_) => {
                task.abort();
                let _joined = task.await;
                RuntimeCapabilityReadinessSupervisorExitV2::DeadlineElapsed
            }
        }
    }
}

impl<P> Drop for RuntimeCapabilityReadinessSupervisorV2<P> {
    fn drop(&mut self) {
        self.shutdown.seal_until_v2(Instant::now());
        self.activation.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl<P> Debug for RuntimeCapabilityReadinessSupervisorV2<P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeCapabilityReadinessSupervisorV2(<redacted>)")
    }
}

enum RuntimeCapabilityReadinessProbeAttemptV2 {
    Available,
    Failed(RuntimeCapabilityReadinessProbeFailureV2),
    TimedOut,
    Shutdown,
}

async fn verify_capability_readiness_until_v2<P>(
    probe: &P,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    deadline: Instant,
) -> RuntimeCapabilityReadinessProbeAttemptV2
where
    P: RuntimeCapabilityReadinessProbePortV2,
{
    if shutdown.borrow().is_some() {
        return RuntimeCapabilityReadinessProbeAttemptV2::Shutdown;
    }
    if Instant::now() >= deadline {
        return RuntimeCapabilityReadinessProbeAttemptV2::TimedOut;
    }
    let verification = timeout_at(
        TokioInstant::from_std(deadline),
        probe.verify_capability_readiness_v2(),
    );
    tokio::pin!(verification);
    tokio::select! {
        biased;
        changed = shutdown.changed() => {
            let _closed = changed.is_err();
            RuntimeCapabilityReadinessProbeAttemptV2::Shutdown
        }
        result = &mut verification => {
            match result {
                Ok(RuntimeCapabilityReadinessProbeDispositionV2::Available) => {
                    RuntimeCapabilityReadinessProbeAttemptV2::Available
                }
                Ok(RuntimeCapabilityReadinessProbeDispositionV2::Failed(failure)) => {
                    RuntimeCapabilityReadinessProbeAttemptV2::Failed(failure)
                }
                Err(_) => RuntimeCapabilityReadinessProbeAttemptV2::TimedOut,
            }
        }
    }
}

async fn run_capability_readiness_supervisor_v2<P, I>(
    mut activation: mpsc::Receiver<RuntimeCapabilityReadinessActivationRequestV2<P>>,
    mut shutdown: watch::Receiver<Option<Instant>>,
    invalidation: I,
    config: RuntimeCapabilityReadinessSupervisorConfigV2,
) -> RuntimeCapabilityReadinessSupervisorExitV2
where
    P: RuntimeCapabilityReadinessProbePortV2,
    I: RuntimeCapabilityReadinessInvalidationPortV2,
{
    let request = tokio::select! {
        biased;
        changed = shutdown.changed() => {
            let _closed = changed.is_err();
            return RuntimeCapabilityReadinessSupervisorExitV2::Commanded;
        }
        request = activation.recv() => {
            match request {
                Some(request) => request,
                None => return RuntimeCapabilityReadinessSupervisorExitV2::ControlClosed,
            }
        }
    };
    drop(activation);
    let verify_deadline = request.deadline.min(Instant::now() + config.verify_timeout);
    let activation_result =
        verify_capability_readiness_until_v2(&request.probe, &mut shutdown, verify_deadline).await;
    match activation_result {
        RuntimeCapabilityReadinessProbeAttemptV2::Available => {
            let _delivered = request.response.send(Ok(())).is_ok();
        }
        RuntimeCapabilityReadinessProbeAttemptV2::Failed(_) => {
            invalidation.invalidate_readiness_v2();
            let _delivered = request
                .response
                .send(Err(
                    RuntimeCapabilityReadinessActivationErrorV2::ReadinessUnavailable,
                ))
                .is_ok();
            return RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost;
        }
        RuntimeCapabilityReadinessProbeAttemptV2::TimedOut => {
            invalidation.invalidate_readiness_v2();
            let error = if verify_deadline == request.deadline {
                RuntimeCapabilityReadinessActivationErrorV2::DeadlineElapsed
            } else {
                RuntimeCapabilityReadinessActivationErrorV2::ReadinessTimedOut
            };
            let _delivered = request.response.send(Err(error)).is_ok();
            return RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost;
        }
        RuntimeCapabilityReadinessProbeAttemptV2::Shutdown => {
            let _delivered = request
                .response
                .send(Err(RuntimeCapabilityReadinessActivationErrorV2::Sealed))
                .is_ok();
            return RuntimeCapabilityReadinessSupervisorExitV2::Commanded;
        }
    }
    let mut cadence =
        tokio::time::interval_at(TokioInstant::now() + config.cadence, config.cadence);
    cadence.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut retryable_episode_started_at = None;
    let mut retryable_attempts = 0u64;
    let mut retryable_failure = None;
    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                let _closed = changed.is_err();
                return RuntimeCapabilityReadinessSupervisorExitV2::Commanded;
            }
            _ = cadence.tick() => {}
        }
        let attempt_started_at = Instant::now();
        if let (Some(started_at), Some(failure)) = (retryable_episode_started_at, retryable_failure)
        {
            let elapsed = attempt_started_at.saturating_duration_since(started_at);
            if elapsed >= config.transient_grace {
                emit_capability_readiness_status_v2(
                    "runtime_capability_readiness_retry_exhausted",
                    failure,
                    retryable_attempts,
                    elapsed,
                );
                invalidation.invalidate_readiness_v2();
                return RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost;
            }
        }
        let deadline = capability_readiness_periodic_deadline_v2(
            attempt_started_at,
            config.verify_timeout,
            retryable_episode_started_at,
            config.transient_grace,
        );
        let result =
            verify_capability_readiness_until_v2(&request.probe, &mut shutdown, deadline).await;
        match result {
            RuntimeCapabilityReadinessProbeAttemptV2::Available => {
                if let (Some(started_at), Some(failure)) = (
                    retryable_episode_started_at.take(),
                    retryable_failure.take(),
                ) {
                    emit_capability_readiness_status_v2(
                        "runtime_capability_readiness_recovered",
                        failure,
                        retryable_attempts,
                        Instant::now().saturating_duration_since(started_at),
                    );
                }
                retryable_attempts = 0;
            }
            RuntimeCapabilityReadinessProbeAttemptV2::Failed(failure)
                if failure.class == RuntimeCapabilityReadinessProbeFailureClassV2::Retryable =>
            {
                let started_at = *retryable_episode_started_at.get_or_insert(attempt_started_at);
                retryable_attempts = retryable_attempts.saturating_add(1);
                retryable_failure = Some(failure);
                let elapsed = Instant::now().saturating_duration_since(started_at);
                if retryable_attempts == 1 {
                    emit_capability_readiness_status_v2(
                        "runtime_capability_readiness_retrying",
                        failure,
                        retryable_attempts,
                        elapsed,
                    );
                }
                if elapsed >= config.transient_grace {
                    emit_capability_readiness_status_v2(
                        "runtime_capability_readiness_retry_exhausted",
                        failure,
                        retryable_attempts,
                        elapsed,
                    );
                    invalidation.invalidate_readiness_v2();
                    return RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost;
                }
            }
            RuntimeCapabilityReadinessProbeAttemptV2::Failed(failure) => {
                emit_capability_readiness_status_v2(
                    "runtime_capability_readiness_terminal",
                    failure,
                    1,
                    Instant::now().saturating_duration_since(attempt_started_at),
                );
                invalidation.invalidate_readiness_v2();
                return RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost;
            }
            RuntimeCapabilityReadinessProbeAttemptV2::TimedOut => {
                let failure = capability_readiness_timeout_failure_v2();
                let started_at = *retryable_episode_started_at.get_or_insert(attempt_started_at);
                retryable_attempts = retryable_attempts.saturating_add(1);
                let elapsed = Instant::now().saturating_duration_since(started_at);
                emit_capability_readiness_status_v2(
                    "runtime_capability_readiness_retry_exhausted",
                    failure,
                    retryable_attempts,
                    elapsed,
                );
                invalidation.invalidate_readiness_v2();
                return RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost;
            }
            RuntimeCapabilityReadinessProbeAttemptV2::Shutdown => {
                return RuntimeCapabilityReadinessSupervisorExitV2::Commanded;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use tokio::sync::Semaphore;

    use super::*;

    #[derive(Clone)]
    struct FakeProbeV2 {
        state: Arc<FakeProbeStateV2>,
    }

    struct FakeProbeStateV2 {
        calls: AtomicUsize,
        completions: AtomicUsize,
        outcomes: Mutex<VecDeque<RuntimeCapabilityReadinessProbeDispositionV2>>,
        permits: Semaphore,
    }

    impl FakeProbeV2 {
        fn new() -> Self {
            Self {
                state: Arc::new(FakeProbeStateV2 {
                    calls: AtomicUsize::new(0),
                    completions: AtomicUsize::new(0),
                    outcomes: Mutex::new(VecDeque::new()),
                    permits: Semaphore::new(0),
                }),
            }
        }

        fn release_v2(&self, outcome: RuntimeCapabilityReadinessProbeDispositionV2) {
            self.state
                .outcomes
                .lock()
                .expect("fake probe outcomes")
                .push_back(outcome);
            self.state.permits.add_permits(1);
        }

        fn calls_v2(&self) -> usize {
            self.state.calls.load(Ordering::Acquire)
        }

        fn completions_v2(&self) -> usize {
            self.state.completions.load(Ordering::Acquire)
        }
    }

    impl RuntimeCapabilityReadinessProbePortV2 for FakeProbeV2 {
        fn verify_capability_readiness_v2(&self) -> RuntimeCapabilityReadinessProbeFutureV2<'_> {
            Box::pin(async move {
                self.state.calls.fetch_add(1, Ordering::AcqRel);
                let permit = self
                    .state
                    .permits
                    .acquire()
                    .await
                    .expect("fake probe permit");
                permit.forget();
                let outcome = self
                    .state
                    .outcomes
                    .lock()
                    .expect("fake probe outcomes")
                    .pop_front()
                    .expect("fake probe outcome");
                self.state.completions.fetch_add(1, Ordering::AcqRel);
                outcome
            })
        }
    }

    #[derive(Clone)]
    struct FakeInvalidationV2 {
        count: Arc<AtomicUsize>,
    }

    impl FakeInvalidationV2 {
        fn new() -> Self {
            Self {
                count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn count_v2(&self) -> usize {
            self.count.load(Ordering::Acquire)
        }
    }

    impl RuntimeCapabilityReadinessInvalidationPortV2 for FakeInvalidationV2 {
        fn invalidate_readiness_v2(&self) {
            self.count.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn test_config_v2() -> RuntimeCapabilityReadinessSupervisorConfigV2 {
        RuntimeCapabilityReadinessSupervisorConfigV2 {
            cadence: Duration::from_millis(10),
            verify_timeout: Duration::from_millis(40),
            transient_grace: Duration::from_millis(20),
        }
    }

    fn fake_failure_v2(
        class: RuntimeCapabilityReadinessProbeFailureClassV2,
    ) -> RuntimeCapabilityReadinessProbeDispositionV2 {
        RuntimeCapabilityReadinessProbeDispositionV2::Failed(
            RuntimeCapabilityReadinessProbeFailureV2 {
                class,
                code: "fake_capability_readiness_failure",
                context: Some("fake"),
            },
        )
    }

    async fn wait_for_calls_v2(probe: &FakeProbeV2, expected: usize) {
        timeout_at(TokioInstant::now() + Duration::from_secs(1), async {
            while probe.calls_v2() < expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fake probe calls");
    }

    async fn wait_for_completions_v2(probe: &FakeProbeV2, expected: usize) {
        timeout_at(TokioInstant::now() + Duration::from_secs(1), async {
            while probe.completions_v2() < expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("fake probe completions");
    }

    #[test]
    fn database_probe_failures_preserve_retry_authority_and_protocol_classes() {
        use crate::DatabaseCapabilityV1;

        let capability = DatabaseCapabilityV1::Serving;
        let cases = [
            (
                RuntimeDatabaseCompositionErrorV1::Unavailable { capability },
                RuntimeCapabilityReadinessProbeFailureClassV2::Retryable,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::ReadinessUnavailable { capability },
                RuntimeCapabilityReadinessProbeFailureClassV2::Retryable,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut,
                RuntimeCapabilityReadinessProbeFailureClassV2::Retryable,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::ReadinessAuthorityMismatch { capability },
                RuntimeCapabilityReadinessProbeFailureClassV2::AuthorityLost,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::AuthorityMismatch,
                RuntimeCapabilityReadinessProbeFailureClassV2::AuthorityLost,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::InvalidConfiguration,
                RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::ConnectionConfiguration { capability },
                RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::UnsafeTransport { capability },
                RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::IdentityVerification,
                RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::ReadinessRejected { capability },
                RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
            ),
            (
                RuntimeDatabaseCompositionErrorV1::StartupCleanupTimedOut,
                RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
            ),
        ];
        for (error, class) in cases {
            let failure = capability_readiness_database_failure_v2(error);
            assert_eq!(failure.class, class);
            assert_eq!(failure.code, error.code());
            assert_eq!(failure.context, error.context());
        }
    }

    #[tokio::test]
    async fn caller_cancellation_does_not_cancel_activated_supervisor() {
        let probe = FakeProbeV2::new();
        let invalidation = FakeInvalidationV2::new();
        let mut supervisor = RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
            invalidation.clone(),
            test_config_v2(),
        );
        let activation = supervisor.activation.take().expect("activation");
        let activation_task = tokio::spawn(
            activation.activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1)),
        );
        wait_for_calls_v2(&probe, 1).await;
        activation_task.abort();
        let _joined = activation_task.await;
        probe.release_v2(RuntimeCapabilityReadinessProbeDispositionV2::Available);
        wait_for_calls_v2(&probe, 2).await;
        probe.release_v2(fake_failure_v2(
            RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
        ));
        let exit = supervisor.terminal_observer_v2().wait_v2().await;
        assert_eq!(
            exit,
            RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost
        );
        assert_eq!(invalidation.count_v2(), 1);
    }

    #[tokio::test]
    async fn shutdown_seal_wins_initial_failure_race() {
        let probe = FakeProbeV2::new();
        let invalidation = FakeInvalidationV2::new();
        let mut supervisor = RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
            invalidation.clone(),
            test_config_v2(),
        );
        let activation = supervisor.activation.take().expect("activation");
        let activation_task = tokio::spawn(
            activation.activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1)),
        );
        wait_for_calls_v2(&probe, 1).await;
        supervisor
            .shutdown_handle_v2()
            .seal_until_v2(Instant::now() + Duration::from_secs(1));
        probe.release_v2(fake_failure_v2(
            RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
        ));
        let activation_result = activation_task.await.expect("activation task");
        assert_eq!(
            activation_result,
            Err(RuntimeCapabilityReadinessActivationErrorV2::Sealed)
        );
        let exit = supervisor
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(exit, RuntimeCapabilityReadinessSupervisorExitV2::Commanded);
        assert_eq!(invalidation.count_v2(), 0);
    }

    #[tokio::test]
    async fn shutdown_control_preempts_periodic_failure() {
        let probe = FakeProbeV2::new();
        let invalidation = FakeInvalidationV2::new();
        let mut supervisor = RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
            invalidation.clone(),
            test_config_v2(),
        );
        probe.release_v2(RuntimeCapabilityReadinessProbeDispositionV2::Available);
        supervisor
            .activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1))
            .await
            .expect("activation");
        wait_for_calls_v2(&probe, 2).await;
        supervisor
            .shutdown_handle_v2()
            .seal_until_v2(Instant::now() + Duration::from_secs(1));
        probe.release_v2(fake_failure_v2(
            RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
        ));
        let exit = supervisor
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(exit, RuntimeCapabilityReadinessSupervisorExitV2::Commanded);
        assert_eq!(invalidation.count_v2(), 0);
    }

    #[tokio::test]
    async fn bounded_verification_timeout_invalidates_once() {
        let probe = FakeProbeV2::new();
        let invalidation = FakeInvalidationV2::new();
        let mut supervisor = RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
            invalidation.clone(),
            test_config_v2(),
        );
        probe.release_v2(RuntimeCapabilityReadinessProbeDispositionV2::Available);
        supervisor
            .activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1))
            .await
            .expect("activation");
        let exit = supervisor.terminal_observer_v2().wait_v2().await;
        assert_eq!(
            exit,
            RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost
        );
        assert_eq!(probe.calls_v2(), 2);
        assert_eq!(invalidation.count_v2(), 1);
    }

    #[tokio::test]
    async fn periodic_retryable_failure_recovers_without_invalidation() {
        let probe = FakeProbeV2::new();
        let invalidation = FakeInvalidationV2::new();
        let mut supervisor = RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
            invalidation.clone(),
            test_config_v2(),
        );
        probe.release_v2(RuntimeCapabilityReadinessProbeDispositionV2::Available);
        probe.release_v2(fake_failure_v2(
            RuntimeCapabilityReadinessProbeFailureClassV2::Retryable,
        ));
        probe.release_v2(RuntimeCapabilityReadinessProbeDispositionV2::Available);
        supervisor
            .activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1))
            .await
            .expect("activation");
        wait_for_completions_v2(&probe, 3).await;
        assert_eq!(invalidation.count_v2(), 0);
        let exit = supervisor
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(exit, RuntimeCapabilityReadinessSupervisorExitV2::Commanded);
    }

    #[tokio::test]
    async fn periodic_success_resets_the_retryable_episode() {
        let probe = FakeProbeV2::new();
        let invalidation = FakeInvalidationV2::new();
        let mut supervisor = RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
            invalidation.clone(),
            test_config_v2(),
        );
        for outcome in [
            RuntimeCapabilityReadinessProbeDispositionV2::Available,
            fake_failure_v2(RuntimeCapabilityReadinessProbeFailureClassV2::Retryable),
            RuntimeCapabilityReadinessProbeDispositionV2::Available,
            fake_failure_v2(RuntimeCapabilityReadinessProbeFailureClassV2::Retryable),
            RuntimeCapabilityReadinessProbeDispositionV2::Available,
        ] {
            probe.release_v2(outcome);
        }
        supervisor
            .activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1))
            .await
            .expect("activation");
        wait_for_completions_v2(&probe, 5).await;
        assert_eq!(invalidation.count_v2(), 0);
        let exit = supervisor
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(exit, RuntimeCapabilityReadinessSupervisorExitV2::Commanded);
    }

    #[tokio::test]
    async fn periodic_retryable_exhaustion_invalidates_once() {
        let probe = FakeProbeV2::new();
        let invalidation = FakeInvalidationV2::new();
        let mut config = test_config_v2();
        config.cadence = Duration::from_millis(20);
        config.transient_grace = Duration::from_millis(5);
        let mut supervisor = RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
            invalidation.clone(),
            config,
        );
        probe.release_v2(RuntimeCapabilityReadinessProbeDispositionV2::Available);
        probe.release_v2(fake_failure_v2(
            RuntimeCapabilityReadinessProbeFailureClassV2::Retryable,
        ));
        supervisor
            .activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1))
            .await
            .expect("activation");
        let exit = supervisor.terminal_observer_v2().wait_v2().await;
        assert_eq!(
            exit,
            RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost
        );
        assert_eq!(probe.calls_v2(), 2);
        assert_eq!(probe.completions_v2(), 2);
        assert_eq!(invalidation.count_v2(), 1);
    }

    #[tokio::test]
    async fn terminal_periodic_failure_has_no_retry_grace() {
        for class in [
            RuntimeCapabilityReadinessProbeFailureClassV2::AuthorityLost,
            RuntimeCapabilityReadinessProbeFailureClassV2::ProtocolViolation,
        ] {
            let probe = FakeProbeV2::new();
            let invalidation = FakeInvalidationV2::new();
            let mut supervisor =
                RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
                    invalidation.clone(),
                    test_config_v2(),
                );
            probe.release_v2(RuntimeCapabilityReadinessProbeDispositionV2::Available);
            probe.release_v2(fake_failure_v2(class));
            supervisor
                .activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1))
                .await
                .expect("activation");
            let exit = supervisor.terminal_observer_v2().wait_v2().await;
            assert_eq!(
                exit,
                RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost
            );
            assert_eq!(probe.calls_v2(), 2);
            assert_eq!(invalidation.count_v2(), 1);
        }
    }

    #[tokio::test]
    async fn activation_deadline_is_absolute_and_bounded() {
        let probe = FakeProbeV2::new();
        let invalidation = FakeInvalidationV2::new();
        let mut supervisor = RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
            invalidation.clone(),
            test_config_v2(),
        );
        let result = supervisor
            .activate_until_v2(probe.clone(), Instant::now() + Duration::from_millis(15))
            .await;
        assert_eq!(
            result,
            Err(RuntimeCapabilityReadinessActivationErrorV2::DeadlineElapsed)
        );
        let exit = supervisor.terminal_observer_v2().wait_v2().await;
        assert_eq!(
            exit,
            RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost
        );
        assert_eq!(probe.calls_v2(), 1);
        assert_eq!(invalidation.count_v2(), 1);
    }

    #[test]
    fn production_contract_is_one_second_by_five_seconds_with_bounded_transient_grace() {
        assert_eq!(CAPABILITY_READINESS_CONTROL_CAPACITY_V2, 1);
        assert_eq!(
            RuntimeCapabilityReadinessSupervisorConfigV2::production_v2(),
            RuntimeCapabilityReadinessSupervisorConfigV2 {
                cadence: Duration::from_secs(1),
                verify_timeout: Duration::from_secs(5),
                transient_grace: Duration::from_secs(5),
            }
        );
    }

    #[test]
    fn periodic_probe_deadline_never_exceeds_the_retryable_episode_grace() {
        let started_at = Instant::now();
        let retry_started_at = started_at + Duration::from_secs(1);
        let retry_attempt_started_at = started_at + Duration::from_secs(4);
        assert_eq!(
            capability_readiness_periodic_deadline_v2(
                retry_attempt_started_at,
                Duration::from_secs(5),
                Some(retry_started_at),
                Duration::from_secs(5),
            ),
            started_at + Duration::from_secs(6)
        );
        assert_eq!(
            capability_readiness_periodic_deadline_v2(
                retry_attempt_started_at,
                Duration::from_secs(1),
                Some(retry_started_at),
                Duration::from_secs(5),
            ),
            started_at + Duration::from_secs(5)
        );
    }

    #[test]
    fn capability_supervisor_values_are_redacted() {
        let exit = RuntimeCapabilityReadinessSupervisorExitV2::ControlClosed;
        let error = RuntimeCapabilityReadinessActivationErrorV2::ReadinessUnavailable;
        assert_eq!(format!("{exit:?}"), "ControlClosed");
        assert_eq!(
            error.to_string(),
            "runtime capability readiness verification failed"
        );
    }
}
