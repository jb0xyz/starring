use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{Arc, Mutex, MutexGuard};

use automation_runtime_worker::RuntimeMutationFinalizerGenerationV1;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, watch, OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tokio::task::{JoinError, JoinHandle};

const RUNTIME_MUTATION_FINALIZER_MAX_CAPACITY: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeMutationFinalizerConfigErrorV1 {
    #[error("runtime mutation finalizer capacity is zero")]
    ZeroCapacity,
    #[error("runtime mutation finalizer capacity exceeds the process bound")]
    CapacityTooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeMutationFinalizerConfigV1 {
    capacity: NonZeroUsize,
}

impl RuntimeMutationFinalizerConfigV1 {
    pub fn new(capacity: usize) -> Result<Self, RuntimeMutationFinalizerConfigErrorV1> {
        let capacity = NonZeroUsize::new(capacity)
            .ok_or(RuntimeMutationFinalizerConfigErrorV1::ZeroCapacity)?;
        if capacity.get() > RUNTIME_MUTATION_FINALIZER_MAX_CAPACITY {
            return Err(RuntimeMutationFinalizerConfigErrorV1::CapacityTooLarge);
        }
        Ok(Self { capacity })
    }

    pub const fn capacity(self) -> NonZeroUsize {
        self.capacity
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeMutationFinalizerJobIdV1 {
    generation: RuntimeMutationFinalizerGenerationV1,
    sequence: NonZeroU64,
}

impl RuntimeMutationFinalizerJobIdV1 {
    pub const fn generation(self) -> RuntimeMutationFinalizerGenerationV1 {
        self.generation
    }

    pub const fn sequence(self) -> NonZeroU64 {
        self.sequence
    }
}

pub enum RuntimeMutationFinalizerJobV1<J> {
    StartupPendingDrain(J),
}

impl<J> RuntimeMutationFinalizerJobV1<J> {
    pub fn into_startup_pending_drain(self) -> J {
        match self {
            Self::StartupPendingDrain(job) => job,
        }
    }
}

impl<J> Debug for RuntimeMutationFinalizerJobV1<J> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerJobV1(<redacted>)")
    }
}

pub trait RuntimeMutationFinalizerPortV1: Send + Sync + 'static {
    type Job: Send + 'static;
    type Output: Send + 'static;
    type Error: Send + 'static;

    fn execute(
        &self,
        job: RuntimeMutationFinalizerJobV1<Self::Job>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send;
}

type RuntimeMutationFinalizerPortCompletionV1<P> = RuntimeMutationFinalizerCompletionV1<
    <P as RuntimeMutationFinalizerPortV1>::Job,
    <P as RuntimeMutationFinalizerPortV1>::Output,
    <P as RuntimeMutationFinalizerPortV1>::Error,
>;

type RuntimeMutationFinalizerCompletionReceiverV1<P> =
    mpsc::Receiver<RuntimeMutationFinalizerPortCompletionV1<P>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSupervisorExitV1 {
    Commanded,
    DependencyTerminal,
    DeadlineElapsed,
    ProtocolViolation,
    Panicked,
    Aborted,
}

impl RuntimeSupervisorExitV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Commanded => "runtime_mutation_finalizer_commanded",
            Self::DependencyTerminal => "runtime_mutation_finalizer_dependency_terminal",
            Self::DeadlineElapsed => "runtime_mutation_finalizer_deadline_elapsed",
            Self::ProtocolViolation => "runtime_mutation_finalizer_protocol_violation",
            Self::Panicked => "runtime_mutation_finalizer_panicked",
            Self::Aborted => "runtime_mutation_finalizer_aborted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeMutationFinalizerRegistrationRejectionReasonV1 {
    Busy,
    IntakeSealed,
    SupervisorTerminal(RuntimeSupervisorExitV1),
}

pub struct RuntimeMutationFinalizerRegistrationRejectedV1<J> {
    job: Box<J>,
    reason: RuntimeMutationFinalizerRegistrationRejectionReasonV1,
}

impl<J> RuntimeMutationFinalizerRegistrationRejectedV1<J> {
    pub fn reason(&self) -> RuntimeMutationFinalizerRegistrationRejectionReasonV1 {
        self.reason
    }

