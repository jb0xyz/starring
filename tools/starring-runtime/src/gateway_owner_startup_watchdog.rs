use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseDurationV1, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeReleaseGatewayOwnerLeaseV1,
};
use automation_runtime_worker::{
    accept_gateway_owner_release_v1, classify_unknown_gateway_owner_release_v1,
    RuntimeAcceptedGatewayOwnerReceiptV1, RuntimeAcceptedGatewayOwnerReleaseV1,
    RuntimeGatewayOwnerLeasePortV1, RuntimeGatewayOwnerMutationErrorV1,
    RuntimeGatewayOwnerObservationCompletionV1, RuntimeGatewayOwnerObservationErrorClassV1,
    RuntimeGatewayOwnerReleaseRecoveryV1, RuntimeGatewayOwnerRenewalCompletionV1,
    RuntimeGatewayOwnerRenewalPolicyV1, RuntimeGatewayOwnerRenewalScheduleErrorV1,
    RuntimeGatewayOwnerWatchdogActionV1, RuntimeGatewayOwnerWatchdogErrorV1,
    RuntimeGatewayOwnerWatchdogV1,
};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{sleep_until, timeout_at, Instant as TokioInstant};

use crate::GatewayOwnerTimingConfigV1;

const SHUTDOWN_COMMAND_CAPACITY: usize = 1;
const OBSERVATION_COMMAND_CAPACITY: usize = 1;
const RELEASE_ATTEMPTS: usize = 2;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(250);
const DEFAULT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);
const MAXIMUM_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) trait RuntimeGatewayOwnerEmergencyInvalidatorV1: Send + Sync + 'static {
    fn invalidate_gateway_ownership(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerStartupWatchdogConfigV1 {
    lease_for: RuntimeGatewayOwnerLeaseDurationV1,
    policy: RuntimeGatewayOwnerRenewalPolicyV1,
    retry_delay: Duration,
    cleanup_timeout: Duration,
}

impl RuntimeGatewayOwnerStartupWatchdogConfigV1 {
    pub fn new(
        lease_for: Duration,
        renew_before: Duration,
        safety_margin: Duration,
        retry_delay: Duration,
        cleanup_timeout: Duration,
    ) -> Result<Self, RuntimeGatewayOwnerStartupWatchdogConfigErrorV1> {
        let lease_for = RuntimeGatewayOwnerLeaseDurationV1::new(lease_for)
            .ok_or(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidLease)?;
        let policy = RuntimeGatewayOwnerRenewalPolicyV1::new(renew_before, safety_margin)
            .map_err(|_| RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidPolicy)?;
        let retry_window = renew_before
            .checked_sub(safety_margin)
            .ok_or(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidPolicy)?;
        if retry_delay.is_zero() || retry_delay >= retry_window {
            return Err(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidRetryDelay);
        }
        if renew_before >= lease_for.get() {
            return Err(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidLease);
        }
        if cleanup_timeout.is_zero() || cleanup_timeout > MAXIMUM_CLEANUP_TIMEOUT {
            return Err(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidCleanupTimeout);
        }
        Ok(Self {
            lease_for,
            policy,
            retry_delay,
            cleanup_timeout,
        })
    }

    pub fn from_runtime_config(
        timing: GatewayOwnerTimingConfigV1,
    ) -> Result<Self, RuntimeGatewayOwnerStartupWatchdogConfigErrorV1> {
        Self::new(
            timing.lease_for(),
            timing.renew_before(),
            timing.safety_margin(),
            DEFAULT_RETRY_DELAY,
            DEFAULT_CLEANUP_TIMEOUT,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerStartupWatchdogConfigErrorV1 {
    #[error("runtime gateway owner startup watchdog lease is invalid")]
    InvalidLease,
    #[error("runtime gateway owner startup watchdog renewal policy is invalid")]
    InvalidPolicy,
    #[error("runtime gateway owner startup watchdog retry delay is invalid")]
    InvalidRetryDelay,
    #[error("runtime gateway owner startup watchdog cleanup timeout is invalid")]
    InvalidCleanupTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerStartupWatchdogStartErrorV1 {
    #[error("runtime gateway owner startup watchdog receipt schedule is invalid")]
    InvalidReceipt,
    #[error("runtime gateway owner startup watchdog requires an active runtime")]
    RuntimeUnavailable,
    #[error("runtime gateway owner startup watchdog is already started")]
    AlreadyStarted,
    #[error("runtime gateway owner startup watchdog safety deadline elapsed")]
    SafetyElapsed,
    #[error("runtime gateway owner startup watchdog process identity mismatched")]
    ProcessMismatch,
    #[error("runtime gateway owner startup watchdog shard identity mismatched")]
    ShardMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerStartupWatchdogExitV1 {
    Shutdown,
    SafetyElapsed,
    OwnershipLost,
    RenewalUnknown,
    ReleaseUnconfirmed,
    ProtocolViolation,
    TaskStopped,
}

impl RuntimeGatewayOwnerStartupWatchdogExitV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Shutdown => "runtime_gateway_owner_shutdown",
            Self::SafetyElapsed => "runtime_gateway_owner_safety_elapsed",
            Self::OwnershipLost => "runtime_gateway_owner_lost",
            Self::RenewalUnknown => "runtime_gateway_owner_renewal_unknown",
            Self::ReleaseUnconfirmed => "runtime_gateway_owner_release_unconfirmed",
            Self::ProtocolViolation => "runtime_gateway_owner_protocol_violation",
            Self::TaskStopped => "runtime_gateway_owner_task_stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerReleaseStatusV1 {
    Confirmed,
    Unconfirmed,
    ProtocolViolation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerCurrentObservationV1 {
    receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    safety_deadline: Instant,
}

impl RuntimeGatewayOwnerCurrentObservationV1 {
    fn from_watchdog(watchdog: &RuntimeGatewayOwnerWatchdogV1) -> Self {
        Self {
            receipt: watchdog.schedule().receipt().clone(),
            safety_deadline: watchdog.schedule().safety_deadline(),
        }
    }

    pub fn receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.receipt
    }

    pub fn safety_deadline(&self) -> Instant {
        self.safety_deadline
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerCurrentObservationErrorV1 {
    #[error("runtime gateway owner observation can be retried")]
    Retryable,
    #[error("runtime gateway owner observation safety deadline elapsed")]
    SafetyElapsed,
    #[error("runtime gateway owner observation found ownership loss")]
    OwnershipLost,
    #[error("runtime gateway owner observation violated its protocol")]
    ProtocolViolation,
    #[error("runtime gateway owner observation supervisor is unavailable")]
    SupervisorUnavailable,
}

pub struct RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P> {
    reason: RuntimeGatewayOwnerStartupWatchdogStartErrorV1,
    port: P,
    lease_id: RuntimeGatewayOwnerLeaseIdV1,
}

impl<P> std::fmt::Debug for RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeGatewayOwnerStartupWatchdogStartFailureV1")
            .field("reason", &self.reason)
            .field("lease_id", &self.lease_id)
            .finish_non_exhaustive()
    }
}

impl<P> RuntimeGatewayOwnerStartupWatchdogStartFailureV1<P> {
    pub(crate) fn new(
        reason: RuntimeGatewayOwnerStartupWatchdogStartErrorV1,
        port: P,
        lease_id: RuntimeGatewayOwnerLeaseIdV1,
    ) -> Self {
        Self {
            reason,
            port,
            lease_id,
        }
    }

    pub fn reason(&self) -> RuntimeGatewayOwnerStartupWatchdogStartErrorV1 {
        self.reason
    }

    pub fn lease_id(&self) -> &RuntimeGatewayOwnerLeaseIdV1 {
        &self.lease_id
    }

    pub async fn cleanup(self, timeout: Duration) -> RuntimeGatewayOwnerReleaseStatusV1
    where
        P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
    {
        if timeout.is_zero() || timeout > MAXIMUM_CLEANUP_TIMEOUT {
            return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed;
        }
        let deadline = TokioInstant::now() + timeout;
        release_gateway_owner_v1(&self.port, self.lease_id, deadline).await
    }
}

pub struct RuntimeGatewayOwnerStartupWatchdogHandleV1 {
    shutdown_commands: mpsc::Sender<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    observation_commands: mpsc::Sender<RuntimeGatewayOwnerStartupObservationCommandV1>,
    terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
    invalidation: Arc<RuntimeGatewayOwnerInvalidationLatchV1>,
}

impl Drop for RuntimeGatewayOwnerStartupWatchdogHandleV1 {
    fn drop(&mut self) {
        self.invalidation.invalidate();
    }
}

impl RuntimeGatewayOwnerStartupWatchdogHandleV1 {
    pub fn terminal_status(&self) -> Option<RuntimeGatewayOwnerStartupWatchdogExitV1> {
        *self.terminal.borrow()
    }

    pub async fn wait_terminal(&mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        loop {
            if let Some(exit) = *self.terminal.borrow() {
                return exit;
            }
            if self.terminal.changed().await.is_err() {
                return RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped;
            }
        }
    }

    pub async fn observe_current_gateway_owner_v1(
        &self,
    ) -> Result<RuntimeGatewayOwnerCurrentObservationV1, RuntimeGatewayOwnerCurrentObservationErrorV1>
    {
        let (response, acknowledgement) = oneshot::channel();
        let terminal = self.terminal.clone();
        if self
            .observation_commands
            .send(RuntimeGatewayOwnerStartupObservationCommandV1 { response })
            .await
            .is_err()
        {
            return Err(wait_for_observation_terminal_v1(terminal).await);
        }
        match acknowledgement.await {
            Ok(result) => result,
            Err(_) => Err(wait_for_observation_terminal_v1(terminal).await),
        }
    }

    pub async fn shutdown(mut self) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
        let (response, acknowledgement) = oneshot::channel();
        if self
            .shutdown_commands
            .send(RuntimeGatewayOwnerStartupShutdownCommandV1 { response })
            .await
            .is_ok()
        {
            if let Ok(exit) = acknowledgement.await {
                return exit;
            }
        }
        self.wait_terminal().await
    }
}

struct RuntimeGatewayOwnerStartupShutdownCommandV1 {
    response: oneshot::Sender<RuntimeGatewayOwnerStartupWatchdogExitV1>,
}

struct RuntimeGatewayOwnerStartupObservationCommandV1 {
    response: oneshot::Sender<
        Result<
            RuntimeGatewayOwnerCurrentObservationV1,
            RuntimeGatewayOwnerCurrentObservationErrorV1,
        >,
    >,
}

async fn wait_for_observation_terminal_v1(
    mut terminal: watch::Receiver<Option<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
) -> RuntimeGatewayOwnerCurrentObservationErrorV1 {
    loop {
        if let Some(exit) = *terminal.borrow() {
            return map_terminal_observation_error_v1(exit);
        }
        if terminal.changed().await.is_err() {
            return RuntimeGatewayOwnerCurrentObservationErrorV1::SupervisorUnavailable;
        }
    }
}

fn map_terminal_observation_error_v1(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
) -> RuntimeGatewayOwnerCurrentObservationErrorV1 {
    match exit {
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::SafetyElapsed
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::OwnershipLost
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::ProtocolViolation
        }
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
        | RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        | RuntimeGatewayOwnerStartupWatchdogExitV1::TaskStopped => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::SupervisorUnavailable
        }
    }
}

pub(crate) fn start_runtime_gateway_owner_startup_watchdog_v1<P, I>(
    port: P,
    invalidator: I,
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
    I: RuntimeGatewayOwnerEmergencyInvalidatorV1,
{
    let lease_id = accepted_receipt.receipt().lease_id.clone();
    let invalidation = Arc::new(RuntimeGatewayOwnerInvalidationLatchV1::new(invalidator));
    let watchdog = match RuntimeGatewayOwnerWatchdogV1::from_accepted_receipt(
        accepted_receipt,
        config.policy,
        request_started_at,
        response_observed_at,
    ) {
        Ok(watchdog) => watchdog,
        Err(error) => {
            invalidation.invalidate();
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                map_schedule_start_error(error),
                port,
                lease_id,
            ));
        }
    };
    if matches!(
        watchdog.action_at(Instant::now()),
        RuntimeGatewayOwnerWatchdogActionV1::InvalidateNow
    ) {
        invalidation.invalidate();
        return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
            RuntimeGatewayOwnerStartupWatchdogStartErrorV1::SafetyElapsed,
            port,
            lease_id,
        ));
    }
    let runtime = match tokio::runtime::Handle::try_current() {
        Ok(runtime) => runtime,
        Err(_) => {
            invalidation.invalidate();
            return Err(RuntimeGatewayOwnerStartupWatchdogStartFailureV1::new(
                RuntimeGatewayOwnerStartupWatchdogStartErrorV1::RuntimeUnavailable,
                port,
                lease_id,
            ));
        }
    };
    let (shutdown_commands, shutdown_receiver) = mpsc::channel(SHUTDOWN_COMMAND_CAPACITY);
    let (observation_commands, observation_receiver) = mpsc::channel(OBSERVATION_COMMAND_CAPACITY);
    let (terminal_sender, terminal) = watch::channel(None);
    let guard = RuntimeGatewayOwnerStartupWatchdogGuardV1::new(invalidation.clone());
    runtime.spawn(async move {
        let exit = run_gateway_owner_startup_watchdog_v1(
            port,
            watchdog,
            config,
            shutdown_receiver,
            observation_receiver,
            guard,
        )
        .await;
        let _result = terminal_sender.send(Some(exit));
    });
    Ok(RuntimeGatewayOwnerStartupWatchdogHandleV1 {
        shutdown_commands,
        observation_commands,
        terminal,
        invalidation,
    })
}

