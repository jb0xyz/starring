use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use automation_runtime_worker::{
    RuntimeAcceptedIngressOpenAcknowledgementV2, RuntimeIngressOpenAcknowledgementMutationErrorV2,
    RuntimeIngressOpenAcknowledgementObservationErrorClassV2,
    RuntimeIngressOpenAcknowledgementPortV2, RuntimeIngressOpenAcknowledgementResolutionV2,
    RuntimeIngressOpenAcknowledgementSingleFlightV2,
};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{sleep_until, timeout_at, Instant as TokioInstant};

const RUNTIME_INGRESS_ACKNOWLEDGEMENT_DATA_CAPACITY: usize = 1;
const RUNTIME_INGRESS_ACKNOWLEDGEMENT_CONTROL_CAPACITY: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeIngressAcknowledgementSupervisorConfigErrorV2 {
    #[error("runtime ingress acknowledgement retry delay is zero")]
    ZeroRetryDelay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeIngressAcknowledgementSupervisorConfigV2 {
    retry_delay: Duration,
}

impl RuntimeIngressAcknowledgementSupervisorConfigV2 {
    pub(crate) fn new(
        retry_delay: Duration,
    ) -> Result<Self, RuntimeIngressAcknowledgementSupervisorConfigErrorV2> {
        if retry_delay.is_zero() {
            return Err(RuntimeIngressAcknowledgementSupervisorConfigErrorV2::ZeroRetryDelay);
        }
        Ok(Self { retry_delay })
    }

    pub(crate) const fn retry_delay(self) -> Duration {
        self.retry_delay
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeIngressAcknowledgementFailureV2 {
    OperationDeadlineElapsed,
    Shutdown,
    AttemptUnavailable,
    ObservationAuthorityLost,
    ObservationProtocolViolation,
    ReplayBudgetExhausted,
    SecondUncertainty,
    Stale,
    Divergent,
    ResolutionProtocolViolation,
}

impl RuntimeIngressAcknowledgementFailureV2 {
    #[cfg(test)]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::OperationDeadlineElapsed => {
                "runtime_ingress_acknowledgement_operation_deadline_elapsed"
            }
            Self::Shutdown => "runtime_ingress_acknowledgement_shutdown",
            Self::AttemptUnavailable => "runtime_ingress_acknowledgement_attempt_unavailable",
            Self::ObservationAuthorityLost => {
                "runtime_ingress_acknowledgement_observation_authority_lost"
            }
            Self::ObservationProtocolViolation => {
                "runtime_ingress_acknowledgement_observation_protocol_violation"
            }
            Self::ReplayBudgetExhausted => {
                "runtime_ingress_acknowledgement_replay_budget_exhausted"
            }
            Self::SecondUncertainty => "runtime_ingress_acknowledgement_second_uncertainty",
            Self::Stale => "runtime_ingress_acknowledgement_stale",
            Self::Divergent => "runtime_ingress_acknowledgement_divergent",
            Self::ResolutionProtocolViolation => {
                "runtime_ingress_acknowledgement_resolution_protocol_violation"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeIngressAcknowledgementSupervisorExitV2 {
    Commanded,
    IntakeClosed,
    DeadlineElapsed,
    ProtocolViolation,
    Panicked,
    Aborted,
}

impl RuntimeIngressAcknowledgementSupervisorExitV2 {
    #[cfg(test)]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Commanded => "runtime_ingress_acknowledgement_supervisor_commanded",
            Self::IntakeClosed => "runtime_ingress_acknowledgement_supervisor_intake_closed",
            Self::DeadlineElapsed => "runtime_ingress_acknowledgement_supervisor_deadline_elapsed",
            Self::ProtocolViolation => {
                "runtime_ingress_acknowledgement_supervisor_protocol_violation"
            }
            Self::Panicked => "runtime_ingress_acknowledgement_supervisor_panicked",
            Self::Aborted => "runtime_ingress_acknowledgement_supervisor_aborted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeIngressAcknowledgementCompletionClassV2 {
    Accepted,
    CompletionRejected,
    FailedClosed(RuntimeIngressAcknowledgementFailureV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeIngressAcknowledgementSupervisorPhaseV2 {
    Accepting,
    Busy,
    ShutdownSealed,
    Terminal,
}

#[derive(Clone)]
pub(crate) struct RuntimeIngressAcknowledgementShutdownHandleV2 {
    control: mpsc::Sender<RuntimeIngressAcknowledgementShutdownCommandV2>,
    shared: Arc<RuntimeIngressAcknowledgementSharedV2>,
}

impl RuntimeIngressAcknowledgementShutdownHandleV2 {
    pub(crate) fn seal_until_v2(&self, deadline: Instant) {
        self.shared.seal_shutdown(deadline);
        let command = RuntimeIngressAcknowledgementShutdownCommandV2 { deadline };
        match self.control.try_send(command) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Closed(_)) => {}
        }
    }
}

impl Debug for RuntimeIngressAcknowledgementShutdownHandleV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressAcknowledgementShutdownHandleV2(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeIngressAcknowledgementTerminalObserverV2 {
    terminal: watch::Receiver<Option<RuntimeIngressAcknowledgementSupervisorExitV2>>,
}

impl RuntimeIngressAcknowledgementTerminalObserverV2 {
    pub(crate) async fn wait_v2(&mut self) -> RuntimeIngressAcknowledgementSupervisorExitV2 {
        loop {
            if let Some(exit) = *self.terminal.borrow() {
                return exit;
            }
            if self.terminal.changed().await.is_err() {
                return RuntimeIngressAcknowledgementSupervisorExitV2::Aborted;
            }
        }
    }
}

impl Debug for RuntimeIngressAcknowledgementTerminalObserverV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressAcknowledgementTerminalObserverV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeIngressAcknowledgementJobIdV2 {
    sequence: NonZeroU64,
}

impl RuntimeIngressAcknowledgementJobIdV2 {
    #[cfg(test)]
    pub(crate) const fn sequence(self) -> NonZeroU64 {
        self.sequence
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeIngressAcknowledgementSupervisorSnapshotV2 {
    phase: RuntimeIngressAcknowledgementSupervisorPhaseV2,
    active_job_id: Option<RuntimeIngressAcknowledgementJobIdV2>,
    completed_jobs: u64,
    last_completion: Option<RuntimeIngressAcknowledgementCompletionClassV2>,
    terminal: Option<RuntimeIngressAcknowledgementSupervisorExitV2>,
}

#[cfg(test)]
impl RuntimeIngressAcknowledgementSupervisorSnapshotV2 {
    pub(crate) const fn phase(self) -> RuntimeIngressAcknowledgementSupervisorPhaseV2 {
        self.phase
    }

    pub(crate) const fn active_job_id(self) -> Option<RuntimeIngressAcknowledgementJobIdV2> {
        self.active_job_id
    }

    pub(crate) const fn completed_jobs(self) -> u64 {
        self.completed_jobs
    }

    pub(crate) const fn last_completion(
        self,
    ) -> Option<RuntimeIngressAcknowledgementCompletionClassV2> {
        self.last_completion
    }

    pub(crate) const fn terminal(self) -> Option<RuntimeIngressAcknowledgementSupervisorExitV2> {
        self.terminal
    }
}

struct RuntimeIngressAcknowledgementSharedStateV2 {
    phase: RuntimeIngressAcknowledgementSupervisorPhaseV2,
    next_sequence: Option<NonZeroU64>,
    active_job_id: Option<RuntimeIngressAcknowledgementJobIdV2>,
    completed_jobs: u64,
    last_completion: Option<RuntimeIngressAcknowledgementCompletionClassV2>,
    terminal: Option<RuntimeIngressAcknowledgementSupervisorExitV2>,
    shutdown_deadline: Option<Instant>,
}

struct RuntimeIngressAcknowledgementSharedV2 {
    state: Mutex<RuntimeIngressAcknowledgementSharedStateV2>,
    terminal: watch::Sender<Option<RuntimeIngressAcknowledgementSupervisorExitV2>>,
    shutdown_deadline: watch::Sender<Option<Instant>>,
}

impl RuntimeIngressAcknowledgementSharedV2 {
    fn lock(&self) -> MutexGuard<'_, RuntimeIngressAcknowledgementSharedStateV2> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn reserve(
        &self,
    ) -> Result<
        RuntimeIngressAcknowledgementJobIdV2,
        RuntimeIngressAcknowledgementRegistrationRejectionReasonV2,
    > {
        let mut state = self.lock();
        if let Some(terminal) = state.terminal {
            return Err(
                RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::SupervisorTerminal(
                    terminal,
                ),
            );
        }
        if matches!(
            state.phase,
            RuntimeIngressAcknowledgementSupervisorPhaseV2::ShutdownSealed
                | RuntimeIngressAcknowledgementSupervisorPhaseV2::Terminal
        ) {
            return Err(RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::IntakeSealed);
        }
        if state.active_job_id.is_some() {
            return Err(RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::Busy);
        }
        let sequence = state
            .next_sequence
            .ok_or(RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::SequenceExhausted)?;
        let id = RuntimeIngressAcknowledgementJobIdV2 { sequence };
        state.next_sequence = sequence.get().checked_add(1).and_then(NonZeroU64::new);
        state.active_job_id = Some(id);
        state.phase = RuntimeIngressAcknowledgementSupervisorPhaseV2::Busy;
        Ok(id)
    }

    fn release_rejected(&self, id: RuntimeIngressAcknowledgementJobIdV2) {
        let mut state = self.lock();
        if state.active_job_id == Some(id) {
            state.active_job_id = None;
            if state.terminal.is_none() {
                state.phase = if state.shutdown_deadline.is_some() {
                    RuntimeIngressAcknowledgementSupervisorPhaseV2::ShutdownSealed
                } else {
                    RuntimeIngressAcknowledgementSupervisorPhaseV2::Accepting
                };
            }
        }
    }

    fn complete(
        &self,
        id: RuntimeIngressAcknowledgementJobIdV2,
        class: RuntimeIngressAcknowledgementCompletionClassV2,
    ) -> bool {
        let mut state = self.lock();
        if state.active_job_id != Some(id) {
            return false;
        }
        state.completed_jobs = state.completed_jobs.saturating_add(1);
        state.last_completion = Some(class);
        true
    }

    fn release_completion(&self, id: RuntimeIngressAcknowledgementJobIdV2) {
        let mut state = self.lock();
        if state.active_job_id == Some(id) {
            state.active_job_id = None;
            if state.terminal.is_none() {
                state.phase = if state.shutdown_deadline.is_some() {
                    RuntimeIngressAcknowledgementSupervisorPhaseV2::ShutdownSealed
                } else {
                    RuntimeIngressAcknowledgementSupervisorPhaseV2::Accepting
                };
            }
        }
    }

    fn seal_shutdown(&self, deadline: Instant) -> Option<Instant> {
        let mut state = self.lock();
        if state.terminal.is_some() {
            return state.shutdown_deadline;
        }
        let deadline = state
            .shutdown_deadline
            .map_or(deadline, |current| current.min(deadline));
        state.shutdown_deadline = Some(deadline);
        state.phase = RuntimeIngressAcknowledgementSupervisorPhaseV2::ShutdownSealed;
        drop(state);
        self.shutdown_deadline.send_replace(Some(deadline));
        Some(deadline)
    }

    fn shutdown_deadline(&self) -> Option<Instant> {
        self.lock().shutdown_deadline
    }

    fn publish_terminal(
        &self,
        exit: RuntimeIngressAcknowledgementSupervisorExitV2,
    ) -> RuntimeIngressAcknowledgementSupervisorExitV2 {
        let mut state = self.lock();
        let published = state.terminal.unwrap_or(exit);
        state.terminal = Some(published);
        state.phase = RuntimeIngressAcknowledgementSupervisorPhaseV2::Terminal;
        drop(state);
        self.terminal.send_replace(Some(published));
        published
    }

    #[cfg(test)]
    fn snapshot(&self) -> RuntimeIngressAcknowledgementSupervisorSnapshotV2 {
        let state = self.lock();
        RuntimeIngressAcknowledgementSupervisorSnapshotV2 {
            phase: state.phase,
            active_job_id: state.active_job_id,
            completed_jobs: state.completed_jobs,
            last_completion: state.last_completion,
            terminal: state.terminal,
        }
    }
}

pub(crate) trait RuntimeIngressAcknowledgementAuthorityV2: Send + 'static {
    type Output: Send + 'static;
    type CompletionError: Send + 'static;

    fn operation_mut(&mut self) -> &mut RuntimeIngressOpenAcknowledgementSingleFlightV2;

    fn complete(
        self,
        accepted: RuntimeAcceptedIngressOpenAcknowledgementV2,
    ) -> Result<Self::Output, (Self, Self::CompletionError)>
    where
        Self: Sized;
}

pub(crate) struct RuntimeWorkerIngressAcknowledgementJobV2<A> {
    authority: A,
}

impl<A> RuntimeWorkerIngressAcknowledgementJobV2<A> {
    pub(crate) fn new(authority: A) -> Self {
        Self { authority }
    }

    pub(crate) fn into_authority(self) -> A {
        self.authority
    }
}

impl<A> Debug for RuntimeWorkerIngressAcknowledgementJobV2<A> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeWorkerIngressAcknowledgementJobV2(<redacted>)")
    }
}

pub(crate) enum RuntimeIngressAcknowledgementExecutionResultV2<J, O, E> {
    Accepted(O),
    CompletionRejected {
        job: J,
        error: E,
    },
    FailedClosed {
        job: J,
        failure: RuntimeIngressAcknowledgementFailureV2,
    },
}

impl<J, O, E> RuntimeIngressAcknowledgementExecutionResultV2<J, O, E> {
    fn class(&self) -> RuntimeIngressAcknowledgementCompletionClassV2 {
        match self {
            Self::Accepted(_) => RuntimeIngressAcknowledgementCompletionClassV2::Accepted,
            Self::CompletionRejected { .. } => {
                RuntimeIngressAcknowledgementCompletionClassV2::CompletionRejected
            }
            Self::FailedClosed { failure, .. } => {
                RuntimeIngressAcknowledgementCompletionClassV2::FailedClosed(*failure)
            }
        }
    }
}

impl<J, O, E> Debug for RuntimeIngressAcknowledgementExecutionResultV2<J, O, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressAcknowledgementExecutionResultV2(<redacted>)")
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeIngressAcknowledgementExecutionContextV2 {
    operation_deadline: Instant,
    retry_delay: Duration,
    shutdown_deadline: watch::Receiver<Option<Instant>>,
}

impl RuntimeIngressAcknowledgementExecutionContextV2 {
    fn effective_deadline(&self) -> Instant {
        self.shutdown_deadline
            .borrow()
            .as_ref()
            .copied()
            .map_or(self.operation_deadline, |shutdown| {
                shutdown.min(self.operation_deadline)
            })
    }

    fn shutdown_requested(&self) -> bool {
        self.shutdown_deadline.borrow().is_some()
    }

    fn cutoff_failure(&self) -> RuntimeIngressAcknowledgementFailureV2 {
        if self.shutdown_requested() {
            RuntimeIngressAcknowledgementFailureV2::Shutdown
        } else {
            RuntimeIngressAcknowledgementFailureV2::OperationDeadlineElapsed
        }
    }
}

pub(crate) trait RuntimeIngressAcknowledgementLaneJobV2<P>: Send + 'static {
    type Output: Send + 'static;
    type CompletionError: Send + 'static;

    fn execute(
        self,
        port: &P,
        context: RuntimeIngressAcknowledgementExecutionContextV2,
    ) -> impl Future<
        Output = RuntimeIngressAcknowledgementExecutionResultV2<
            Self,
            Self::Output,
            Self::CompletionError,
        >,
    > + Send
    where
        Self: Sized;
}

impl<P, A> RuntimeIngressAcknowledgementLaneJobV2<P> for RuntimeWorkerIngressAcknowledgementJobV2<A>
where
    P: RuntimeIngressOpenAcknowledgementPortV2 + Send + Sync + 'static,
    P::Error: Send,
    A: RuntimeIngressAcknowledgementAuthorityV2,
{
    type Output = A::Output;
    type CompletionError = A::CompletionError;

    async fn execute(
        mut self,
        port: &P,
        context: RuntimeIngressAcknowledgementExecutionContextV2,
    ) -> RuntimeIngressAcknowledgementExecutionResultV2<Self, Self::Output, Self::CompletionError>
    {
        let mut uncertainty_seen = false;
        loop {
            if Instant::now() >= context.effective_deadline() {
                let failure = context.cutoff_failure();
                return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                    job: self,
                    failure,
                };
            }
            if context.shutdown_requested() {
                return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                    job: self,
                    failure: RuntimeIngressAcknowledgementFailureV2::Shutdown,
                };
            }
            let attempt = match self.authority.operation_mut().begin_attempt() {
                Ok(attempt) => attempt,
                Err(_) => {
                    return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                        job: self,
                        failure: RuntimeIngressAcknowledgementFailureV2::AttemptUnavailable,
                    };
                }
            };
            let completion = port.publish_ingress_open_acknowledgement(attempt).await;
            let (attempt, result) = completion.into_parts();
            let resolution = match result {
                Ok(outcome) => attempt.resolve_outcome(outcome),
                Err(RuntimeIngressOpenAcknowledgementMutationErrorV2::DefinitelyNotApplied {
                    ..
                }) => attempt.resolve_definitely_not_applied(),
                Err(RuntimeIngressOpenAcknowledgementMutationErrorV2::OutcomeUnknown {
                    ..
                }) => {
                    if uncertainty_seen {
                        return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                            job: self,
                            failure: RuntimeIngressAcknowledgementFailureV2::SecondUncertainty,
                        };
                    }
                    uncertainty_seen = true;
                    match observe_unknown_until_v2(port, &attempt, &context).await {
                        Ok(observation) => attempt.resolve_unknown(observation),
                        Err(failure) => {
                            return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                                job: self,
                                failure,
                            };
                        }
                    }
                }
            };
            match resolution {
                RuntimeIngressOpenAcknowledgementResolutionV2::AppliedExact(accepted)
                | RuntimeIngressOpenAcknowledgementResolutionV2::ReplayedExact(accepted)
                | RuntimeIngressOpenAcknowledgementResolutionV2::AdoptExact(accepted) => {
                    return match self.authority.complete(accepted) {
                        Ok(output) => {
                            RuntimeIngressAcknowledgementExecutionResultV2::Accepted(output)
                        }
                        Err((authority, error)) => {
                            self.authority = authority;
                            RuntimeIngressAcknowledgementExecutionResultV2::CompletionRejected {
                                job: self,
                                error,
                            }
                        }
                    };
                }
                RuntimeIngressOpenAcknowledgementResolutionV2::ReplaySameRequest => {
                    if context.shutdown_requested() {
                        return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                            job: self,
                            failure: RuntimeIngressAcknowledgementFailureV2::Shutdown,
                        };
                    }
                }
                RuntimeIngressOpenAcknowledgementResolutionV2::ReplayBudgetExhausted => {
                    return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                        job: self,
                        failure: RuntimeIngressAcknowledgementFailureV2::ReplayBudgetExhausted,
                    };
                }
                RuntimeIngressOpenAcknowledgementResolutionV2::Stale => {
                    return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                        job: self,
                        failure: RuntimeIngressAcknowledgementFailureV2::Stale,
                    };
                }
                RuntimeIngressOpenAcknowledgementResolutionV2::Divergent => {
                    return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                        job: self,
                        failure: RuntimeIngressAcknowledgementFailureV2::Divergent,
                    };
                }
                RuntimeIngressOpenAcknowledgementResolutionV2::ProtocolViolation(_) => {
                    return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                        job: self,
                        failure:
                            RuntimeIngressAcknowledgementFailureV2::ResolutionProtocolViolation,
                    };
                }
            }
        }
    }
}