    pub fn into_job(self) -> J {
        *self.job
    }
}

impl<J> Debug for RuntimeMutationFinalizerRegistrationRejectedV1<J> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerRegistrationRejectedV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeMutationFinalizerWaitStatusV1 {
    Settled,
    Failed,
    FailedClosed(RuntimeSupervisorExitV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeMutationFinalizerWaitOutcomeV1 {
    job_id: RuntimeMutationFinalizerJobIdV1,
    status: RuntimeMutationFinalizerWaitStatusV1,
}

impl RuntimeMutationFinalizerWaitOutcomeV1 {
    pub const fn job_id(self) -> RuntimeMutationFinalizerJobIdV1 {
        self.job_id
    }

    pub const fn status(self) -> RuntimeMutationFinalizerWaitStatusV1 {
        self.status
    }
}

pub struct RuntimeMutationFinalizerWaiterV1 {
    job_id: RuntimeMutationFinalizerJobIdV1,
    receiver: oneshot::Receiver<RuntimeMutationFinalizerWaitStatusV1>,
    shared: Arc<RuntimeMutationFinalizerSharedV1>,
}

impl RuntimeMutationFinalizerWaiterV1 {
    pub const fn job_id(&self) -> RuntimeMutationFinalizerJobIdV1 {
        self.job_id
    }

    pub async fn wait(self) -> RuntimeMutationFinalizerWaitOutcomeV1 {
        let status = match self.receiver.await {
            Ok(status) => status,
            Err(_) => RuntimeMutationFinalizerWaitStatusV1::FailedClosed(
                self.shared
                    .terminal()
                    .unwrap_or(RuntimeSupervisorExitV1::Aborted),
            ),
        };
        RuntimeMutationFinalizerWaitOutcomeV1 {
            job_id: self.job_id,
            status,
        }
    }
}

impl Debug for RuntimeMutationFinalizerWaiterV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerWaiterV1(<redacted>)")
    }
}

pub enum RuntimeMutationFinalizerCompletionResultV1<J, O, E> {
    Settled(O),
    Failed(E),
    Undispatched {
        job: RuntimeMutationFinalizerJobV1<J>,
        exit: RuntimeSupervisorExitV1,
    },
    DispatchedTerminal(RuntimeSupervisorExitV1),
}

impl<J, O, E> Debug for RuntimeMutationFinalizerCompletionResultV1<J, O, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerCompletionResultV1(<redacted>)")
    }
}

pub struct RuntimeMutationFinalizerCompletionV1<J, O, E> {
    job_id: RuntimeMutationFinalizerJobIdV1,
    result: RuntimeMutationFinalizerCompletionResultV1<J, O, E>,
    slot: OwnedSemaphorePermit,
}

impl<J, O, E> RuntimeMutationFinalizerCompletionV1<J, O, E> {
    pub const fn job_id(&self) -> RuntimeMutationFinalizerJobIdV1 {
        self.job_id
    }

    pub fn result(&self) -> &RuntimeMutationFinalizerCompletionResultV1<J, O, E> {
        &self.result
    }

    pub fn into_result(self) -> RuntimeMutationFinalizerCompletionResultV1<J, O, E> {
        self.result
    }
}

impl<J, O, E> Debug for RuntimeMutationFinalizerCompletionV1<J, O, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let _ = &self.slot;
        formatter.write_str("RuntimeMutationFinalizerCompletionV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeMutationFinalizerHandoffStateV1 {
    finalizer_generation: RuntimeMutationFinalizerGenerationV1,
    startup_intake_sealed: bool,
    startup_jobs_settled: bool,
}

impl RuntimeMutationFinalizerHandoffStateV1 {
    pub const fn finalizer_generation(self) -> RuntimeMutationFinalizerGenerationV1 {
        self.finalizer_generation
    }

    pub const fn startup_intake_sealed(self) -> bool {
        self.startup_intake_sealed
    }