fn map_schedule_start_error(
    error: RuntimeGatewayOwnerRenewalScheduleErrorV1,
) -> RuntimeGatewayOwnerStartupWatchdogStartErrorV1 {
    match error {
        RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed => {
            RuntimeGatewayOwnerStartupWatchdogStartErrorV1::SafetyElapsed
        }
        RuntimeGatewayOwnerRenewalScheduleErrorV1::NonFreshReceipt
        | RuntimeGatewayOwnerRenewalScheduleErrorV1::LeaseTooShort
        | RuntimeGatewayOwnerRenewalScheduleErrorV1::ClockReversed
        | RuntimeGatewayOwnerRenewalScheduleErrorV1::InstantOverflow => {
            RuntimeGatewayOwnerStartupWatchdogStartErrorV1::InvalidReceipt
        }
    }
}

async fn run_gateway_owner_startup_watchdog_v1<P>(
    port: P,
    watchdog: RuntimeGatewayOwnerWatchdogV1,
    config: RuntimeGatewayOwnerStartupWatchdogConfigV1,
    mut shutdown_commands: mpsc::Receiver<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    mut observation_commands: mpsc::Receiver<RuntimeGatewayOwnerStartupObservationCommandV1>,
    mut guard: RuntimeGatewayOwnerStartupWatchdogGuardV1,
) -> RuntimeGatewayOwnerStartupWatchdogExitV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync + 'static,
    P::Error: Send + 'static,
{
    let lease_id = watchdog.schedule().receipt().lease_id.clone();
    let mut current = Some(watchdog);
    let mut shutdown_acknowledgement = None;
    let stop = 'supervisor: loop {
        match shutdown_commands.try_recv() {
            Ok(command) => {
                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                    receive_shutdown(Some(command), &mut shutdown_acknowledgement),
                    config.cleanup_timeout,
                );
            }
            Err(TryRecvError::Disconnected) => {
                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                    RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                    config.cleanup_timeout,
                );
            }
            Err(TryRecvError::Empty) => {}
        }
        let watchdog = current.take().expect("gateway owner watchdog state");
        match watchdog.action_at(Instant::now()) {
            RuntimeGatewayOwnerWatchdogActionV1::WaitUntil(renew_at) => {
                tokio::select! {
                    biased;
                    command = shutdown_commands.recv() => {
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            receive_shutdown(command, &mut shutdown_acknowledgement),
                            config.cleanup_timeout,
                        );
                    }
                    _ = sleep_until(TokioInstant::from_std(renew_at)) => {
                        current = Some(watchdog);
                    }
                    command = observation_commands.recv() => {
                        let Some(command) = command else {
                            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown,
                                config.cleanup_timeout,
                            );
                        };
                        if command.response.is_closed() {
                            current = Some(watchdog);
                            continue 'supervisor;
                        }
                        match observe_current_gateway_owner_v1(
                            &port,
                            watchdog,
                            command,
                            &mut shutdown_commands,
                            &mut shutdown_acknowledgement,
                            &mut guard,
                            config.cleanup_timeout,
                        ).await {
                            RuntimeGatewayOwnerStartupObservationStepV1::Continue {
                                successor,
                                response,
                                result,
                            } => {
                                current = Some(*successor);
                                let _result = response.send(result);
                            }
                            RuntimeGatewayOwnerStartupObservationStepV1::Stop(stop) => {
                                break 'supervisor stop;
                            }
                        }
                    }
                }
            }
            RuntimeGatewayOwnerWatchdogActionV1::RenewNow => {
                let request_started_at = Instant::now();
                let inflight = match watchdog.begin_renewal(config.lease_for, request_started_at) {
                    Ok(inflight) => inflight,
                    Err(error) => {
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            map_watchdog_error(error),
                            config.cleanup_timeout,
                        );
                    }
                };
                let safety_deadline = inflight.previous_schedule().safety_deadline();
                let request = inflight.request().clone();
                let renewal = port.renew_gateway_owner(request);
                tokio::pin!(renewal);
                let result = tokio::select! {
                    biased;
                    command = shutdown_commands.recv() => {
                        guard.invalidate_now();
                        let stop = RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            receive_shutdown(command, &mut shutdown_acknowledgement),
                            config.cleanup_timeout,
                        );
                        let _joined_result = timeout_at(stop.cleanup_deadline, &mut renewal).await;
                        break 'supervisor stop;
                    }
                    _ = sleep_until(TokioInstant::from_std(safety_deadline)) => {
                        guard.invalidate_now();
                        let stop = RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                            config.cleanup_timeout,
                        );
                        let _joined_result = timeout_at(stop.cleanup_deadline, &mut renewal).await;
                        break 'supervisor stop;
                    }
                    result = &mut renewal => result,
                };
                let response_observed_at = Instant::now();
                match result {
                    Ok(outcome) => match inflight.complete(outcome, response_observed_at) {
                        Ok(RuntimeGatewayOwnerRenewalCompletionV1::Renewed(successor)) => {
                            current = Some(successor);
                        }
                        Ok(RuntimeGatewayOwnerRenewalCompletionV1::OwnershipLost(_)) => {
                            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                                config.cleanup_timeout,
                            );
                        }
                        Err(error) => {
                            break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                map_watchdog_error(error),
                                config.cleanup_timeout,
                            );
                        }
                    },
                    Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied { .. }) => {
                        let restored = match inflight.definitely_not_applied(response_observed_at) {
                            Ok(restored) => restored,
                            Err(error) => {
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    map_watchdog_error(error),
                                    config.cleanup_timeout,
                                );
                            }
                        };
                        let retry_at = Instant::now()
                            .checked_add(config.retry_delay)
                            .map(|candidate| candidate.min(restored.schedule().safety_deadline()))
                            .unwrap_or(restored.schedule().safety_deadline());
                        tokio::select! {
                            biased;
                            command = shutdown_commands.recv() => {
                                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                                    receive_shutdown(command, &mut shutdown_acknowledgement),
                                    config.cleanup_timeout,
                                );
                            }
                            _ = sleep_until(TokioInstant::from_std(retry_at)) => {
                                current = Some(restored);
                            }
                        }
                    }
                    Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown { .. }) => {
                        let _unknown = inflight.into_unknown();
                        guard.invalidate_now();
                        break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                            RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown,
                            config.cleanup_timeout,
                        );
                    }
                }
            }
            RuntimeGatewayOwnerWatchdogActionV1::InvalidateNow => {
                break 'supervisor RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                    RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                    config.cleanup_timeout,
                );
            }
        }
    };
    guard.invalidate_now();
    let release = release_gateway_owner_v1(&port, lease_id, stop.cleanup_deadline).await;
    let exit = finalize_gateway_owner_exit_v1(stop.exit, release);
    guard.disarm();
    if let Some(response) = shutdown_acknowledgement {
        let _result = response.send(exit);
    }
    exit
}

