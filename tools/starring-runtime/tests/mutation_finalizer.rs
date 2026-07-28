use std::future::Future;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use automation_runtime_worker::RuntimeMutationFinalizerGenerationV1;
use starring_runtime::{
    RuntimeMutationFinalizerCompletionResultV1, RuntimeMutationFinalizerConfigErrorV1,
    RuntimeMutationFinalizerConfigV1, RuntimeMutationFinalizerJobV1,
    RuntimeMutationFinalizerPortV1, RuntimeMutationFinalizerRegistrationRejectionReasonV1,
    RuntimeMutationFinalizerSealOutcomeV1, RuntimeMutationFinalizerStartErrorV1,
    RuntimeMutationFinalizerSupervisorV1, RuntimeMutationFinalizerWaitStatusV1,
    RuntimeSupervisorExitV1,
};
use tokio::sync::Notify;

enum TestJob {
    Complete(u64),
    Block(u64),
    BlockThenPanic,
}

struct TestPort {
    calls: Arc<AtomicUsize>,
    in_flight: Arc<AtomicUsize>,
    maximum_in_flight: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

struct TestInFlightGuard {
    in_flight: Arc<AtomicUsize>,
}

impl Drop for TestInFlightGuard {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl RuntimeMutationFinalizerPortV1 for TestPort {
    type Job = TestJob;
    type Output = u64;
    type Error = &'static str;

    fn execute(
        &self,
        job: RuntimeMutationFinalizerJobV1<Self::Job>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        let calls = self.calls.clone();
        let in_flight = self.in_flight.clone();
        let maximum_in_flight = self.maximum_in_flight.clone();
        let completed = self.completed.clone();
        let entered = self.entered.clone();
        let release = self.release.clone();
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            let _in_flight = TestInFlightGuard {
                in_flight: in_flight.clone(),
            };
            maximum_in_flight.fetch_max(current, Ordering::SeqCst);
            let result = match job.into_startup_pending_drain() {
                TestJob::Complete(value) => Ok(value),
                TestJob::Block(value) => {
                    entered.notify_one();
                    release.notified().await;
                    Ok(value)
                }
                TestJob::BlockThenPanic => {
                    entered.notify_one();
                    release.notified().await;
                    panic!("injected finalizer panic")
                }
            };
            completed.fetch_add(1, Ordering::SeqCst);
            result
        }
    }
}

struct Fixture {
    calls: Arc<AtomicUsize>,
    in_flight: Arc<AtomicUsize>,
    maximum_in_flight: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            in_flight: Arc::new(AtomicUsize::new(0)),
            maximum_in_flight: Arc::new(AtomicUsize::new(0)),
            completed: Arc::new(AtomicUsize::new(0)),
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        }
    }

    fn port(&self) -> TestPort {
        TestPort {
            calls: self.calls.clone(),
            in_flight: self.in_flight.clone(),
            maximum_in_flight: self.maximum_in_flight.clone(),
            completed: self.completed.clone(),
            entered: self.entered.clone(),
            release: self.release.clone(),
        }
    }
}

fn generation() -> RuntimeMutationFinalizerGenerationV1 {
    RuntimeMutationFinalizerGenerationV1::new(NonZeroU64::new(7).unwrap()).unwrap()
}

fn supervisor(
    capacity: usize,
    fixture: &Fixture,
) -> RuntimeMutationFinalizerSupervisorV1<TestPort> {
    RuntimeMutationFinalizerSupervisorV1::start(
        RuntimeMutationFinalizerConfigV1::new(capacity).unwrap(),
        generation(),
        fixture.port(),
    )
    .unwrap()
}

#[test]
fn configuration_and_start_require_bounded_runtime_context() {
    assert_eq!(
        RuntimeMutationFinalizerConfigV1::new(0).unwrap_err(),
        RuntimeMutationFinalizerConfigErrorV1::ZeroCapacity
    );
    assert_eq!(
        RuntimeMutationFinalizerConfigV1::new(1_025).unwrap_err(),
        RuntimeMutationFinalizerConfigErrorV1::CapacityTooLarge
    );
    let fixture = Fixture::new();
    assert_eq!(
        RuntimeMutationFinalizerSupervisorV1::start(
            RuntimeMutationFinalizerConfigV1::new(1).unwrap(),
            generation(),
            fixture.port(),
        )
        .unwrap_err(),
        RuntimeMutationFinalizerStartErrorV1::AsyncRuntimeUnavailable
    );
}

