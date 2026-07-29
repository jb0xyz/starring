use std::collections::VecDeque;
use std::future::{pending, ready};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use super::*;
use crate::RuntimeDiscordGatewayShutdownFailureV1;

struct TrackedPendingWaitV2 {
    polled: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

struct FakeCancelableContinueProcessV2 {
    polled: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
    cleanup_count: Arc<AtomicUsize>,
}

#[derive(Clone, Copy)]
enum FakeLoopStepV2 {
    FixedPoint,
    ForeignFresh,
    ObservationFailure,
    FinalizeFailure,
    WaitFailure,
    ReadinessFailure,
    Recover(RuntimeStartupRecoveryClassV2),
    RecoveryFailure(RuntimeStartupRecoveryClassV2),
}

struct FakeLoopStateV2 {
    steps: Mutex<VecDeque<FakeLoopStepV2>>,
    events: Mutex<Vec<&'static str>>,
}

struct FakeReadyProcessV2 {
    state: Arc<FakeLoopStateV2>,
}

struct FakeObservedIterationV2 {
    step: FakeLoopStepV2,
}

struct FakeContinueProcessV2 {
    state: Arc<FakeLoopStateV2>,
    continuation: RuntimeStartupRecoveryContinuationV2,
    wait_failure: bool,
    recovery_failure: bool,
    readiness_failure: bool,
}

struct FakeFinalizeFailureV2 {
    state: Arc<FakeLoopStateV2>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FakeLoopErrorV2 {
    Observation,
    Finalize,
    Wait,
    Recovery,
    Readiness,
}

fn fake_loop_v2(steps: impl IntoIterator<Item = FakeLoopStepV2>) -> FakeReadyProcessV2 {
    FakeReadyProcessV2 {
        state: Arc::new(FakeLoopStateV2 {
            steps: Mutex::new(steps.into_iter().collect()),
            events: Mutex::new(Vec::new()),
        }),
    }
}

fn push_fake_loop_event_v2(state: &Arc<FakeLoopStateV2>, event: &'static str) {
    state
        .events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(event);
}

fn fake_loop_events_v2(state: &Arc<FakeLoopStateV2>) -> Vec<&'static str> {
    state
        .events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

impl RuntimeStartupRecoveryLoopReadyStepV2 for FakeReadyProcessV2 {
    type Observed = FakeObservedIterationV2;
    type Continue = FakeContinueProcessV2;
    type FixedPoint = Arc<FakeLoopStateV2>;
    type ObservationFailure = ();
    type FinalizeFailure = FakeFinalizeFailureV2;
    type Error = FakeLoopErrorV2;