enum RuntimeGatewayOwnerStartupObservationStepV1 {
    Continue {
        successor: Box<RuntimeGatewayOwnerWatchdogV1>,
        response: oneshot::Sender<
            Result<
                RuntimeGatewayOwnerCurrentObservationV1,
                RuntimeGatewayOwnerCurrentObservationErrorV1,
            >,
        >,
        result: Result<
            RuntimeGatewayOwnerCurrentObservationV1,
            RuntimeGatewayOwnerCurrentObservationErrorV1,
        >,
    },
    Stop(RuntimeGatewayOwnerStartupWatchdogStopV1),
}

async fn observe_current_gateway_owner_v1<P>(
    port: &P,
    watchdog: RuntimeGatewayOwnerWatchdogV1,
    command: RuntimeGatewayOwnerStartupObservationCommandV1,
    shutdown_commands: &mut mpsc::Receiver<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    shutdown_acknowledgement: &mut Option<
        oneshot::Sender<RuntimeGatewayOwnerStartupWatchdogExitV1>,
    >,
    guard: &mut RuntimeGatewayOwnerStartupWatchdogGuardV1,
    cleanup_timeout: Duration,
) -> RuntimeGatewayOwnerStartupObservationStepV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
    P::Error: Send,
{
    let request_started_at = Instant::now();
    let inflight = match watchdog.begin_current_observation(request_started_at) {
        Ok(inflight) => inflight,
        Err(error) => {
            return stop_after_observation_error_v1(
                command,
                map_current_observation_error(error),
                map_watchdog_error(error),
                guard,
                cleanup_timeout,
            );
        }
    };
    let renew_at = inflight.previous_schedule().renew_at();
    let safety_deadline = inflight.previous_schedule().safety_deadline();
    let request = inflight.request().clone();
    let observation = port.observe_gateway_owner(request);
    tokio::pin!(observation);
    let result = tokio::select! {
        biased;
        shutdown = shutdown_commands.recv() => {
            guard.invalidate_now();
            let stop = RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                receive_shutdown(shutdown, shutdown_acknowledgement),
                cleanup_timeout,
            );
            let _result = command.response.send(Err(
                RuntimeGatewayOwnerCurrentObservationErrorV1::SupervisorUnavailable,
            ));
            return RuntimeGatewayOwnerStartupObservationStepV1::Stop(stop);
        }
        _ = sleep_until(TokioInstant::from_std(renew_at)) => {
            let response_observed_at = Instant::now();
            return match inflight.observation_failed(response_observed_at) {
                Ok(restored) => RuntimeGatewayOwnerStartupObservationStepV1::Continue {
                    successor: Box::new(restored),
                    response: command.response,
                    result: Err(RuntimeGatewayOwnerCurrentObservationErrorV1::Retryable),
                },
                Err(error) => stop_after_observation_error_v1(
                    command,
                    map_current_observation_error(error),
                    map_watchdog_error(error),
                    guard,
                    cleanup_timeout,
                ),
            };
        }
        _ = sleep_until(TokioInstant::from_std(safety_deadline)) => {
            guard.invalidate_now();
            let stop = RuntimeGatewayOwnerStartupWatchdogStopV1::new(
                RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
                cleanup_timeout,
            );
            let _result = command.response.send(Err(
                RuntimeGatewayOwnerCurrentObservationErrorV1::SafetyElapsed,
            ));
            return RuntimeGatewayOwnerStartupObservationStepV1::Stop(stop);
        }
        result = &mut observation => result,
    };
    let response_observed_at = Instant::now();
    match result {
        Ok(observation) => match inflight.complete(observation, response_observed_at) {
            Ok(RuntimeGatewayOwnerObservationCompletionV1::Current(successor)) => {
                let projection = RuntimeGatewayOwnerCurrentObservationV1::from_watchdog(&successor);
                RuntimeGatewayOwnerStartupObservationStepV1::Continue {
                    successor,
                    response: command.response,
                    result: Ok(projection),
                }
            }
            Ok(RuntimeGatewayOwnerObservationCompletionV1::OwnershipLost(_)) => {
                stop_after_observation_error_v1(
                    command,
                    RuntimeGatewayOwnerCurrentObservationErrorV1::OwnershipLost,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                    guard,
                    cleanup_timeout,
                )
            }
            Err(error) => stop_after_observation_error_v1(
                command,
                map_current_observation_error(error),
                map_watchdog_error(error),
                guard,
                cleanup_timeout,
            ),
        },
        Err(error) => match P::classify_observation_error(&error) {
            RuntimeGatewayOwnerObservationErrorClassV1::Retryable => {
                match inflight.observation_failed(response_observed_at) {
                    Ok(restored) => RuntimeGatewayOwnerStartupObservationStepV1::Continue {
                        successor: Box::new(restored),
                        response: command.response,
                        result: Err(RuntimeGatewayOwnerCurrentObservationErrorV1::Retryable),
                    },
                    Err(error) => stop_after_observation_error_v1(
                        command,
                        map_current_observation_error(error),
                        map_watchdog_error(error),
                        guard,
                        cleanup_timeout,
                    ),
                }
            }
            RuntimeGatewayOwnerObservationErrorClassV1::OwnershipLost => {
                stop_after_observation_error_v1(
                    command,
                    RuntimeGatewayOwnerCurrentObservationErrorV1::OwnershipLost,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
                    guard,
                    cleanup_timeout,
                )
            }
            RuntimeGatewayOwnerObservationErrorClassV1::ProtocolViolation => {
                stop_after_observation_error_v1(
                    command,
                    RuntimeGatewayOwnerCurrentObservationErrorV1::ProtocolViolation,
                    RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
                    guard,
                    cleanup_timeout,
                )
            }
        },
    }
}