    pub const fn startup_jobs_settled(self) -> bool {
        self.startup_jobs_settled
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeMutationFinalizerSnapshotV1 {
    generation: RuntimeMutationFinalizerGenerationV1,
    intake_open: bool,
    queued_jobs: usize,
    in_flight_jobs: usize,
    unsettled_jobs: usize,
    settled_jobs: u64,
    failed_jobs: u64,
    failed_closed_jobs: u64,
    startup_jobs_settled: bool,
    terminal: Option<RuntimeSupervisorExitV1>,
}

impl RuntimeMutationFinalizerSnapshotV1 {
    pub const fn generation(self) -> RuntimeMutationFinalizerGenerationV1 {
        self.generation
    }

    pub const fn intake_open(self) -> bool {
        self.intake_open
    }

    pub const fn queued_jobs(self) -> usize {
        self.queued_jobs
    }

    pub const fn in_flight_jobs(self) -> usize {
        self.in_flight_jobs
    }

    pub const fn unsettled_jobs(self) -> usize {
        self.unsettled_jobs
    }

    pub const fn settled_jobs(self) -> u64 {
        self.settled_jobs
    }

    pub const fn failed_jobs(self) -> u64 {
        self.failed_jobs
    }

    pub const fn failed_closed_jobs(self) -> u64 {
        self.failed_closed_jobs
    }

    pub const fn startup_jobs_settled(self) -> bool {
        self.startup_jobs_settled
    }

    pub const fn terminal(self) -> Option<RuntimeSupervisorExitV1> {
        self.terminal
    }

    pub fn handoff_state(self) -> RuntimeMutationFinalizerHandoffStateV1 {
        RuntimeMutationFinalizerHandoffStateV1 {
            finalizer_generation: self.generation,
            startup_intake_sealed: !self.intake_open,
            startup_jobs_settled: !self.intake_open
                && self.startup_jobs_settled
                && self.terminal.is_none(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeMutationFinalizerSealOutcomeV1 {
    First(RuntimeMutationFinalizerSnapshotV1),
    AlreadySealed(RuntimeMutationFinalizerSnapshotV1),
    Terminal(RuntimeMutationFinalizerSnapshotV1),
}

impl RuntimeMutationFinalizerSealOutcomeV1 {
    pub const fn snapshot(self) -> RuntimeMutationFinalizerSnapshotV1 {
        match self {
            Self::First(snapshot) | Self::AlreadySealed(snapshot) | Self::Terminal(snapshot) => {
                snapshot
            }
        }
    }
}

struct RuntimeMutationFinalizerStateV1 {
    generation: RuntimeMutationFinalizerGenerationV1,
    accepting: bool,
    next_sequence: Option<NonZeroU64>,
    queued_jobs: usize,
    in_flight_jobs: usize,
    unsettled_jobs: usize,
    settled_jobs: u64,
    failed_jobs: u64,
    failed_closed_jobs: u64,
    startup_jobs_settled: bool,
    terminal: Option<RuntimeSupervisorExitV1>,
}

struct RuntimeMutationFinalizerSharedV1 {
    state: Mutex<RuntimeMutationFinalizerStateV1>,
    terminal_publisher: watch::Sender<Option<RuntimeSupervisorExitV1>>,
    startup_settlement_publisher: watch::Sender<bool>,
}

impl RuntimeMutationFinalizerSharedV1 {
    fn lock(&self) -> MutexGuard<'_, RuntimeMutationFinalizerStateV1> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn terminal(&self) -> Option<RuntimeSupervisorExitV1> {
        self.lock().terminal
    }

    fn closed_registration_reason(&self) -> RuntimeMutationFinalizerRegistrationRejectionReasonV1 {
        let state = self.lock();
        match (state.terminal, state.accepting) {
            (Some(exit), _) => {
                RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(exit)
            }
            (None, false) => RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed,
            (None, true) => {
                RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(
                    RuntimeSupervisorExitV1::ProtocolViolation,
                )
            }
        }
    }

    fn snapshot(&self) -> RuntimeMutationFinalizerSnapshotV1 {
        snapshot_from_state_v1(&self.lock())
    }

    fn publish_terminal(&self, exit: RuntimeSupervisorExitV1) -> RuntimeSupervisorExitV1 {
        let published = {
            let mut state = self.lock();
            state.accepting = false;
            *state.terminal.get_or_insert(exit)
        };
        self.terminal_publisher.send_replace(Some(published));
        published
    }

    fn publish_startup_settled(&self) {
        {
            let mut state = self.lock();
            state.startup_jobs_settled = true;
        }
        self.startup_settlement_publisher.send_replace(true);
    }
}

struct RuntimeMutationFinalizerEnvelopeV1<J> {
    job_id: RuntimeMutationFinalizerJobIdV1,
    job: RuntimeMutationFinalizerJobV1<J>,
    waiter: oneshot::Sender<RuntimeMutationFinalizerWaitStatusV1>,
    slot: OwnedSemaphorePermit,
}

enum RuntimeMutationFinalizerControlV1 {
    Seal,
    Shutdown,
    ProtocolViolation,
}

pub struct RuntimeMutationFinalizerIntakeV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    shared: Arc<RuntimeMutationFinalizerSharedV1>,
    jobs: mpsc::Sender<RuntimeMutationFinalizerEnvelopeV1<P::Job>>,
    controls: mpsc::Sender<RuntimeMutationFinalizerControlV1>,
    slots: Arc<Semaphore>,
}

impl<P> Clone for RuntimeMutationFinalizerIntakeV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            jobs: self.jobs.clone(),
            controls: self.controls.clone(),
            slots: self.slots.clone(),
        }
    }
}

impl<P> RuntimeMutationFinalizerIntakeV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    pub fn try_register(
        &self,
        job: RuntimeMutationFinalizerJobV1<P::Job>,
    ) -> Result<
        RuntimeMutationFinalizerWaiterV1,
        RuntimeMutationFinalizerRegistrationRejectedV1<RuntimeMutationFinalizerJobV1<P::Job>>,
    > {
        let slot = match self.slots.clone().try_acquire_owned() {
            Ok(slot) => slot,
            Err(TryAcquireError::NoPermits) => {
                return Err(registration_rejected_v1(
                    job,
                    RuntimeMutationFinalizerRegistrationRejectionReasonV1::Busy,
                ));
            }
            Err(TryAcquireError::Closed) => {
                return Err(registration_rejected_v1(
                    job,
                    self.shared.closed_registration_reason(),
                ));
            }
        };
        let permit = match self.jobs.try_reserve() {
            Ok(permit) => permit,
            Err(TrySendError::Full(_)) => {
                return Err(registration_rejected_v1(
                    job,
                    RuntimeMutationFinalizerRegistrationRejectionReasonV1::Busy,
                ));
            }
            Err(TrySendError::Closed(_)) => {
                return Err(registration_rejected_v1(
                    job,
                    self.shared.closed_registration_reason(),
                ));
            }
        };
        let (waiter, receiver) = oneshot::channel();
        let mut state = self.shared.lock();
        if let Some(exit) = state.terminal {
            return Err(registration_rejected_v1(
                job,
                RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(exit),
            ));
        }
        if !state.accepting {
            return Err(registration_rejected_v1(
                job,
                RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed,
            ));
        }
        let Some(sequence) = state.next_sequence else {
            state.accepting = false;
            state.terminal = Some(RuntimeSupervisorExitV1::ProtocolViolation);
            drop(state);
            self.shared
                .terminal_publisher
                .send_replace(Some(RuntimeSupervisorExitV1::ProtocolViolation));
            let _ = self
                .controls
                .try_send(RuntimeMutationFinalizerControlV1::ProtocolViolation);
            return Err(registration_rejected_v1(
                job,
                RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(
                    RuntimeSupervisorExitV1::ProtocolViolation,
                ),
            ));
        };
        state.next_sequence = sequence.get().checked_add(1).and_then(NonZeroU64::new);
        let job_id = RuntimeMutationFinalizerJobIdV1 {
            generation: state.generation,
            sequence,
        };
        state.queued_jobs = state.queued_jobs.saturating_add(1);
        state.unsettled_jobs = state.unsettled_jobs.saturating_add(1);
        permit.send(RuntimeMutationFinalizerEnvelopeV1 {
            job_id,
            job,
            waiter,
            slot,
        });
        drop(state);
        Ok(RuntimeMutationFinalizerWaiterV1 {
            job_id,
            receiver,
            shared: self.shared.clone(),
        })
    }