    fn observe_in_place_v2(
        &mut self,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<'_, Self::Observed, Self::ObservationFailure>
    {
        let step = self
            .state
            .steps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .expect("fake startup recovery loop step");
        push_fake_loop_event_v2(&self.state, "observe");
        Box::pin(ready(
            if matches!(step, FakeLoopStepV2::ObservationFailure) {
                Err(())
            } else {
                Ok(FakeObservedIterationV2 { step })
            },
        ))
    }

    async fn cleanup_after_observation_failure_v2(
        self,
        _failure: Self::ObservationFailure,
    ) -> Self::Error {
        push_fake_loop_event_v2(&self.state, "cleanup_observation");
        FakeLoopErrorV2::Observation
    }

    fn finalize_observation_v2(
        self,
        observed: Self::Observed,
    ) -> Result<
        RuntimeStartupRecoveryLoopIterationOutcomeV2<Self::Continue, Self::FixedPoint>,
        Self::FinalizeFailure,
    > {
        push_fake_loop_event_v2(&self.state, "finalize");
        match observed.step {
            FakeLoopStepV2::FixedPoint => Ok(
                RuntimeStartupRecoveryLoopIterationOutcomeV2::FixedPoint(self.state),
            ),
            FakeLoopStepV2::ForeignFresh => Ok(
                RuntimeStartupRecoveryLoopIterationOutcomeV2::Continue(FakeContinueProcessV2 {
                    state: self.state,
                    continuation: RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh {
                        retry_after: Duration::from_millis(1),
                    },
                    wait_failure: false,
                    recovery_failure: false,
                    readiness_failure: false,
                }),
            ),
            FakeLoopStepV2::WaitFailure => Ok(
                RuntimeStartupRecoveryLoopIterationOutcomeV2::Continue(FakeContinueProcessV2 {
                    state: self.state,
                    continuation: RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh {
                        retry_after: Duration::from_millis(1),
                    },
                    wait_failure: true,
                    recovery_failure: false,
                    readiness_failure: false,
                }),
            ),
            FakeLoopStepV2::ReadinessFailure => Ok(
                RuntimeStartupRecoveryLoopIterationOutcomeV2::Continue(FakeContinueProcessV2 {
                    state: self.state,
                    continuation: RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh {
                        retry_after: Duration::from_millis(1),
                    },
                    wait_failure: false,
                    recovery_failure: false,
                    readiness_failure: true,
                }),
            ),
            FakeLoopStepV2::Recover(class) => Ok(
                RuntimeStartupRecoveryLoopIterationOutcomeV2::Continue(FakeContinueProcessV2 {
                    state: self.state,
                    continuation: RuntimeStartupRecoveryContinuationV2::Recover(class),
                    wait_failure: false,
                    recovery_failure: false,
                    readiness_failure: false,
                }),
            ),
            FakeLoopStepV2::RecoveryFailure(class) => Ok(
                RuntimeStartupRecoveryLoopIterationOutcomeV2::Continue(FakeContinueProcessV2 {
                    state: self.state,
                    continuation: RuntimeStartupRecoveryContinuationV2::Recover(class),
                    wait_failure: false,
                    recovery_failure: true,
                    readiness_failure: false,
                }),
            ),
            FakeLoopStepV2::FinalizeFailure => Err(FakeFinalizeFailureV2 { state: self.state }),
            FakeLoopStepV2::ObservationFailure => {
                unreachable!("observation failure cannot be finalized")
            }
        }
    }

    async fn cleanup_after_finalize_failure_v2(failure: Self::FinalizeFailure) -> Self::Error {
        push_fake_loop_event_v2(&failure.state, "cleanup_finalize");
        FakeLoopErrorV2::Finalize
    }
}

impl RuntimeStartupRecoveryLoopContinueStepV2 for FakeContinueProcessV2 {
    type Ready = FakeReadyProcessV2;
    type WaitCompletion = ();
    type WaitFailure = ();
    type RecoveryCompletion = ();
    type RecoveryFailure = ();
    type Error = FakeLoopErrorV2;

    fn continuation_v2(&self) -> RuntimeStartupRecoveryContinuationV2 {
        self.continuation
    }

