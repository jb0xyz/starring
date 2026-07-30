use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use automation_runtime_worker::RuntimeMutationFinalizerGenerationV1;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, watch, OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tokio::task::{JoinError, JoinHandle};
use tokio::time::{timeout_at, Instant as TokioInstant};

const RUNTIME_MUTATION_FINALIZER_MAX_CAPACITY: usize = 1_024;
const RUNTIME_MUTATION_FINALIZER_ACTOR_ABORT_RESERVE: Duration = Duration::from_millis(25);
const RUNTIME_MUTATION_FINALIZER_IN_FLIGHT_ABORT_RESERVE: Duration = Duration::from_millis(50);
static NEXT_RUNTIME_MUTATION_FINALIZER_SUPERVISOR_ID: AtomicU64 = AtomicU64::new(1);
#[cfg_attr(not(test), allow(dead_code))]
static NEXT_RUNTIME_MUTATION_FINALIZER_ACTIVATION_NONCE: AtomicU64 = AtomicU64::new(1);

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
    supervisor_id: NonZeroU64,
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
    ProcessMutation(J),
}

impl<J> RuntimeMutationFinalizerJobV1<J> {
    pub fn into_startup_pending_drain(self) -> J {
        self.into_inner()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn into_process_mutation(self) -> J {
        self.into_inner()
    }

    const fn is_startup_pending_drain(&self) -> bool {
        matches!(self, Self::StartupPendingDrain(_))
    }

    const fn is_process_mutation(&self) -> bool {
        matches!(self, Self::ProcessMutation(_))
    }

    pub(crate) fn into_inner(self) -> J {
        match self {
            Self::StartupPendingDrain(job) | Self::ProcessMutation(job) => job,
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
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeMutationFinalizerPhaseV1 {
    StartupAccepting,
    StartupSealing,
    StartupSettled,
    ProcessActivationReserved,
    ProcessAccepting,
    ShutdownSealed,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeMutationFinalizerSnapshotV1 {
    generation: RuntimeMutationFinalizerGenerationV1,
    phase: RuntimeMutationFinalizerPhaseV1,
    intake_open: bool,
    queued_jobs: usize,
    in_flight_jobs: usize,
    unsettled_jobs: usize,
    settled_jobs: u64,
    failed_jobs: u64,
    failed_closed_jobs: u64,
    next_job_sequence: Option<NonZeroU64>,
    startup_intake_sealed: bool,
    startup_jobs_settled: bool,
    shutdown_sealed: bool,
    terminal: Option<RuntimeSupervisorExitV1>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeMutationFinalizerSnapshotV1 {
    pub const fn generation(self) -> RuntimeMutationFinalizerGenerationV1 {
        self.generation
    }

    pub(crate) const fn phase(self) -> RuntimeMutationFinalizerPhaseV1 {
        self.phase
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

    pub(crate) const fn next_job_sequence(self) -> Option<NonZeroU64> {
        self.next_job_sequence
    }

    pub const fn startup_intake_sealed(self) -> bool {
        self.startup_intake_sealed
    }

    pub const fn startup_jobs_settled(self) -> bool {
        self.startup_jobs_settled
    }

    pub(crate) const fn process_activation_reserved(self) -> bool {
        matches!(
            self.phase,
            RuntimeMutationFinalizerPhaseV1::ProcessActivationReserved
        )
    }

    pub(crate) const fn process_accepting(self) -> bool {
        matches!(
            self.phase,
            RuntimeMutationFinalizerPhaseV1::ProcessAccepting
        )
    }

    pub(crate) const fn shutdown_sealed(self) -> bool {
        self.shutdown_sealed
    }

    pub const fn terminal(self) -> Option<RuntimeSupervisorExitV1> {
        self.terminal
    }

    pub fn handoff_state(self) -> RuntimeMutationFinalizerHandoffStateV1 {
        RuntimeMutationFinalizerHandoffStateV1 {
            finalizer_generation: self.generation,
            startup_intake_sealed: self.startup_intake_sealed,
            startup_jobs_settled: self.startup_intake_sealed
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
    supervisor_id: NonZeroU64,
    generation: RuntimeMutationFinalizerGenerationV1,
    phase: RuntimeMutationFinalizerPhaseV1,
    next_sequence: Option<NonZeroU64>,
    queued_jobs: usize,
    in_flight_jobs: usize,
    unsettled_jobs: usize,
    settled_jobs: u64,
    failed_jobs: u64,
    failed_closed_jobs: u64,
    activation_nonce: Option<NonZeroU64>,
    startup_intake_sealed: bool,
    startup_jobs_settled: bool,
    shutdown_sealed: bool,
    terminal: Option<RuntimeSupervisorExitV1>,
}

struct RuntimeMutationFinalizerSharedV1 {
    state: Mutex<RuntimeMutationFinalizerStateV1>,
    terminal_publisher: watch::Sender<Option<RuntimeSupervisorExitV1>>,
    startup_settlement_publisher: watch::Sender<bool>,
}

struct RuntimeMutationFinalizerInFlightAbortV1 {
    abort: Mutex<Option<tokio::task::AbortHandle>>,
    forced_exit: Mutex<Option<RuntimeSupervisorExitV1>>,
    stopped: watch::Sender<bool>,
}

impl RuntimeMutationFinalizerInFlightAbortV1 {
    fn new() -> Self {
        let (stopped, _) = watch::channel(true);
        Self {
            abort: Mutex::new(None),
            forced_exit: Mutex::new(None),
            stopped,
        }
    }

    fn publish(&self, abort: tokio::task::AbortHandle) {
        self.forced_exit
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        *self
            .abort
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(abort);
        self.stopped.send_replace(false);
    }

    fn clear(&self) {
        self.abort
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        self.stopped.send_replace(true);
    }

    fn abort(&self) {
        if let Some(abort) = self
            .abort
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            abort.abort();
        }
    }

    fn abort_with(&self, exit: RuntimeSupervisorExitV1) {
        *self
            .forced_exit
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(exit);
        self.abort();
    }

    fn take_forced_exit(&self) -> Option<RuntimeSupervisorExitV1> {
        self.forced_exit
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    async fn wait_stopped(&self) {
        let mut stopped = self.stopped.subscribe();
        loop {
            if *stopped.borrow_and_update() {
                return;
            }
            if stopped.changed().await.is_err() {
                return;
            }
        }
    }
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
        match (state.terminal, state.phase) {
            (Some(exit), _) => {
                RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(exit)
            }
            (None, RuntimeMutationFinalizerPhaseV1::StartupAccepting) => {
                RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(
                    RuntimeSupervisorExitV1::ProtocolViolation,
                )
            }
            (None, _) => RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed,
        }
    }

    fn snapshot(&self) -> RuntimeMutationFinalizerSnapshotV1 {
        snapshot_from_state_v1(&self.lock())
    }

    fn publish_terminal(&self, exit: RuntimeSupervisorExitV1) -> RuntimeSupervisorExitV1 {
        let published = {
            let mut state = self.lock();
            let published = *state.terminal.get_or_insert(exit);
            state.startup_intake_sealed = true;
            state.shutdown_sealed = true;
            if !matches!(state.phase, RuntimeMutationFinalizerPhaseV1::Terminal) {
                state.phase = RuntimeMutationFinalizerPhaseV1::ShutdownSealed;
                state.phase = RuntimeMutationFinalizerPhaseV1::Terminal;
            }
            published
        };
        self.terminal_publisher.send_replace(Some(published));
        published
    }

    fn publish_startup_settled_if_ready(&self) -> bool {
        let settled = {
            let mut state = self.lock();
            if matches!(state.phase, RuntimeMutationFinalizerPhaseV1::StartupSealing)
                && state.queued_jobs == 0
                && state.in_flight_jobs == 0
                && state.terminal.is_none()
            {
                state.phase = RuntimeMutationFinalizerPhaseV1::StartupSettled;
                state.startup_jobs_settled = true;
                true
            } else {
                false
            }
        };
        if settled {
            self.startup_settlement_publisher.send_replace(true);
        }
        settled
    }
}

struct RuntimeMutationFinalizerEnvelopeV1<J> {
    job_id: RuntimeMutationFinalizerJobIdV1,
    job: RuntimeMutationFinalizerJobV1<J>,
    waiter: oneshot::Sender<RuntimeMutationFinalizerWaitStatusV1>,
    slot: OwnedSemaphorePermit,
}

#[cfg_attr(not(test), allow(dead_code))]
enum RuntimeMutationFinalizerControlV1 {
    StartupSeal,
    ShutdownSeal,
    ActivateProcess {
        supervisor_id: NonZeroU64,
        generation: RuntimeMutationFinalizerGenerationV1,
        nonce: NonZeroU64,
        acknowledgement: oneshot::Sender<Result<(), RuntimeSupervisorExitV1>>,
    },
    Shutdown,
    ShutdownUntil(Instant),
    ProtocolViolation,
}

#[derive(Clone)]
pub(crate) struct RuntimeMutationFinalizerSealHandleV1 {
    shared: Arc<RuntimeMutationFinalizerSharedV1>,
    controls: mpsc::Sender<RuntimeMutationFinalizerControlV1>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeMutationFinalizerSealHandleV1 {
    pub(crate) fn seal_intake(&self) -> RuntimeMutationFinalizerSealOutcomeV1 {
        seal_runtime_mutation_finalizer_shutdown_v1(&self.shared, &self.controls)
    }

    pub(crate) fn snapshot(&self) -> RuntimeMutationFinalizerSnapshotV1 {
        self.shared.snapshot()
    }
}

impl Debug for RuntimeMutationFinalizerSealHandleV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerSealHandleV1(<redacted>)")
    }
}

pub(crate) struct RuntimeMutationFinalizerTerminalObserverV1 {
    shared: Arc<RuntimeMutationFinalizerSharedV1>,
    terminal: watch::Receiver<Option<RuntimeSupervisorExitV1>>,
}

impl RuntimeMutationFinalizerTerminalObserverV1 {
    pub(crate) async fn wait(&mut self) -> RuntimeSupervisorExitV1 {
        loop {
            if let Some(exit) = *self.terminal.borrow_and_update() {
                return exit;
            }
            if self.terminal.changed().await.is_err() {
                return self
                    .shared
                    .terminal()
                    .unwrap_or(RuntimeSupervisorExitV1::Aborted);
            }
        }
    }
}

impl Debug for RuntimeMutationFinalizerTerminalObserverV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerTerminalObserverV1(<redacted>)")
    }
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
        self.try_register_for_phase_v1(job, RuntimeMutationFinalizerPhaseV1::StartupAccepting)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn try_register_process(
        &self,
        job: P::Job,
        reservation: &RuntimeMutationFinalizerProcessIntakeReservationV1,
    ) -> Result<
        RuntimeMutationFinalizerWaiterV1,
        RuntimeMutationFinalizerRegistrationRejectedV1<P::Job>,
    > {
        {
            let state = self.shared.lock();
            if state.supervisor_id != reservation.supervisor_id
                || state.generation != reservation.generation
                || state.activation_nonce != Some(reservation.nonce)
            {
                return Err(registration_rejected_v1(
                    job,
                    RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(
                        RuntimeSupervisorExitV1::ProtocolViolation,
                    ),
                ));
            }
        }
        match self.try_register_for_phase_v1(
            RuntimeMutationFinalizerJobV1::ProcessMutation(job),
            RuntimeMutationFinalizerPhaseV1::ProcessAccepting,
        ) {
            Ok(waiter) => Ok(waiter),
            Err(rejected) => {
                let reason = rejected.reason();
                let job = rejected.into_job().into_process_mutation();
                Err(registration_rejected_v1(job, reason))
            }
        }
    }

    fn try_register_for_phase_v1(
        &self,
        job: RuntimeMutationFinalizerJobV1<P::Job>,
        expected_phase: RuntimeMutationFinalizerPhaseV1,
    ) -> Result<
        RuntimeMutationFinalizerWaiterV1,
        RuntimeMutationFinalizerRegistrationRejectedV1<RuntimeMutationFinalizerJobV1<P::Job>>,
    > {
        let job_matches_phase = match expected_phase {
            RuntimeMutationFinalizerPhaseV1::StartupAccepting => job.is_startup_pending_drain(),
            RuntimeMutationFinalizerPhaseV1::ProcessAccepting => job.is_process_mutation(),
            RuntimeMutationFinalizerPhaseV1::StartupSealing
            | RuntimeMutationFinalizerPhaseV1::StartupSettled
            | RuntimeMutationFinalizerPhaseV1::ProcessActivationReserved
            | RuntimeMutationFinalizerPhaseV1::ShutdownSealed
            | RuntimeMutationFinalizerPhaseV1::Terminal => false,
        };
        if !job_matches_phase {
            return Err(registration_rejected_v1(
                job,
                RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed,
            ));
        }
        {
            let state = self.shared.lock();
            if let Some(exit) = state.terminal {
                return Err(registration_rejected_v1(
                    job,
                    RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(exit),
                ));
            }
            if state.phase != expected_phase {
                return Err(registration_rejected_v1(
                    job,
                    RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed,
                ));
            }
        }
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
        if state.phase != expected_phase {
            return Err(registration_rejected_v1(
                job,
                RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed,
            ));
        }
        let Some(sequence) = state.next_sequence else {
            state.startup_intake_sealed = true;
            state.shutdown_sealed = true;
            state.phase = RuntimeMutationFinalizerPhaseV1::ShutdownSealed;
            state.phase = RuntimeMutationFinalizerPhaseV1::Terminal;
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
            supervisor_id: state.supervisor_id,
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
    #[error("runtime mutation finalizer supervisor identity exhausted")]
    SupervisorIdentityExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeMutationFinalizerProcessActivationReserveErrorV1 {
    #[error("runtime mutation finalizer startup jobs are not settled")]
    StartupNotSettled,
    #[error("runtime mutation finalizer process activation is already reserved")]
    AlreadyReserved,
    #[error("runtime mutation finalizer process is already accepting")]
    AlreadyActive,
    #[error("runtime mutation finalizer shutdown is sealed")]
    ShutdownSealed,
    #[error("runtime mutation finalizer is terminal")]
    Terminal,
    #[error("runtime mutation finalizer process activation identity exhausted")]
    IdentityExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeMutationFinalizerProcessActivationErrorV1 {
    #[error("runtime mutation finalizer process activation authority does not match")]
    AuthorityMismatch,
    #[error("runtime mutation finalizer process activation deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime mutation finalizer process activation acknowledgement was lost")]
    AcknowledgementLost,
    #[error("runtime mutation finalizer process activation lost to shutdown")]
    ShutdownWon,
    #[error("runtime mutation finalizer process activation violated its protocol")]
    ProtocolViolation,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeMutationFinalizerProcessActivationV1 {
    supervisor_id: NonZeroU64,
    generation: RuntimeMutationFinalizerGenerationV1,
    nonce: NonZeroU64,
    shared: Arc<RuntimeMutationFinalizerSharedV1>,
    controls: mpsc::Sender<RuntimeMutationFinalizerControlV1>,
    armed: bool,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeMutationFinalizerProcessActivationV1 {
    pub(crate) const fn generation(&self) -> RuntimeMutationFinalizerGenerationV1 {
        self.generation
    }

    pub(crate) const fn nonce(&self) -> NonZeroU64 {
        self.nonce
    }
}

impl Debug for RuntimeMutationFinalizerProcessActivationV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerProcessActivationV1(<redacted>)")
    }
}

impl Drop for RuntimeMutationFinalizerProcessActivationV1 {
    fn drop(&mut self) {
        if self.armed {
            self.shared
                .publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation);
            let _ = self
                .controls
                .try_send(RuntimeMutationFinalizerControlV1::ProtocolViolation);
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
struct RuntimeMutationFinalizerProcessActivationCancellationGuardV1 {
    shared: Arc<RuntimeMutationFinalizerSharedV1>,
    controls: mpsc::Sender<RuntimeMutationFinalizerControlV1>,
    armed: bool,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeMutationFinalizerProcessActivationCancellationGuardV1 {
    fn new(
        shared: Arc<RuntimeMutationFinalizerSharedV1>,
        controls: mpsc::Sender<RuntimeMutationFinalizerControlV1>,
    ) -> Self {
        Self {
            shared,
            controls,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for RuntimeMutationFinalizerProcessActivationCancellationGuardV1 {
    fn drop(&mut self) {
        if self.armed {
            self.shared
                .publish_terminal(RuntimeSupervisorExitV1::Aborted);
            let _ = self
                .controls
                .try_send(RuntimeMutationFinalizerControlV1::ProtocolViolation);
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeMutationFinalizerProcessIntakeReservationV1 {
    supervisor_id: NonZeroU64,
    generation: RuntimeMutationFinalizerGenerationV1,
    nonce: NonZeroU64,
}

impl Debug for RuntimeMutationFinalizerProcessIntakeReservationV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerProcessIntakeReservationV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum RuntimeMutationFinalizerProcessIntakeHealthV1 {
    Ready,
    ShutdownSealed,
    Terminal(RuntimeSupervisorExitV1),
    ActorStopped,
    AuthorityMismatch,
}

#[cfg_attr(not(test), allow(dead_code))]
impl RuntimeMutationFinalizerProcessIntakeHealthV1 {
    pub(crate) const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Ready => "runtime_mutation_finalizer_process_intake_ready",
            Self::ShutdownSealed => "runtime_mutation_finalizer_process_intake_shutdown_sealed",
            Self::Terminal(_) => "runtime_mutation_finalizer_process_intake_terminal",
            Self::ActorStopped => "runtime_mutation_finalizer_process_intake_actor_stopped",
            Self::AuthorityMismatch => {
                "runtime_mutation_finalizer_process_intake_authority_mismatch"
            }
        }
    }
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
    in_flight_abort: Arc<RuntimeMutationFinalizerInFlightAbortV1>,
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
        let supervisor_id = NEXT_RUNTIME_MUTATION_FINALIZER_SUPERVISOR_ID
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(RuntimeMutationFinalizerStartErrorV1::SupervisorIdentityExhausted)?;
        let capacity = config.capacity().get();
        let (jobs, job_receiver) = mpsc::channel(capacity);
        let (controls, control_receiver) = mpsc::channel(1);
        let (completions, completion_receiver) = mpsc::channel(capacity);
        let (terminal_publisher, terminal) = watch::channel(None);
        let (startup_settlement_publisher, startup_settlement) = watch::channel(false);
        let shared = Arc::new(RuntimeMutationFinalizerSharedV1 {
            state: Mutex::new(RuntimeMutationFinalizerStateV1 {
                supervisor_id,
                generation,
                phase: RuntimeMutationFinalizerPhaseV1::StartupAccepting,
                next_sequence: Some(NonZeroU64::MIN),
                queued_jobs: 0,
                in_flight_jobs: 0,
                unsettled_jobs: 0,
                settled_jobs: 0,
                failed_jobs: 0,
                failed_closed_jobs: 0,
                activation_nonce: None,
                startup_intake_sealed: false,
                startup_jobs_settled: false,
                shutdown_sealed: false,
                terminal: None,
            }),
            terminal_publisher,
            startup_settlement_publisher,
        });
        let in_flight_abort = Arc::new(RuntimeMutationFinalizerInFlightAbortV1::new());
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
            in_flight_abort.clone(),
        ));
        Ok(Self {
            intake,
            controls,
            completions: Some(completion_receiver),
            terminal,
            startup_settlement,
            actor: Some(actor),
            in_flight_abort,
        })
    }

    pub fn intake(&self) -> &RuntimeMutationFinalizerIntakeV1<P> {
        &self.intake
    }

    pub fn snapshot(&self) -> RuntimeMutationFinalizerSnapshotV1 {
        self.intake.snapshot()
    }

    pub(crate) fn seal_handle(&self) -> RuntimeMutationFinalizerSealHandleV1 {
        RuntimeMutationFinalizerSealHandleV1 {
            shared: self.intake.shared.clone(),
            controls: self.controls.clone(),
        }
    }

    pub(crate) fn terminal_observer(&self) -> RuntimeMutationFinalizerTerminalObserverV1 {
        RuntimeMutationFinalizerTerminalObserverV1 {
            shared: self.intake.shared.clone(),
            terminal: self.terminal.clone(),
        }
    }

    pub fn seal_intake(&self) -> RuntimeMutationFinalizerSealOutcomeV1 {
        seal_runtime_mutation_finalizer_startup_v1(&self.intake.shared, &self.controls)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reserve_process_activation(
        &self,
    ) -> Result<
        RuntimeMutationFinalizerProcessActivationV1,
        RuntimeMutationFinalizerProcessActivationReserveErrorV1,
    > {
        let nonce = NEXT_RUNTIME_MUTATION_FINALIZER_ACTIVATION_NONCE
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(RuntimeMutationFinalizerProcessActivationReserveErrorV1::IdentityExhausted)?;
        let (supervisor_id, generation) = {
            let mut state = self.intake.shared.lock();
            match state.phase {
                RuntimeMutationFinalizerPhaseV1::StartupSettled => {}
                RuntimeMutationFinalizerPhaseV1::ProcessActivationReserved => {
                    return Err(
                        RuntimeMutationFinalizerProcessActivationReserveErrorV1::AlreadyReserved,
                    );
                }
                RuntimeMutationFinalizerPhaseV1::ProcessAccepting => {
                    return Err(
                        RuntimeMutationFinalizerProcessActivationReserveErrorV1::AlreadyActive,
                    );
                }
                RuntimeMutationFinalizerPhaseV1::ShutdownSealed => {
                    return Err(
                        RuntimeMutationFinalizerProcessActivationReserveErrorV1::ShutdownSealed,
                    );
                }
                RuntimeMutationFinalizerPhaseV1::Terminal => {
                    return Err(RuntimeMutationFinalizerProcessActivationReserveErrorV1::Terminal);
                }
                RuntimeMutationFinalizerPhaseV1::StartupAccepting
                | RuntimeMutationFinalizerPhaseV1::StartupSealing => {
                    return Err(
                        RuntimeMutationFinalizerProcessActivationReserveErrorV1::StartupNotSettled,
                    );
                }
            }
            state.phase = RuntimeMutationFinalizerPhaseV1::ProcessActivationReserved;
            state.activation_nonce = Some(nonce);
            (state.supervisor_id, state.generation)
        };
        Ok(RuntimeMutationFinalizerProcessActivationV1 {
            supervisor_id,
            generation,
            nonce,
            shared: self.intake.shared.clone(),
            controls: self.controls.clone(),
            armed: true,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn activate_process_until(
        self,
        mut activation: RuntimeMutationFinalizerProcessActivationV1,
        deadline: Instant,
    ) -> Result<
        RuntimeMutationFinalizerProcessSupervisorV1<P>,
        RuntimeMutationFinalizerProcessActivationFailureV1<P>,
    > {
        let shared_matches = Arc::ptr_eq(&self.intake.shared, &activation.shared);
        let (shutdown_won, reserved) = {
            let state = self.intake.shared.lock();
            let identity_matches = shared_matches
                && state.supervisor_id == activation.supervisor_id
                && state.generation == activation.generation
                && state.activation_nonce == Some(activation.nonce);
            (
                identity_matches
                    && (state.shutdown_sealed
                        || state.terminal.is_some()
                        || matches!(
                            state.phase,
                            RuntimeMutationFinalizerPhaseV1::ShutdownSealed
                                | RuntimeMutationFinalizerPhaseV1::Terminal
                        )),
                identity_matches
                    && matches!(
                        state.phase,
                        RuntimeMutationFinalizerPhaseV1::ProcessActivationReserved
                    )
                    && state.terminal.is_none(),
            )
        };
        if shutdown_won {
            activation.armed = false;
            return Err(RuntimeMutationFinalizerProcessActivationFailureV1 {
                error: RuntimeMutationFinalizerProcessActivationErrorV1::ShutdownWon,
                supervisor: self,
            });
        }
        if !reserved {
            self.intake
                .shared
                .publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation);
            return Err(RuntimeMutationFinalizerProcessActivationFailureV1 {
                error: RuntimeMutationFinalizerProcessActivationErrorV1::AuthorityMismatch,
                supervisor: self,
            });
        }
        activation.armed = false;
        let mut cancellation = RuntimeMutationFinalizerProcessActivationCancellationGuardV1::new(
            self.intake.shared.clone(),
            self.controls.clone(),
        );
        if Instant::now() >= deadline {
            self.intake
                .shared
                .publish_terminal(RuntimeSupervisorExitV1::DeadlineElapsed);
            cancellation.disarm();
            return Err(RuntimeMutationFinalizerProcessActivationFailureV1 {
                error: RuntimeMutationFinalizerProcessActivationErrorV1::DeadlineElapsed,
                supervisor: self,
            });
        }
        let (acknowledgement, observed) = oneshot::channel();
        let command = RuntimeMutationFinalizerControlV1::ActivateProcess {
            supervisor_id: activation.supervisor_id,
            generation: activation.generation,
            nonce: activation.nonce,
            acknowledgement,
        };
        match timeout_at(
            TokioInstant::from_std(deadline),
            self.controls.send(command),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                self.intake
                    .shared
                    .publish_terminal(RuntimeSupervisorExitV1::Aborted);
                cancellation.disarm();
                return Err(RuntimeMutationFinalizerProcessActivationFailureV1 {
                    error: RuntimeMutationFinalizerProcessActivationErrorV1::AcknowledgementLost,
                    supervisor: self,
                });
            }
            Err(_) => {
                self.intake
                    .shared
                    .publish_terminal(RuntimeSupervisorExitV1::DeadlineElapsed);
                cancellation.disarm();
                return Err(RuntimeMutationFinalizerProcessActivationFailureV1 {
                    error: RuntimeMutationFinalizerProcessActivationErrorV1::DeadlineElapsed,
                    supervisor: self,
                });
            }
        }
        let acknowledgement = match timeout_at(TokioInstant::from_std(deadline), observed).await {
            Ok(Ok(acknowledgement)) => acknowledgement,
            Ok(Err(_)) => {
                self.intake
                    .shared
                    .publish_terminal(RuntimeSupervisorExitV1::Aborted);
                cancellation.disarm();
                return Err(RuntimeMutationFinalizerProcessActivationFailureV1 {
                    error: RuntimeMutationFinalizerProcessActivationErrorV1::AcknowledgementLost,
                    supervisor: self,
                });
            }
            Err(_) => {
                self.intake
                    .shared
                    .publish_terminal(RuntimeSupervisorExitV1::DeadlineElapsed);
                cancellation.disarm();
                return Err(RuntimeMutationFinalizerProcessActivationFailureV1 {
                    error: RuntimeMutationFinalizerProcessActivationErrorV1::DeadlineElapsed,
                    supervisor: self,
                });
            }
        };
        if let Err(exit) = acknowledgement {
            cancellation.disarm();
            return Err(RuntimeMutationFinalizerProcessActivationFailureV1 {
                error: match exit {
                    RuntimeSupervisorExitV1::Commanded
                    | RuntimeSupervisorExitV1::DeadlineElapsed => {
                        RuntimeMutationFinalizerProcessActivationErrorV1::ShutdownWon
                    }
                    RuntimeSupervisorExitV1::ProtocolViolation => {
                        RuntimeMutationFinalizerProcessActivationErrorV1::ProtocolViolation
                    }
                    RuntimeSupervisorExitV1::DependencyTerminal
                    | RuntimeSupervisorExitV1::Panicked
                    | RuntimeSupervisorExitV1::Aborted => {
                        RuntimeMutationFinalizerProcessActivationErrorV1::AcknowledgementLost
                    }
                },
                supervisor: self,
            });
        }
        let activated = {
            let state = self.intake.shared.lock();
            state.supervisor_id == activation.supervisor_id
                && state.generation == activation.generation
                && state.activation_nonce == Some(activation.nonce)
                && matches!(
                    state.phase,
                    RuntimeMutationFinalizerPhaseV1::ProcessAccepting
                )
                && state.terminal.is_none()
                && !state.shutdown_sealed
        };
        if !activated {
            let shutdown_won = self.intake.shared.snapshot().shutdown_sealed();
            if !shutdown_won {
                self.intake
                    .shared
                    .publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation);
            }
            cancellation.disarm();
            return Err(RuntimeMutationFinalizerProcessActivationFailureV1 {
                error: if shutdown_won {
                    RuntimeMutationFinalizerProcessActivationErrorV1::ShutdownWon
                } else {
                    RuntimeMutationFinalizerProcessActivationErrorV1::ProtocolViolation
                },
                supervisor: self,
            });
        }
        cancellation.disarm();
        Ok(RuntimeMutationFinalizerProcessSupervisorV1 {
            supervisor: self,
            process_intake: RuntimeMutationFinalizerProcessIntakeReservationV1 {
                supervisor_id: activation.supervisor_id,
                generation: activation.generation,
                nonce: activation.nonce,
            },
        })
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
        seal_runtime_mutation_finalizer_shutdown_v1(&self.intake.shared, &self.controls);
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

    pub async fn shutdown_until(
        mut self,
        deadline: Instant,
    ) -> RuntimeMutationFinalizerJoinReportV1<P::Job, P::Output, P::Error> {
        seal_runtime_mutation_finalizer_shutdown_v1(&self.intake.shared, &self.controls);
        if Instant::now() >= deadline {
            return self.abort_and_finish_until(deadline).await;
        }
        let actor_abort_cutoff = deadline
            .checked_sub(RUNTIME_MUTATION_FINALIZER_ACTOR_ABORT_RESERVE)
            .unwrap_or(deadline);
        let in_flight_abort_cutoff = deadline
            .checked_sub(RUNTIME_MUTATION_FINALIZER_IN_FLIGHT_ABORT_RESERVE)
            .unwrap_or(deadline);
        let shutdown = self
            .controls
            .send(RuntimeMutationFinalizerControlV1::ShutdownUntil(
                in_flight_abort_cutoff,
            ));
        if timeout_at(TokioInstant::from_std(actor_abort_cutoff), shutdown)
            .await
            .is_err()
        {
            return self.abort_and_finish_until(deadline).await;
        }
        let exit = match self.actor.as_mut() {
            Some(actor) => {
                match timeout_at(TokioInstant::from_std(actor_abort_cutoff), actor).await {
                    Ok(result) => classify_actor_join_v1(result, &self.intake.shared),
                    Err(_) => return self.abort_and_finish_until(deadline).await,
                }
            }
            None => self
                .intake
                .shared
                .publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation),
        };
        self.actor.take();
        self.finish_report(exit).await
    }

    async fn abort_and_finish_until(
        mut self,
        deadline: Instant,
    ) -> RuntimeMutationFinalizerJoinReportV1<P::Job, P::Output, P::Error> {
        let exit = self
            .intake
            .shared
            .publish_terminal(RuntimeSupervisorExitV1::DeadlineElapsed);
        self.in_flight_abort
            .abort_with(RuntimeSupervisorExitV1::DeadlineElapsed);
        let _ = self
            .controls
            .send(RuntimeMutationFinalizerControlV1::ShutdownUntil(deadline))
            .await;
        if let Some(actor) = self.actor.take() {
            let _ = actor.await;
        }
        self.in_flight_abort.wait_stopped().await;
        self.in_flight_abort.take_forced_exit();
        self.intake.slots.close();
        self.finish_report_now(exit)
    }

    async fn finish_report(
        &mut self,
        exit: RuntimeSupervisorExitV1,
    ) -> RuntimeMutationFinalizerJoinReportV1<P::Job, P::Output, P::Error> {
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

    fn finish_report_now(
        &mut self,
        exit: RuntimeSupervisorExitV1,
    ) -> RuntimeMutationFinalizerJoinReportV1<P::Job, P::Output, P::Error> {
        let mut completions = Vec::with_capacity(self.snapshot().unsettled_jobs());
        if let Some(receiver) = self.completions.as_mut() {
            while let Ok(completion) = receiver.try_recv() {
                completions.push(completion);
            }
        }
        self.completions.take();
        RuntimeMutationFinalizerJoinReportV1 {
            exit,
            snapshot: self.snapshot(),
            completions,
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeMutationFinalizerProcessActivationFailureV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    error: RuntimeMutationFinalizerProcessActivationErrorV1,
    supervisor: RuntimeMutationFinalizerSupervisorV1<P>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<P> RuntimeMutationFinalizerProcessActivationFailureV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    pub(crate) const fn error(&self) -> RuntimeMutationFinalizerProcessActivationErrorV1 {
        self.error
    }

    pub(crate) fn into_shutdown_supervisor(self) -> RuntimeMutationFinalizerSupervisorV1<P> {
        self.supervisor
    }
}

impl<P> Debug for RuntimeMutationFinalizerProcessActivationFailureV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerProcessActivationFailureV1(<redacted>)")
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RuntimeMutationFinalizerProcessSupervisorV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    supervisor: RuntimeMutationFinalizerSupervisorV1<P>,
    process_intake: RuntimeMutationFinalizerProcessIntakeReservationV1,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<P> RuntimeMutationFinalizerProcessSupervisorV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    pub(crate) fn snapshot(&self) -> RuntimeMutationFinalizerSnapshotV1 {
        self.supervisor.snapshot()
    }

    pub(crate) fn seal_handle(&self) -> RuntimeMutationFinalizerSealHandleV1 {
        self.supervisor.seal_handle()
    }

    pub(crate) fn terminal_observation(&self) -> Option<RuntimeSupervisorExitV1> {
        self.supervisor.terminal_observation()
    }

    pub(crate) fn process_intake_health(&self) -> RuntimeMutationFinalizerProcessIntakeHealthV1 {
        let state = self.supervisor.intake.shared.lock();
        if let Some(exit) = state.terminal {
            return RuntimeMutationFinalizerProcessIntakeHealthV1::Terminal(exit);
        }
        if state.shutdown_sealed
            || matches!(state.phase, RuntimeMutationFinalizerPhaseV1::ShutdownSealed)
        {
            return RuntimeMutationFinalizerProcessIntakeHealthV1::ShutdownSealed;
        }
        let actor_running = self
            .supervisor
            .actor
            .as_ref()
            .is_some_and(|actor| !actor.is_finished());
        if !actor_running {
            return RuntimeMutationFinalizerProcessIntakeHealthV1::ActorStopped;
        }
        if state.supervisor_id != self.process_intake.supervisor_id
            || state.generation != self.process_intake.generation
            || state.activation_nonce != Some(self.process_intake.nonce)
            || !matches!(
                state.phase,
                RuntimeMutationFinalizerPhaseV1::ProcessAccepting
            )
        {
            return RuntimeMutationFinalizerProcessIntakeHealthV1::AuthorityMismatch;
        }
        RuntimeMutationFinalizerProcessIntakeHealthV1::Ready
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn try_register_process_job(
        &self,
        job: P::Job,
    ) -> Result<
        RuntimeMutationFinalizerWaiterV1,
        RuntimeMutationFinalizerRegistrationRejectedV1<P::Job>,
    > {
        self.supervisor
            .intake
            .try_register_process(job, &self.process_intake)
    }

    pub(crate) async fn next_completion(
        &mut self,
    ) -> Option<RuntimeMutationFinalizerPortCompletionV1<P>> {
        self.supervisor.next_completion().await
    }

    pub(crate) async fn join(
        self,
    ) -> RuntimeMutationFinalizerJoinReportV1<P::Job, P::Output, P::Error> {
        self.supervisor.join().await
    }

    pub(crate) async fn shutdown_until(
        self,
        deadline: Instant,
    ) -> RuntimeMutationFinalizerJoinReportV1<P::Job, P::Output, P::Error> {
        self.supervisor.shutdown_until(deadline).await
    }
}

impl<P> Debug for RuntimeMutationFinalizerProcessSupervisorV1<P>
where
    P: RuntimeMutationFinalizerPortV1,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeMutationFinalizerProcessSupervisorV1(<redacted>)")
    }
}

fn seal_runtime_mutation_finalizer_startup_v1(
    shared: &RuntimeMutationFinalizerSharedV1,
    controls: &mpsc::Sender<RuntimeMutationFinalizerControlV1>,
) -> RuntimeMutationFinalizerSealOutcomeV1 {
    let outcome = {
        let mut state = shared.lock();
        let terminal = state.terminal.is_some();
        let was_open = matches!(
            state.phase,
            RuntimeMutationFinalizerPhaseV1::StartupAccepting
        );
        if was_open {
            state.phase = RuntimeMutationFinalizerPhaseV1::StartupSealing;
            state.startup_intake_sealed = true;
        }
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
        let _ = controls.try_send(RuntimeMutationFinalizerControlV1::StartupSeal);
    }
    shared.publish_startup_settled_if_ready();
    outcome
}

fn seal_runtime_mutation_finalizer_shutdown_v1(
    shared: &RuntimeMutationFinalizerSharedV1,
    controls: &mpsc::Sender<RuntimeMutationFinalizerControlV1>,
) -> RuntimeMutationFinalizerSealOutcomeV1 {
    let outcome = {
        let mut state = shared.lock();
        let terminal = state.terminal.is_some();
        let was_open = !state.shutdown_sealed;
        state.startup_intake_sealed = true;
        state.shutdown_sealed = true;
        if !terminal {
            state.phase = RuntimeMutationFinalizerPhaseV1::ShutdownSealed;
        }
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
        let _ = controls.try_send(RuntimeMutationFinalizerControlV1::ShutdownSeal);
    }
    outcome
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
    in_flight_abort: Arc<RuntimeMutationFinalizerInFlightAbortV1>,
) -> RuntimeSupervisorExitV1
where
    P: RuntimeMutationFinalizerPortV1,
{
    let mut shutdown = false;
    loop {
        shared.publish_startup_settled_if_ready();
        let snapshot = shared.snapshot();
        if let Some(exit) = snapshot.terminal() {
            jobs.close();
            fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
            return exit;
        }
        if snapshot.shutdown_sealed() {
            let exit = shared.publish_terminal(RuntimeSupervisorExitV1::Commanded);
            jobs.close();
            fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
            return exit;
        }
        let control_only = matches!(
            snapshot.phase(),
            RuntimeMutationFinalizerPhaseV1::StartupSettled
                | RuntimeMutationFinalizerPhaseV1::ProcessActivationReserved
        );
        let event = if control_only {
            RuntimeMutationFinalizerActorEventV1::Control(controls.recv().await)
        } else {
            tokio::select! {
                biased;
                control = controls.recv() => {
                    RuntimeMutationFinalizerActorEventV1::Control(control)
                }
                job = jobs.recv() => RuntimeMutationFinalizerActorEventV1::Job(job),
            }
        };
        let envelope = match event {
            RuntimeMutationFinalizerActorEventV1::Control(control) => match control {
                Some(RuntimeMutationFinalizerControlV1::StartupSeal)
                | Some(RuntimeMutationFinalizerControlV1::ShutdownSeal) => {
                    continue;
                }
                Some(RuntimeMutationFinalizerControlV1::Shutdown) => {
                    let exit = shared.publish_terminal(RuntimeSupervisorExitV1::Commanded);
                    jobs.close();
                    fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
                    return exit;
                }
                Some(RuntimeMutationFinalizerControlV1::ShutdownUntil(_)) => {
                    let exit = shared.publish_terminal(RuntimeSupervisorExitV1::Commanded);
                    jobs.close();
                    fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
                    return exit;
                }
                Some(RuntimeMutationFinalizerControlV1::ActivateProcess {
                    supervisor_id,
                    generation,
                    nonce,
                    acknowledgement,
                }) => {
                    let activation = {
                        let mut state = shared.lock();
                        if state.shutdown_sealed {
                            Err(RuntimeSupervisorExitV1::Commanded)
                        } else if state.supervisor_id == supervisor_id
                            && state.generation == generation
                            && state.activation_nonce == Some(nonce)
                            && matches!(
                                state.phase,
                                RuntimeMutationFinalizerPhaseV1::ProcessActivationReserved
                            )
                            && state.startup_intake_sealed
                            && state.startup_jobs_settled
                            && state.queued_jobs == 0
                            && state.in_flight_jobs == 0
                            && state.terminal.is_none()
                        {
                            state.phase = RuntimeMutationFinalizerPhaseV1::ProcessAccepting;
                            Ok(())
                        } else {
                            Err(RuntimeSupervisorExitV1::ProtocolViolation)
                        }
                    };
                    let protocol_violation =
                        matches!(activation, Err(RuntimeSupervisorExitV1::ProtocolViolation));
                    if acknowledgement.send(activation).is_err() {
                        let exit = shared.publish_terminal(RuntimeSupervisorExitV1::Aborted);
                        jobs.close();
                        fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
                        return exit;
                    }
                    if protocol_violation {
                        let exit =
                            shared.publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation);
                        jobs.close();
                        fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
                        return exit;
                    }
                    continue;
                }
                Some(RuntimeMutationFinalizerControlV1::ProtocolViolation) | None => {
                    let exit = shared.publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation);
                    jobs.close();
                    fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
                    return exit;
                }
            },
            RuntimeMutationFinalizerActorEventV1::Job(Some(envelope)) => envelope,
            RuntimeMutationFinalizerActorEventV1::Job(None) => {
                let exit = shared.publish_terminal(RuntimeSupervisorExitV1::ProtocolViolation);
                jobs.close();
                fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
                return exit;
            }
        };
        let phase = shared.snapshot().phase();
        let may_dispatch = match &envelope.job {
            RuntimeMutationFinalizerJobV1::StartupPendingDrain(_) => matches!(
                phase,
                RuntimeMutationFinalizerPhaseV1::StartupAccepting
                    | RuntimeMutationFinalizerPhaseV1::StartupSealing
            ),
            RuntimeMutationFinalizerJobV1::ProcessMutation(_) => {
                matches!(phase, RuntimeMutationFinalizerPhaseV1::ProcessAccepting)
            }
        };
        if !may_dispatch {
            let exit = if shared.snapshot().shutdown_sealed() {
                RuntimeSupervisorExitV1::Commanded
            } else {
                RuntimeSupervisorExitV1::ProtocolViolation
            };
            let exit = shared.publish_terminal(exit);
            jobs.close();
            fail_registered_job_v1(&shared, &completions, envelope, exit).await;
            fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
            return exit;
        }
        let exit = execute_registered_job_v1(
            &port,
            &shared,
            &completions,
            &mut controls,
            &mut shutdown,
            &in_flight_abort,
            envelope,
        )
        .await;
        if let Some(exit) = exit {
            let exit = shared.publish_terminal(exit);
            jobs.close();
            fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
            return exit;
        }
        if shutdown {
            let exit = shared.publish_terminal(RuntimeSupervisorExitV1::Commanded);
            jobs.close();
            fail_queued_jobs_v1(&shared, &mut jobs, &completions, exit).await;
            return exit;
        }
    }
}

enum RuntimeMutationFinalizerActorEventV1<J> {
    Control(Option<RuntimeMutationFinalizerControlV1>),
    Job(Option<RuntimeMutationFinalizerEnvelopeV1<J>>),
}

async fn execute_registered_job_v1<P>(
    port: &Arc<P>,
    shared: &RuntimeMutationFinalizerSharedV1,
    completions: &mpsc::Sender<RuntimeMutationFinalizerCompletionV1<P::Job, P::Output, P::Error>>,
    controls: &mut mpsc::Receiver<RuntimeMutationFinalizerControlV1>,
    shutdown: &mut bool,
    in_flight_abort: &Arc<RuntimeMutationFinalizerInFlightAbortV1>,
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
    let task_abort = in_flight_abort.clone();
    let (start, started) = oneshot::channel();
    let mut task = tokio::spawn(async move {
        let _ = started.await;
        let stopped_guard = RuntimeMutationFinalizerInFlightStoppedGuardV1 {
            in_flight_abort: task_abort,
        };
        let result = port.execute(job).await;
        drop(stopped_guard);
        result
    });
    in_flight_abort.publish(task.abort_handle());
    let _ = start.send(());
    let in_flight_guard = RuntimeMutationFinalizerInFlightAbortGuardV1 {
        in_flight_abort: in_flight_abort.clone(),
    };
    let mut forced_exit = None;
    let result = loop {
        tokio::select! {
            biased;
            result = &mut task => break result,
            control = controls.recv() => {
                match control {
                    Some(RuntimeMutationFinalizerControlV1::StartupSeal) => {}
                    Some(RuntimeMutationFinalizerControlV1::ShutdownSeal) => {
                        *shutdown = true;
                    }
                    Some(RuntimeMutationFinalizerControlV1::ActivateProcess {
                        acknowledgement,
                        ..
                    }) => {
                        let _ = acknowledgement.send(Err(
                            RuntimeSupervisorExitV1::ProtocolViolation,
                        ));
                        task.abort();
                        forced_exit = Some(RuntimeSupervisorExitV1::ProtocolViolation);
                        break task.await;
                    }
                    Some(RuntimeMutationFinalizerControlV1::Shutdown) => {
                        *shutdown = true;
                    }
                    Some(RuntimeMutationFinalizerControlV1::ShutdownUntil(deadline)) => {
                        *shutdown = true;
                        match timeout_at(TokioInstant::from_std(deadline), &mut task).await {
                            Ok(result) => break result,
                            Err(_) => {
                                task.abort();
                                forced_exit = Some(RuntimeSupervisorExitV1::DeadlineElapsed);
                                break task.await;
                            }
                        }
                    }
                    Some(RuntimeMutationFinalizerControlV1::ProtocolViolation) | None => {
                        task.abort();
                        forced_exit = Some(RuntimeSupervisorExitV1::ProtocolViolation);
                        break task.await;
                    }
                }
            }
        }
    };
    let external_forced_exit = in_flight_abort.take_forced_exit();
    in_flight_abort.clear();
    drop(in_flight_guard);
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
            let exit = forced_exit.or(external_forced_exit).unwrap_or_else(|| {
                if error.is_cancelled() {
                    RuntimeSupervisorExitV1::Aborted
                } else {
                    RuntimeSupervisorExitV1::Panicked
                }
            });
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

struct RuntimeMutationFinalizerInFlightAbortGuardV1 {
    in_flight_abort: Arc<RuntimeMutationFinalizerInFlightAbortV1>,
}

impl Drop for RuntimeMutationFinalizerInFlightAbortGuardV1 {
    fn drop(&mut self) {
        self.in_flight_abort.abort();
    }
}

struct RuntimeMutationFinalizerInFlightStoppedGuardV1 {
    in_flight_abort: Arc<RuntimeMutationFinalizerInFlightAbortV1>,
}

impl Drop for RuntimeMutationFinalizerInFlightStoppedGuardV1 {
    fn drop(&mut self) {
        self.in_flight_abort.clear();
    }
}

async fn fail_registered_job_v1<J, O, E>(
    shared: &RuntimeMutationFinalizerSharedV1,
    completions: &mpsc::Sender<RuntimeMutationFinalizerCompletionV1<J, O, E>>,
    envelope: RuntimeMutationFinalizerEnvelopeV1<J>,
    exit: RuntimeSupervisorExitV1,
) where
    J: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
{
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
        fail_registered_job_v1(shared, completions, envelope, exit).await;
    }
}

fn snapshot_from_state_v1(
    state: &RuntimeMutationFinalizerStateV1,
) -> RuntimeMutationFinalizerSnapshotV1 {
    RuntimeMutationFinalizerSnapshotV1 {
        generation: state.generation,
        phase: state.phase,
        intake_open: matches!(
            state.phase,
            RuntimeMutationFinalizerPhaseV1::StartupAccepting
                | RuntimeMutationFinalizerPhaseV1::ProcessAccepting
        ),
        queued_jobs: state.queued_jobs,
        in_flight_jobs: state.in_flight_jobs,
        unsettled_jobs: state.unsettled_jobs,
        settled_jobs: state.settled_jobs,
        failed_jobs: state.failed_jobs,
        failed_closed_jobs: state.failed_closed_jobs,
        next_job_sequence: state.next_sequence,
        startup_intake_sealed: state.startup_intake_sealed,
        startup_jobs_settled: state.startup_jobs_settled,
        shutdown_sealed: state.shutdown_sealed,
        terminal: state.terminal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Notify;

    struct NoopPort;

    impl RuntimeMutationFinalizerPortV1 for NoopPort {
        type Job = ();
        type Output = ();
        type Error = ();

        async fn execute(
            &self,
            _job: RuntimeMutationFinalizerJobV1<Self::Job>,
        ) -> Result<Self::Output, Self::Error> {
            Ok(())
        }
    }

    enum ProcessTestJobV1 {
        Block,
        Marker,
        Rejected,
    }

    struct ProcessTestPortV1 {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    impl RuntimeMutationFinalizerPortV1 for ProcessTestPortV1 {
        type Job = ProcessTestJobV1;
        type Output = ProcessTestJobV1;
        type Error = ();

        async fn execute(
            &self,
            job: RuntimeMutationFinalizerJobV1<Self::Job>,
        ) -> Result<Self::Output, Self::Error> {
            let job = job.into_inner();
            if matches!(&job, ProcessTestJobV1::Block) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            Ok(job)
        }
    }

    fn generation_v1() -> RuntimeMutationFinalizerGenerationV1 {
        RuntimeMutationFinalizerGenerationV1::new(NonZeroU64::new(19).unwrap()).unwrap()
    }

    fn supervisor_v1() -> RuntimeMutationFinalizerSupervisorV1<NoopPort> {
        RuntimeMutationFinalizerSupervisorV1::start(
            RuntimeMutationFinalizerConfigV1::new(1).unwrap(),
            generation_v1(),
            NoopPort,
        )
        .unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn actor_side_acknowledgement_loss_is_terminal_without_reservation() {
        let mut supervisor = supervisor_v1();
        supervisor.seal_intake();
        assert!(supervisor.wait_startup_jobs_settled().await);
        let mut activation = supervisor.reserve_process_activation().unwrap();
        let permit = supervisor.controls.reserve().await.unwrap();
        let (acknowledgement, observation) = oneshot::channel();
        drop(observation);
        let command = RuntimeMutationFinalizerControlV1::ActivateProcess {
            supervisor_id: activation.supervisor_id,
            generation: activation.generation,
            nonce: activation.nonce,
            acknowledgement,
        };
        activation.armed = false;
        drop(activation);

        permit.send(command);
        assert_eq!(
            supervisor.wait_terminal().await,
            RuntimeSupervisorExitV1::Aborted
        );

        let snapshot = supervisor.snapshot();
        assert_eq!(snapshot.terminal(), Some(RuntimeSupervisorExitV1::Aborted));
        assert!(!snapshot.process_accepting());
        assert!(!snapshot.intake_open());
        assert_eq!(
            supervisor.join().await.exit(),
            RuntimeSupervisorExitV1::Aborted
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_queued_after_activation_command_wins_before_actor_ack() {
        let mut supervisor = supervisor_v1();
        supervisor.seal_intake();
        assert!(supervisor.wait_startup_jobs_settled().await);
        let mut activation = supervisor.reserve_process_activation().unwrap();
        let permit = supervisor.controls.reserve().await.unwrap();
        let (acknowledgement, observation) = oneshot::channel();
        let command = RuntimeMutationFinalizerControlV1::ActivateProcess {
            supervisor_id: activation.supervisor_id,
            generation: activation.generation,
            nonce: activation.nonce,
            acknowledgement,
        };
        activation.armed = false;
        drop(activation);

        permit.send(command);
        supervisor.seal_handle().seal_intake();
        assert_eq!(
            observation.await.unwrap(),
            Err(RuntimeSupervisorExitV1::Commanded)
        );
        tokio::task::yield_now().await;

        let snapshot = supervisor.snapshot();
        assert!(snapshot.shutdown_sealed());
        assert_eq!(
            snapshot.terminal(),
            Some(RuntimeSupervisorExitV1::Commanded)
        );
        assert!(!snapshot.process_accepting());
        assert!(!snapshot.intake_open());
        assert_eq!(
            supervisor.join().await.exit(),
            RuntimeSupervisorExitV1::Commanded
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn process_intake_health_detects_a_finished_tracked_actor() {
        let mut supervisor = supervisor_v1();
        let startup_intake = supervisor.intake().clone();
        let process_job = startup_intake
            .try_register(RuntimeMutationFinalizerJobV1::ProcessMutation(()))
            .unwrap_err();
        assert_eq!(
            process_job.reason(),
            RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed
        );
        process_job.into_job().into_process_mutation();
        let waiter = startup_intake
            .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(()))
            .unwrap();
        assert_eq!(
            waiter.wait().await.status(),
            RuntimeMutationFinalizerWaitStatusV1::Settled
        );
        drop(supervisor.next_completion().await.unwrap());
        supervisor.seal_intake();
        assert!(supervisor.wait_startup_jobs_settled().await);
        let before = supervisor.snapshot();
        let activation = supervisor.reserve_process_activation().unwrap();
        assert_eq!(activation.generation(), generation_v1());
        assert!(activation.nonce().get() > 0);
        assert!(supervisor.snapshot().process_activation_reserved());
        let process = supervisor
            .activate_process_until(activation, Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        let after = process.snapshot();
        assert_eq!(after.generation(), before.generation());
        assert_eq!(after.next_job_sequence(), before.next_job_sequence());
        assert!(after.startup_intake_sealed());
        assert!(after.startup_jobs_settled());
        assert!(after.process_accepting());
        assert!(after.intake_open());
        assert!(process.process_intake_health().is_ready());
        assert_eq!(
            process.process_intake_health().code(),
            "runtime_mutation_finalizer_process_intake_ready"
        );
        let rejected = startup_intake
            .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(()))
            .unwrap_err();
        assert_eq!(
            rejected.reason(),
            RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed
        );
        rejected.into_job().into_startup_pending_drain();

        process.supervisor.actor.as_ref().unwrap().abort();
        loop {
            if process
                .supervisor
                .actor
                .as_ref()
                .is_some_and(|actor| actor.is_finished())
            {
                break;
            }
            tokio::task::yield_now().await;
        }

        assert_eq!(
            process.process_intake_health(),
            RuntimeMutationFinalizerProcessIntakeHealthV1::ActorStopped
        );
        let report = process.join().await;
        assert_eq!(report.exit(), RuntimeSupervisorExitV1::Aborted);
        assert_eq!(
            report.snapshot().next_job_sequence(),
            before.next_job_sequence()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn process_registration_reuses_capacity_and_returns_undispatched_affine_job() {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut supervisor = RuntimeMutationFinalizerSupervisorV1::start(
            RuntimeMutationFinalizerConfigV1::new(2).unwrap(),
            generation_v1(),
            ProcessTestPortV1 {
                entered: entered.clone(),
                release: release.clone(),
            },
        )
        .unwrap();
        supervisor.seal_intake();
        assert!(supervisor.wait_startup_jobs_settled().await);
        let activation = supervisor.reserve_process_activation().unwrap();
        let mut process = supervisor
            .activate_process_until(activation, Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        let first = process
            .try_register_process_job(ProcessTestJobV1::Block)
            .unwrap();
        entered.notified().await;
        let second = process
            .try_register_process_job(ProcessTestJobV1::Marker)
            .unwrap();
        let second_id = second.job_id();
        drop(second);
        let rejected = process
            .try_register_process_job(ProcessTestJobV1::Rejected)
            .unwrap_err();
        assert_eq!(
            rejected.reason(),
            RuntimeMutationFinalizerRegistrationRejectionReasonV1::Busy
        );
        assert!(matches!(rejected.into_job(), ProcessTestJobV1::Rejected));
        process.seal_handle().seal_intake();
        release.notify_waiters();

        assert_eq!(
            first.wait().await.status(),
            RuntimeMutationFinalizerWaitStatusV1::Settled
        );
        let first_completion = process.next_completion().await.unwrap();
        assert!(matches!(
            first_completion.result(),
            RuntimeMutationFinalizerCompletionResultV1::Settled(ProcessTestJobV1::Block)
        ));
        drop(first_completion);
        let second_completion = process.next_completion().await.unwrap();
        assert_eq!(second_completion.job_id(), second_id);
        assert!(matches!(
            second_completion.result(),
            RuntimeMutationFinalizerCompletionResultV1::Undispatched {
                job: RuntimeMutationFinalizerJobV1::ProcessMutation(ProcessTestJobV1::Marker),
                exit: RuntimeSupervisorExitV1::Commanded,
            }
        ));
        assert_eq!(
            process
                .shutdown_until(Instant::now() + Duration::from_secs(1))
                .await
                .exit(),
            RuntimeSupervisorExitV1::Commanded
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_after_actor_ack_cannot_strand_process_accepting() {
        let mut supervisor = supervisor_v1();
        supervisor.seal_intake();
        assert!(supervisor.wait_startup_jobs_settled().await);
        let capacity = supervisor.controls.reserve().await.unwrap();
        drop(capacity);
        let observation = supervisor.seal_handle();
        let activation = supervisor.reserve_process_activation().unwrap();
        let mut activation_future = Box::pin(
            supervisor.activate_process_until(activation, Instant::now() + Duration::from_secs(1)),
        );

        std::future::poll_fn(|context| {
            assert!(activation_future.as_mut().poll(context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        tokio::task::yield_now().await;
        assert!(observation.snapshot().process_accepting());
        assert_eq!(observation.snapshot().terminal(), None);

        drop(activation_future);

        let snapshot = observation.snapshot();
        assert_eq!(snapshot.terminal(), Some(RuntimeSupervisorExitV1::Aborted));
        assert!(snapshot.shutdown_sealed());
        assert!(!snapshot.process_accepting());
        assert!(!snapshot.intake_open());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn activation_reservation_drop_and_shutdown_races_are_irreversible() {
        let mut dropped = supervisor_v1();
        dropped.seal_intake();
        assert!(dropped.wait_startup_jobs_settled().await);
        let activation = dropped.reserve_process_activation().unwrap();
        assert!(dropped.reserve_process_activation().is_err());
        drop(activation);
        tokio::task::yield_now().await;
        assert_eq!(
            dropped.terminal_observation(),
            Some(RuntimeSupervisorExitV1::ProtocolViolation)
        );
        assert!(dropped.snapshot().shutdown_sealed());
        assert_eq!(
            dropped.join().await.exit(),
            RuntimeSupervisorExitV1::ProtocolViolation
        );

        let mut before = supervisor_v1();
        before.seal_intake();
        assert!(before.wait_startup_jobs_settled().await);
        let shutdown = before.seal_handle();
        let activation = before.reserve_process_activation().unwrap();
        shutdown.seal_intake();
        let failure = before
            .activate_process_until(activation, Instant::now() + Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(
            failure.error(),
            RuntimeMutationFinalizerProcessActivationErrorV1::ShutdownWon
        );
        let before = failure.into_shutdown_supervisor();
        assert!(before.snapshot().shutdown_sealed());
        assert_eq!(
            before
                .shutdown_until(Instant::now() + Duration::from_secs(1))
                .await
                .exit(),
            RuntimeSupervisorExitV1::Commanded
        );

        let mut after = supervisor_v1();
        after.seal_intake();
        assert!(after.wait_startup_jobs_settled().await);
        let shutdown = after.seal_handle();
        let activation = after.reserve_process_activation().unwrap();
        let mut process = after
            .activate_process_until(activation, Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(process.process_intake_health().is_ready());
        let process_shutdown = process.seal_handle();
        shutdown.seal_intake();
        process_shutdown.seal_intake();
        assert!(!process.process_intake_health().is_ready());
        assert_eq!(
            process.terminal_observation(),
            process.snapshot().terminal()
        );
        assert!(process.next_completion().await.is_none());
        assert_eq!(
            process
                .shutdown_until(Instant::now() + Duration::from_secs(1))
                .await
                .exit(),
            RuntimeSupervisorExitV1::Commanded
        );
    }
}