    pub fn snapshot(&self) -> RuntimeMutationFinalizerSnapshotV1 {
        self.shared.snapshot()
    }
}

impl<P> Debug for RuntimeMutationFinalizerIntakeV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerIntakeV1(<redacted>)")
    }
}

fn registration_rejected_v1<J>(
    job: J,
    reason: RuntimeMutationFinalizerRegistrationRejectionReasonV1,
) -> RuntimeMutationFinalizerRegistrationRejectedV1<J> {
    RuntimeMutationFinalizerRegistrationRejectedV1 {
        job: Box::new(job),
        reason,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeMutationFinalizerStartErrorV1 {
    #[error("runtime mutation finalizer asynchronous executor is unavailable")]
    AsyncRuntimeUnavailable,
}

pub struct RuntimeMutationFinalizerSupervisorV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    intake: RuntimeMutationFinalizerIntakeV1<P>,
    controls: mpsc::Sender<RuntimeMutationFinalizerControlV1>,
    completions: Option<RuntimeMutationFinalizerCompletionReceiverV1<P>>,
    terminal: watch::Receiver<Option<RuntimeSupervisorExitV1>>,
    startup_settlement: watch::Receiver<bool>,
    actor: Option<JoinHandle<RuntimeSupervisorExitV1>>,
}

impl<P> RuntimeMutationFinalizerSupervisorV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    pub fn start(
        config: RuntimeMutationFinalizerConfigV1,
        generation: RuntimeMutationFinalizerGenerationV1,
        port: P,
    ) -> Result<Self, RuntimeMutationFinalizerStartErrorV1> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| RuntimeMutationFinalizerStartErrorV1::AsyncRuntimeUnavailable)?;
        let capacity = config.capacity().get();
        let (jobs, job_receiver) = mpsc::channel(capacity);
        let (controls, control_receiver) = mpsc::channel(1);
        let (completions, completion_receiver) = mpsc::channel(capacity);
        let (terminal_publisher, terminal) = watch::channel(None);
        let (startup_settlement_publisher, startup_settlement) = watch::channel(false);
        let shared = Arc::new(RuntimeMutationFinalizerSharedV1 {
            state: Mutex::new(RuntimeMutationFinalizerStateV1 {
                generation,
                accepting: true,
                next_sequence: Some(NonZeroU64::MIN),
                queued_jobs: 0,
                in_flight_jobs: 0,
                unsettled_jobs: 0,
                settled_jobs: 0,
                failed_jobs: 0,
                failed_closed_jobs: 0,
                startup_jobs_settled: false,
                terminal: None,
            }),
            terminal_publisher,
            startup_settlement_publisher,
        });
        let slots = Arc::new(Semaphore::new(capacity));
        let intake = RuntimeMutationFinalizerIntakeV1 {
            shared: shared.clone(),
            jobs,
            controls: controls.clone(),
            slots,
        };
        let actor = runtime.spawn(run_runtime_mutation_finalizer_actor_v1(
            Arc::new(port),
            shared,
            job_receiver,
            control_receiver,
            completions,
        ));
        Ok(Self {
            intake,
            controls,
            completions: Some(completion_receiver),
            terminal,
            startup_settlement,
            actor: Some(actor),
        })
    }

    pub fn intake(&self) -> &RuntimeMutationFinalizerIntakeV1<P> {
        &self.intake
    }

    pub fn snapshot(&self) -> RuntimeMutationFinalizerSnapshotV1 {
        self.intake.snapshot()
    }

    pub fn seal_intake(&self) -> RuntimeMutationFinalizerSealOutcomeV1 {
        let outcome = {
            let mut state = self.intake.shared.lock();
            let terminal = state.terminal.is_some();
            let was_open = state.accepting;
            state.accepting = false;
            let snapshot = snapshot_from_state_v1(&state);
            if terminal {
                RuntimeMutationFinalizerSealOutcomeV1::Terminal(snapshot)
            } else if was_open {
                RuntimeMutationFinalizerSealOutcomeV1::First(snapshot)
            } else {
                RuntimeMutationFinalizerSealOutcomeV1::AlreadySealed(snapshot)
            }
        };
        if matches!(outcome, RuntimeMutationFinalizerSealOutcomeV1::First(_)) {
            let _ = self
                .controls
                .try_send(RuntimeMutationFinalizerControlV1::Seal);
        }
        outcome
    }

    pub async fn next_completion(&mut self) -> Option<RuntimeMutationFinalizerPortCompletionV1<P>> {
        match &mut self.completions {
            Some(completions) => completions.recv().await,
            None => None,
        }
    }

    pub fn terminal_observation(&self) -> Option<RuntimeSupervisorExitV1> {
        self.intake.shared.terminal()
    }

    pub async fn wait_terminal(&mut self) -> RuntimeSupervisorExitV1 {
        loop {
            if let Some(exit) = *self.terminal.borrow_and_update() {
                return exit;
            }
            if self.terminal.changed().await.is_err() {
                return self
                    .intake
                    .shared
                    .terminal()
                    .unwrap_or(RuntimeSupervisorExitV1::Aborted);
            }
        }
    }

    pub async fn wait_startup_jobs_settled(&mut self) -> bool {
        loop {
            let snapshot = self.snapshot();
            if snapshot.startup_jobs_settled() {
                return snapshot.terminal().is_none();
            }
            if snapshot.terminal().is_some() {
                return false;
            }
            tokio::select! {
                changed = self.startup_settlement.changed() => {
                    if changed.is_err() {
                        return false;
                    }
                    self.startup_settlement.borrow_and_update();
                }
                changed = self.terminal.changed() => {
                    if changed.is_err() {
                        return false;
                    }
                    self.terminal.borrow_and_update();
                }
            }
        }
    }

    pub async fn join(
        mut self,
    ) -> RuntimeMutationFinalizerJoinReportV1<P::Job, P::Output, P::Error> {
        self.seal_intake();
        let _ = self
            .controls
            .send(RuntimeMutationFinalizerControlV1::Shutdown)
            .await;
        let exit = match self.actor.take() {
            Some(actor) => classify_actor_join_v1(actor.await, &self.intake.shared),
            None => self
                .intake
                .shared
                .publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation),
        };
        let mut completions = Vec::with_capacity(self.snapshot().unsettled_jobs());
        if let Some(mut receiver) = self.completions.take() {
            while let Some(completion) = receiver.recv().await {
                completions.push(completion);
            }
        }
        RuntimeMutationFinalizerJoinReportV1 {
            exit,
            snapshot: self.snapshot(),
            completions,
        }
    }
}

