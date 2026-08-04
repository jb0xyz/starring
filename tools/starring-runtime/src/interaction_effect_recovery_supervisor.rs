use std::collections::VecDeque;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{sleep, timeout, timeout_at, Instant as TokioInstant};

const RUNTIME_INTERACTION_EFFECT_RECOVERY_CADENCE_V1: Duration = Duration::from_secs(15);
const RUNTIME_INTERACTION_EFFECT_RECOVERY_PAGE_LIMIT_V1: usize = 64;
const RUNTIME_INTERACTION_EFFECT_RECOVERY_MAX_PAGES_V1: usize = 16;
const RUNTIME_INTERACTION_EFFECT_RECOVERY_CONCURRENCY_V1: usize = 8;
const RUNTIME_INTERACTION_EFFECT_RECOVERY_SCAN_TIMEOUT_V1: Duration = Duration::from_secs(5);
const RUNTIME_INTERACTION_EFFECT_RECOVERY_CANDIDATE_TIMEOUT_V1: Duration = Duration::from_secs(10);
const RUNTIME_INTERACTION_EFFECT_RECOVERY_FAILED_SWEEP_LIMIT_V1: u64 = 3;

pub(crate) type RuntimeInteractionEffectRecoveryScanFutureV1<C, K, E> = Pin<
    Box<
        dyn Future<Output = Result<RuntimeInteractionEffectRecoveryScanPageV1<C, K>, E>>
            + Send
            + 'static,
    >,
>;

pub(crate) type RuntimeInteractionEffectRecoveryCandidateFutureV1<E> = Pin<
    Box<
        dyn Future<Output = Result<RuntimeInteractionEffectRecoveryDispositionV1, E>>
            + Send
            + 'static,
    >,
>;

pub(crate) struct RuntimeInteractionEffectRecoveryScanRequestV1<C> {
    cursor: Option<C>,
    limit: NonZeroUsize,
}

impl<C> RuntimeInteractionEffectRecoveryScanRequestV1<C> {
    pub(crate) fn into_parts(self) -> (Option<C>, NonZeroUsize) {
        (self.cursor, self.limit)
    }
}

impl<C> Debug for RuntimeInteractionEffectRecoveryScanRequestV1<C> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryScanRequestV1(<redacted>)")
    }
}

pub(crate) struct RuntimeInteractionEffectRecoveryScanPageV1<C, K> {
    candidates: Vec<K>,
    next_cursor: Option<C>,
    exhausted: bool,
}

impl<C, K> RuntimeInteractionEffectRecoveryScanPageV1<C, K> {
    pub(crate) fn new(
        candidates: Vec<K>,
        next_cursor: Option<C>,
        exhausted: bool,
    ) -> Result<Self, RuntimeInteractionEffectRecoveryScanPageErrorV1> {
        if candidates.is_empty() && !exhausted
            || !exhausted && next_cursor.is_none()
            || exhausted && next_cursor.is_some()
        {
            return Err(RuntimeInteractionEffectRecoveryScanPageErrorV1::Invalid);
        }
        Ok(Self {
            candidates,
            next_cursor,
            exhausted,
        })
    }

    fn into_parts(self) -> (Vec<K>, Option<C>, bool) {
        (self.candidates, self.next_cursor, self.exhausted)
    }
}