fn stop_after_observation_error_v1(
    command: RuntimeGatewayOwnerStartupObservationCommandV1,
    response_error: RuntimeGatewayOwnerCurrentObservationErrorV1,
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
    guard: &mut RuntimeGatewayOwnerStartupWatchdogGuardV1,
    cleanup_timeout: Duration,
) -> RuntimeGatewayOwnerStartupObservationStepV1 {
    guard.invalidate_now();
    let _result = command.response.send(Err(response_error));
    RuntimeGatewayOwnerStartupObservationStepV1::Stop(
        RuntimeGatewayOwnerStartupWatchdogStopV1::new(exit, cleanup_timeout),
    )
}

struct RuntimeGatewayOwnerStartupWatchdogStopV1 {
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
    cleanup_deadline: TokioInstant,
}

impl RuntimeGatewayOwnerStartupWatchdogStopV1 {
    fn new(exit: RuntimeGatewayOwnerStartupWatchdogExitV1, cleanup_timeout: Duration) -> Self {
        Self {
            exit,
            cleanup_deadline: TokioInstant::now() + cleanup_timeout,
        }
    }
}

fn receive_shutdown(
    command: Option<RuntimeGatewayOwnerStartupShutdownCommandV1>,
    acknowledgement: &mut Option<oneshot::Sender<RuntimeGatewayOwnerStartupWatchdogExitV1>>,
) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
    if let Some(RuntimeGatewayOwnerStartupShutdownCommandV1 { response }) = command {
        *acknowledgement = Some(response);
    }
    RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
}