#[tokio::test]
async fn dropped_waiter_cannot_cancel_registered_job_or_root_completion() {
    let fixture = Fixture::new();
    let mut supervisor = supervisor(1, &fixture);
    let waiter = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Block(41),
        ))
        .unwrap();
    let job_id = waiter.job_id();
    drop(waiter);
    fixture.entered.notified().await;
    fixture.release.notify_waiters();
    let completion = supervisor.next_completion().await.unwrap();
    assert_eq!(completion.job_id(), job_id);
    assert!(matches!(
        completion.result(),
        RuntimeMutationFinalizerCompletionResultV1::Settled(41)
    ));
    drop(completion);
    let report = supervisor.join().await;
    assert_eq!(report.exit(), RuntimeSupervisorExitV1::Commanded);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    assert_eq!(report.snapshot().settled_jobs(), 1);
}

#[tokio::test]
async fn dropping_root_aborts_the_tracked_in_flight_task_fail_closed() {
    let fixture = Fixture::new();
    let supervisor = supervisor(1, &fixture);
    let waiter = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Block(47),
        ))
        .unwrap();
    fixture.entered.notified().await;
    drop(supervisor);
    assert_eq!(
        waiter.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::FailedClosed(RuntimeSupervisorExitV1::Aborted)
    );
    fixture.release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(fixture.completed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn saturation_returns_the_complete_undispatched_job() {
    let fixture = Fixture::new();
    let supervisor = supervisor(1, &fixture);
    let first = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Block(1),
        ))
        .unwrap();
    fixture.entered.notified().await;
    let rejected = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Complete(2),
        ))
        .unwrap_err();
    assert_eq!(
        rejected.reason(),
        RuntimeMutationFinalizerRegistrationRejectionReasonV1::Busy
    );
    assert!(matches!(
        rejected.into_job().into_startup_pending_drain(),
        TestJob::Complete(2)
    ));
    fixture.release.notify_waiters();
    assert_eq!(
        first.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::Settled
    );
    let report = supervisor.join().await;
    assert_eq!(report.completions().len(), 1);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn actor_executes_at_most_one_registered_job_at_a_time() {
    let fixture = Fixture::new();
    let supervisor = supervisor(3, &fixture);
    let first = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Complete(1),
        ))
        .unwrap();
    let second = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Complete(2),
        ))
        .unwrap();
    let third = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Complete(3),
        ))
        .unwrap();
    assert_eq!(
        first.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::Settled
    );
    assert_eq!(
        second.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::Settled
    );
    assert_eq!(
        third.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::Settled
    );
    let report = supervisor.join().await;
    assert_eq!(report.completions().len(), 3);
    assert_eq!(fixture.maximum_in_flight.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn job_identity_is_bound_to_its_supervisor_instance() {
    let first_fixture = Fixture::new();
    let second_fixture = Fixture::new();
    let first = supervisor(1, &first_fixture);
    let second = supervisor(1, &second_fixture);
    let first_waiter = first
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Complete(1),
        ))
        .unwrap();
    let second_waiter = second
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Complete(2),
        ))
        .unwrap();
    assert_eq!(first_waiter.job_id().generation(), generation());
    assert_eq!(second_waiter.job_id().generation(), generation());
    assert_eq!(first_waiter.job_id().sequence(), NonZeroU64::MIN);
    assert_eq!(second_waiter.job_id().sequence(), NonZeroU64::MIN);
    assert_ne!(first_waiter.job_id(), second_waiter.job_id());
    assert_eq!(
        first_waiter.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::Settled
    );
    assert_eq!(
        second_waiter.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::Settled
    );
    assert_eq!(
        first.join().await.exit(),
        RuntimeSupervisorExitV1::Commanded
    );
    assert_eq!(
        second.join().await.exit(),
        RuntimeSupervisorExitV1::Commanded
    );
}