impl<P> Debug for RuntimeMutationFinalizerSupervisorV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerSupervisorV1(<redacted>)")
    }
}

impl<P> Drop for RuntimeMutationFinalizerSupervisorV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    fn drop(&mut self) {
        if let Some(actor) = self.actor.take() {
            self.intake
                .shared
                .publish_terminal(RuntimeSupervisorExitV1::Aborted);
            self.intake.slots.close();
            actor.abort();
        }
    }
}

pub struct RuntimeMutationFinalizerJoinReportV1<J, O, E> {
    exit: RuntimeSupervisorExitV1,
    snapshot: RuntimeMutationFinalizerSnapshotV1,
    completions: Vec<RuntimeMutationFinalizerCompletionV1<J, O, E>>,
}

impl<J, O, E> RuntimeMutationFinalizerJoinReportV1<J, O, E> {
    pub const fn exit(&self) -> RuntimeSupervisorExitV1 {
        self.exit
    }

    pub const fn snapshot(&self) -> RuntimeMutationFinalizerSnapshotV1 {
        self.snapshot
    }

    pub fn completions(&self) -> &[RuntimeMutationFinalizerCompletionV1<J, O, E>] {
        &self.completions
    }

    pub fn into_completions(self) -> Vec<RuntimeMutationFinalizerCompletionV1<J, O, E>> {
        self.completions
    }
}