async fn observe_unknown_until_v2<P>(
    port: &P,
    attempt: &automation_runtime_worker::RuntimeIngressOpenAcknowledgementAttemptV2<'_>,
    context: &RuntimeIngressAcknowledgementExecutionContextV2,
) -> Result<
    automation_runtime_controller::RuntimeObservedIngressOpenAcknowledgementV2,
    RuntimeIngressAcknowledgementFailureV2,
>
where
    P: RuntimeIngressOpenAcknowledgementPortV2 + Send + Sync + 'static,
    P::Error: Send,
{
    let mut shutdown = context.shutdown_deadline.clone();
    loop {
        let deadline = context.effective_deadline();
        if Instant::now() >= deadline {
            return Err(context.cutoff_failure());
        }
        let observation = tokio::select! {
            biased;
            changed = shutdown.changed() => {
                let _ = changed;
                continue;
            }
            observation = timeout_at(
                TokioInstant::from_std(deadline),
                port.observe_ingress_open_acknowledgement(attempt),
            ) => observation,
        };
        match observation {
            Ok(Ok(observation)) => return Ok(observation),
            Ok(Err(error)) => match P::classify_observation_error(&error) {
                RuntimeIngressOpenAcknowledgementObservationErrorClassV2::Retryable => {
                    let wake = Instant::now()
                        .checked_add(context.retry_delay)
                        .unwrap_or(deadline)
                        .min(deadline);
                    if wake >= deadline {
                        return Err(context.cutoff_failure());
                    }
                    tokio::select! {
                        biased;
                        changed = shutdown.changed() => {
                            let _ = changed;
                        }
                        _ = sleep_until(TokioInstant::from_std(wake)) => {}
                    }
                }
                RuntimeIngressOpenAcknowledgementObservationErrorClassV2::AuthorityLost => {
                    return Err(RuntimeIngressAcknowledgementFailureV2::ObservationAuthorityLost);
                }
                RuntimeIngressOpenAcknowledgementObservationErrorClassV2::ProtocolViolation => {
                    return Err(
                        RuntimeIngressAcknowledgementFailureV2::ObservationProtocolViolation,
                    );
                }
            },
            Err(_) => return Err(context.cutoff_failure()),
        }
    }
}

