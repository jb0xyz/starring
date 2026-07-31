use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use automation_runtime_interaction::{InteractionReceiptIdentityV1, InteractionReceiptStateV1};
use chrono::{DateTime, Utc};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, timeout_at, Instant as TokioInstant};

const RUNTIME_INTERACTION_RECEIPT_RECOVERY_CADENCE_V1: Duration = Duration::from_secs(15);
const RUNTIME_INTERACTION_RECEIPT_RECOVERY_PAGE_LIMIT_V1: usize = 64;
const RUNTIME_INTERACTION_RECEIPT_RECOVERY_SCAN_TIMEOUT_V1: Duration = Duration::from_secs(5);
const RUNTIME_INTERACTION_RECEIPT_RECOVERY_MUTATION_TIMEOUT_V1: Duration = Duration::from_secs(5);

pub(crate) type RuntimeInteractionReceiptRecoveryScanFutureV1<C, E> = Pin<
    Box<
        dyn Future<Output = Result<RuntimeInteractionReceiptRecoveryScanPageV1<C>, E>>
            + Send
            + 'static,
    >,
>;

pub(crate) type RuntimeInteractionReceiptRecoveryMutationFutureV1<E> = Pin<
    Box<
        dyn Future<Output = Result<RuntimeInteractionReceiptRecoveryMutationDispositionV1, E>>
            + Send
            + 'static,
    >,
>;

#[derive(Clone)]
pub(crate) struct RuntimeInteractionReceiptRecoveryCandidateV1 {
    identity: InteractionReceiptIdentityV1,
    head_revision: u64,
    claim_revision: u64,
    claim_expires_at: DateTime<Utc>,
    token_expires_at: Option<DateTime<Utc>>,
}

impl RuntimeInteractionReceiptRecoveryCandidateV1 {
    pub(crate) fn new(
        identity: InteractionReceiptIdentityV1,
        state: InteractionReceiptStateV1,
        head_revision: u64,
        claim_revision: u64,
        claim_expires_at: DateTime<Utc>,
        token_expires_at: Option<DateTime<Utc>>,
    ) -> Result<Self, RuntimeInteractionReceiptRecoveryCandidateErrorV1> {
        if !state.is_in_flight()
            || head_revision == 0
            || claim_revision == 0
            || claim_revision > head_revision
            || head_revision >= i64::MAX as u64
            || claim_revision > i64::MAX as u64
        {
            return Err(RuntimeInteractionReceiptRecoveryCandidateErrorV1::Invalid);
        }
        Ok(Self {
            identity,
            head_revision,
            claim_revision,
            claim_expires_at,
            token_expires_at,
        })
    }

    pub(crate) const fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub(crate) const fn head_revision(&self) -> u64 {
        self.head_revision
    }

    pub(crate) const fn claim_revision(&self) -> u64 {
        self.claim_revision
    }

    pub(crate) const fn claim_expires_at(&self) -> DateTime<Utc> {
        self.claim_expires_at
    }

    pub(crate) const fn token_expires_at(&self) -> Option<DateTime<Utc>> {
        self.token_expires_at
    }
}

impl Debug for RuntimeInteractionReceiptRecoveryCandidateV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveryCandidateV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeInteractionReceiptRecoveryCandidateErrorV1 {
    #[error("runtime interaction receipt recovery candidate is invalid")]
    Invalid,
}

pub(crate) struct RuntimeInteractionReceiptRecoveryScanRequestV1<C> {
    cursor: Option<C>,
    limit: NonZeroUsize,
}

impl<C> RuntimeInteractionReceiptRecoveryScanRequestV1<C> {
    pub(crate) fn into_parts(self) -> (Option<C>, NonZeroUsize) {
        (self.cursor, self.limit)
    }
}

impl<C> Debug for RuntimeInteractionReceiptRecoveryScanRequestV1<C> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveryScanRequestV1(<redacted>)")
    }
}

pub(crate) struct RuntimeInteractionReceiptRecoveryScanPageV1<C> {
    candidates: Vec<RuntimeInteractionReceiptRecoveryCandidateV1>,
    next_cursor: Option<C>,
    exhausted: bool,
    observed_database_now: Option<DateTime<Utc>>,
}

