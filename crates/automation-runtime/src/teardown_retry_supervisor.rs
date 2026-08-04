use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use automation_instance::{
    InstanceStoreError, InstanceTeardownRetryKeyV2, InstanceTeardownRetryScanCursorV2,
    InstanceTeardownRetryScanPageV2, MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2,
};
use automation_instance_teardown::{TeardownError, TeardownOutcome};
use futures::{stream, StreamExt};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, timeout_at, Instant as TokioInstant};

const MAX_TEARDOWN_RETRY_CONCURRENCY_V1: usize = 32;
const MAX_TEARDOWN_RETRY_CADENCE_V1: Duration = Duration::from_secs(60 * 60);
const MAX_TEARDOWN_RETRY_SCAN_TIMEOUT_V1: Duration = Duration::from_secs(60);
const MAX_TEARDOWN_RETRY_INSTANCE_TIMEOUT_V1: Duration = Duration::from_secs(60);

pub type InstanceTeardownRetryScanFutureV1 = Pin<
    Box<
        dyn Future<Output = Result<InstanceTeardownRetryScanPageV2, InstanceStoreError>>
            + Send
            + 'static,
    >,
>;
pub type InstanceTeardownRetryExecutionFutureV1 =
    Pin<Box<dyn Future<Output = Result<TeardownOutcome, TeardownError>> + Send + 'static>>;

pub struct InstanceTeardownRetryScanRequestV1 {
    cursor: InstanceTeardownRetryScanCursorV2,
    limit: NonZeroUsize,
}

impl InstanceTeardownRetryScanRequestV1 {
    pub fn into_parts(self) -> (InstanceTeardownRetryScanCursorV2, NonZeroUsize) {
        (self.cursor, self.limit)
    }
}

pub struct InstanceTeardownRetryExecutionRequestV1 {
    key: InstanceTeardownRetryKeyV2,
}

impl InstanceTeardownRetryExecutionRequestV1 {
    pub fn into_parts(self) -> (discord_model::GuildId, automation_instance::InstanceId) {
        (self.key.guild_id(), self.key.instance_id().clone())
    }
}