type RuntimeIngressAcknowledgementJobExecutionV2<J, P> =
    RuntimeIngressAcknowledgementExecutionResultV2<
        J,
        <J as RuntimeIngressAcknowledgementLaneJobV2<P>>::Output,
        <J as RuntimeIngressAcknowledgementLaneJobV2<P>>::CompletionError,
    >;

pub(crate) struct RuntimeIngressAcknowledgementCompletionV2<J, P>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    job_id: RuntimeIngressAcknowledgementJobIdV2,
    result: Option<RuntimeIngressAcknowledgementJobExecutionV2<J, P>>,
    shared: Arc<RuntimeIngressAcknowledgementSharedV2>,
}

impl<J, P> RuntimeIngressAcknowledgementCompletionV2<J, P>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    #[cfg(test)]
    pub(crate) const fn job_id(&self) -> RuntimeIngressAcknowledgementJobIdV2 {
        self.job_id
    }

    #[cfg(test)]
    pub(crate) fn result(&self) -> &RuntimeIngressAcknowledgementJobExecutionV2<J, P> {
        self.result
            .as_ref()
            .expect("runtime ingress acknowledgement completion is present")
    }

    pub(crate) fn into_result(mut self) -> RuntimeIngressAcknowledgementJobExecutionV2<J, P> {
        self.result
            .take()
            .expect("runtime ingress acknowledgement completion is present")
    }
}