impl<J, O, E> Debug for RuntimeMutationFinalizerJoinReportV1<J, O, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerJoinReportV1(<redacted>)")
    }
}

fn classify_actor_join_v1(
    result: Result<RuntimeSupervisorExitV1, JoinError>,
    shared: &RuntimeMutationFinalizerSharedV1,
) -> RuntimeSupervisorExitV1 {
    match result {
        Ok(exit) => shared.publish_terminal(exit),
        Err(error) if error.is_cancelled() => {
            shared.publish_terminal(RuntimeSupervisorExitV1::Aborted)
        }
        Err(_) => shared.publish_terminal(RuntimeSupervisorExitV1::Panicked),
    }
}

async fn run_runtime_mutation_finalizer_actor_v1<P>(
    port: Arc<P>,
    shared: Arc<RuntimeMutationFinalizerSharedV1>,
    mut jobs: mpsc::Receiver<RuntimeMutationFinalizerEnvelopeV1<P::Job>>,
    mut controls: mpsc::Receiver<RuntimeMutationFinalizerControlV1>,
    completions: mpsc::Sender<RuntimeMutationFinalizerCompletionV1<P::Job, P::Output, P::Error>>,
) -> RuntimeSupervisorExitV1
where
    P: RuntimeMutationFinalizerPortV1,
{
    let mut sealed = false;
    let mut shutdown = false;
    loop {
        let envelope = if sealed {
            jobs.recv().await
        } else {
            tokio::select! {
                biased;
                control = controls.recv() => {
                    match control {
                        Some(RuntimeMutationFinalizerControlV1::Seal) => {
                            sealed = true;
                            jobs.close();
                            continue;
                        }
                        Some(RuntimeMutationFinalizerControlV1::Shutdown) => {
                            sealed = true;
                            shutdown = true;
                            jobs.close();
                            continue;
                        }
                        Some(RuntimeMutationFinalizerControlV1::ProtocolViolation) | None => {
                            let exit = shared.publish_terminal(
                                RuntimeSupervisorExitV1::ProtocolViolation,
                            );
                            jobs.close();
                            fail_queued_jobs_v1(
                                &shared,
                                &mut jobs,
                                &completions,
                                exit,
                            ).await;
                            return exit;
                        }
                    }
                }
                job = jobs.recv() => job,
            }
        };
        let Some(envelope) = envelope else {
            if !sealed {
                return shared.publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation);
            }
            shared.publish_startup_settled();
            if shutdown {
                return shared.publish_terminal(RuntimeSupervisorExitV1::Commanded);
            }
            match controls.recv().await {
                Some(RuntimeMutationFinalizerControlV1::Seal) => continue,
                Some(RuntimeMutationFinalizerControlV1::Shutdown) => {
                    return shared.publish_terminal(RuntimeSupervisorExitV1::Commanded);
                }
                Some(RuntimeMutationFinalizerControlV1::ProtocolViolation) | None => {
                    return shared.publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation);
                }
            }
        };
        let exit = execute_registered_job_v1(&port, &shared, &completions, envelope).await;
        if let Some(exit) = exit {
            let exit = shared.publish_terminal(exit);
            jobs.close();
            fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
            return exit;
        }
    }
}