    fn wait_in_place_v2(
        &mut self,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<'_, Self::WaitCompletion, Self::WaitFailure>
    {
        push_fake_loop_event_v2(&self.state, "wait");
        Box::pin(ready(if self.wait_failure { Err(()) } else { Ok(()) }))
    }

    async fn cleanup_after_wait_failure_v2(self, _failure: Self::WaitFailure) -> Self::Error {
        push_fake_loop_event_v2(&self.state, "cleanup_wait");
        FakeLoopErrorV2::Wait
    }

    fn execute_recovery_in_place_v2(
        &mut self,
        _class: RuntimeStartupRecoveryClassV2,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<
        '_,
        Self::RecoveryCompletion,
        Self::RecoveryFailure,
    > {
        push_fake_loop_event_v2(&self.state, "recovery");
        Box::pin(ready(if self.recovery_failure {
            Err(())
        } else {
            Ok(())
        }))
    }

    fn execute_recovery_owned_v3(
        self,
        _class: RuntimeStartupRecoveryClassV2,
    ) -> RuntimeStartupRecoveryOwnedStepFutureV3<
        Self,
        Self::RecoveryCompletion,
        Self::RecoveryFailure,
        Self::Error,
    > {
        push_fake_loop_event_v2(&self.state, "recovery");
        Box::pin(ready(Ok(if self.recovery_failure {
            RuntimeStartupRecoveryOwnedStepOutcomeV3::Failed(self, ())
        } else {
            RuntimeStartupRecoveryOwnedStepOutcomeV3::Completed(self, ())
        })))
    }

    async fn cleanup_after_recovery_failure_v2(
        self,
        _failure: Self::RecoveryFailure,
    ) -> Self::Error {
        push_fake_loop_event_v2(&self.state, "cleanup_recovery");
        FakeLoopErrorV2::Recovery
    }

    async fn into_next_ready_after_recovery_v2(
        self,
        _completion: Self::RecoveryCompletion,
    ) -> Result<Self::Ready, Self::Error> {
        push_fake_loop_event_v2(&self.state, "readiness");
        Ok(FakeReadyProcessV2 { state: self.state })
    }

    async fn into_next_ready_v2(
        self,
        _completion: Self::WaitCompletion,
    ) -> Result<Self::Ready, Self::Error> {
        if self.readiness_failure {
            push_fake_loop_event_v2(&self.state, "cleanup_readiness");
            Err(FakeLoopErrorV2::Readiness)
        } else {
            push_fake_loop_event_v2(&self.state, "readiness");
            Ok(FakeReadyProcessV2 { state: self.state })
        }
    }
}

impl RuntimeStartupRecoveryLoopContinueStepV2 for FakeCancelableContinueProcessV2 {
    type Ready = ();
    type WaitCompletion = ();
    type WaitFailure = ();
    type RecoveryCompletion = ();
    type RecoveryFailure = ();
    type Error = FakeLoopErrorV2;

    fn continuation_v2(&self) -> RuntimeStartupRecoveryContinuationV2 {
        RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh {
            retry_after: Duration::from_millis(1),
        }
    }

    fn wait_in_place_v2(
        &mut self,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<'_, Self::WaitCompletion, Self::WaitFailure>
    {
        let wait = TrackedPendingWaitV2 {
            polled: self.polled.clone(),
            dropped: self.dropped.clone(),
        };
        Box::pin(async move {
            wait.await;
            Ok(())
        })
    }

    async fn cleanup_after_wait_failure_v2(self, _failure: Self::WaitFailure) -> Self::Error {
        self.cleanup_count.fetch_add(1, Ordering::AcqRel);
        FakeLoopErrorV2::Wait
    }

    fn execute_recovery_in_place_v2(
        &mut self,
        _class: RuntimeStartupRecoveryClassV2,
    ) -> RuntimeStartupRecoveryBorrowedStepFutureV2<
        '_,
        Self::RecoveryCompletion,
        Self::RecoveryFailure,
    > {
        let recovery = TrackedPendingWaitV2 {
            polled: self.polled.clone(),
            dropped: self.dropped.clone(),
        };
        Box::pin(async move {
            recovery.await;
            Ok(())
        })
    }

    fn execute_recovery_owned_v3(
        self,
        _class: RuntimeStartupRecoveryClassV2,
    ) -> RuntimeStartupRecoveryOwnedStepFutureV3<
        Self,
        Self::RecoveryCompletion,
        Self::RecoveryFailure,
        Self::Error,
    > {
        let recovery = TrackedPendingWaitV2 {
            polled: self.polled.clone(),
            dropped: self.dropped.clone(),
        };
        Box::pin(async move {
            recovery.await;
            Ok(RuntimeStartupRecoveryOwnedStepOutcomeV3::Completed(
                self,
                (),
            ))
        })
    }

    async fn cleanup_after_recovery_failure_v2(
        self,
        _failure: Self::RecoveryFailure,
    ) -> Self::Error {
        self.cleanup_count.fetch_add(1, Ordering::AcqRel);
        FakeLoopErrorV2::Recovery
    }

    async fn into_next_ready_after_recovery_v2(
        self,
        _completion: Self::RecoveryCompletion,
    ) -> Result<Self::Ready, Self::Error> {
        Ok(())
    }

    async fn into_next_ready_v2(
        self,
        _completion: Self::WaitCompletion,
    ) -> Result<Self::Ready, Self::Error> {
        Ok(())
    }
}

impl Future for TrackedPendingWaitV2 {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        self.polled.store(true, Ordering::Release);
        Poll::Pending
    }
}

impl Drop for TrackedPendingWaitV2 {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

#[tokio::test]
async fn production_used_driver_reobserves_after_foreign_fresh_and_stops_at_fixed_point() {
    let ready = fake_loop_v2([FakeLoopStepV2::ForeignFresh, FakeLoopStepV2::FixedPoint]);
    let state = ready.state.clone();

    let fixed_point = drive_startup_recovery_loop_v2(ready).await.unwrap();

    assert!(Arc::ptr_eq(&state, &fixed_point));
    assert_eq!(
        fake_loop_events_v2(&state),
        [
            "observe",
            "finalize",
            "wait",
            "readiness",
            "observe",
            "finalize"
        ]
    );
    assert!(state
        .steps
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_empty());
}

#[tokio::test]
async fn production_used_driver_refreshes_and_reobserves_after_supported_recovery_execution() {
    for class in [
        RuntimeStartupRecoveryClassV2::StaleLive,
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
    ] {
        let ready = fake_loop_v2([FakeLoopStepV2::Recover(class), FakeLoopStepV2::FixedPoint]);
        let state = ready.state.clone();

        let fixed_point = drive_startup_recovery_loop_v2(ready).await.unwrap();

        assert!(Arc::ptr_eq(&state, &fixed_point));
        assert_eq!(
            fake_loop_events_v2(&state),
            [
                "observe",
                "finalize",
                "recovery",
                "readiness",
                "observe",
                "finalize"
            ]
        );
    }
}

#[tokio::test]
async fn production_used_driver_cleans_each_failure_authority_exactly_once() {
    let cases = [
        (
            FakeLoopStepV2::ObservationFailure,
            FakeLoopErrorV2::Observation,
            vec!["observe", "cleanup_observation"],
        ),
        (
            FakeLoopStepV2::FinalizeFailure,
            FakeLoopErrorV2::Finalize,
            vec!["observe", "finalize", "cleanup_finalize"],
        ),
        (
            FakeLoopStepV2::WaitFailure,
            FakeLoopErrorV2::Wait,
            vec!["observe", "finalize", "wait", "cleanup_wait"],
        ),
        (
            FakeLoopStepV2::ReadinessFailure,
            FakeLoopErrorV2::Readiness,
            vec!["observe", "finalize", "wait", "cleanup_readiness"],
        ),
        (
            FakeLoopStepV2::RecoveryFailure(
                RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
            ),
            FakeLoopErrorV2::Recovery,
            vec!["observe", "finalize", "recovery", "cleanup_recovery"],
        ),
    ];

    for (step, expected, events) in cases {
        let ready = fake_loop_v2([step]);
        let state = ready.state.clone();

        let error = match drive_startup_recovery_loop_v2(ready).await {
            Ok(_) => panic!("fake startup recovery loop unexpectedly reached fixed point"),
            Err(error) => error,
        };

        assert_eq!(error, expected);
        assert_eq!(fake_loop_events_v2(&state), events);
    }
}

#[tokio::test]
async fn foreign_fresh_wait_has_deterministic_deadline_discord_owner_retry_priority() {
    let deadline = RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed;
    let all_ready = await_bounded_startup_retry_v2(
        deadline,
        ready(()),
        ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
        ready(()),
        ready(()),
        pending(),
    )
    .await;
    assert_eq!(all_ready, Err(deadline));

    let discord_ready = await_bounded_startup_retry_v2(
        deadline,
        pending::<()>(),
        ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
        ready(()),
        ready(()),
        pending(),
    )
    .await;
    assert_eq!(
        discord_ready,
        Err(
            RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            )
        )
    );

    let owner_ready = await_bounded_startup_retry_v2(
        deadline,
        pending::<()>(),
        pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
        ready(()),
        ready(()),
        pending(),
    )
    .await;
    assert_eq!(
        owner_ready,
        Err(
            RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            )
        )
    );

    let retry_ready = await_bounded_startup_retry_v2(
        deadline,
        pending::<()>(),
        pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
        pending::<()>(),
        ready(()),
        pending(),
    )
    .await;
    assert_eq!(retry_ready, Ok(()));
}

#[tokio::test]
async fn dropping_a_polled_foreign_fresh_wait_drops_only_the_wait_future() {
    let polled = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let retry = TrackedPendingWaitV2 {
        polled: polled.clone(),
        dropped: dropped.clone(),
    };
    let mut wait = Box::pin(await_bounded_startup_retry_v2(
        RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed,
        pending::<()>(),
        pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
        pending::<()>(),
        retry,
        pending(),
    ));

    std::future::poll_fn(|context| {
        assert!(Future::poll(wait.as_mut(), context).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(polled.load(Ordering::Acquire));
    assert!(!dropped.load(Ordering::Acquire));

    drop(wait);

    assert!(dropped.load(Ordering::Acquire));
}

#[tokio::test]
async fn canceling_the_production_used_borrowed_wait_retains_exactly_one_cleanup_authority() {
    let polled = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let cleanup_count = Arc::new(AtomicUsize::new(0));
    let mut process = FakeCancelableContinueProcessV2 {
        polled: polled.clone(),
        dropped: dropped.clone(),
        cleanup_count: cleanup_count.clone(),
    };
    let mut wait = process.wait_in_place_v2();

    std::future::poll_fn(|context| {
        assert!(Future::poll(wait.as_mut(), context).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(polled.load(Ordering::Acquire));
    assert!(!dropped.load(Ordering::Acquire));
    assert_eq!(cleanup_count.load(Ordering::Acquire), 0);

    drop(wait);

    assert!(dropped.load(Ordering::Acquire));
    assert_eq!(cleanup_count.load(Ordering::Acquire), 0);
    assert_eq!(
        process.cleanup_after_wait_failure_v2(()).await,
        FakeLoopErrorV2::Wait
    );
    assert_eq!(cleanup_count.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn canceling_a_borrowed_recovery_retains_exactly_one_cleanup_authority() {
    let polled = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let cleanup_count = Arc::new(AtomicUsize::new(0));
    let mut process = FakeCancelableContinueProcessV2 {
        polled: polled.clone(),
        dropped: dropped.clone(),
        cleanup_count: cleanup_count.clone(),
    };
    let mut recovery =
        process.execute_recovery_in_place_v2(RuntimeStartupRecoveryClassV2::StaleLive);

    std::future::poll_fn(|context| {
        assert!(Future::poll(recovery.as_mut(), context).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(polled.load(Ordering::Acquire));
    assert!(!dropped.load(Ordering::Acquire));
    assert_eq!(cleanup_count.load(Ordering::Acquire), 0);

    drop(recovery);

    assert!(dropped.load(Ordering::Acquire));
    assert_eq!(cleanup_count.load(Ordering::Acquire), 0);
    assert_eq!(
        process.cleanup_after_recovery_failure_v2(()).await,
        FakeLoopErrorV2::Recovery
    );
    assert_eq!(cleanup_count.load(Ordering::Acquire), 1);
}

#[test]
fn all_recovery_classes_map_to_distinct_finite_fail_closed_failures() {
    let cases = [
        (
            RuntimeStartupRecoveryClassV2::StaleLive,
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveRecoveryUnavailable,
        ),
        (
            RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationRecoveryUnavailable,
        ),
        (
            RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectRecoveryUnavailable,
        ),
        (
            RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
            RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainRecoveryUnavailable,
        ),
    ];
    let mut codes = std::collections::BTreeSet::new();

    for (class, expected) in cases {
        let failure = unavailable_recovery_failure_v2(class);
        assert_eq!(failure, expected);
        assert!(codes.insert(failure.code()));
        assert_eq!(failure.context(), None);
    }

    assert_eq!(codes.len(), 4);
}

#[test]
fn foreign_fresh_deadline_classification_never_extends_owner_or_operation_lifetime() {
    let now = Instant::now();
    assert_eq!(
        classify_bounded_startup_retry_deadline_v2(
            now + Duration::from_secs(2),
            now + Duration::from_secs(1),
        ),
        RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        )
    );
    assert_eq!(
        classify_bounded_startup_retry_deadline_v2(
            now + Duration::from_secs(1),
            now + Duration::from_secs(2),
        ),
        RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed,
    );
}

#[test]
fn foreign_fresh_current_state_uses_cutoff_then_discord_then_owner_priority() {
    let now = Instant::now();
    let discord = Some(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated);
    assert_eq!(
        classify_current_bounded_startup_retry_transition_v2(
            now,
            now,
            now + Duration::from_secs(1),
            discord,
            true,
        ),
        Some(RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed)
    );
    assert_eq!(
        classify_current_bounded_startup_retry_transition_v2(
            now,
            now + Duration::from_secs(2),
            now + Duration::from_secs(1),
            discord,
            true,
        ),
        Some(
            RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            )
        )
    );
    assert_eq!(
        classify_current_bounded_startup_retry_transition_v2(
            now,
            now + Duration::from_secs(2),
            now + Duration::from_secs(1),
            None,
            true,
        ),
        Some(
            RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            )
        )
    );
}

#[test]
fn loop_transition_preserves_primary_failure_and_classifies_cleanup_separately() {
    let transition =
        RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainRecoveryUnavailable;
    let clean = finish_startup_recovery_loop_transition_v2(transition, Ok(()));
    assert_eq!(
        clean,
        RuntimeProcessStartupRecoveryLoopErrorV2::Transition(transition)
    );
    assert!(!clean.cleanup_class());

    let cleanup = RuntimeClosedRecoveryProcessCleanupFailureV2::Discord(
        RuntimeDiscordGatewayShutdownFailureV1::TaskStopped,
    );
    let cleanup_error = finish_startup_recovery_loop_transition_v2(transition, Err(cleanup));
    assert_eq!(
        cleanup_error,
        RuntimeProcessStartupRecoveryLoopErrorV2::CleanupAfterTransition {
            transition,
            cleanup,
        }
    );
    assert!(cleanup_error.cleanup_class());
    assert_eq!(
        cleanup_error.code(),
        "runtime_process_startup_recovery_loop_cleanup_after_transition"
    );
}

#[test]
fn loop_errors_are_finite_contextual_and_redacted() {
    let errors = [
        RuntimeProcessStartupRecoveryLoopErrorV2::Transition(
            RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed,
        ),
        RuntimeProcessStartupRecoveryLoopErrorV2::Observation(
            RuntimeProcessStartupRecoveryObservationErrorV2::Transition(
                crate::RuntimeProcessStartupRecoveryObservationFailureV2::ObservationUnavailable,
            ),
        ),
        RuntimeProcessStartupRecoveryLoopErrorV2::Readiness(
            RuntimeProcessRecoveryReadinessTransitionErrorV2::Transition(
                crate::RuntimeProcessRecoveryReadinessTransitionFailureV2::OperationDeadlineElapsed,
            ),
        ),
    ];

    for error in errors {
        assert!(!error.code().is_empty());
        assert_eq!(error.context(), None);
        assert!(!error.to_string().is_empty());
        assert_eq!(
            format!("{error:?}"),
            "RuntimeProcessStartupRecoveryLoopErrorV2(<redacted>)"
        );
        assert!(std::error::Error::source(&error).is_none());
    }
}

#[test]
fn supported_recovery_database_failures_preserve_exact_persistence_codes() {
    for error in [
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1::InvalidInput,
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1::OwnershipLost,
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1::Unavailable,
        automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1::Indeterminate,
    ] {
        assert_eq!(
            RuntimeProcessStartupRecoveryLoopFailureV2::StaleLiveExecution(error).code(),
            error.code()
        );
        assert_eq!(
            RuntimeProcessStartupRecoveryLoopFailureV2::ReservedAwaitingCertificationExecution(
                error,
            )
            .code(),
            error.code()
        );
        assert_eq!(
            RuntimeProcessStartupRecoveryLoopFailureV2::SuspendedLocalEffectExecution(error).code(),
            error.code()
        );
    }
}