impl<J, P> Debug for RuntimeIngressAcknowledgementCompletionV2<J, P>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressAcknowledgementCompletionV2(<redacted>)")
    }
}

impl<J, P> Drop for RuntimeIngressAcknowledgementCompletionV2<J, P>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    fn drop(&mut self) {
        self.shared.release_completion(self.job_id);
    }
}

pub(crate) struct RuntimeIngressAcknowledgementWaiterV2 {
    job_id: RuntimeIngressAcknowledgementJobIdV2,
    receiver: oneshot::Receiver<RuntimeIngressAcknowledgementCompletionClassV2>,
    terminal: watch::Receiver<Option<RuntimeIngressAcknowledgementSupervisorExitV2>>,
}

impl RuntimeIngressAcknowledgementWaiterV2 {
    #[cfg(test)]
    pub(crate) const fn job_id(&self) -> RuntimeIngressAcknowledgementJobIdV2 {
        self.job_id
    }

    pub(crate) fn cancel_v2(self) {
        let Self {
            job_id,
            receiver,
            terminal,
        } = self;
        drop((job_id, receiver, terminal));
    }

    #[cfg(test)]
    pub(crate) async fn wait(
        mut self,
    ) -> Result<
        RuntimeIngressAcknowledgementCompletionClassV2,
        RuntimeIngressAcknowledgementSupervisorExitV2,
    > {
        tokio::select! {
            biased;
            completion = &mut self.receiver => {
                completion.map_err(|_| {
                    self.terminal.borrow().unwrap_or(
                        RuntimeIngressAcknowledgementSupervisorExitV2::Aborted,
                    )
                })
            }
            changed = self.terminal.changed() => {
                let _ = changed;
                Err(self.terminal.borrow().unwrap_or(
                    RuntimeIngressAcknowledgementSupervisorExitV2::Aborted,
                ))
            }
        }
    }
}

impl Debug for RuntimeIngressAcknowledgementWaiterV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressAcknowledgementWaiterV2(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeIngressAcknowledgementRegistrationRejectionReasonV2 {
    Busy,
    IntakeSealed,
    DeadlineElapsed,
    SequenceExhausted,
    SupervisorTerminal(RuntimeIngressAcknowledgementSupervisorExitV2),
}

pub(crate) struct RuntimeIngressAcknowledgementRegistrationRejectedV2<J> {
    job: J,
    reason: RuntimeIngressAcknowledgementRegistrationRejectionReasonV2,
}

impl<J> RuntimeIngressAcknowledgementRegistrationRejectedV2<J> {
    pub(crate) const fn reason(
        &self,
    ) -> RuntimeIngressAcknowledgementRegistrationRejectionReasonV2 {
        self.reason
    }

    pub(crate) fn into_job(self) -> J {
        self.job
    }
}

impl<J> Debug for RuntimeIngressAcknowledgementRegistrationRejectedV2<J> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressAcknowledgementRegistrationRejectedV2(<redacted>)")
    }
}

struct RuntimeIngressAcknowledgementCommandV2<J> {
    job_id: RuntimeIngressAcknowledgementJobIdV2,
    job: J,
    operation_deadline: Instant,
    waiter: oneshot::Sender<RuntimeIngressAcknowledgementCompletionClassV2>,
}

struct RuntimeIngressAcknowledgementShutdownCommandV2 {
    deadline: Instant,
}

pub(crate) struct RuntimeIngressAcknowledgementSupervisorV2<P, J>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    data: mpsc::Sender<RuntimeIngressAcknowledgementCommandV2<J>>,
    control: mpsc::Sender<RuntimeIngressAcknowledgementShutdownCommandV2>,
    completions: mpsc::Receiver<RuntimeIngressAcknowledgementCompletionV2<J, P>>,
    shared: Arc<RuntimeIngressAcknowledgementSharedV2>,
    terminal: watch::Receiver<Option<RuntimeIngressAcknowledgementSupervisorExitV2>>,
    actor: Option<JoinHandle<RuntimeIngressAcknowledgementSupervisorExitV2>>,
}