fn map_current_observation_error(
    error: RuntimeGatewayOwnerWatchdogErrorV1,
) -> RuntimeGatewayOwnerCurrentObservationErrorV1 {
    match error {
        RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(
            RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed,
        ) => RuntimeGatewayOwnerCurrentObservationErrorV1::SafetyElapsed,
        RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed
        | RuntimeGatewayOwnerWatchdogErrorV1::RequestedLeaseTooShort
        | RuntimeGatewayOwnerWatchdogErrorV1::RevisionExhausted
        | RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { .. }
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(_) => {
            RuntimeGatewayOwnerCurrentObservationErrorV1::ProtocolViolation
        }
    }
}

fn map_watchdog_error(
    error: RuntimeGatewayOwnerWatchdogErrorV1,
) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
    match error {
        RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(
            RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed,
        ) => RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed,
        RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed
        | RuntimeGatewayOwnerWatchdogErrorV1::RequestedLeaseTooShort
        | RuntimeGatewayOwnerWatchdogErrorV1::RevisionExhausted
        | RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { .. }
        | RuntimeGatewayOwnerWatchdogErrorV1::Schedule(_) => {
            RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
        }
    }
}

fn finalize_gateway_owner_exit_v1(
    exit: RuntimeGatewayOwnerStartupWatchdogExitV1,
    release: RuntimeGatewayOwnerReleaseStatusV1,
) -> RuntimeGatewayOwnerStartupWatchdogExitV1 {
    match release {
        RuntimeGatewayOwnerReleaseStatusV1::Confirmed => exit,
        RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed => {
            RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
        }
        RuntimeGatewayOwnerReleaseStatusV1::ProtocolViolation => {
            RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
        }
    }
}