pub trait InstanceTeardownRetrySupervisorPortV1: Send + Sync + 'static {
    fn scan_retryable_v1(
        self: Arc<Self>,
        request: InstanceTeardownRetryScanRequestV1,
    ) -> InstanceTeardownRetryScanFutureV1;

    fn retry_teardown_v1(
        self: Arc<Self>,
        request: InstanceTeardownRetryExecutionRequestV1,
    ) -> InstanceTeardownRetryExecutionFutureV1;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InstanceTeardownRetrySupervisorConfigurationErrorV1 {
    #[error("instance teardown retry cadence is invalid")]
    Cadence,
    #[error("instance teardown retry page limit is invalid")]
    PageLimit,
    #[error("instance teardown retry concurrency is invalid")]
    Concurrency,
    #[error("instance teardown retry scan timeout is invalid")]
    ScanTimeout,
    #[error("instance teardown retry timeout is invalid")]
    InstanceTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstanceTeardownRetrySupervisorConfigV1 {
    cadence: Duration,
    page_limit: NonZeroUsize,
    max_concurrency: NonZeroUsize,
    scan_timeout: Duration,
    per_instance_timeout: Duration,
}

impl InstanceTeardownRetrySupervisorConfigV1 {
    pub fn new(
        cadence: Duration,
        page_limit: NonZeroUsize,
        max_concurrency: NonZeroUsize,
        scan_timeout: Duration,
        per_instance_timeout: Duration,
    ) -> Result<Self, InstanceTeardownRetrySupervisorConfigurationErrorV1> {
        if cadence.is_zero() || cadence > MAX_TEARDOWN_RETRY_CADENCE_V1 {
            return Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::Cadence);
        }
        if page_limit.get() > MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2 {
            return Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::PageLimit);
        }
        if max_concurrency.get() > MAX_TEARDOWN_RETRY_CONCURRENCY_V1 {
            return Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::Concurrency);
        }
        if scan_timeout.is_zero() || scan_timeout > MAX_TEARDOWN_RETRY_SCAN_TIMEOUT_V1 {
            return Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::ScanTimeout);
        }
        if per_instance_timeout.is_zero()
            || per_instance_timeout > MAX_TEARDOWN_RETRY_INSTANCE_TIMEOUT_V1
        {
            return Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::InstanceTimeout);
        }
        Ok(Self {
            cadence,
            page_limit,
            max_concurrency,
            scan_timeout,
            per_instance_timeout,
        })
    }

    pub fn cadence(self) -> Duration {
        self.cadence
    }

    pub fn page_limit(self) -> NonZeroUsize {
        self.page_limit
    }

    pub fn max_concurrency(self) -> NonZeroUsize {
        self.max_concurrency
    }

    pub fn scan_timeout(self) -> Duration {
        self.scan_timeout
    }

    pub fn per_instance_timeout(self) -> Duration {
        self.per_instance_timeout
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceTeardownRetrySupervisorExitV1 {
    Commanded,
    ControlClosed,
    Panicked,
    DeadlineElapsed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InstanceTeardownRetrySupervisorProgressV1 {
    pub scan_succeeded: u64,
    pub scan_failed: u64,
    pub scan_timed_out: u64,
    pub candidates: u64,
    pub completed: u64,
    pub in_progress: u64,
    pub failed: u64,
    pub timed_out: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstanceTeardownRetrySupervisorReportV1 {
    exit: InstanceTeardownRetrySupervisorExitV1,
    progress: InstanceTeardownRetrySupervisorProgressV1,
}

impl InstanceTeardownRetrySupervisorReportV1 {
    pub fn exit(self) -> InstanceTeardownRetrySupervisorExitV1 {
        self.exit
    }

    pub fn progress(self) -> InstanceTeardownRetrySupervisorProgressV1 {
        self.progress
    }
}

pub struct InstanceTeardownRetrySupervisorV1 {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<InstanceTeardownRetrySupervisorReportV1>>,
}

impl InstanceTeardownRetrySupervisorV1 {
    pub fn start<P>(port: P, config: InstanceTeardownRetrySupervisorConfigV1) -> Self
    where
        P: InstanceTeardownRetrySupervisorPortV1,
    {
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let task = tokio::spawn(run_instance_teardown_retry_supervisor_v1(
            Arc::new(port),
            config,
            shutdown_receiver,
        ));
        Self {
            shutdown: Some(shutdown),
            task: Some(task),
        }
    }

    pub async fn shutdown_until(
        mut self,
        deadline: Instant,
    ) -> InstanceTeardownRetrySupervisorReportV1 {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let Some(mut task) = self.task.take() else {
            return InstanceTeardownRetrySupervisorReportV1 {
                exit: InstanceTeardownRetrySupervisorExitV1::Panicked,
                progress: InstanceTeardownRetrySupervisorProgressV1::default(),
            };
        };
        match timeout_at(TokioInstant::from_std(deadline), &mut task).await {
            Ok(Ok(report)) => report,
            Ok(Err(_)) => InstanceTeardownRetrySupervisorReportV1 {
                exit: InstanceTeardownRetrySupervisorExitV1::Panicked,
                progress: InstanceTeardownRetrySupervisorProgressV1::default(),
            },
            Err(_) => {
                task.abort();
                let _ = task.await;
                InstanceTeardownRetrySupervisorReportV1 {
                    exit: InstanceTeardownRetrySupervisorExitV1::DeadlineElapsed,
                    progress: InstanceTeardownRetrySupervisorProgressV1::default(),
                }
            }
        }
    }
}

impl Drop for InstanceTeardownRetrySupervisorV1 {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Debug for InstanceTeardownRetrySupervisorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InstanceTeardownRetrySupervisorV1(<redacted>)")
    }
}

async fn run_instance_teardown_retry_supervisor_v1<P>(
    port: Arc<P>,
    config: InstanceTeardownRetrySupervisorConfigV1,
    mut shutdown: oneshot::Receiver<()>,
) -> InstanceTeardownRetrySupervisorReportV1
where
    P: InstanceTeardownRetrySupervisorPortV1,
{
    let mut cursor = InstanceTeardownRetryScanCursorV2::initial();
    let mut progress = InstanceTeardownRetrySupervisorProgressV1::default();
    loop {
        let page = tokio::select! {
            biased;
            command = &mut shutdown => {
                let exit = if command.is_ok() {
                    InstanceTeardownRetrySupervisorExitV1::Commanded
                } else {
                    InstanceTeardownRetrySupervisorExitV1::ControlClosed
                };
                return InstanceTeardownRetrySupervisorReportV1 { exit, progress };
            }
            page = timeout(
                config.scan_timeout,
                Arc::clone(&port).scan_retryable_v1(InstanceTeardownRetryScanRequestV1 {
                    cursor: cursor.clone(),
                    limit: config.page_limit,
                }),
            ) => page,
        };
        match page {
            Ok(Ok(page)) => {
                increment(&mut progress.scan_succeeded);
                progress.candidates = progress
                    .candidates
                    .saturating_add(u64::try_from(page.keys().len()).unwrap_or(u64::MAX));
                let next_cursor = page
                    .next_cursor_v2()
                    .unwrap_or_else(InstanceTeardownRetryScanCursorV2::initial);
                tokio::select! {
                    biased;
                    command = &mut shutdown => {
                        let exit = if command.is_ok() {
                            InstanceTeardownRetrySupervisorExitV1::Commanded
                        } else {
                            InstanceTeardownRetrySupervisorExitV1::ControlClosed
                        };
                        return InstanceTeardownRetrySupervisorReportV1 { exit, progress };
                    }
                    () = run_teardown_retry_page_v1(
                        Arc::clone(&port),
                        page,
                        config,
                        &mut progress,
                    ) => {}
                }
                cursor = next_cursor;
            }
            Ok(Err(_)) => increment(&mut progress.scan_failed),
            Err(_) => {
                increment(&mut progress.scan_failed);
                increment(&mut progress.scan_timed_out);
            }
        }
        tokio::select! {
            biased;
            command = &mut shutdown => {
                let exit = if command.is_ok() {
                    InstanceTeardownRetrySupervisorExitV1::Commanded
                } else {
                    InstanceTeardownRetrySupervisorExitV1::ControlClosed
                };
                return InstanceTeardownRetrySupervisorReportV1 { exit, progress };
            }
            () = sleep(config.cadence) => {}
        }
    }
}

async fn run_teardown_retry_page_v1<P>(
    port: Arc<P>,
    page: InstanceTeardownRetryScanPageV2,
    config: InstanceTeardownRetrySupervisorConfigV1,
    progress: &mut InstanceTeardownRetrySupervisorProgressV1,
) where
    P: InstanceTeardownRetrySupervisorPortV1,
{
    let timeout_duration = config.per_instance_timeout;
    let outcomes = stream::iter(page.keys().iter().cloned().map(|key| {
        let port = Arc::clone(&port);
        async move {
            match timeout(
                timeout_duration,
                Arc::clone(&port)
                    .retry_teardown_v1(InstanceTeardownRetryExecutionRequestV1 { key }),
            )
            .await
            {
                Ok(Ok(outcome)) => InstanceTeardownRetryEntryOutcomeV1::Completed(outcome),
                Ok(Err(_)) => InstanceTeardownRetryEntryOutcomeV1::Failed,
                Err(_) => InstanceTeardownRetryEntryOutcomeV1::TimedOut,
            }
        }
    }))
    .buffer_unordered(config.max_concurrency.get())
    .collect::<Vec<_>>()
    .await;
    for outcome in outcomes {
        match outcome {
            InstanceTeardownRetryEntryOutcomeV1::Completed(TeardownOutcome::InProgress) => {
                increment(&mut progress.in_progress)
            }
            InstanceTeardownRetryEntryOutcomeV1::Completed(
                TeardownOutcome::Completed
                | TeardownOutcome::ResumedAndCompleted
                | TeardownOutcome::AlreadyDeleted,
            ) => increment(&mut progress.completed),
            InstanceTeardownRetryEntryOutcomeV1::Failed => increment(&mut progress.failed),
            InstanceTeardownRetryEntryOutcomeV1::TimedOut => increment(&mut progress.timed_out),
        }
    }
}

enum InstanceTeardownRetryEntryOutcomeV1 {
    Completed(TeardownOutcome),
    Failed,
    TimedOut,
}

fn increment(value: &mut u64) {
    *value = value.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use automation_instance::{InstanceId, InstanceTeardownRetryKeyV2};
    use discord_model::GuildId;
    use static_assertions::assert_not_impl_any;

    use super::*;

    assert_not_impl_any!(
        InstanceTeardownRetryScanRequestV1:
            Clone,
            Debug,
            serde::Serialize
    );
    assert_not_impl_any!(
        InstanceTeardownRetryExecutionRequestV1:
            Clone,
            Debug,
            serde::Serialize
    );
    assert_not_impl_any!(InstanceTeardownRetrySupervisorV1: Clone, serde::Serialize);

    struct FairPort {
        attempts: Mutex<BTreeMap<String, usize>>,
        events: Mutex<Vec<String>>,
    }

    impl FairPort {
        fn new() -> Self {
            Self {
                attempts: Mutex::new(BTreeMap::new()),
                events: Mutex::new(Vec::new()),
            }
        }

        fn attempts(&self, id: &str) -> usize {
            self.attempts
                .lock()
                .unwrap()
                .get(id)
                .copied()
                .unwrap_or_default()
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }

    impl InstanceTeardownRetrySupervisorPortV1 for FairPort {
        fn scan_retryable_v1(
            self: Arc<Self>,
            request: InstanceTeardownRetryScanRequestV1,
        ) -> InstanceTeardownRetryScanFutureV1 {
            Box::pin(async move {
                let (cursor, limit) = request.into_parts();
                let all = ["a", "b", "c"]
                    .into_iter()
                    .map(|id| {
                        InstanceTeardownRetryKeyV2::new(GuildId(7), InstanceId::parse(id).unwrap())
                            .unwrap()
                    })
                    .collect::<Vec<_>>();
                let through = all.last().cloned();
                let keys = all
                    .into_iter()
                    .filter(|key| {
                        cursor
                            .after()
                            .is_none_or(|after| key.cmp_c_v2(after).is_gt())
                    })
                    .take(limit.get())
                    .collect();
                Ok(InstanceTeardownRetryScanPageV2::new(keys, through, limit).unwrap())
            })
        }

        fn retry_teardown_v1(
            self: Arc<Self>,
            request: InstanceTeardownRetryExecutionRequestV1,
        ) -> InstanceTeardownRetryExecutionFutureV1 {
            Box::pin(async move {
                let (_, instance_id) = request.into_parts();
                let id = instance_id.as_str().to_string();
                *self.attempts.lock().unwrap().entry(id.clone()).or_default() += 1;
                self.events.lock().unwrap().push(id.clone());
                if id == "a" {
                    Err(TeardownError::Store(InstanceStoreError::Backend(
                        "item-backend-sensitive".to_string(),
                    )))
                } else {
                    Ok(TeardownOutcome::ResumedAndCompleted)
                }
            })
        }
    }

    struct TimeoutPort {
        active: AtomicUsize,
        max_active: AtomicUsize,
    }

    struct PendingScanPort;

    impl InstanceTeardownRetrySupervisorPortV1 for PendingScanPort {
        fn scan_retryable_v1(
            self: Arc<Self>,
            _: InstanceTeardownRetryScanRequestV1,
        ) -> InstanceTeardownRetryScanFutureV1 {
            Box::pin(std::future::pending())
        }

        fn retry_teardown_v1(
            self: Arc<Self>,
            _: InstanceTeardownRetryExecutionRequestV1,
        ) -> InstanceTeardownRetryExecutionFutureV1 {
            Box::pin(std::future::pending())
        }
    }

    struct FlakyScanPort {
        scans: AtomicUsize,
        attempts: AtomicUsize,
    }

    struct TimeoutThenSuccessPort {
        scans: AtomicUsize,
        attempts: AtomicUsize,
    }

    impl InstanceTeardownRetrySupervisorPortV1 for TimeoutThenSuccessPort {
        fn scan_retryable_v1(
            self: Arc<Self>,
            request: InstanceTeardownRetryScanRequestV1,
        ) -> InstanceTeardownRetryScanFutureV1 {
            Box::pin(async move {
                if self.scans.fetch_add(1, Ordering::SeqCst) == 0 {
                    return std::future::pending().await;
                }
                let (_, limit) = request.into_parts();
                let key =
                    InstanceTeardownRetryKeyV2::new(GuildId(7), InstanceId::parse("a").unwrap())
                        .unwrap();
                InstanceTeardownRetryScanPageV2::new(vec![key.clone()], Some(key), limit)
                    .ok_or_else(|| InstanceStoreError::Backend("invalid".to_string()))
            })
        }

        fn retry_teardown_v1(
            self: Arc<Self>,
            _: InstanceTeardownRetryExecutionRequestV1,
        ) -> InstanceTeardownRetryExecutionFutureV1 {
            Box::pin(async move {
                self.attempts.fetch_add(1, Ordering::SeqCst);
                Ok(TeardownOutcome::ResumedAndCompleted)
            })
        }
    }

    impl InstanceTeardownRetrySupervisorPortV1 for FlakyScanPort {
        fn scan_retryable_v1(
            self: Arc<Self>,
            request: InstanceTeardownRetryScanRequestV1,
        ) -> InstanceTeardownRetryScanFutureV1 {
            Box::pin(async move {
                let (_, limit) = request.into_parts();
                if self.scans.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(InstanceStoreError::Backend("transient".to_string()));
                }
                let key =
                    InstanceTeardownRetryKeyV2::new(GuildId(7), InstanceId::parse("a").unwrap())
                        .unwrap();
                InstanceTeardownRetryScanPageV2::new(vec![key.clone()], Some(key), limit)
                    .ok_or_else(|| InstanceStoreError::Backend("invalid".to_string()))
            })
        }

        fn retry_teardown_v1(
            self: Arc<Self>,
            _: InstanceTeardownRetryExecutionRequestV1,
        ) -> InstanceTeardownRetryExecutionFutureV1 {
            Box::pin(async move {
                self.attempts.fetch_add(1, Ordering::SeqCst);
                Ok(TeardownOutcome::ResumedAndCompleted)
            })
        }
    }

    struct ActiveAttempt<'a> {
        active: &'a AtomicUsize,
    }

    impl Drop for ActiveAttempt<'_> {
        fn drop(&mut self) {
            self.active.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl TimeoutPort {
        fn new() -> Self {
            Self {
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
            }
        }
    }

    impl InstanceTeardownRetrySupervisorPortV1 for TimeoutPort {
        fn scan_retryable_v1(
            self: Arc<Self>,
            request: InstanceTeardownRetryScanRequestV1,
        ) -> InstanceTeardownRetryScanFutureV1 {
            Box::pin(async move {
                let (_, limit) = request.into_parts();
                let keys = ["a", "b", "c"]
                    .into_iter()
                    .map(|id| {
                        InstanceTeardownRetryKeyV2::new(GuildId(7), InstanceId::parse(id).unwrap())
                            .unwrap()
                    })
                    .collect::<Vec<_>>();
                let through = keys.last().cloned();
                Ok(InstanceTeardownRetryScanPageV2::new(keys, through, limit).unwrap())
            })
        }

        fn retry_teardown_v1(
            self: Arc<Self>,
            _: InstanceTeardownRetryExecutionRequestV1,
        ) -> InstanceTeardownRetryExecutionFutureV1 {
            Box::pin(async move {
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                let _attempt = ActiveAttempt {
                    active: &self.active,
                };
                self.max_active.fetch_max(active, Ordering::SeqCst);
                sleep(Duration::from_millis(50)).await;
                Ok(TeardownOutcome::ResumedAndCompleted)
            })
        }
    }

    fn config(
        cadence: Duration,
        page_limit: usize,
        concurrency: usize,
        scan_timeout: Duration,
        instance_timeout: Duration,
    ) -> InstanceTeardownRetrySupervisorConfigV1 {
        InstanceTeardownRetrySupervisorConfigV1::new(
            cadence,
            NonZeroUsize::new(page_limit).unwrap(),
            NonZeroUsize::new(concurrency).unwrap(),
            scan_timeout,
            instance_timeout,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn persistent_failure_does_not_starve_later_scan_keys() {
        let port = FairPort::new();
        let attempts = Arc::new(port);
        let supervisor = InstanceTeardownRetrySupervisorV1::start(
            ArcPort(Arc::clone(&attempts)),
            config(
                Duration::from_millis(2),
                1,
                1,
                Duration::from_millis(20),
                Duration::from_millis(20),
            ),
        );
        timeout(Duration::from_millis(100), async {
            while attempts.attempts("a") < 2 {
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_millis(50))
            .await;
        assert_eq!(
            report.exit(),
            InstanceTeardownRetrySupervisorExitV1::Commanded
        );
        assert_eq!(attempts.attempts("a"), 2);
        assert_eq!(attempts.attempts("b"), 1);
        assert_eq!(attempts.attempts("c"), 1);
        assert_eq!(&attempts.events()[..4], ["a", "b", "c", "a"]);
        assert_eq!(report.progress().scan_succeeded, 4);
        assert_eq!(report.progress().failed, 2);
        assert_eq!(report.progress().completed, 2);
        assert!(!format!("{report:?}").contains("item-backend-sensitive"));
    }

    struct ArcPort<P>(Arc<P>);

    impl<P> InstanceTeardownRetrySupervisorPortV1 for ArcPort<P>
    where
        P: InstanceTeardownRetrySupervisorPortV1,
    {
        fn scan_retryable_v1(
            self: Arc<Self>,
            request: InstanceTeardownRetryScanRequestV1,
        ) -> InstanceTeardownRetryScanFutureV1 {
            Arc::clone(&self.0).scan_retryable_v1(request)
        }

        fn retry_teardown_v1(
            self: Arc<Self>,
            request: InstanceTeardownRetryExecutionRequestV1,
        ) -> InstanceTeardownRetryExecutionFutureV1 {
            Arc::clone(&self.0).retry_teardown_v1(request)
        }
    }

    #[tokio::test]
    async fn page_concurrency_and_each_attempt_timeout_are_bounded() {
        let port = Arc::new(TimeoutPort::new());
        let supervisor = InstanceTeardownRetrySupervisorV1::start(
            ArcPort(Arc::clone(&port)),
            config(
                Duration::from_secs(1),
                3,
                2,
                Duration::from_millis(20),
                Duration::from_millis(5),
            ),
        );
        sleep(Duration::from_millis(20)).await;
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_millis(50))
            .await;
        assert_eq!(
            report.exit(),
            InstanceTeardownRetrySupervisorExitV1::Commanded
        );
        assert!(port.max_active.load(Ordering::SeqCst) <= 2);
        assert_eq!(report.progress().timed_out, 3);
    }

    #[tokio::test]
    async fn scan_failure_is_observable_and_does_not_terminate_supervision() {
        let port = Arc::new(FlakyScanPort {
            scans: AtomicUsize::new(0),
            attempts: AtomicUsize::new(0),
        });
        let supervisor = InstanceTeardownRetrySupervisorV1::start(
            ArcPort(Arc::clone(&port)),
            config(
                Duration::from_millis(2),
                1,
                1,
                Duration::from_millis(20),
                Duration::from_millis(20),
            ),
        );
        timeout(Duration::from_millis(100), async {
            while port.attempts.load(Ordering::SeqCst) == 0 {
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_millis(50))
            .await;
        assert_eq!(
            report.exit(),
            InstanceTeardownRetrySupervisorExitV1::Commanded
        );
        assert_eq!(report.progress().scan_failed, 1);
        assert!(report.progress().scan_succeeded >= 1);
        assert!(report.progress().completed >= 1);
        let rendered = format!("{report:?}");
        assert!(!rendered.contains("transient"));
        assert!(!rendered.contains("invalid"));
    }

    #[tokio::test]
    async fn pending_scan_times_out_and_a_later_scan_succeeds() {
        let port = Arc::new(TimeoutThenSuccessPort {
            scans: AtomicUsize::new(0),
            attempts: AtomicUsize::new(0),
        });
        let supervisor = InstanceTeardownRetrySupervisorV1::start(
            ArcPort(Arc::clone(&port)),
            config(
                Duration::from_millis(2),
                1,
                1,
                Duration::from_millis(2),
                Duration::from_millis(20),
            ),
        );
        timeout(Duration::from_millis(100), async {
            while port.attempts.load(Ordering::SeqCst) == 0 {
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_millis(50))
            .await;
        assert_eq!(
            report.exit(),
            InstanceTeardownRetrySupervisorExitV1::Commanded
        );
        assert_eq!(report.progress().scan_failed, 1);
        assert_eq!(report.progress().scan_timed_out, 1);
        assert!(report.progress().scan_succeeded >= 1);
        assert!(report.progress().completed >= 1);
    }

    #[tokio::test]
    async fn first_scan_is_immediate_and_shutdown_interrupts_a_pending_scan() {
        let port = Arc::new(FlakyScanPort {
            scans: AtomicUsize::new(1),
            attempts: AtomicUsize::new(0),
        });
        let supervisor = InstanceTeardownRetrySupervisorV1::start(
            ArcPort(Arc::clone(&port)),
            config(
                Duration::from_secs(60),
                1,
                1,
                Duration::from_millis(20),
                Duration::from_millis(20),
            ),
        );
        timeout(Duration::from_millis(50), async {
            while port.attempts.load(Ordering::SeqCst) == 0 {
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_millis(50))
            .await;
        assert_eq!(
            report.exit(),
            InstanceTeardownRetrySupervisorExitV1::Commanded
        );

        let pending = InstanceTeardownRetrySupervisorV1::start(
            PendingScanPort,
            config(
                Duration::from_secs(60),
                1,
                1,
                Duration::from_millis(20),
                Duration::from_millis(20),
            ),
        );
        let pending_report = pending
            .shutdown_until(Instant::now() + Duration::from_millis(50))
            .await;
        assert_eq!(
            pending_report.exit(),
            InstanceTeardownRetrySupervisorExitV1::Commanded
        );
    }

    #[tokio::test]
    async fn shutdown_command_cancels_an_in_flight_page() {
        let port = Arc::new(TimeoutPort::new());
        let supervisor = InstanceTeardownRetrySupervisorV1::start(
            ArcPort(Arc::clone(&port)),
            config(
                Duration::from_secs(60),
                3,
                2,
                Duration::from_millis(20),
                Duration::from_secs(1),
            ),
        );
        timeout(Duration::from_millis(50), async {
            while port.active.load(Ordering::SeqCst) == 0 {
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_millis(5))
            .await;
        assert_eq!(
            report.exit(),
            InstanceTeardownRetrySupervisorExitV1::Commanded
        );
        assert_eq!(port.active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn configuration_rejects_unbounded_values() {
        let one = NonZeroUsize::MIN;
        assert_eq!(
            InstanceTeardownRetrySupervisorConfigV1::new(
                Duration::ZERO,
                one,
                one,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::Cadence)
        );
        assert_eq!(
            InstanceTeardownRetrySupervisorConfigV1::new(
                Duration::from_secs(1),
                NonZeroUsize::new(MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2 + 1).unwrap(),
                one,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::PageLimit)
        );
        assert_eq!(
            InstanceTeardownRetrySupervisorConfigV1::new(
                Duration::from_secs(1),
                one,
                NonZeroUsize::new(MAX_TEARDOWN_RETRY_CONCURRENCY_V1 + 1).unwrap(),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
            Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::Concurrency)
        );
        assert_eq!(
            InstanceTeardownRetrySupervisorConfigV1::new(
                Duration::from_secs(1),
                one,
                one,
                Duration::ZERO,
                Duration::from_secs(1),
            ),
            Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::ScanTimeout)
        );
        assert_eq!(
            InstanceTeardownRetrySupervisorConfigV1::new(
                Duration::from_secs(1),
                one,
                one,
                MAX_TEARDOWN_RETRY_SCAN_TIMEOUT_V1 + Duration::from_nanos(1),
                Duration::from_secs(1),
            ),
            Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::ScanTimeout)
        );
        assert_eq!(
            InstanceTeardownRetrySupervisorConfigV1::new(
                Duration::from_secs(1),
                one,
                one,
                Duration::from_secs(1),
                Duration::ZERO,
            ),
            Err(InstanceTeardownRetrySupervisorConfigurationErrorV1::InstanceTimeout)
        );
    }

    #[tokio::test]
    async fn supervisor_debug_is_redacted() {
        let supervisor = InstanceTeardownRetrySupervisorV1::start(
            FairPort::new(),
            config(
                Duration::from_secs(1),
                1,
                1,
                Duration::from_secs(1),
                Duration::from_secs(1),
            ),
        );
        assert_eq!(
            format!("{supervisor:?}"),
            "InstanceTeardownRetrySupervisorV1(<redacted>)"
        );
        let report = supervisor
            .shutdown_until(Instant::now() + Duration::from_millis(50))
            .await;
        assert_eq!(
            report.exit(),
            InstanceTeardownRetrySupervisorExitV1::Commanded
        );
    }
}