impl<C> RuntimeInteractionReceiptRecoveryScanPageV1<C> {
    pub(crate) fn new(
        candidates: Vec<RuntimeInteractionReceiptRecoveryCandidateV1>,
        next_cursor: Option<C>,
        exhausted: bool,
        observed_database_now: Option<DateTime<Utc>>,
    ) -> Result<Self, RuntimeInteractionReceiptRecoveryScanPageErrorV1> {
        if candidates.is_empty() && !exhausted
            || !exhausted && next_cursor.is_none()
            || candidates.is_empty() && observed_database_now.is_some()
            || !candidates.is_empty() && observed_database_now.is_none()
            || observed_database_now.is_some_and(|observed| {
                candidates
                    .iter()
                    .any(|candidate| candidate.claim_expires_at() > observed)
            })
        {
            return Err(RuntimeInteractionReceiptRecoveryScanPageErrorV1::Invalid);
        }
        Ok(Self {
            candidates,
            next_cursor,
            exhausted,
            observed_database_now,
        })
    }

    fn into_parts(
        self,
    ) -> (
        Vec<RuntimeInteractionReceiptRecoveryCandidateV1>,
        Option<C>,
        bool,
        Option<DateTime<Utc>>,
    ) {
        (
            self.candidates,
            self.next_cursor,
            self.exhausted,
            self.observed_database_now,
        )
    }
}

impl<C> Debug for RuntimeInteractionReceiptRecoveryScanPageV1<C> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveryScanPageV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeInteractionReceiptRecoveryScanPageErrorV1 {
    #[error("runtime interaction receipt recovery scan page is invalid")]
    Invalid,
}

pub(crate) struct RuntimeInteractionReceiptRecoveryMutationRequestV1 {
    candidate: RuntimeInteractionReceiptRecoveryCandidateV1,
}

impl RuntimeInteractionReceiptRecoveryMutationRequestV1 {
    pub(crate) fn into_candidate(self) -> RuntimeInteractionReceiptRecoveryCandidateV1 {
        self.candidate
    }
}

impl Debug for RuntimeInteractionReceiptRecoveryMutationRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveryMutationRequestV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionReceiptRecoveryMutationDispositionV1 {
    RecoveryRequired,
    Converged,
    Deferred,
}