async fn release_gateway_owner_v1<P>(
    port: &P,
    lease_id: RuntimeGatewayOwnerLeaseIdV1,
    cleanup_deadline: TokioInstant,
) -> RuntimeGatewayOwnerReleaseStatusV1
where
    P: RuntimeGatewayOwnerLeasePortV1 + Send + Sync,
{
    let request = RuntimeReleaseGatewayOwnerLeaseV1 { lease_id };
    for _attempt in 0..RELEASE_ATTEMPTS {
        let release = match timeout_at(
            cleanup_deadline,
            port.release_gateway_owner(request.clone()),
        )
        .await
        {
            Ok(release) => release,
            Err(_) => return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed,
        };
        match release {
            Ok(outcome) => match accept_gateway_owner_release_v1(&request, outcome) {
                Ok(RuntimeAcceptedGatewayOwnerReleaseV1::Released)
                | Ok(RuntimeAcceptedGatewayOwnerReleaseV1::NotHeld(_)) => {
                    return RuntimeGatewayOwnerReleaseStatusV1::Confirmed;
                }
                Err(_) => return RuntimeGatewayOwnerReleaseStatusV1::ProtocolViolation,
            },
            Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied { .. }) => {}
            Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown { .. }) => {
                let observation = match timeout_at(
                    cleanup_deadline,
                    port.observe_gateway_owner(
                        automation_runtime_controller::RuntimeObserveGatewayOwnerLeaseV1 {
                            gateway_shard_id: request.lease_id.gateway_shard_id.clone(),
                        },
                    ),
                )
                .await
                {
                    Ok(Ok(observation)) => observation,
                    Ok(Err(_)) | Err(_) => {
                        return RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed;
                    }
                };
                match classify_unknown_gateway_owner_release_v1(&request, observation) {
                    RuntimeGatewayOwnerReleaseRecoveryV1::ReplaySameRequest => {}
                    RuntimeGatewayOwnerReleaseRecoveryV1::CompleteWithoutOwnership(_) => {
                        return RuntimeGatewayOwnerReleaseStatusV1::Confirmed;
                    }
                    RuntimeGatewayOwnerReleaseRecoveryV1::ProtocolViolation => {
                        return RuntimeGatewayOwnerReleaseStatusV1::ProtocolViolation;
                    }
                }
            }
        }
    }
    RuntimeGatewayOwnerReleaseStatusV1::Unconfirmed
}