impl<P, J> RuntimeIngressAcknowledgementSupervisorV2<P, J>
where
    P: Send + Sync + 'static,
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    pub(crate) fn start(port: P, config: RuntimeIngressAcknowledgementSupervisorConfigV2) -> Self {
        let (data, data_receiver) = mpsc::channel(RUNTIME_INGRESS_ACKNOWLEDGEMENT_DATA_CAPACITY);
        let (control, control_receiver) =
            mpsc::channel(RUNTIME_INGRESS_ACKNOWLEDGEMENT_CONTROL_CAPACITY);
        let (completion_sender, completions) =
            mpsc::channel(RUNTIME_INGRESS_ACKNOWLEDGEMENT_DATA_CAPACITY);
        let (terminal_sender, terminal) = watch::channel(None);
        let (shutdown_sender, shutdown_receiver) = watch::channel(None);
        let shared = Arc::new(RuntimeIngressAcknowledgementSharedV2 {
            state: Mutex::new(RuntimeIngressAcknowledgementSharedStateV2 {
                phase: RuntimeIngressAcknowledgementSupervisorPhaseV2::Accepting,
                next_sequence: Some(NonZeroU64::MIN),
                active_job_id: None,
                completed_jobs: 0,
                last_completion: None,
                terminal: None,
                shutdown_deadline: None,
            }),
            terminal: terminal_sender,
            shutdown_deadline: shutdown_sender,
        });
        let actor_shared = Arc::clone(&shared);
        let actor = tokio::spawn(async move {
            let exit = run_ingress_acknowledgement_actor_v2(
                port,
                config,
                data_receiver,
                control_receiver,
                completion_sender,
                shutdown_receiver,
                Arc::clone(&actor_shared),
            )
            .await;
            actor_shared.publish_terminal(exit)
        });
        Self {
            data,
            control,
            completions,
            shared,
            terminal,
            actor: Some(actor),
        }
    }

    pub(crate) fn try_submit(
        &self,
        job: J,
        operation_deadline: Instant,
    ) -> Result<
        RuntimeIngressAcknowledgementWaiterV2,
        RuntimeIngressAcknowledgementRegistrationRejectedV2<J>,
    > {
        if Instant::now() >= operation_deadline {
            return Err(RuntimeIngressAcknowledgementRegistrationRejectedV2 {
                job,
                reason: RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::DeadlineElapsed,
            });
        }
        let job_id = match self.shared.reserve() {
            Ok(job_id) => job_id,
            Err(reason) => {
                return Err(RuntimeIngressAcknowledgementRegistrationRejectedV2 { job, reason });
            }
        };
        let (waiter, receiver) = oneshot::channel();
        let command = RuntimeIngressAcknowledgementCommandV2 {
            job_id,
            job,
            operation_deadline,
            waiter,
        };
        match self.data.try_send(command) {
            Ok(()) => Ok(RuntimeIngressAcknowledgementWaiterV2 {
                job_id,
                receiver,
                terminal: self.terminal.clone(),
            }),
            Err(TrySendError::Full(command)) => {
                self.shared.release_rejected(job_id);
                Err(RuntimeIngressAcknowledgementRegistrationRejectedV2 {
                    job: command.job,
                    reason: RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::Busy,
                })
            }
            Err(TrySendError::Closed(command)) => {
                self.shared.release_rejected(job_id);
                Err(RuntimeIngressAcknowledgementRegistrationRejectedV2 {
                    job: command.job,
                    reason: self.shared.lock().terminal.map_or(
                        RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::IntakeSealed,
                        RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::SupervisorTerminal,
                    ),
                })
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> RuntimeIngressAcknowledgementSupervisorSnapshotV2 {
        self.shared.snapshot()
    }

    pub(crate) fn shutdown_handle_v2(&self) -> RuntimeIngressAcknowledgementShutdownHandleV2 {
        RuntimeIngressAcknowledgementShutdownHandleV2 {
            control: self.control.clone(),
            shared: Arc::clone(&self.shared),
        }
    }

    pub(crate) fn terminal_observer_v2(&self) -> RuntimeIngressAcknowledgementTerminalObserverV2 {
        RuntimeIngressAcknowledgementTerminalObserverV2 {
            terminal: self.terminal.clone(),
        }
    }

    pub(crate) fn terminal_observation(
        &self,
    ) -> Option<RuntimeIngressAcknowledgementSupervisorExitV2> {
        *self.terminal.borrow()
    }

    #[cfg(test)]
    pub(crate) async fn wait_terminal(&mut self) -> RuntimeIngressAcknowledgementSupervisorExitV2 {
        loop {
            if let Some(exit) = *self.terminal.borrow() {
                return exit;
            }
            if self.terminal.changed().await.is_err() {
                return RuntimeIngressAcknowledgementSupervisorExitV2::Aborted;
            }
        }
    }

    pub(crate) async fn recv_completion(
        &mut self,
    ) -> Option<RuntimeIngressAcknowledgementCompletionV2<J, P>> {
        self.completions.recv().await
    }

    pub(crate) async fn shutdown_until(
        mut self,
        deadline: Instant,
    ) -> RuntimeIngressAcknowledgementShutdownReportV2<J, P> {
        self.shutdown_handle_v2().seal_until_v2(deadline);
        let exit = self.join_until(deadline).await;
        let completion = self.completions.try_recv().ok();
        RuntimeIngressAcknowledgementShutdownReportV2 { exit, completion }
    }

    async fn join_until(
        &mut self,
        deadline: Instant,
    ) -> RuntimeIngressAcknowledgementSupervisorExitV2 {
        let Some(mut actor) = self.actor.take() else {
            return self
                .terminal_observation()
                .unwrap_or(RuntimeIngressAcknowledgementSupervisorExitV2::Aborted);
        };
        match timeout_at(TokioInstant::from_std(deadline), &mut actor).await {
            Ok(Ok(exit)) => self.shared.publish_terminal(exit),
            Ok(Err(error)) if error.is_panic() => self
                .shared
                .publish_terminal(RuntimeIngressAcknowledgementSupervisorExitV2::Panicked),
            Ok(Err(_)) => self
                .shared
                .publish_terminal(RuntimeIngressAcknowledgementSupervisorExitV2::Aborted),
            Err(_) => {
                actor.abort();
                let _ = actor.await;
                self.shared.publish_terminal(
                    RuntimeIngressAcknowledgementSupervisorExitV2::DeadlineElapsed,
                )
            }
        }
    }
}

impl<P, J> Debug for RuntimeIngressAcknowledgementSupervisorV2<P, J>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressAcknowledgementSupervisorV2(<redacted>)")
    }
}

impl<P, J> Drop for RuntimeIngressAcknowledgementSupervisorV2<P, J>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    fn drop(&mut self) {
        if let Some(actor) = self.actor.take() {
            actor.abort();
            self.shared
                .publish_terminal(RuntimeIngressAcknowledgementSupervisorExitV2::Aborted);
        }
    }
}