impl<C, K> Debug for RuntimeInteractionEffectRecoveryScanPageV1<C, K> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryScanPageV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeInteractionEffectRecoveryScanPageErrorV1 {
    #[error("runtime interaction effect recovery scan page is invalid")]
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionEffectRecoveryDispositionV1 {
    Reconciled,
    Compensated,
    Deferred,
    RouteBlocked,
}

pub(crate) trait RuntimeInteractionEffectRecoverySupervisorPortV1:
    Send + Sync + 'static
{
    type Cursor: Send + 'static;
    type Candidate: Send + 'static;
    type Error: Send + 'static;

    fn scan_recoverable_v1(
        self: Arc<Self>,
        request: RuntimeInteractionEffectRecoveryScanRequestV1<Self::Cursor>,
    ) -> RuntimeInteractionEffectRecoveryScanFutureV1<Self::Cursor, Self::Candidate, Self::Error>;

    fn recover_candidate_v1(
        self: Arc<Self>,
        candidate: Self::Candidate,
    ) -> RuntimeInteractionEffectRecoveryCandidateFutureV1<Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeInteractionEffectRecoverySupervisorConfigV1 {
    cadence: Duration,
    page_limit: NonZeroUsize,
    max_pages_per_sweep: NonZeroUsize,
    max_concurrency: NonZeroUsize,
    scan_timeout: Duration,
    candidate_timeout: Duration,
}

impl RuntimeInteractionEffectRecoverySupervisorConfigV1 {
    pub(crate) fn production_v1() -> Self {
        Self {
            cadence: RUNTIME_INTERACTION_EFFECT_RECOVERY_CADENCE_V1,
            page_limit: NonZeroUsize::new(RUNTIME_INTERACTION_EFFECT_RECOVERY_PAGE_LIMIT_V1)
                .expect("runtime interaction effect recovery page limit is non-zero"),
            max_pages_per_sweep: NonZeroUsize::new(
                RUNTIME_INTERACTION_EFFECT_RECOVERY_MAX_PAGES_V1,
            )
            .expect("runtime interaction effect recovery page budget is non-zero"),
            max_concurrency: NonZeroUsize::new(RUNTIME_INTERACTION_EFFECT_RECOVERY_CONCURRENCY_V1)
                .expect("runtime interaction effect recovery concurrency is non-zero"),
            scan_timeout: RUNTIME_INTERACTION_EFFECT_RECOVERY_SCAN_TIMEOUT_V1,
            candidate_timeout: RUNTIME_INTERACTION_EFFECT_RECOVERY_CANDIDATE_TIMEOUT_V1,
        }
    }

    #[cfg(test)]
    fn for_test_v1(
        cadence: Duration,
        page_limit: usize,
        max_pages_per_sweep: usize,
        max_concurrency: usize,
        scan_timeout: Duration,
        candidate_timeout: Duration,
    ) -> Self {
        Self {
            cadence,
            page_limit: NonZeroUsize::new(page_limit).unwrap(),
            max_pages_per_sweep: NonZeroUsize::new(max_pages_per_sweep).unwrap(),
            max_concurrency: NonZeroUsize::new(max_concurrency).unwrap(),
            scan_timeout,
            candidate_timeout,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionEffectRecoverySupervisorExitV1 {
    Commanded,
    ControlClosed,
    Panicked,
    DeadlineElapsed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuntimeInteractionEffectRecoverySupervisorProgressV1 {
    pub(crate) sweeps_started: u64,
    pub(crate) sweeps_completed: u64,
    pub(crate) scans_succeeded: u64,
    pub(crate) scans_failed: u64,
    pub(crate) scans_timed_out: u64,
    pub(crate) scan_protocol_violations: u64,
    pub(crate) pages: u64,
    pub(crate) page_budget_exhausted: u64,
    pub(crate) candidates: u64,
    pub(crate) candidates_attempted: u64,
    pub(crate) candidates_reconciled: u64,
    pub(crate) candidates_compensated: u64,
    pub(crate) candidates_deferred: u64,
    pub(crate) routes_blocked: u64,
    pub(crate) candidates_failed: u64,
    pub(crate) candidates_timed_out: u64,
    pub(crate) candidates_panicked: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeInteractionEffectRecoverySupervisorReportV1 {
    exit: RuntimeInteractionEffectRecoverySupervisorExitV1,
    progress: RuntimeInteractionEffectRecoverySupervisorProgressV1,
}

impl RuntimeInteractionEffectRecoverySupervisorReportV1 {
    pub(crate) const fn exit(self) -> RuntimeInteractionEffectRecoverySupervisorExitV1 {
        self.exit
    }

    pub(crate) const fn progress(self) -> RuntimeInteractionEffectRecoverySupervisorProgressV1 {
        self.progress
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RuntimeInteractionEffectRecoverySupervisorObservationV1 {
    progress: RuntimeInteractionEffectRecoverySupervisorProgressV1,
    consecutive_failed_sweeps: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeInteractionEffectRecoverySupervisorHealthV1 {
    running: bool,
    progress: RuntimeInteractionEffectRecoverySupervisorProgressV1,
    consecutive_failed_sweeps: u64,
}

impl RuntimeInteractionEffectRecoverySupervisorHealthV1 {
    pub(crate) const fn is_ready_v1(self) -> bool {
        self.running
            && self.consecutive_failed_sweeps
                < RUNTIME_INTERACTION_EFFECT_RECOVERY_FAILED_SWEEP_LIMIT_V1
    }

    pub(crate) const fn progress_v1(self) -> RuntimeInteractionEffectRecoverySupervisorProgressV1 {
        self.progress
    }

    pub(crate) const fn consecutive_failed_sweeps_v1(self) -> u64 {
        self.consecutive_failed_sweeps
    }
}

pub(crate) struct RuntimeInteractionEffectRecoverySupervisorV1 {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<RuntimeInteractionEffectRecoverySupervisorReportV1>>,
    observation: watch::Receiver<RuntimeInteractionEffectRecoverySupervisorObservationV1>,
}

impl RuntimeInteractionEffectRecoverySupervisorV1 {
    pub(crate) fn start<P>(
        port: P,
        config: RuntimeInteractionEffectRecoverySupervisorConfigV1,
    ) -> Self
    where
        P: RuntimeInteractionEffectRecoverySupervisorPortV1,
    {
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let (observation_sender, observation) =
            watch::channel(RuntimeInteractionEffectRecoverySupervisorObservationV1::default());
        let task = tokio::spawn(run_runtime_interaction_effect_recovery_supervisor_v1(
            Arc::new(port),
            config,
            shutdown_receiver,
            observation_sender,
        ));
        Self {
            shutdown: Some(shutdown),
            task: Some(task),
            observation,
        }
    }

    pub(crate) fn health_v1(&self) -> RuntimeInteractionEffectRecoverySupervisorHealthV1 {
        let observation = *self.observation.borrow();
        RuntimeInteractionEffectRecoverySupervisorHealthV1 {
            running: self.task.as_ref().is_some_and(|task| !task.is_finished()),
            progress: observation.progress,
            consecutive_failed_sweeps: observation.consecutive_failed_sweeps,
        }
    }

    pub(crate) async fn shutdown_until(
        mut self,
        deadline: Instant,
    ) -> RuntimeInteractionEffectRecoverySupervisorReportV1 {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let Some(mut task) = self.task.take() else {
            return supervisor_report_v1(
                RuntimeInteractionEffectRecoverySupervisorExitV1::Panicked,
                RuntimeInteractionEffectRecoverySupervisorProgressV1::default(),
            );
        };
        match timeout_at(TokioInstant::from_std(deadline), &mut task).await {
            Ok(Ok(report)) => report,
            Ok(Err(_)) => supervisor_report_v1(
                RuntimeInteractionEffectRecoverySupervisorExitV1::Panicked,
                RuntimeInteractionEffectRecoverySupervisorProgressV1::default(),
            ),
            Err(_) => {
                task.abort();
                let _ = task.await;
                supervisor_report_v1(
                    RuntimeInteractionEffectRecoverySupervisorExitV1::DeadlineElapsed,
                    RuntimeInteractionEffectRecoverySupervisorProgressV1::default(),
                )
            }
        }
    }
}

impl Drop for RuntimeInteractionEffectRecoverySupervisorV1 {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Debug for RuntimeInteractionEffectRecoverySupervisorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoverySupervisorV1(<redacted>)")
    }
}

enum RuntimeInteractionEffectRecoverySweepV1 {
    Completed,
    Exit(RuntimeInteractionEffectRecoverySupervisorExitV1),
}

enum RuntimeInteractionEffectRecoveryCandidateTaskV1 {
    Completed(RuntimeInteractionEffectRecoveryDispositionV1),
    Failed,
    TimedOut,
}

async fn run_runtime_interaction_effect_recovery_supervisor_v1<P>(
    port: Arc<P>,
    config: RuntimeInteractionEffectRecoverySupervisorConfigV1,
    mut shutdown: oneshot::Receiver<()>,
    observation: watch::Sender<RuntimeInteractionEffectRecoverySupervisorObservationV1>,
) -> RuntimeInteractionEffectRecoverySupervisorReportV1
where
    P: RuntimeInteractionEffectRecoverySupervisorPortV1,
{
    let mut progress = RuntimeInteractionEffectRecoverySupervisorProgressV1::default();
    let mut consecutive_failed_sweeps = 0u64;
    loop {
        let scans_failed_before = progress.scans_failed;
        let candidates_failed_before = progress.candidates_failed;
        increment_v1(&mut progress.sweeps_started);
        publish_observation_v1(&observation, progress, consecutive_failed_sweeps);
        match run_runtime_interaction_effect_recovery_sweep_v1(
            Arc::clone(&port),
            config,
            &mut shutdown,
            &mut progress,
            &observation,
            consecutive_failed_sweeps,
        )
        .await
        {
            RuntimeInteractionEffectRecoverySweepV1::Completed => {
                increment_v1(&mut progress.sweeps_completed);
                if progress.scans_failed > scans_failed_before
                    || progress.candidates_failed > candidates_failed_before
                {
                    consecutive_failed_sweeps = consecutive_failed_sweeps.saturating_add(1);
                } else {
                    consecutive_failed_sweeps = 0;
                }
                publish_observation_v1(&observation, progress, consecutive_failed_sweeps);
            }
            RuntimeInteractionEffectRecoverySweepV1::Exit(exit) => {
                publish_observation_v1(&observation, progress, consecutive_failed_sweeps);
                return supervisor_report_v1(exit, progress);
            }
        }
        tokio::select! {
            biased;
            command = &mut shutdown => {
                return supervisor_report_v1(shutdown_exit_v1(command), progress);
            }
            () = sleep(config.cadence) => {}
        }
    }
}

async fn run_runtime_interaction_effect_recovery_sweep_v1<P>(
    port: Arc<P>,
    config: RuntimeInteractionEffectRecoverySupervisorConfigV1,
    shutdown: &mut oneshot::Receiver<()>,
    progress: &mut RuntimeInteractionEffectRecoverySupervisorProgressV1,
    observation: &watch::Sender<RuntimeInteractionEffectRecoverySupervisorObservationV1>,
    consecutive_failed_sweeps: u64,
) -> RuntimeInteractionEffectRecoverySweepV1
where
    P: RuntimeInteractionEffectRecoverySupervisorPortV1,
{
    let mut cursor = None;
    for page_index in 0..config.max_pages_per_sweep.get() {
        let scan = timeout(
            config.scan_timeout,
            Arc::clone(&port).scan_recoverable_v1(RuntimeInteractionEffectRecoveryScanRequestV1 {
                cursor,
                limit: config.page_limit,
            }),
        );
        tokio::pin!(scan);
        let page = tokio::select! {
            biased;
            command = &mut *shutdown => {
                return RuntimeInteractionEffectRecoverySweepV1::Exit(shutdown_exit_v1(command));
            }
            result = &mut scan => result,
        };
        let page = match page {
            Ok(Ok(page)) => {
                increment_v1(&mut progress.scans_succeeded);
                publish_observation_v1(observation, *progress, consecutive_failed_sweeps);
                page
            }
            Ok(Err(_)) => {
                increment_v1(&mut progress.scans_failed);
                publish_observation_v1(observation, *progress, consecutive_failed_sweeps);
                return RuntimeInteractionEffectRecoverySweepV1::Completed;
            }
            Err(_) => {
                increment_v1(&mut progress.scans_failed);
                increment_v1(&mut progress.scans_timed_out);
                publish_observation_v1(observation, *progress, consecutive_failed_sweeps);
                return RuntimeInteractionEffectRecoverySweepV1::Completed;
            }
        };
        let (candidates, next_cursor, exhausted) = page.into_parts();
        if candidates.len() > config.page_limit.get()
            || candidates.is_empty() && !exhausted
            || !exhausted && next_cursor.is_none()
            || exhausted && next_cursor.is_some()
        {
            increment_v1(&mut progress.scans_failed);
            increment_v1(&mut progress.scan_protocol_violations);
            publish_observation_v1(observation, *progress, consecutive_failed_sweeps);
            return RuntimeInteractionEffectRecoverySweepV1::Completed;
        }
        increment_v1(&mut progress.pages);
        progress.candidates = progress
            .candidates
            .saturating_add(u64::try_from(candidates.len()).unwrap_or(u64::MAX));
        publish_observation_v1(observation, *progress, consecutive_failed_sweeps);
        match run_runtime_interaction_effect_recovery_candidates_v1(
            Arc::clone(&port),
            candidates,
            config,
            shutdown,
            progress,
            observation,
            consecutive_failed_sweeps,
        )
        .await
        {
            RuntimeInteractionEffectRecoverySweepV1::Completed => {}
            exit => return exit,
        }
        if exhausted {
            return RuntimeInteractionEffectRecoverySweepV1::Completed;
        }
        cursor = next_cursor;
        if page_index + 1 == config.max_pages_per_sweep.get() {
            increment_v1(&mut progress.page_budget_exhausted);
            publish_observation_v1(observation, *progress, consecutive_failed_sweeps);
        }
    }
    RuntimeInteractionEffectRecoverySweepV1::Completed
}

async fn run_runtime_interaction_effect_recovery_candidates_v1<P>(
    port: Arc<P>,
    candidates: Vec<P::Candidate>,
    config: RuntimeInteractionEffectRecoverySupervisorConfigV1,
    shutdown: &mut oneshot::Receiver<()>,
    progress: &mut RuntimeInteractionEffectRecoverySupervisorProgressV1,
    observation: &watch::Sender<RuntimeInteractionEffectRecoverySupervisorObservationV1>,
    consecutive_failed_sweeps: u64,
) -> RuntimeInteractionEffectRecoverySweepV1
where
    P: RuntimeInteractionEffectRecoverySupervisorPortV1,
{
    let mut pending = VecDeque::from(candidates);
    let mut in_flight = JoinSet::new();
    loop {
        while in_flight.len() < config.max_concurrency.get() {
            let Some(candidate) = pending.pop_front() else {
                break;
            };
            increment_v1(&mut progress.candidates_attempted);
            publish_observation_v1(observation, *progress, consecutive_failed_sweeps);
            let candidate_port = Arc::clone(&port);
            let candidate_timeout = config.candidate_timeout;
            in_flight.spawn(async move {
                match timeout(
                    candidate_timeout,
                    candidate_port.recover_candidate_v1(candidate),
                )
                .await
                {
                    Ok(Ok(disposition)) => {
                        RuntimeInteractionEffectRecoveryCandidateTaskV1::Completed(disposition)
                    }
                    Ok(Err(_)) => RuntimeInteractionEffectRecoveryCandidateTaskV1::Failed,
                    Err(_) => RuntimeInteractionEffectRecoveryCandidateTaskV1::TimedOut,
                }
            });
        }
        if in_flight.is_empty() {
            return RuntimeInteractionEffectRecoverySweepV1::Completed;
        }
        let joined = tokio::select! {
            biased;
            command = &mut *shutdown => {
                in_flight.abort_all();
                while in_flight.join_next().await.is_some() {}
                return RuntimeInteractionEffectRecoverySweepV1::Exit(shutdown_exit_v1(command));
            }
            joined = in_flight.join_next() => joined,
        };
        match joined {
            Some(Ok(RuntimeInteractionEffectRecoveryCandidateTaskV1::Completed(
                RuntimeInteractionEffectRecoveryDispositionV1::Reconciled,
            ))) => increment_v1(&mut progress.candidates_reconciled),
            Some(Ok(RuntimeInteractionEffectRecoveryCandidateTaskV1::Completed(
                RuntimeInteractionEffectRecoveryDispositionV1::Compensated,
            ))) => increment_v1(&mut progress.candidates_compensated),
            Some(Ok(RuntimeInteractionEffectRecoveryCandidateTaskV1::Completed(
                RuntimeInteractionEffectRecoveryDispositionV1::Deferred,
            ))) => increment_v1(&mut progress.candidates_deferred),
            Some(Ok(RuntimeInteractionEffectRecoveryCandidateTaskV1::Completed(
                RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked,
            ))) => increment_v1(&mut progress.routes_blocked),
            Some(Ok(RuntimeInteractionEffectRecoveryCandidateTaskV1::Failed)) => {
                increment_v1(&mut progress.candidates_failed);
            }
            Some(Ok(RuntimeInteractionEffectRecoveryCandidateTaskV1::TimedOut)) => {
                increment_v1(&mut progress.candidates_failed);
                increment_v1(&mut progress.candidates_timed_out);
            }
            Some(Err(_)) => {
                increment_v1(&mut progress.candidates_failed);
                increment_v1(&mut progress.candidates_panicked);
            }
            None => return RuntimeInteractionEffectRecoverySweepV1::Completed,
        }
        publish_observation_v1(observation, *progress, consecutive_failed_sweeps);
    }
}

fn shutdown_exit_v1(
    command: Result<(), oneshot::error::RecvError>,
) -> RuntimeInteractionEffectRecoverySupervisorExitV1 {
    if command.is_ok() {
        RuntimeInteractionEffectRecoverySupervisorExitV1::Commanded
    } else {
        RuntimeInteractionEffectRecoverySupervisorExitV1::ControlClosed
    }
}

fn supervisor_report_v1(
    exit: RuntimeInteractionEffectRecoverySupervisorExitV1,
    progress: RuntimeInteractionEffectRecoverySupervisorProgressV1,
) -> RuntimeInteractionEffectRecoverySupervisorReportV1 {
    RuntimeInteractionEffectRecoverySupervisorReportV1 { exit, progress }
}

fn increment_v1(value: &mut u64) {
    *value = value.saturating_add(1);
}

fn publish_observation_v1(
    observation: &watch::Sender<RuntimeInteractionEffectRecoverySupervisorObservationV1>,
    progress: RuntimeInteractionEffectRecoverySupervisorProgressV1,
    consecutive_failed_sweeps: u64,
) {
    observation.send_replace(RuntimeInteractionEffectRecoverySupervisorObservationV1 {
        progress,
        consecutive_failed_sweeps,
    });
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::pending;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use tokio::time::sleep;

    use super::*;

    enum FakeScanV1 {
        Page(RuntimeInteractionEffectRecoveryScanPageV1<usize, usize>),
        Failed,
        Pending(Arc<AtomicBool>),
    }

    enum FakeCandidateV1 {
        Disposition(RuntimeInteractionEffectRecoveryDispositionV1),
        Failed,
        Delayed(RuntimeInteractionEffectRecoveryDispositionV1, Duration),
        Pending(Arc<AtomicBool>),
        Panic,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct FakeErrorV1;

    struct PendingDropV1(Arc<AtomicBool>);

    impl Drop for PendingDropV1 {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    struct InFlightDropV1 {
        current: Arc<AtomicUsize>,
    }

    impl Drop for InFlightDropV1 {
        fn drop(&mut self) {
            self.current.fetch_sub(1, Ordering::AcqRel);
        }
    }

    struct FakePortV1 {
        scans: Mutex<VecDeque<FakeScanV1>>,
        candidates: Mutex<VecDeque<FakeCandidateV1>>,
        scan_requests: Mutex<Vec<(Option<usize>, usize)>>,
        scan_calls: AtomicUsize,
        candidate_calls: AtomicUsize,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: AtomicUsize,
    }

    impl FakePortV1 {
        fn new(scans: Vec<FakeScanV1>, candidates: Vec<FakeCandidateV1>) -> Self {
            Self {
                scans: Mutex::new(scans.into()),
                candidates: Mutex::new(candidates.into()),
                scan_requests: Mutex::new(Vec::new()),
                scan_calls: AtomicUsize::new(0),
                candidate_calls: AtomicUsize::new(0),
                in_flight: Arc::new(AtomicUsize::new(0)),
                max_in_flight: AtomicUsize::new(0),
            }
        }
    }

    impl RuntimeInteractionEffectRecoverySupervisorPortV1 for Arc<FakePortV1> {
        type Cursor = usize;
        type Candidate = usize;
        type Error = FakeErrorV1;

        fn scan_recoverable_v1(
            self: Arc<Self>,
            request: RuntimeInteractionEffectRecoveryScanRequestV1<Self::Cursor>,
        ) -> RuntimeInteractionEffectRecoveryScanFutureV1<Self::Cursor, Self::Candidate, Self::Error>
        {
            let port = Arc::clone(self.as_ref());
            Box::pin(async move {
                let (cursor, limit) = request.into_parts();
                port.scan_requests
                    .lock()
                    .unwrap()
                    .push((cursor, limit.get()));
                port.scan_calls.fetch_add(1, Ordering::AcqRel);
                let scan = port.scans.lock().unwrap().pop_front();
                match scan {
                    Some(FakeScanV1::Page(page)) => Ok(page),
                    Some(FakeScanV1::Failed) => Err(FakeErrorV1),
                    Some(FakeScanV1::Pending(dropped)) => {
                        let _drop = PendingDropV1(dropped);
                        pending().await
                    }
                    None => empty_page_v1(),
                }
            })
        }

        fn recover_candidate_v1(
            self: Arc<Self>,
            _candidate: Self::Candidate,
        ) -> RuntimeInteractionEffectRecoveryCandidateFutureV1<Self::Error> {
            let port = Arc::clone(self.as_ref());
            Box::pin(async move {
                port.candidate_calls.fetch_add(1, Ordering::AcqRel);
                let current = port.in_flight.fetch_add(1, Ordering::AcqRel) + 1;
                port.max_in_flight.fetch_max(current, Ordering::AcqRel);
                let _in_flight = InFlightDropV1 {
                    current: Arc::clone(&port.in_flight),
                };
                let candidate = port.candidates.lock().unwrap().pop_front().unwrap_or(
                    FakeCandidateV1::Disposition(
                        RuntimeInteractionEffectRecoveryDispositionV1::Reconciled,
                    ),
                );
                match candidate {
                    FakeCandidateV1::Disposition(disposition) => Ok(disposition),
                    FakeCandidateV1::Failed => Err(FakeErrorV1),
                    FakeCandidateV1::Delayed(disposition, delay) => {
                        sleep(delay).await;
                        Ok(disposition)
                    }
                    FakeCandidateV1::Pending(dropped) => {
                        let _drop = PendingDropV1(dropped);
                        pending().await
                    }
                    FakeCandidateV1::Panic => panic!("candidate panic"),
                }
            })
        }
    }

    fn page_v1(
        candidates: Vec<usize>,
        next_cursor: Option<usize>,
        exhausted: bool,
    ) -> RuntimeInteractionEffectRecoveryScanPageV1<usize, usize> {
        RuntimeInteractionEffectRecoveryScanPageV1::new(candidates, next_cursor, exhausted).unwrap()
    }

    fn empty_page_v1(
    ) -> Result<RuntimeInteractionEffectRecoveryScanPageV1<usize, usize>, FakeErrorV1> {
        Ok(RuntimeInteractionEffectRecoveryScanPageV1::new(Vec::new(), None, true).unwrap())
    }

    fn config_v1(
        cadence: Duration,
        page_limit: usize,
        max_pages: usize,
        concurrency: usize,
        scan_timeout: Duration,
        candidate_timeout: Duration,
    ) -> RuntimeInteractionEffectRecoverySupervisorConfigV1 {
        RuntimeInteractionEffectRecoverySupervisorConfigV1::for_test_v1(
            cadence,
            page_limit,
            max_pages,
            concurrency,
            scan_timeout,
            candidate_timeout,
        )
    }

    async fn wait_for_v1(predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !predicate() {
            assert!(Instant::now() < deadline);
            sleep(Duration::from_millis(1)).await;
        }
    }

    #[test]
    fn page_shape_is_fail_closed() {
        assert!(
            RuntimeInteractionEffectRecoveryScanPageV1::<usize, usize>::new(
                Vec::new(),
                Some(1),
                false
            )
            .is_err()
        );
        assert!(
            RuntimeInteractionEffectRecoveryScanPageV1::<usize, usize>::new(vec![1], None, false)
                .is_err()
        );
        assert!(
            RuntimeInteractionEffectRecoveryScanPageV1::<usize, usize>::new(vec![1], Some(1), true)
                .is_err()
        );
        assert!(
            RuntimeInteractionEffectRecoveryScanPageV1::<usize, usize>::new(Vec::new(), None, true)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn pages_are_immediate_ordered_and_candidates_are_concurrency_bounded() {
        let port = Arc::new(FakePortV1::new(
            vec![
                FakeScanV1::Page(page_v1(vec![1, 2, 3, 4], Some(4), false)),
                FakeScanV1::Page(page_v1(vec![5, 6], None, true)),
            ],
            (0..6)
                .map(|_| {
                    FakeCandidateV1::Delayed(
                        RuntimeInteractionEffectRecoveryDispositionV1::Reconciled,
                        Duration::from_millis(20),
                    )
                })
                .collect(),
        ));
        let supervisor = RuntimeInteractionEffectRecoverySupervisorV1::start(
            Arc::clone(&port),
            config_v1(
                Duration::from_secs(60),
                4,
                4,
                2,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
        );
        wait_for_v1(|| port.candidate_calls.load(Ordering::Acquire) == 6).await;
        wait_for_v1(|| port.in_flight.load(Ordering::Acquire) == 0).await;
        assert_eq!(port.max_in_flight.load(Ordering::Acquire), 2);
        assert_eq!(
            *port.scan_requests.lock().unwrap(),
            vec![(None, 4), (Some(4), 4)]
        );
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            report.exit(),
            RuntimeInteractionEffectRecoverySupervisorExitV1::Commanded
        );
        assert_eq!(report.progress().pages, 2);
        assert_eq!(report.progress().candidates_reconciled, 6);
        assert_eq!(report.progress().sweeps_completed, 1);
    }

    #[tokio::test]
    async fn all_dispositions_failures_timeouts_and_panics_are_counted() {
        let timed_out = Arc::new(AtomicBool::new(false));
        let port = Arc::new(FakePortV1::new(
            vec![FakeScanV1::Page(page_v1(
                vec![1, 2, 3, 4, 5, 6, 7],
                None,
                true,
            ))],
            vec![
                FakeCandidateV1::Disposition(
                    RuntimeInteractionEffectRecoveryDispositionV1::Reconciled,
                ),
                FakeCandidateV1::Disposition(
                    RuntimeInteractionEffectRecoveryDispositionV1::Compensated,
                ),
                FakeCandidateV1::Disposition(
                    RuntimeInteractionEffectRecoveryDispositionV1::Deferred,
                ),
                FakeCandidateV1::Disposition(
                    RuntimeInteractionEffectRecoveryDispositionV1::RouteBlocked,
                ),
                FakeCandidateV1::Failed,
                FakeCandidateV1::Pending(Arc::clone(&timed_out)),
                FakeCandidateV1::Panic,
            ],
        ));
        let supervisor = RuntimeInteractionEffectRecoverySupervisorV1::start(
            Arc::clone(&port),
            config_v1(
                Duration::from_secs(60),
                7,
                1,
                7,
                Duration::from_secs(1),
                Duration::from_millis(20),
            ),
        );
        wait_for_v1(|| timed_out.load(Ordering::Acquire)).await;
        wait_for_v1(|| port.in_flight.load(Ordering::Acquire) == 0).await;
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            report.exit(),
            RuntimeInteractionEffectRecoverySupervisorExitV1::Commanded
        );
        let progress = report.progress();
        assert_eq!(progress.candidates_reconciled, 1);
        assert_eq!(progress.candidates_compensated, 1);
        assert_eq!(progress.candidates_deferred, 1);
        assert_eq!(progress.routes_blocked, 1);
        assert_eq!(progress.candidates_failed, 3);
        assert_eq!(progress.candidates_timed_out, 1);
        assert_eq!(progress.candidates_panicked, 1);
    }

    #[tokio::test]
    async fn scan_failure_and_timeout_wait_for_cadence() {
        let failed = Arc::new(FakePortV1::new(vec![FakeScanV1::Failed], Vec::new()));
        let supervisor = RuntimeInteractionEffectRecoverySupervisorV1::start(
            Arc::clone(&failed),
            config_v1(
                Duration::from_millis(100),
                4,
                1,
                1,
                Duration::from_millis(20),
                Duration::from_millis(20),
            ),
        );
        wait_for_v1(|| failed.scan_calls.load(Ordering::Acquire) == 1).await;
        sleep(Duration::from_millis(30)).await;
        assert_eq!(failed.scan_calls.load(Ordering::Acquire), 1);
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            report.exit(),
            RuntimeInteractionEffectRecoverySupervisorExitV1::Commanded
        );
        assert_eq!(report.progress().scans_failed, 1);

        let dropped = Arc::new(AtomicBool::new(false));
        let pending_port = Arc::new(FakePortV1::new(
            vec![FakeScanV1::Pending(Arc::clone(&dropped))],
            Vec::new(),
        ));
        let pending_supervisor = RuntimeInteractionEffectRecoverySupervisorV1::start(
            Arc::clone(&pending_port),
            config_v1(
                Duration::from_millis(100),
                4,
                1,
                1,
                Duration::from_millis(20),
                Duration::from_millis(20),
            ),
        );
        wait_for_v1(|| dropped.load(Ordering::Acquire)).await;
        sleep(Duration::from_millis(30)).await;
        assert_eq!(pending_port.scan_calls.load(Ordering::Acquire), 1);
        let pending_report = pending_supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            pending_report.exit(),
            RuntimeInteractionEffectRecoverySupervisorExitV1::Commanded
        );
        assert_eq!(pending_report.progress().scans_failed, 1);
        assert_eq!(pending_report.progress().scans_timed_out, 1);
    }

    #[tokio::test]
    async fn health_reports_live_progress_and_fails_after_persistent_sweep_failures() {
        let port = Arc::new(FakePortV1::new(
            (0..16).map(|_| FakeScanV1::Failed).collect(),
            Vec::new(),
        ));
        let supervisor = RuntimeInteractionEffectRecoverySupervisorV1::start(
            Arc::clone(&port),
            config_v1(
                Duration::from_millis(10),
                4,
                1,
                1,
                Duration::from_millis(20),
                Duration::from_millis(20),
            ),
        );
        wait_for_v1(|| supervisor.health_v1().progress_v1().sweeps_completed >= 1).await;
        let transient = supervisor.health_v1();
        assert!(transient.is_ready_v1());
        assert_eq!(transient.consecutive_failed_sweeps_v1(), 1);
        wait_for_v1(|| {
            supervisor.health_v1().consecutive_failed_sweeps_v1()
                >= RUNTIME_INTERACTION_EFFECT_RECOVERY_FAILED_SWEEP_LIMIT_V1
        })
        .await;
        let persistent = supervisor.health_v1();
        assert!(!persistent.is_ready_v1());
        assert!(persistent.progress_v1().scans_failed >= 3);
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            report.exit(),
            RuntimeInteractionEffectRecoverySupervisorExitV1::Commanded
        );
    }

    #[tokio::test]
    async fn health_fails_closed_when_the_supervisor_task_stops() {
        let port = Arc::new(FakePortV1::new(Vec::new(), Vec::new()));
        let supervisor = RuntimeInteractionEffectRecoverySupervisorV1::start(
            Arc::clone(&port),
            config_v1(
                Duration::from_secs(60),
                4,
                1,
                1,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
        );
        assert!(supervisor.health_v1().is_ready_v1());
        supervisor.task.as_ref().unwrap().abort();
        wait_for_v1(|| !supervisor.health_v1().is_ready_v1()).await;
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            report.exit(),
            RuntimeInteractionEffectRecoverySupervisorExitV1::Panicked
        );
    }

    #[tokio::test]
    async fn page_budget_bounds_each_sweep() {
        let port = Arc::new(FakePortV1::new(
            vec![
                FakeScanV1::Page(page_v1(vec![1], Some(1), false)),
                FakeScanV1::Page(page_v1(vec![2], Some(2), false)),
                FakeScanV1::Page(page_v1(vec![3], None, true)),
            ],
            Vec::new(),
        ));
        let supervisor = RuntimeInteractionEffectRecoverySupervisorV1::start(
            Arc::clone(&port),
            config_v1(
                Duration::from_secs(60),
                1,
                2,
                1,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
        );
        wait_for_v1(|| port.candidate_calls.load(Ordering::Acquire) == 2).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(port.scan_calls.load(Ordering::Acquire), 2);
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(report.progress().page_budget_exhausted, 1);
    }

    #[tokio::test]
    async fn shutdown_cancels_and_joins_all_in_flight_candidates() {
        let first = Arc::new(AtomicBool::new(false));
        let second = Arc::new(AtomicBool::new(false));
        let port = Arc::new(FakePortV1::new(
            vec![FakeScanV1::Page(page_v1(vec![1, 2, 3], None, true))],
            vec![
                FakeCandidateV1::Pending(Arc::clone(&first)),
                FakeCandidateV1::Pending(Arc::clone(&second)),
            ],
        ));
        let supervisor = RuntimeInteractionEffectRecoverySupervisorV1::start(
            Arc::clone(&port),
            config_v1(
                Duration::from_secs(60),
                3,
                1,
                2,
                Duration::from_secs(1),
                Duration::from_secs(60),
            ),
        );
        wait_for_v1(|| port.candidate_calls.load(Ordering::Acquire) == 2).await;
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            report.exit(),
            RuntimeInteractionEffectRecoverySupervisorExitV1::Commanded
        );
        assert!(first.load(Ordering::Acquire));
        assert!(second.load(Ordering::Acquire));
        assert_eq!(port.in_flight.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn shutdown_cancels_a_pending_scan() {
        let dropped = Arc::new(AtomicBool::new(false));
        let port = Arc::new(FakePortV1::new(
            vec![FakeScanV1::Pending(Arc::clone(&dropped))],
            Vec::new(),
        ));
        let supervisor = RuntimeInteractionEffectRecoverySupervisorV1::start(
            Arc::clone(&port),
            config_v1(
                Duration::from_secs(60),
                4,
                1,
                1,
                Duration::from_secs(60),
                Duration::from_secs(60),
            ),
        );
        wait_for_v1(|| port.scan_calls.load(Ordering::Acquire) == 1).await;
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            report.exit(),
            RuntimeInteractionEffectRecoverySupervisorExitV1::Commanded
        );
        assert!(dropped.load(Ordering::Acquire));
    }

    #[test]
    fn production_bounds_are_exact() {
        let config = RuntimeInteractionEffectRecoverySupervisorConfigV1::production_v1();
        assert_eq!(config.cadence, Duration::from_secs(15));
        assert_eq!(config.page_limit.get(), 64);
        assert_eq!(config.max_pages_per_sweep.get(), 16);
        assert_eq!(config.max_concurrency.get(), 8);
        assert_eq!(config.scan_timeout, Duration::from_secs(5));
        assert_eq!(config.candidate_timeout, Duration::from_secs(10));
    }
}