async fn execute_registered_job_v1<P>(
    port: &Arc<P>,
    shared: &RuntimeMutationFinalizerSharedV1,
    completions: &mpsc::Sender<RuntimeMutationFinalizerCompletionV1<P::Job, P::Output, P::Error>>,
    envelope: RuntimeMutationFinalizerEnvelopeV1<P::Job>,
) -> Option<RuntimeSupervisorExitV1>
where
    P: RuntimeMutationFinalizerPortV1,
{
    {
        let mut state = shared.lock();
        state.queued_jobs = state.queued_jobs.saturating_sub(1);
        state.in_flight_jobs = 1;
    }
    let RuntimeMutationFinalizerEnvelopeV1 {
        job_id,
        job,
        waiter,
        slot,
    } = envelope;
    let port = port.clone();
    let task =
        RuntimeMutationFinalizerInFlightTaskV1::spawn(async move { port.execute(job).await });
    let result = task.join().await;
    let (completion, wait_status, exit, settled, failed, failed_closed) = match result {
        Ok(Ok(output)) => (
            RuntimeMutationFinalizerCompletionResultV1::Settled(output),
            RuntimeMutationFinalizerWaitStatusV1::Settled,
            None,
            1,
            0,
            0,
        ),
        Ok(Err(error)) => (
            RuntimeMutationFinalizerCompletionResultV1::Failed(error),
            RuntimeMutationFinalizerWaitStatusV1::Failed,
            None,
            0,
            1,
            0,
        ),
        Err(error) => {
            let exit = if error.is_cancelled() {
                RuntimeSupervisorExitV1::Aborted
            } else {
                RuntimeSupervisorExitV1::Panicked
            };
            (
                RuntimeMutationFinalizerCompletionResultV1::DispatchedTerminal(exit),
                RuntimeMutationFinalizerWaitStatusV1::FailedClosed(exit),
                Some(exit),
                0,
                0,
                1,
            )
        }
    };
    if completions
        .send(RuntimeMutationFinalizerCompletionV1 {
            job_id,
            result: completion,
            slot,
        })
        .await
        .is_err()
    {
        let exit = shared.publish_terminal(RuntimeSupervisorExitV1::Aborted);
        let _ = waiter.send(RuntimeMutationFinalizerWaitStatusV1::FailedClosed(exit));
        return Some(exit);
    }
    {
        let mut state = shared.lock();
        state.in_flight_jobs = 0;
        state.unsettled_jobs = state.unsettled_jobs.saturating_sub(1);
        state.settled_jobs = state.settled_jobs.saturating_add(settled);
        state.failed_jobs = state.failed_jobs.saturating_add(failed);
        state.failed_closed_jobs = state.failed_closed_jobs.saturating_add(failed_closed);
    }
    let _ = waiter.send(wait_status);
    exit
}