struct RuntimeGatewayOwnerInvalidationLatchV1 {
    invalidator: Arc<dyn RuntimeGatewayOwnerEmergencyInvalidatorV1>,
    invalidated: AtomicBool,
}

impl RuntimeGatewayOwnerInvalidationLatchV1 {
    fn new(invalidator: impl RuntimeGatewayOwnerEmergencyInvalidatorV1) -> Self {
        Self {
            invalidator: Arc::new(invalidator),
            invalidated: AtomicBool::new(false),
        }
    }

    fn invalidate(&self) {
        if !self.invalidated.swap(true, Ordering::AcqRel) {
            self.invalidator.invalidate_gateway_ownership();
        }
    }
}

struct RuntimeGatewayOwnerStartupWatchdogGuardV1 {
    invalidation: Arc<RuntimeGatewayOwnerInvalidationLatchV1>,
    armed: bool,
}

impl RuntimeGatewayOwnerStartupWatchdogGuardV1 {
    fn new(invalidation: Arc<RuntimeGatewayOwnerInvalidationLatchV1>) -> Self {
        Self {
            invalidation,
            armed: true,
        }
    }

    fn invalidate_now(&mut self) {
        self.invalidation.invalidate();
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for RuntimeGatewayOwnerStartupWatchdogGuardV1 {
    fn drop(&mut self) {
        if self.armed {
            self.invalidation.invalidate();
        }
    }
}