pub(crate) struct RuntimeIngressAcknowledgementShutdownReportV2<J, P>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    exit: RuntimeIngressAcknowledgementSupervisorExitV2,
    completion: Option<RuntimeIngressAcknowledgementCompletionV2<J, P>>,
}

impl<J, P> RuntimeIngressAcknowledgementShutdownReportV2<J, P>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    pub(crate) const fn exit(&self) -> RuntimeIngressAcknowledgementSupervisorExitV2 {
        self.exit
    }

    pub(crate) fn completion(&self) -> Option<&RuntimeIngressAcknowledgementCompletionV2<J, P>> {
        self.completion.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn into_completion(self) -> Option<RuntimeIngressAcknowledgementCompletionV2<J, P>> {
        self.completion
    }
}

impl<J, P> Debug for RuntimeIngressAcknowledgementShutdownReportV2<J, P>
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeIngressAcknowledgementShutdownReportV2(<redacted>)")
    }
}

async fn run_ingress_acknowledgement_actor_v2<P, J>(
    port: P,
    config: RuntimeIngressAcknowledgementSupervisorConfigV2,
    mut data: mpsc::Receiver<RuntimeIngressAcknowledgementCommandV2<J>>,
    mut control: mpsc::Receiver<RuntimeIngressAcknowledgementShutdownCommandV2>,
    completions: mpsc::Sender<RuntimeIngressAcknowledgementCompletionV2<J, P>>,
    shutdown_deadline: watch::Receiver<Option<Instant>>,
    shared: Arc<RuntimeIngressAcknowledgementSharedV2>,
) -> RuntimeIngressAcknowledgementSupervisorExitV2
where
    P: Send + Sync + 'static,
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    loop {
        tokio::select! {
            biased;
            command = control.recv() => {
                let Some(command) = command else {
                    return RuntimeIngressAcknowledgementSupervisorExitV2::IntakeClosed;
                };
                shared.seal_shutdown(command.deadline);
                if let Ok(command) = data.try_recv() {
                    let result = RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                        job: command.job,
                        failure: RuntimeIngressAcknowledgementFailureV2::Shutdown,
                    };
                    if !publish_completion_v2(
                        command.job_id,
                        result,
                        command.waiter,
                        &completions,
                        &shared,
                    )
                    .await
                    {
                        return RuntimeIngressAcknowledgementSupervisorExitV2::ProtocolViolation;
                    }
                }
                return RuntimeIngressAcknowledgementSupervisorExitV2::Commanded;
            }
            command = data.recv() => {
                let Some(command) = command else {
                    return RuntimeIngressAcknowledgementSupervisorExitV2::IntakeClosed;
                };
                if shared.shutdown_deadline().is_some() {
                    let result = RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                        job: command.job,
                        failure: RuntimeIngressAcknowledgementFailureV2::Shutdown,
                    };
                    if !publish_completion_v2(
                        command.job_id,
                        result,
                        command.waiter,
                        &completions,
                        &shared,
                    )
                    .await
                    {
                        return RuntimeIngressAcknowledgementSupervisorExitV2::ProtocolViolation;
                    }
                    return RuntimeIngressAcknowledgementSupervisorExitV2::Commanded;
                }
                let context = RuntimeIngressAcknowledgementExecutionContextV2 {
                    operation_deadline: command.operation_deadline,
                    retry_delay: config.retry_delay(),
                    shutdown_deadline: shutdown_deadline.clone(),
                };
                let result = command.job.execute(&port, context).await;
                if !publish_completion_v2(
                    command.job_id,
                    result,
                    command.waiter,
                    &completions,
                    &shared,
                )
                .await
                {
                    return RuntimeIngressAcknowledgementSupervisorExitV2::ProtocolViolation;
                }
                if shared.shutdown_deadline().is_some() {
                    return RuntimeIngressAcknowledgementSupervisorExitV2::Commanded;
                }
            }
        }
    }
}