struct RuntimeMutationFinalizerInFlightTaskV1<T> {
    task: Option<JoinHandle<T>>,
}

impl<T> RuntimeMutationFinalizerInFlightTaskV1<T>
where
    T: Send + 'static,
{
    fn spawn(future: impl Future<Output = T> + Send + 'static) -> Self {
        Self {
            task: Some(tokio::spawn(future)),
        }
    }

    async fn join(mut self) -> Result<T, JoinError> {
        let result = match self.task.as_mut() {
            Some(task) => task.await,
            None => unreachable!(),
        };
        self.task.take();
        result
    }
}

impl<T> Drop for RuntimeMutationFinalizerInFlightTaskV1<T> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn fail_queued_jobs_v1<J, O, E>(
    shared: &RuntimeMutationFinalizerSharedV1,
    jobs: &mut mpsc::Receiver<RuntimeMutationFinalizerEnvelopeV1<J>>,
    completions: &mpsc::Sender<RuntimeMutationFinalizerCompletionV1<J, O, E>>,
    exit: RuntimeSupervisorExitV1,
) where
    J: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
{
    while let Some(envelope) = jobs.recv().await {
        let RuntimeMutationFinalizerEnvelopeV1 {
            job_id,
            job,
            waiter,
            slot,
        } = envelope;
        if completions
            .send(RuntimeMutationFinalizerCompletionV1 {
                job_id,
                result: RuntimeMutationFinalizerCompletionResultV1::Undispatched { job, exit },
                slot,
            })
            .await
            .is_err()
        {
            shared.publish_terminal(RuntimeSupervisorExitV1::Aborted);
            let _ = waiter.send(RuntimeMutationFinalizerWaitStatusV1::FailedClosed(
                RuntimeSupervisorExitV1::Aborted,
            ));
            return;
        }
        {
            let mut state = shared.lock();
            state.queued_jobs = state.queued_jobs.saturating_sub(1);
            state.unsettled_jobs = state.unsettled_jobs.saturating_sub(1);
            state.failed_closed_jobs = state.failed_closed_jobs.saturating_add(1);
        }
        let _ = waiter.send(RuntimeMutationFinalizerWaitStatusV1::FailedClosed(exit));
    }
}

fn snapshot_from_state_v1(
    state: &RuntimeMutationFinalizerStateV1,
) -> RuntimeMutationFinalizerSnapshotV1 {
    RuntimeMutationFinalizerSnapshotV1 {
        generation: state.generation,
        intake_open: state.accepting,
        queued_jobs: state.queued_jobs,
        in_flight_jobs: state.in_flight_jobs,
        unsettled_jobs: state.unsettled_jobs,
        settled_jobs: state.settled_jobs,
        failed_jobs: state.failed_jobs,
        failed_closed_jobs: state.failed_closed_jobs,
        startup_jobs_settled: state.startup_jobs_settled,
        terminal: state.terminal,
    }
}
