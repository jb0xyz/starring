use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{timeout_at, Instant as TokioInstant, MissedTickBehavior};

use crate::database::RuntimeDatabaseReadinessProbeV2;
use crate::process_supervisor::RuntimeProcessInvalidationTriggerV1;
use crate::RuntimeShutdownCauseV1;

const CAPABILITY_READINESS_CONTROL_CAPACITY_V2: usize = 1;
const CAPABILITY_READINESS_CADENCE_V2: Duration = Duration::from_secs(1);
const CAPABILITY_READINESS_VERIFY_TIMEOUT_V2: Duration = Duration::from_secs(5);

type RuntimeCapabilityReadinessProbeFutureV2<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

pub(crate) trait RuntimeCapabilityReadinessProbePortV2:
    Clone + Send + Sync + 'static
{
    fn verify_capability_readiness_v2(&self) -> RuntimeCapabilityReadinessProbeFutureV2<'_>;
}

impl RuntimeCapabilityReadinessProbePortV2 for RuntimeDatabaseReadinessProbeV2 {
    fn verify_capability_readiness_v2(&self) -> RuntimeCapabilityReadinessProbeFutureV2<'_> {
        Box::pin(async move { self.verify_v2().await.is_ok() })
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
}

impl RuntimeCapabilityReadinessSupervisorConfigV2 {
    const fn production_v2() -> Self {
        Self {
            cadence: CAPABILITY_READINESS_CADENCE_V2,
            verify_timeout: CAPABILITY_READINESS_VERIFY_TIMEOUT_V2,
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
    Unavailable,
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
                Ok(true) => RuntimeCapabilityReadinessProbeAttemptV2::Available,
                Ok(false) => RuntimeCapabilityReadinessProbeAttemptV2::Unavailable,
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
        RuntimeCapabilityReadinessProbeAttemptV2::Unavailable => {
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
    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                let _closed = changed.is_err();
                return RuntimeCapabilityReadinessSupervisorExitV2::Commanded;
            }
            _ = cadence.tick() => {}
        }
        let deadline = Instant::now() + config.verify_timeout;
        match verify_capability_readiness_until_v2(&request.probe, &mut shutdown, deadline).await {
            RuntimeCapabilityReadinessProbeAttemptV2::Available => {}
            RuntimeCapabilityReadinessProbeAttemptV2::Unavailable
            | RuntimeCapabilityReadinessProbeAttemptV2::TimedOut => {
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
        outcomes: Mutex<VecDeque<bool>>,
        permits: Semaphore,
    }

    impl FakeProbeV2 {
        fn new() -> Self {
            Self {
                state: Arc::new(FakeProbeStateV2 {
                    calls: AtomicUsize::new(0),
                    outcomes: Mutex::new(VecDeque::new()),
                    permits: Semaphore::new(0),
                }),
            }
        }

        fn release_v2(&self, outcome: bool) {
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
                self.state
                    .outcomes
                    .lock()
                    .expect("fake probe outcomes")
                    .pop_front()
                    .expect("fake probe outcome")
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
        }
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
        probe.release_v2(true);
        wait_for_calls_v2(&probe, 2).await;
        probe.release_v2(false);
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
        probe.release_v2(false);
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
        probe.release_v2(true);
        supervisor
            .activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1))
            .await
            .expect("activation");
        wait_for_calls_v2(&probe, 2).await;
        supervisor
            .shutdown_handle_v2()
            .seal_until_v2(Instant::now() + Duration::from_secs(1));
        probe.release_v2(false);
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
        probe.release_v2(true);
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
    async fn first_periodic_failure_has_no_retry_grace() {
        let probe = FakeProbeV2::new();
        let invalidation = FakeInvalidationV2::new();
        let mut supervisor = RuntimeCapabilityReadinessSupervisorV2::start_dormant_with_config_v2(
            invalidation.clone(),
            test_config_v2(),
        );
        probe.release_v2(true);
        probe.release_v2(false);
        supervisor
            .activate_until_v2(probe.clone(), Instant::now() + Duration::from_secs(1))
            .await
            .expect("activation");
        let exit = supervisor.terminal_observer_v2().wait_v2().await;
        assert_eq!(
            exit,
            RuntimeCapabilityReadinessSupervisorExitV2::ReadinessLost
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(probe.calls_v2(), 2);
        assert_eq!(invalidation.count_v2(), 1);
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
    fn production_contract_is_one_second_by_five_seconds() {
        assert_eq!(CAPABILITY_READINESS_CONTROL_CAPACITY_V2, 1);
        assert_eq!(
            RuntimeCapabilityReadinessSupervisorConfigV2::production_v2(),
            RuntimeCapabilityReadinessSupervisorConfigV2 {
                cadence: Duration::from_secs(1),
                verify_timeout: Duration::from_secs(5),
            }
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