#[tokio::test]
async fn panic_is_terminal_and_returns_queued_authority_undispatched() {
    let fixture = Fixture::new();
    let supervisor = supervisor(2, &fixture);
    let panicking = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::BlockThenPanic,
        ))
        .unwrap();
    fixture.entered.notified().await;
    let queued = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Complete(9),
        ))
        .unwrap();
    fixture.release.notify_waiters();
    assert_eq!(
        panicking.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::FailedClosed(RuntimeSupervisorExitV1::Panicked)
    );
    assert_eq!(
        queued.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::FailedClosed(RuntimeSupervisorExitV1::Panicked)
    );
    let report = supervisor.join().await;
    assert_eq!(report.exit(), RuntimeSupervisorExitV1::Panicked);
    assert_eq!(report.completions().len(), 2);
    assert!(report.completions().iter().any(|completion| matches!(
        completion.result(),
        RuntimeMutationFinalizerCompletionResultV1::DispatchedTerminal(
            RuntimeSupervisorExitV1::Panicked
        )
    )));
    assert!(report.completions().iter().any(|completion| matches!(
        completion.result(),
        RuntimeMutationFinalizerCompletionResultV1::Undispatched {
            job: RuntimeMutationFinalizerJobV1::StartupPendingDrain(TestJob::Complete(9)),
            exit: RuntimeSupervisorExitV1::Panicked,
        }
    )));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn sealing_racing_registration_has_one_winner_and_never_dispatches_rejection() {
    for _ in 0..64 {
        let fixture = Fixture::new();
        let supervisor = supervisor(1, &fixture);
        let intake = supervisor.intake().clone();
        let registration = tokio::spawn(async move {
            tokio::task::yield_now().await;
            intake.try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
                TestJob::Complete(5),
            ))
        });
        let seal = supervisor.seal_intake();
        assert!(matches!(
            seal,
            RuntimeMutationFinalizerSealOutcomeV1::First(_)
        ));
        match registration.await.unwrap() {
            Ok(waiter) => {
                assert_eq!(
                    waiter.wait().await.status(),
                    RuntimeMutationFinalizerWaitStatusV1::Settled
                );
            }
            Err(rejected) => {
                assert!(matches!(
                    rejected.reason(),
                    RuntimeMutationFinalizerRegistrationRejectionReasonV1::IntakeSealed
                        | RuntimeMutationFinalizerRegistrationRejectionReasonV1::SupervisorTerminal(
                            RuntimeSupervisorExitV1::Commanded
                        )
                ));
                assert!(matches!(
                    rejected.into_job().into_startup_pending_drain(),
                    TestJob::Complete(5)
                ));
            }
        }
        let report = supervisor.join().await;
        assert_eq!(report.exit(), RuntimeSupervisorExitV1::Commanded);
        assert_eq!(
            fixture.calls.load(Ordering::SeqCst),
            report.snapshot().settled_jobs() as usize
        );
    }
}

#[tokio::test]
async fn sealed_settled_actor_exposes_exact_production_handoff_fields() {
    let fixture = Fixture::new();
    let mut supervisor = supervisor(1, &fixture);
    let initial = supervisor.snapshot().handoff_state();
    assert_eq!(initial.finalizer_generation(), generation());
    assert!(!initial.startup_intake_sealed());
    assert!(!initial.startup_jobs_settled());
    assert!(matches!(
        supervisor.seal_intake(),
        RuntimeMutationFinalizerSealOutcomeV1::First(_)
    ));
    assert!(supervisor.wait_startup_jobs_settled().await);
    let handoff = supervisor.snapshot().handoff_state();
    assert_eq!(handoff.finalizer_generation(), generation());
    assert!(handoff.startup_intake_sealed());
    assert!(handoff.startup_jobs_settled());
    assert_eq!(supervisor.terminal_observation(), None);
    let report = supervisor.join().await;
    assert_eq!(report.exit(), RuntimeSupervisorExitV1::Commanded);
    assert!(!report.snapshot().handoff_state().startup_jobs_settled());
}

#[tokio::test]
async fn absolute_shutdown_deadline_aborts_and_joins_a_hung_registered_job() {
    let fixture = Fixture::new();
    let supervisor = supervisor(1, &fixture);
    let waiter = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Block(61),
        ))
        .unwrap();
    fixture.entered.notified().await;

    let report = supervisor
        .shutdown_until(Instant::now() + Duration::from_millis(75))
        .await;

    assert_eq!(report.exit(), RuntimeSupervisorExitV1::DeadlineElapsed);
    assert_eq!(
        waiter.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::FailedClosed(
            RuntimeSupervisorExitV1::DeadlineElapsed
        )
    );
    assert_eq!(report.snapshot().terminal(), Some(report.exit()));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.in_flight.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.completed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn expired_shutdown_deadline_never_detaches_the_registered_port_future() {
    let fixture = Fixture::new();
    let supervisor = supervisor(1, &fixture);
    let waiter = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Block(63),
        ))
        .unwrap();
    fixture.entered.notified().await;

    let report = supervisor.shutdown_until(Instant::now()).await;

    assert_eq!(report.exit(), RuntimeSupervisorExitV1::DeadlineElapsed);
    assert_eq!(
        waiter.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::FailedClosed(
            RuntimeSupervisorExitV1::DeadlineElapsed
        )
    );
    assert_eq!(fixture.in_flight.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.completed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn bounded_shutdown_preserves_a_completion_that_settles_before_deadline() {
    let fixture = Fixture::new();
    let supervisor = supervisor(1, &fixture);
    let waiter = supervisor
        .intake()
        .try_register(RuntimeMutationFinalizerJobV1::StartupPendingDrain(
            TestJob::Block(67),
        ))
        .unwrap();
    fixture.entered.notified().await;
    fixture.release.notify_waiters();

    let report = supervisor
        .shutdown_until(Instant::now() + Duration::from_secs(1))
        .await;

    assert_eq!(
        waiter.wait().await.status(),
        RuntimeMutationFinalizerWaitStatusV1::Settled
    );
    assert_eq!(report.exit(), RuntimeSupervisorExitV1::Commanded);
    assert_eq!(report.completions().len(), 1);
    assert_eq!(report.snapshot().settled_jobs(), 1);
}