async fn publish_completion_v2<P, J>(
    job_id: RuntimeIngressAcknowledgementJobIdV2,
    result: RuntimeIngressAcknowledgementJobExecutionV2<J, P>,
    waiter: oneshot::Sender<RuntimeIngressAcknowledgementCompletionClassV2>,
    completions: &mpsc::Sender<RuntimeIngressAcknowledgementCompletionV2<J, P>>,
    shared: &Arc<RuntimeIngressAcknowledgementSharedV2>,
) -> bool
where
    J: RuntimeIngressAcknowledgementLaneJobV2<P>,
{
    let class = result.class();
    if !shared.complete(job_id, class) {
        return false;
    }
    let _ = waiter.send(class);
    completions
        .send(RuntimeIngressAcknowledgementCompletionV2 {
            job_id,
            result: Some(result),
            shared: Arc::clone(shared),
        })
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use tokio::sync::Notify;
    use tokio::time::timeout;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum FakeStep {
        Applied,
        DefinitelyNotApplied,
        OutcomeUnknown,
        ObservationRetryable,
        ObservationApplied,
        ObservationMissing,
    }

    struct FakePort {
        steps: Mutex<VecDeque<FakeStep>>,
        mutations: AtomicUsize,
        observations: AtomicUsize,
        started: AtomicBool,
        block_mutation: AtomicBool,
        release: Notify,
    }

    impl FakePort {
        fn new(steps: impl IntoIterator<Item = FakeStep>) -> Self {
            Self {
                steps: Mutex::new(steps.into_iter().collect()),
                mutations: AtomicUsize::new(0),
                observations: AtomicUsize::new(0),
                started: AtomicBool::new(false),
                block_mutation: AtomicBool::new(false),
                release: Notify::new(),
            }
        }

        fn blocking(steps: impl IntoIterator<Item = FakeStep>) -> Self {
            let port = Self::new(steps);
            port.block_mutation.store(true, Ordering::Release);
            port
        }

        fn next(&self) -> FakeStep {
            self.steps
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
                .expect("fake ingress acknowledgement step")
        }

        async fn mutate(&self) -> FakeStep {
            self.mutations.fetch_add(1, Ordering::AcqRel);
            self.started.store(true, Ordering::Release);
            while self.block_mutation.load(Ordering::Acquire) {
                self.release.notified().await;
            }
            self.next()
        }

        async fn observe(&self) -> FakeStep {
            self.observations.fetch_add(1, Ordering::AcqRel);
            self.next()
        }

        fn unblock(&self) {
            self.block_mutation.store(false, Ordering::Release);
            self.release.notify_waiters();
        }
    }

    #[derive(Clone, Copy)]
    struct FakeJob {
        authority: u64,
    }

    impl Debug for FakeJob {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("FakeJob(<redacted>)")
        }
    }

    impl RuntimeIngressAcknowledgementLaneJobV2<Arc<FakePort>> for FakeJob {
        type Output = u64;
        type CompletionError = ();

        async fn execute(
            self,
            port: &Arc<FakePort>,
            context: RuntimeIngressAcknowledgementExecutionContextV2,
        ) -> RuntimeIngressAcknowledgementExecutionResultV2<Self, Self::Output, ()> {
            let mut replayed = false;
            let mut uncertainty_seen = false;
            loop {
                if Instant::now() >= context.effective_deadline() {
                    return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                        job: self,
                        failure: context.cutoff_failure(),
                    };
                }
                if context.shutdown_requested() {
                    return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                        job: self,
                        failure: RuntimeIngressAcknowledgementFailureV2::Shutdown,
                    };
                }
                let mutation = port.mutate().await;
                match mutation {
                    FakeStep::Applied => {
                        return RuntimeIngressAcknowledgementExecutionResultV2::Accepted(
                            self.authority,
                        );
                    }
                    FakeStep::DefinitelyNotApplied => {
                        if replayed {
                            return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                                job: self,
                                failure:
                                    RuntimeIngressAcknowledgementFailureV2::ReplayBudgetExhausted,
                            };
                        }
                        if context.shutdown_requested() {
                            return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                                job: self,
                                failure: RuntimeIngressAcknowledgementFailureV2::Shutdown,
                            };
                        }
                        replayed = true;
                    }
                    FakeStep::OutcomeUnknown => {
                        if uncertainty_seen {
                            return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                                job: self,
                                failure: RuntimeIngressAcknowledgementFailureV2::SecondUncertainty,
                            };
                        }
                        uncertainty_seen = true;
                        loop {
                            match port.observe().await {
                                FakeStep::ObservationRetryable => {
                                    let wake = Instant::now()
                                        .checked_add(context.retry_delay)
                                        .unwrap_or_else(|| context.effective_deadline());
                                    if wake >= context.effective_deadline() {
                                        return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                                            job: self,
                                            failure: context.cutoff_failure(),
                                        };
                                    }
                                    sleep_until(TokioInstant::from_std(wake)).await;
                                }
                                FakeStep::ObservationApplied => {
                                    return RuntimeIngressAcknowledgementExecutionResultV2::Accepted(
                                        self.authority,
                                    );
                                }
                                FakeStep::ObservationMissing => {
                                    if replayed {
                                        return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                                            job: self,
                                            failure:
                                                RuntimeIngressAcknowledgementFailureV2::ReplayBudgetExhausted,
                                        };
                                    }
                                    replayed = true;
                                    break;
                                }
                                _ => {
                                    return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                                        job: self,
                                        failure:
                                            RuntimeIngressAcknowledgementFailureV2::ResolutionProtocolViolation,
                                    };
                                }
                            }
                        }
                    }
                    _ => {
                        return RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                            job: self,
                            failure:
                                RuntimeIngressAcknowledgementFailureV2::ResolutionProtocolViolation,
                        };
                    }
                }
            }
        }
    }

    fn config() -> RuntimeIngressAcknowledgementSupervisorConfigV2 {
        RuntimeIngressAcknowledgementSupervisorConfigV2::new(Duration::from_millis(1)).unwrap()
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }

    async fn wait_started(port: &FakePort) {
        timeout(Duration::from_secs(1), async {
            while !port.started.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn cancelled_waiter_does_not_cancel_the_owned_operation_or_completion() {
        let port = Arc::new(FakePort::new([FakeStep::Applied]));
        let mut supervisor =
            RuntimeIngressAcknowledgementSupervisorV2::start(Arc::clone(&port), config());
        let waiter = supervisor
            .try_submit(FakeJob { authority: 41 }, deadline())
            .unwrap();
        let job_id = waiter.job_id();
        assert_eq!(job_id.sequence(), NonZeroU64::MIN);
        assert_eq!(supervisor.snapshot().active_job_id(), Some(job_id));
        drop(waiter);

        let completion = timeout(Duration::from_secs(1), supervisor.recv_completion())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completion.job_id(), job_id);
        assert!(matches!(
            completion.result(),
            RuntimeIngressAcknowledgementExecutionResultV2::Accepted(41)
        ));
        assert_eq!(port.mutations.load(Ordering::Acquire), 1);
        assert_eq!(
            supervisor.snapshot().last_completion(),
            Some(RuntimeIngressAcknowledgementCompletionClassV2::Accepted)
        );
        assert_eq!(supervisor.snapshot().completed_jobs(), 1);
        assert!(matches!(
            completion.into_result(),
            RuntimeIngressAcknowledgementExecutionResultV2::Accepted(41)
        ));
        assert_eq!(
            supervisor.snapshot().phase(),
            RuntimeIngressAcknowledgementSupervisorPhaseV2::Accepting
        );
        let report = supervisor.shutdown_until(deadline()).await;
        assert_eq!(
            report.exit(),
            RuntimeIngressAcknowledgementSupervisorExitV2::Commanded
        );
    }

    #[tokio::test]
    async fn unknown_observation_retries_only_retryable_failures_until_exact_acceptance() {
        let port = Arc::new(FakePort::new([
            FakeStep::OutcomeUnknown,
            FakeStep::ObservationRetryable,
            FakeStep::ObservationRetryable,
            FakeStep::ObservationApplied,
        ]));
        let mut supervisor =
            RuntimeIngressAcknowledgementSupervisorV2::start(Arc::clone(&port), config());
        let waiter = supervisor
            .try_submit(FakeJob { authority: 42 }, deadline())
            .unwrap();

        assert_eq!(
            waiter.wait().await,
            Ok(RuntimeIngressAcknowledgementCompletionClassV2::Accepted)
        );
        let completion = supervisor.recv_completion().await.unwrap();
        assert!(matches!(
            completion.result(),
            RuntimeIngressAcknowledgementExecutionResultV2::Accepted(42)
        ));
        assert_eq!(port.mutations.load(Ordering::Acquire), 1);
        assert_eq!(port.observations.load(Ordering::Acquire), 3);
        drop(completion);
        let _ = supervisor.shutdown_until(deadline()).await;
    }

    #[tokio::test]
    async fn second_uncertainty_fails_closed_without_a_third_mutation() {
        let port = Arc::new(FakePort::new([
            FakeStep::OutcomeUnknown,
            FakeStep::ObservationMissing,
            FakeStep::OutcomeUnknown,
            FakeStep::Applied,
        ]));
        let mut supervisor =
            RuntimeIngressAcknowledgementSupervisorV2::start(Arc::clone(&port), config());
        let waiter = supervisor
            .try_submit(FakeJob { authority: 43 }, deadline())
            .unwrap();

        assert_eq!(
            waiter.wait().await,
            Ok(
                RuntimeIngressAcknowledgementCompletionClassV2::FailedClosed(
                    RuntimeIngressAcknowledgementFailureV2::SecondUncertainty,
                ),
            )
        );
        let completion = supervisor.recv_completion().await.unwrap();
        assert!(matches!(
            completion.result(),
            RuntimeIngressAcknowledgementExecutionResultV2::FailedClosed {
                job: FakeJob { authority: 43 },
                failure: RuntimeIngressAcknowledgementFailureV2::SecondUncertainty,
            }
        ));
        assert_eq!(port.mutations.load(Ordering::Acquire), 2);
        assert_eq!(port.observations.load(Ordering::Acquire), 1);
        drop(completion);
        let _ = supervisor.shutdown_until(deadline()).await;
    }

    #[tokio::test]
    async fn definitely_not_applied_replays_the_same_owned_authority_once() {
        let port = Arc::new(FakePort::new([
            FakeStep::DefinitelyNotApplied,
            FakeStep::Applied,
        ]));
        let mut supervisor =
            RuntimeIngressAcknowledgementSupervisorV2::start(Arc::clone(&port), config());
        let waiter = supervisor
            .try_submit(FakeJob { authority: 47 }, deadline())
            .unwrap();

        assert_eq!(
            waiter.wait().await,
            Ok(RuntimeIngressAcknowledgementCompletionClassV2::Accepted)
        );
        let completion = supervisor.recv_completion().await.unwrap();
        assert!(matches!(
            completion.result(),
            RuntimeIngressAcknowledgementExecutionResultV2::Accepted(47)
        ));
        assert_eq!(port.mutations.load(Ordering::Acquire), 2);
        assert_eq!(port.observations.load(Ordering::Acquire), 0);
        drop(completion);
        let _ = supervisor.shutdown_until(deadline()).await;
    }

    #[tokio::test]
    async fn occupied_lane_rejects_saturation_and_returns_the_second_authority() {
        let port = Arc::new(FakePort::blocking([FakeStep::Applied]));
        let mut supervisor =
            RuntimeIngressAcknowledgementSupervisorV2::start(Arc::clone(&port), config());
        let first = supervisor
            .try_submit(FakeJob { authority: 44 }, deadline())
            .unwrap();
        wait_started(&port).await;

        let rejected = supervisor
            .try_submit(FakeJob { authority: 45 }, deadline())
            .unwrap_err();
        assert_eq!(
            rejected.reason(),
            RuntimeIngressAcknowledgementRegistrationRejectionReasonV2::Busy
        );
        assert_eq!(rejected.into_job().authority, 45);
        port.unblock();
        assert_eq!(
            first.wait().await,
            Ok(RuntimeIngressAcknowledgementCompletionClassV2::Accepted)
        );
        let completion = supervisor.recv_completion().await.unwrap();
        drop(completion);
        let _ = supervisor.shutdown_until(deadline()).await;
    }

    #[tokio::test]
    async fn reserved_shutdown_waits_for_a_dispatched_mutation_and_preserves_completion() {
        let port = Arc::new(FakePort::blocking([FakeStep::Applied]));
        let supervisor =
            RuntimeIngressAcknowledgementSupervisorV2::start(Arc::clone(&port), config());
        let waiter = supervisor
            .try_submit(FakeJob { authority: 46 }, deadline())
            .unwrap();
        wait_started(&port).await;
        drop(waiter);

        let shutdown = tokio::spawn(async move { supervisor.shutdown_until(deadline()).await });
        tokio::task::yield_now().await;
        port.unblock();
        let report = shutdown.await.unwrap();
        assert_eq!(
            report.exit(),
            RuntimeIngressAcknowledgementSupervisorExitV2::Commanded
        );
        assert_eq!(
            report.completion().unwrap().job_id().sequence(),
            NonZeroU64::MIN
        );
        let completion = report.into_completion().unwrap();
        assert!(matches!(
            completion.result(),
            RuntimeIngressAcknowledgementExecutionResultV2::Accepted(46)
        ));
        assert_eq!(port.mutations.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn shutdown_classifies_an_already_dispatched_unknown_before_terminal_exit() {
        let port = Arc::new(FakePort::blocking([
            FakeStep::OutcomeUnknown,
            FakeStep::ObservationApplied,
        ]));
        let supervisor =
            RuntimeIngressAcknowledgementSupervisorV2::start(Arc::clone(&port), config());
        let waiter = supervisor
            .try_submit(FakeJob { authority: 48 }, deadline())
            .unwrap();
        wait_started(&port).await;
        drop(waiter);

        let shutdown = tokio::spawn(async move { supervisor.shutdown_until(deadline()).await });
        tokio::task::yield_now().await;
        port.unblock();
        let report = shutdown.await.unwrap();
        assert_eq!(
            report.exit(),
            RuntimeIngressAcknowledgementSupervisorExitV2::Commanded
        );
        let completion = report.into_completion().unwrap();
        assert!(matches!(
            completion.result(),
            RuntimeIngressAcknowledgementExecutionResultV2::Accepted(48)
        ));
        assert_eq!(port.mutations.load(Ordering::Acquire), 1);
        assert_eq!(port.observations.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn errors_handles_and_terminal_observation_are_finite_and_redacted() {
        assert_eq!(
            RuntimeIngressAcknowledgementFailureV2::SecondUncertainty.code(),
            "runtime_ingress_acknowledgement_second_uncertainty"
        );
        assert_eq!(
            RuntimeIngressAcknowledgementSupervisorExitV2::Commanded.code(),
            "runtime_ingress_acknowledgement_supervisor_commanded"
        );
        assert_eq!(
            format!("{:?}", FakeJob { authority: 7 }),
            "FakeJob(<redacted>)"
        );
        assert_eq!(
            format!(
                "{:?}",
                RuntimeWorkerIngressAcknowledgementJobV2::new(FakeJob { authority: 8 })
            ),
            "RuntimeWorkerIngressAcknowledgementJobV2(<redacted>)"
        );
        assert_eq!(
            RuntimeWorkerIngressAcknowledgementJobV2::new(FakeJob { authority: 9 })
                .into_authority()
                .authority,
            9
        );
        let rejected =
            RuntimeIngressAcknowledgementExecutionResultV2::<FakeJob, u64, u8>::CompletionRejected {
                job: FakeJob { authority: 10 },
                error: 11,
            };
        assert!(matches!(
            rejected,
            RuntimeIngressAcknowledgementExecutionResultV2::CompletionRejected {
                job: FakeJob { authority: 10 },
                error: 11,
            }
        ));
        let port = Arc::new(FakePort::new([]));
        let mut supervisor =
            RuntimeIngressAcknowledgementSupervisorV2::<Arc<FakePort>, FakeJob>::start(
                port,
                config(),
            );
        supervisor
            .shared
            .publish_terminal(RuntimeIngressAcknowledgementSupervisorExitV2::ProtocolViolation);
        assert_eq!(
            supervisor.wait_terminal().await,
            RuntimeIngressAcknowledgementSupervisorExitV2::ProtocolViolation
        );
        assert_eq!(
            supervisor.snapshot().terminal(),
            Some(RuntimeIngressAcknowledgementSupervisorExitV2::ProtocolViolation)
        );
        let _ = supervisor.shutdown_until(deadline()).await;
    }
}