pub(crate) trait RuntimeInteractionReceiptRecoverySupervisorPortV1:
    Send + Sync + 'static
{
    type Cursor: Send + 'static;
    type Error: Send + 'static;

    fn scan_recoverable_v1(
        self: Arc<Self>,
        request: RuntimeInteractionReceiptRecoveryScanRequestV1<Self::Cursor>,
    ) -> RuntimeInteractionReceiptRecoveryScanFutureV1<Self::Cursor, Self::Error>;

    fn expire_token_v1(
        self: Arc<Self>,
        request: RuntimeInteractionReceiptRecoveryMutationRequestV1,
    ) -> RuntimeInteractionReceiptRecoveryMutationFutureV1<Self::Error>;

    fn terminalize_expired_v1(
        self: Arc<Self>,
        request: RuntimeInteractionReceiptRecoveryMutationRequestV1,
    ) -> RuntimeInteractionReceiptRecoveryMutationFutureV1<Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeInteractionReceiptRecoverySupervisorConfigV1 {
    cadence: Duration,
    page_limit: NonZeroUsize,
    scan_timeout: Duration,
    mutation_timeout: Duration,
}

impl RuntimeInteractionReceiptRecoverySupervisorConfigV1 {
    pub(crate) fn production_v1() -> Self {
        Self {
            cadence: RUNTIME_INTERACTION_RECEIPT_RECOVERY_CADENCE_V1,
            page_limit: NonZeroUsize::new(RUNTIME_INTERACTION_RECEIPT_RECOVERY_PAGE_LIMIT_V1)
                .expect("runtime interaction receipt recovery page limit is non-zero"),
            scan_timeout: RUNTIME_INTERACTION_RECEIPT_RECOVERY_SCAN_TIMEOUT_V1,
            mutation_timeout: RUNTIME_INTERACTION_RECEIPT_RECOVERY_MUTATION_TIMEOUT_V1,
        }
    }

    #[cfg(test)]
    fn for_test_v1(cadence: Duration, scan_timeout: Duration, mutation_timeout: Duration) -> Self {
        Self {
            cadence,
            page_limit: NonZeroUsize::new(64).unwrap(),
            scan_timeout,
            mutation_timeout,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeInteractionReceiptRecoverySupervisorExitV1 {
    Commanded,
    ControlClosed,
    Panicked,
    DeadlineElapsed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuntimeInteractionReceiptRecoverySupervisorProgressV1 {
    pub(crate) scans_succeeded: u64,
    pub(crate) scans_failed: u64,
    pub(crate) scans_timed_out: u64,
    pub(crate) candidates: u64,
    pub(crate) token_expiry_attempted: u64,
    pub(crate) terminalization_attempted: u64,
    pub(crate) recovery_required: u64,
    pub(crate) converged: u64,
    pub(crate) deferred: u64,
    pub(crate) mutations_failed: u64,
    pub(crate) mutations_timed_out: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeInteractionReceiptRecoverySupervisorReportV1 {
    exit: RuntimeInteractionReceiptRecoverySupervisorExitV1,
    progress: RuntimeInteractionReceiptRecoverySupervisorProgressV1,
}

impl RuntimeInteractionReceiptRecoverySupervisorReportV1 {
    pub(crate) const fn exit(self) -> RuntimeInteractionReceiptRecoverySupervisorExitV1 {
        self.exit
    }

    #[cfg(test)]
    pub(crate) const fn progress(self) -> RuntimeInteractionReceiptRecoverySupervisorProgressV1 {
        self.progress
    }
}

pub(crate) struct RuntimeInteractionReceiptRecoverySupervisorV1 {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<RuntimeInteractionReceiptRecoverySupervisorReportV1>>,
}

impl RuntimeInteractionReceiptRecoverySupervisorV1 {
    pub(crate) fn start<P>(
        port: P,
        config: RuntimeInteractionReceiptRecoverySupervisorConfigV1,
    ) -> Self
    where
        P: RuntimeInteractionReceiptRecoverySupervisorPortV1,
    {
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let task = tokio::spawn(run_runtime_interaction_receipt_recovery_supervisor_v1(
            Arc::new(port),
            config,
            shutdown_receiver,
        ));
        Self {
            shutdown: Some(shutdown),
            task: Some(task),
        }
    }

    pub(crate) async fn shutdown_until(
        mut self,
        deadline: Instant,
    ) -> RuntimeInteractionReceiptRecoverySupervisorReportV1 {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let Some(mut task) = self.task.take() else {
            return supervisor_report_v1(
                RuntimeInteractionReceiptRecoverySupervisorExitV1::Panicked,
                RuntimeInteractionReceiptRecoverySupervisorProgressV1::default(),
            );
        };
        match timeout_at(TokioInstant::from_std(deadline), &mut task).await {
            Ok(Ok(report)) => report,
            Ok(Err(_)) => supervisor_report_v1(
                RuntimeInteractionReceiptRecoverySupervisorExitV1::Panicked,
                RuntimeInteractionReceiptRecoverySupervisorProgressV1::default(),
            ),
            Err(_) => {
                task.abort();
                let _ = task.await;
                supervisor_report_v1(
                    RuntimeInteractionReceiptRecoverySupervisorExitV1::DeadlineElapsed,
                    RuntimeInteractionReceiptRecoverySupervisorProgressV1::default(),
                )
            }
        }
    }
}

impl Drop for RuntimeInteractionReceiptRecoverySupervisorV1 {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Debug for RuntimeInteractionReceiptRecoverySupervisorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoverySupervisorV1(<redacted>)")
    }
}

enum RuntimeInteractionReceiptRecoverySweepV1 {
    Completed,
    Exit(RuntimeInteractionReceiptRecoverySupervisorExitV1),
}

enum RuntimeInteractionReceiptRecoveryCandidateActionV1 {
    ExpireToken,
    Terminalize,
}

async fn run_runtime_interaction_receipt_recovery_supervisor_v1<P>(
    port: Arc<P>,
    config: RuntimeInteractionReceiptRecoverySupervisorConfigV1,
    mut shutdown: oneshot::Receiver<()>,
) -> RuntimeInteractionReceiptRecoverySupervisorReportV1
where
    P: RuntimeInteractionReceiptRecoverySupervisorPortV1,
{
    let mut progress = RuntimeInteractionReceiptRecoverySupervisorProgressV1::default();
    loop {
        match run_runtime_interaction_receipt_recovery_sweep_v1(
            Arc::clone(&port),
            config,
            &mut shutdown,
            &mut progress,
        )
        .await
        {
            RuntimeInteractionReceiptRecoverySweepV1::Completed => {}
            RuntimeInteractionReceiptRecoverySweepV1::Exit(exit) => {
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

async fn run_runtime_interaction_receipt_recovery_sweep_v1<P>(
    port: Arc<P>,
    config: RuntimeInteractionReceiptRecoverySupervisorConfigV1,
    shutdown: &mut oneshot::Receiver<()>,
    progress: &mut RuntimeInteractionReceiptRecoverySupervisorProgressV1,
) -> RuntimeInteractionReceiptRecoverySweepV1
where
    P: RuntimeInteractionReceiptRecoverySupervisorPortV1,
{
    let mut cursor = None;
    loop {
        let scan = timeout(
            config.scan_timeout,
            Arc::clone(&port).scan_recoverable_v1(RuntimeInteractionReceiptRecoveryScanRequestV1 {
                cursor,
                limit: config.page_limit,
            }),
        );
        tokio::pin!(scan);
        let page = tokio::select! {
            biased;
            command = &mut *shutdown => {
                return RuntimeInteractionReceiptRecoverySweepV1::Exit(shutdown_exit_v1(command));
            }
            result = &mut scan => result,
        };
        let page = match page {
            Ok(Ok(page)) => {
                increment_v1(&mut progress.scans_succeeded);
                page
            }
            Ok(Err(_)) => {
                increment_v1(&mut progress.scans_failed);
                return RuntimeInteractionReceiptRecoverySweepV1::Completed;
            }
            Err(_) => {
                increment_v1(&mut progress.scans_failed);
                increment_v1(&mut progress.scans_timed_out);
                return RuntimeInteractionReceiptRecoverySweepV1::Completed;
            }
        };
        let (candidates, next_cursor, exhausted, observed_database_now) = page.into_parts();
        progress.candidates = progress
            .candidates
            .saturating_add(u64::try_from(candidates.len()).unwrap_or(u64::MAX));
        let observed_database_now = match observed_database_now {
            Some(observed) => observed,
            None if candidates.is_empty() => {
                return RuntimeInteractionReceiptRecoverySweepV1::Completed;
            }
            None => {
                increment_v1(&mut progress.scans_failed);
                return RuntimeInteractionReceiptRecoverySweepV1::Completed;
            }
        };
        for candidate in candidates {
            let action = classify_recovery_candidate_v1(&candidate, observed_database_now);
            let mutation = match action {
                RuntimeInteractionReceiptRecoveryCandidateActionV1::ExpireToken => {
                    increment_v1(&mut progress.token_expiry_attempted);
                    Arc::clone(&port).expire_token_v1(
                        RuntimeInteractionReceiptRecoveryMutationRequestV1 { candidate },
                    )
                }
                RuntimeInteractionReceiptRecoveryCandidateActionV1::Terminalize => {
                    increment_v1(&mut progress.terminalization_attempted);
                    Arc::clone(&port).terminalize_expired_v1(
                        RuntimeInteractionReceiptRecoveryMutationRequestV1 { candidate },
                    )
                }
            };
            let mutation = timeout(config.mutation_timeout, mutation);
            tokio::pin!(mutation);
            let outcome = tokio::select! {
                biased;
                command = &mut *shutdown => {
                    return RuntimeInteractionReceiptRecoverySweepV1::Exit(
                        shutdown_exit_v1(command),
                    );
                }
                result = &mut mutation => result,
            };
            match outcome {
                Ok(Ok(
                    RuntimeInteractionReceiptRecoveryMutationDispositionV1::RecoveryRequired,
                )) => {
                    increment_v1(&mut progress.recovery_required);
                }
                Ok(Ok(RuntimeInteractionReceiptRecoveryMutationDispositionV1::Converged)) => {
                    increment_v1(&mut progress.converged);
                }
                Ok(Ok(RuntimeInteractionReceiptRecoveryMutationDispositionV1::Deferred)) => {
                    increment_v1(&mut progress.deferred);
                }
                Ok(Err(_)) => increment_v1(&mut progress.mutations_failed),
                Err(_) => {
                    increment_v1(&mut progress.mutations_failed);
                    increment_v1(&mut progress.mutations_timed_out);
                }
            }
        }
        if exhausted {
            return RuntimeInteractionReceiptRecoverySweepV1::Completed;
        }
        let Some(next) = next_cursor else {
            increment_v1(&mut progress.scans_failed);
            return RuntimeInteractionReceiptRecoverySweepV1::Completed;
        };
        cursor = Some(next);
    }
}

fn classify_recovery_candidate_v1(
    candidate: &RuntimeInteractionReceiptRecoveryCandidateV1,
    observed_database_now: DateTime<Utc>,
) -> RuntimeInteractionReceiptRecoveryCandidateActionV1 {
    if candidate
        .token_expires_at()
        .is_none_or(|expires_at| expires_at <= observed_database_now)
    {
        RuntimeInteractionReceiptRecoveryCandidateActionV1::ExpireToken
    } else {
        RuntimeInteractionReceiptRecoveryCandidateActionV1::Terminalize
    }
}

fn shutdown_exit_v1(
    command: Result<(), oneshot::error::RecvError>,
) -> RuntimeInteractionReceiptRecoverySupervisorExitV1 {
    if command.is_ok() {
        RuntimeInteractionReceiptRecoverySupervisorExitV1::Commanded
    } else {
        RuntimeInteractionReceiptRecoverySupervisorExitV1::ControlClosed
    }
}

fn supervisor_report_v1(
    exit: RuntimeInteractionReceiptRecoverySupervisorExitV1,
    progress: RuntimeInteractionReceiptRecoverySupervisorProgressV1,
) -> RuntimeInteractionReceiptRecoverySupervisorReportV1 {
    RuntimeInteractionReceiptRecoverySupervisorReportV1 { exit, progress }
}

fn increment_v1(value: &mut u64) {
    *value = value.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::pending;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use automation_runtime_interaction::{DiscordApplicationIdV1, DiscordInteractionIdV1};
    use chrono::TimeZone;
    use tokio::time::sleep;

    use super::*;

    enum FakeScanV1 {
        Page(RuntimeInteractionReceiptRecoveryScanPageV1<usize>),
        Failed,
        Pending(Arc<AtomicBool>),
    }

    enum FakeMutationV1 {
        Succeed(RuntimeInteractionReceiptRecoveryMutationDispositionV1),
        Failed,
        Pending(Arc<AtomicBool>),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct FakeErrorV1;

    struct PendingDropV1 {
        dropped: Arc<AtomicBool>,
    }

    impl Drop for PendingDropV1 {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    struct FakePortV1 {
        scans: Mutex<VecDeque<FakeScanV1>>,
        scan_requests: Mutex<Vec<(Option<usize>, usize)>>,
        expiry: Mutex<VecDeque<FakeMutationV1>>,
        terminalization: Mutex<VecDeque<FakeMutationV1>>,
        scan_calls: AtomicUsize,
        expiry_calls: AtomicUsize,
        terminalization_calls: AtomicUsize,
    }

    impl FakePortV1 {
        fn new(scans: Vec<FakeScanV1>) -> Self {
            Self {
                scans: Mutex::new(scans.into()),
                scan_requests: Mutex::new(Vec::new()),
                expiry: Mutex::new(VecDeque::new()),
                terminalization: Mutex::new(VecDeque::new()),
                scan_calls: AtomicUsize::new(0),
                expiry_calls: AtomicUsize::new(0),
                terminalization_calls: AtomicUsize::new(0),
            }
        }

        fn with_expiry(self, mutations: Vec<FakeMutationV1>) -> Self {
            *self.expiry.lock().unwrap() = mutations.into();
            self
        }

        fn with_terminalization(self, mutations: Vec<FakeMutationV1>) -> Self {
            *self.terminalization.lock().unwrap() = mutations.into();
            self
        }
    }

    impl RuntimeInteractionReceiptRecoverySupervisorPortV1 for Arc<FakePortV1> {
        type Cursor = usize;
        type Error = FakeErrorV1;

        fn scan_recoverable_v1(
            self: Arc<Self>,
            request: RuntimeInteractionReceiptRecoveryScanRequestV1<Self::Cursor>,
        ) -> RuntimeInteractionReceiptRecoveryScanFutureV1<Self::Cursor, Self::Error> {
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
                        let _drop = PendingDropV1 { dropped };
                        pending().await
                    }
                    None => empty_page_v1(),
                }
            })
        }

        fn expire_token_v1(
            self: Arc<Self>,
            request: RuntimeInteractionReceiptRecoveryMutationRequestV1,
        ) -> RuntimeInteractionReceiptRecoveryMutationFutureV1<Self::Error> {
            let port = Arc::clone(self.as_ref());
            Box::pin(async move {
                let _ = request.into_candidate();
                port.expiry_calls.fetch_add(1, Ordering::AcqRel);
                let mutation =
                    port.expiry
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or(FakeMutationV1::Succeed(
                            RuntimeInteractionReceiptRecoveryMutationDispositionV1::Converged,
                        ));
                run_fake_mutation_v1(mutation).await
            })
        }

        fn terminalize_expired_v1(
            self: Arc<Self>,
            request: RuntimeInteractionReceiptRecoveryMutationRequestV1,
        ) -> RuntimeInteractionReceiptRecoveryMutationFutureV1<Self::Error> {
            let port = Arc::clone(self.as_ref());
            Box::pin(async move {
                let _ = request.into_candidate();
                port.terminalization_calls.fetch_add(1, Ordering::AcqRel);
                let mutation = port.terminalization.lock().unwrap().pop_front().unwrap_or(
                    FakeMutationV1::Succeed(
                        RuntimeInteractionReceiptRecoveryMutationDispositionV1::Converged,
                    ),
                );
                run_fake_mutation_v1(mutation).await
            })
        }
    }

    async fn run_fake_mutation_v1(
        mutation: FakeMutationV1,
    ) -> Result<RuntimeInteractionReceiptRecoveryMutationDispositionV1, FakeErrorV1> {
        match mutation {
            FakeMutationV1::Succeed(disposition) => Ok(disposition),
            FakeMutationV1::Failed => Err(FakeErrorV1),
            FakeMutationV1::Pending(dropped) => {
                let _drop = PendingDropV1 { dropped };
                pending().await
            }
        }
    }

    fn candidate_v1(
        interaction_id: u64,
        state: InteractionReceiptStateV1,
        token_expires_at: Option<i64>,
    ) -> RuntimeInteractionReceiptRecoveryCandidateV1 {
        RuntimeInteractionReceiptRecoveryCandidateV1::new(
            InteractionReceiptIdentityV1::new(
                DiscordApplicationIdV1::new(1).unwrap(),
                DiscordInteractionIdV1::new(interaction_id).unwrap(),
            ),
            state,
            2,
            1,
            Utc.timestamp_millis_opt(1_000).single().unwrap(),
            token_expires_at.map(|millis| Utc.timestamp_millis_opt(millis).single().unwrap()),
        )
        .unwrap()
    }

    fn page_v1(
        candidates: Vec<RuntimeInteractionReceiptRecoveryCandidateV1>,
        next_cursor: Option<usize>,
        exhausted: bool,
    ) -> RuntimeInteractionReceiptRecoveryScanPageV1<usize> {
        RuntimeInteractionReceiptRecoveryScanPageV1::new(
            candidates,
            next_cursor,
            exhausted,
            Some(Utc.timestamp_millis_opt(2_000).single().unwrap()),
        )
        .unwrap()
    }

    fn empty_page_v1() -> Result<RuntimeInteractionReceiptRecoveryScanPageV1<usize>, FakeErrorV1> {
        Ok(RuntimeInteractionReceiptRecoveryScanPageV1::new(Vec::new(), None, true, None).unwrap())
    }

    fn test_config_v1(
        cadence: Duration,
        scan_timeout: Duration,
        mutation_timeout: Duration,
    ) -> RuntimeInteractionReceiptRecoverySupervisorConfigV1 {
        RuntimeInteractionReceiptRecoverySupervisorConfigV1::for_test_v1(
            cadence,
            scan_timeout,
            mutation_timeout,
        )
    }

    async fn wait_for_v1(predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !predicate() {
            assert!(Instant::now() < deadline);
            sleep(Duration::from_millis(1)).await;
        }
    }

    #[tokio::test]
    async fn first_scan_is_immediate_and_all_snapshot_pages_finish_before_cadence() {
        let port = Arc::new(FakePortV1::new(vec![
            FakeScanV1::Page(page_v1(
                vec![candidate_v1(
                    1,
                    InteractionReceiptStateV1::Claimed,
                    Some(3_000),
                )],
                Some(1),
                false,
            )),
            FakeScanV1::Page(page_v1(
                vec![candidate_v1(
                    2,
                    InteractionReceiptStateV1::Prepared,
                    Some(3_000),
                )],
                None,
                true,
            )),
        ]));
        let supervisor = RuntimeInteractionReceiptRecoverySupervisorV1::start(
            Arc::clone(&port),
            test_config_v1(
                Duration::from_secs(60),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
        );

        wait_for_v1(|| port.scan_calls.load(Ordering::Acquire) == 2).await;
        assert_eq!(
            *port.scan_requests.lock().unwrap(),
            vec![(None, 64), (Some(1), 64)]
        );
        assert_eq!(port.expiry_calls.load(Ordering::Acquire), 0);
        assert_eq!(port.terminalization_calls.load(Ordering::Acquire), 2);
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            report.exit(),
            RuntimeInteractionReceiptRecoverySupervisorExitV1::Commanded
        );
        assert_eq!(report.progress().terminalization_attempted, 2);
    }

    #[tokio::test]
    async fn database_time_terminalizes_valid_pristine_and_nonpristine_candidates_exactly_once() {
        let port = Arc::new(
            FakePortV1::new(vec![FakeScanV1::Page(page_v1(
                vec![
                    candidate_v1(1, InteractionReceiptStateV1::Claimed, Some(3_000)),
                    candidate_v1(2, InteractionReceiptStateV1::Claimed, None),
                    candidate_v1(3, InteractionReceiptStateV1::Claimed, Some(2_000)),
                    candidate_v1(4, InteractionReceiptStateV1::Executing, Some(3_000)),
                ],
                None,
                true,
            ))])
            .with_expiry(vec![
                FakeMutationV1::Succeed(
                    RuntimeInteractionReceiptRecoveryMutationDispositionV1::RecoveryRequired,
                ),
                FakeMutationV1::Succeed(
                    RuntimeInteractionReceiptRecoveryMutationDispositionV1::RecoveryRequired,
                ),
            ])
            .with_terminalization(vec![
                FakeMutationV1::Succeed(
                    RuntimeInteractionReceiptRecoveryMutationDispositionV1::RecoveryRequired,
                ),
                FakeMutationV1::Succeed(
                    RuntimeInteractionReceiptRecoveryMutationDispositionV1::RecoveryRequired,
                ),
            ]),
        );
        let supervisor = RuntimeInteractionReceiptRecoverySupervisorV1::start(
            Arc::clone(&port),
            test_config_v1(
                Duration::from_secs(60),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
        );

        wait_for_v1(|| port.expiry_calls.load(Ordering::Acquire) == 2).await;
        wait_for_v1(|| port.terminalization_calls.load(Ordering::Acquire) == 2).await;
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(report.progress().token_expiry_attempted, 2);
        assert_eq!(report.progress().terminalization_attempted, 2);
        assert_eq!(report.progress().recovery_required, 4);
    }

    #[tokio::test]
    async fn scan_failure_and_timeout_wait_for_cadence_without_busy_looping() {
        let failed = Arc::new(FakePortV1::new(vec![FakeScanV1::Failed]));
        let failed_supervisor = RuntimeInteractionReceiptRecoverySupervisorV1::start(
            Arc::clone(&failed),
            test_config_v1(
                Duration::from_millis(100),
                Duration::from_millis(20),
                Duration::from_millis(20),
            ),
        );
        wait_for_v1(|| failed.scan_calls.load(Ordering::Acquire) == 1).await;
        sleep(Duration::from_millis(30)).await;
        assert_eq!(failed.scan_calls.load(Ordering::Acquire), 1);
        let failed_report = failed_supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(failed_report.progress().scans_failed, 1);

        let dropped = Arc::new(AtomicBool::new(false));
        let pending_port = Arc::new(FakePortV1::new(vec![FakeScanV1::Pending(Arc::clone(
            &dropped,
        ))]));
        let pending_supervisor = RuntimeInteractionReceiptRecoverySupervisorV1::start(
            Arc::clone(&pending_port),
            test_config_v1(
                Duration::from_millis(100),
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
        assert_eq!(pending_report.progress().scans_failed, 1);
        assert_eq!(pending_report.progress().scans_timed_out, 1);
    }

    #[tokio::test]
    async fn shutdown_cancels_and_joins_a_pending_scan() {
        let dropped = Arc::new(AtomicBool::new(false));
        let port = Arc::new(FakePortV1::new(vec![FakeScanV1::Pending(Arc::clone(
            &dropped,
        ))]));
        let supervisor = RuntimeInteractionReceiptRecoverySupervisorV1::start(
            Arc::clone(&port),
            test_config_v1(
                Duration::from_secs(60),
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
            RuntimeInteractionReceiptRecoverySupervisorExitV1::Commanded
        );
        assert!(dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn shutdown_cancels_and_joins_a_pending_mutation() {
        let dropped = Arc::new(AtomicBool::new(false));
        let port = Arc::new(
            FakePortV1::new(vec![FakeScanV1::Page(page_v1(
                vec![candidate_v1(
                    1,
                    InteractionReceiptStateV1::Executing,
                    Some(3_000),
                )],
                None,
                true,
            ))])
            .with_terminalization(vec![FakeMutationV1::Pending(Arc::clone(&dropped))]),
        );
        let supervisor = RuntimeInteractionReceiptRecoverySupervisorV1::start(
            Arc::clone(&port),
            test_config_v1(
                Duration::from_secs(60),
                Duration::from_secs(60),
                Duration::from_secs(60),
            ),
        );
        wait_for_v1(|| port.terminalization_calls.load(Ordering::Acquire) == 1).await;

        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert_eq!(
            report.exit(),
            RuntimeInteractionReceiptRecoverySupervisorExitV1::Commanded
        );
        assert!(dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn mutation_failure_and_timeout_do_not_stop_later_candidates() {
        let dropped = Arc::new(AtomicBool::new(false));
        let port = Arc::new(
            FakePortV1::new(vec![FakeScanV1::Page(page_v1(
                vec![
                    candidate_v1(1, InteractionReceiptStateV1::Prepared, Some(3_000)),
                    candidate_v1(2, InteractionReceiptStateV1::Prepared, Some(3_000)),
                    candidate_v1(3, InteractionReceiptStateV1::Prepared, Some(3_000)),
                    candidate_v1(4, InteractionReceiptStateV1::Prepared, Some(3_000)),
                ],
                None,
                true,
            ))])
            .with_terminalization(vec![
                FakeMutationV1::Failed,
                FakeMutationV1::Pending(Arc::clone(&dropped)),
                FakeMutationV1::Succeed(
                    RuntimeInteractionReceiptRecoveryMutationDispositionV1::Converged,
                ),
                FakeMutationV1::Succeed(
                    RuntimeInteractionReceiptRecoveryMutationDispositionV1::Deferred,
                ),
            ]),
        );
        let supervisor = RuntimeInteractionReceiptRecoverySupervisorV1::start(
            Arc::clone(&port),
            test_config_v1(
                Duration::from_secs(60),
                Duration::from_secs(1),
                Duration::from_millis(20),
            ),
        );
        wait_for_v1(|| port.terminalization_calls.load(Ordering::Acquire) == 4).await;

        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_secs(1))
            .await;
        assert!(dropped.load(Ordering::Acquire));
        assert_eq!(report.progress().mutations_failed, 2);
        assert_eq!(report.progress().mutations_timed_out, 1);
        assert_eq!(report.progress().converged, 1);
        assert_eq!(report.progress().deferred, 1);
    }

    #[test]
    fn production_limits_and_debug_surfaces_are_exact_and_redacted() {
        let config = RuntimeInteractionReceiptRecoverySupervisorConfigV1::production_v1();
        assert_eq!(config.cadence, Duration::from_secs(15));
        assert_eq!(config.page_limit.get(), 64);
        assert_eq!(config.scan_timeout, Duration::from_secs(5));
        assert_eq!(config.mutation_timeout, Duration::from_secs(5));
        let candidate = candidate_v1(1, InteractionReceiptStateV1::Claimed, Some(3_000));
        assert_eq!(candidate.identity().application_id().get(), 1);
        assert_eq!(candidate.identity().interaction_id().get(), 1);
        assert_eq!(candidate.head_revision(), 2);
        assert_eq!(candidate.claim_revision(), 1);
        assert_eq!(
            format!("{candidate:?}"),
            "RuntimeInteractionReceiptRecoveryCandidateV1(<redacted>)"
        );
    }
}
