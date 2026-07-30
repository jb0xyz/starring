use std::future::{pending, ready};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use automation_runtime_worker::RuntimeGatewayClosedTransitionErrorV2;

use super::*;
use crate::gateway_owner_startup_watchdog::RuntimeGatewayOwnerClosedRecoveryCommitErrorV2;
use crate::{
    RuntimeDatabasePoolShutdownErrorV1, RuntimeDiscordGatewayShutdownFailureV1,
    RuntimeOwnerHeldProcessShutdownErrorV1,
};

#[test]
fn ingress_acknowledgement_schedule_is_fail_closed_at_expiry_and_safety_equality() {
    let now = Instant::now();
    assert_eq!(
        ingress_acknowledgement_schedule_from_remaining_v2(Duration::ZERO, now, now),
        None
    );
    assert_eq!(
        ingress_acknowledgement_schedule_from_remaining_v2(
            INGRESS_ACKNOWLEDGEMENT_SAFETY_MARGIN_V2,
            now,
            now,
        ),
        None
    );

    let remaining = INGRESS_ACKNOWLEDGEMENT_SAFETY_MARGIN_V2 + Duration::from_nanos(1);
    let schedule = ingress_acknowledgement_schedule_from_remaining_v2(remaining, now, now).unwrap();
    assert_eq!(schedule.refresh_at, now);
    assert_eq!(schedule.safety_deadline, now + Duration::from_nanos(1));
}

#[test]
fn ingress_acknowledgement_schedule_refreshes_before_its_safety_deadline() {
    let now = Instant::now();
    let schedule = ingress_acknowledgement_schedule_from_remaining_v2(
        INGRESS_ACKNOWLEDGEMENT_LEASE_V2,
        now,
        now,
    )
    .unwrap();
    assert_eq!(
        schedule.refresh_at,
        now + INGRESS_ACKNOWLEDGEMENT_LEASE_V2
            .checked_sub(INGRESS_ACKNOWLEDGEMENT_REFRESH_ADVANCE_V2)
            .unwrap()
    );
    assert_eq!(
        schedule.safety_deadline,
        now + INGRESS_ACKNOWLEDGEMENT_LEASE_V2
            .checked_sub(INGRESS_ACKNOWLEDGEMENT_SAFETY_MARGIN_V2)
            .unwrap()
    );
    assert!(schedule.refresh_at < schedule.safety_deadline);
}

#[test]
fn ingress_acknowledgement_schedule_keeps_the_pre_observation_monotonic_anchor() {
    let observation_started_at = Instant::now();
    let observed_at = observation_started_at + Duration::from_secs(7);
    let schedule = ingress_acknowledgement_schedule_from_remaining_v2(
        INGRESS_ACKNOWLEDGEMENT_LEASE_V2,
        observation_started_at,
        observed_at,
    )
    .unwrap();
    assert_eq!(
        schedule.refresh_at,
        observation_started_at + Duration::from_secs(5)
    );
    assert_eq!(
        schedule.safety_deadline,
        observation_started_at + Duration::from_secs(8)
    );
    assert_eq!(
        ingress_acknowledgement_schedule_from_remaining_v2(
            INGRESS_ACKNOWLEDGEMENT_LEASE_V2,
            observation_started_at,
            schedule.safety_deadline,
        ),
        None
    );
}

const FAKE_PROCESS_CLEANUP_EVENTS_V2: [&str; 8] = [
    "discord_start",
    "discord_done",
    "owner_start",
    "owner_done",
    "database_start",
    "database_done",
    "finish_owner",
    "finish_all",
];
const FAKE_FINALIZE_CLEANUP_EVENTS_V2: [&str; 9] = [
    "finalize_cleanup",
    "discord_start",
    "discord_done",
    "owner_start",
    "owner_done",
    "database_start",
    "database_done",
    "finish_owner",
    "finish_all",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FakeObservedProcessV2 {
    Continue,
    FixedPoint,
}

#[derive(Clone)]
enum FakeObservationStepV2 {
    Complete(FakeObservedProcessV2),
    Failure(RuntimeProcessStartupRecoveryObservationFailureV2),
    Interrupt {
        owner: bool,
        active: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    },
    Pending {
        active: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    },
}

struct FakeObservationProcessResourceV2 {
    checks: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    pre_failure: Option<RuntimeProcessStartupRecoveryObservationFailureV2>,
    post_failure: Option<RuntimeProcessStartupRecoveryObservationFailureV2>,
    step: FakeObservationStepV2,
}

struct FakePendingObservationGuardV2 {
    active: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}

impl Drop for FakePendingObservationGuardV2 {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
        self.dropped.fetch_add(1, Ordering::AcqRel);
    }
}

impl RuntimeStartupRecoveryObservationProcessStepV2<()> for FakeObservationProcessResourceV2 {
    type Observed = FakeObservedProcessV2;

    fn current_failure_v2(&self) -> Option<RuntimeProcessStartupRecoveryObservationFailureV2> {
        match self.checks.fetch_add(1, Ordering::AcqRel) {
            0 => self.pre_failure,
            _ => self.post_failure,
        }
    }

    fn observe_once_v2<'a>(
        &'a mut self,
        _observer: &'a (),
    ) -> impl Future<
        Output = Result<Self::Observed, RuntimeProcessStartupRecoveryObservationFailureV2>,
    > + Send
           + 'a
    where
        (): 'a,
    {
        let calls = self.calls.clone();
        let step = self.step.clone();
        async move {
            calls.fetch_add(1, Ordering::AcqRel);
            match step {
                FakeObservationStepV2::Complete(observed) => Ok(observed),
                FakeObservationStepV2::Failure(failure) => Err(failure),
                FakeObservationStepV2::Interrupt {
                    owner,
                    active,
                    dropped,
                } => {
                    let discord_active = active.clone();
                    let discord_terminal = async move {
                        if owner {
                            pending::<RuntimeProcessPausedConnectedTransitionFailureV1>().await
                        } else {
                            while discord_active.load(Ordering::Acquire) == 0 {
                                tokio::task::yield_now().await;
                            }
                            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated
                        }
                    };
                    let owner_active = active.clone();
                    let owner_terminal = async move {
                        if owner {
                            while owner_active.load(Ordering::Acquire) == 0 {
                                tokio::task::yield_now().await;
                            }
                        } else {
                            pending::<()>().await;
                        }
                    };
                    let interrupt = await_startup_recovery_observation_interrupt_v2(
                        discord_terminal,
                        owner_terminal,
                        pending(),
                    );
                    let pending_observation = async move {
                        active.fetch_add(1, Ordering::AcqRel);
                        let _guard = FakePendingObservationGuardV2 { active, dropped };
                        pending::<FakeObservedProcessV2>().await
                    };
                    tokio::pin!(interrupt);
                    tokio::pin!(pending_observation);
                    tokio::select! {
                        biased;
                        interrupt = &mut interrupt => {
                            Err(match interrupt {
                                RuntimeStartupRecoveryObservationInterruptV2::Discord(error) => {
                                    RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(error)
                                }
                                RuntimeStartupRecoveryObservationInterruptV2::Owner => {
                                    RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                                        RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                                    )
                                }
                                RuntimeStartupRecoveryObservationInterruptV2::Shutdown(cause) => {
                                    RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                                        RuntimeProcessPausedConnectedTransitionFailureV1::ProcessShutdown(cause),
                                    )
                                }
                            })
                        }
                        observed = &mut pending_observation => Ok(observed),
                    }
                }
                FakeObservationStepV2::Pending { active, dropped } => {
                    active.fetch_add(1, Ordering::AcqRel);
                    let _guard = FakePendingObservationGuardV2 { active, dropped };
                    pending::<
                        Result<
                            FakeObservedProcessV2,
                            RuntimeProcessStartupRecoveryObservationFailureV2,
                        >,
                    >()
                    .await
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FakeFinalizeCleanupKindV2 {
    Ready,
    Retained,
    Outcome,
}

struct FakeFinalizeResourceV2 {
    current: bool,
    events: Arc<Mutex<Vec<&'static str>>>,
    finalize_calls: Arc<AtomicUsize>,
}

#[derive(Debug)]
struct FakeFinalizedOutcomeV2 {
    kind: FakeObservedProcessV2,
    current: bool,
    events: Arc<Mutex<Vec<&'static str>>>,
}

#[derive(Debug)]
struct FakeFinalizeFailureV2 {
    transition: RuntimeProcessStartupRecoveryObservationFailureV2,
    cleanup_kind: FakeFinalizeCleanupKindV2,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl FakeFinalizeFailureV2 {
    async fn cleanup(self) {
        push_fake_event_v2(&self.events, "finalize_cleanup");
        run_fake_process_cleanup_v2(self.events).await;
    }
}

fn finalize_fake_process_step_v2(
    resource: FakeFinalizeResourceV2,
    kind: FakeObservedProcessV2,
    finalizer_failure: bool,
    outcome_current: bool,
) -> Result<FakeFinalizedOutcomeV2, FakeFinalizeFailureV2> {
    finalize_startup_recovery_process_step_v2(
        resource,
        kind,
        |resource| {
            resource.current.then_some(
                RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
                ),
            )
        },
        |resource, transition| FakeFinalizeFailureV2 {
            transition,
            cleanup_kind: FakeFinalizeCleanupKindV2::Ready,
            events: resource.events,
        },
        move |resource, observed| {
            resource.finalize_calls.fetch_add(1, Ordering::AcqRel);
            if finalizer_failure {
                return Err(FakeFinalizeFailureV2 {
                    transition:
                        RuntimeProcessStartupRecoveryObservationFailureV2::GatewayProtocolViolation,
                    cleanup_kind: FakeFinalizeCleanupKindV2::Retained,
                    events: resource.events,
                });
            }
            Ok(FakeFinalizedOutcomeV2 {
                kind: observed,
                current: outcome_current,
                events: resource.events,
            })
        },
        |outcome| {
            outcome.current.then_some(
                RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                    RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
                ),
            )
        },
        |outcome, transition| FakeFinalizeFailureV2 {
            transition,
            cleanup_kind: FakeFinalizeCleanupKindV2::Outcome,
            events: outcome.events,
        },
    )
}

fn push_fake_event_v2(events: &Arc<Mutex<Vec<&'static str>>>, event: &'static str) {
    events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(event);
}

async fn run_fake_process_cleanup_v2(events: Arc<Mutex<Vec<&'static str>>>) {
    let discord_start = events.clone();
    let owner_start = events.clone();
    let database_start = events.clone();
    let finish_owner = events.clone();
    let finish_all = events;
    sequence_startup_observation_cleanup_v2(
        move || {
            push_fake_event_v2(&discord_start, "discord_start");
            let events = discord_start.clone();
            async move {
                push_fake_event_v2(&events, "discord_done");
                Ok::<(), ()>(())
            }
        },
        move || {
            push_fake_event_v2(&owner_start, "owner_start");
            let events = owner_start.clone();
            async move {
                push_fake_event_v2(&events, "owner_done");
                Ok::<(), ()>(())
            }
        },
        move || {
            push_fake_event_v2(&database_start, "database_start");
            let events = database_start.clone();
            async move {
                push_fake_event_v2(&events, "database_done");
                Ok::<(), ()>(())
            }
        },
        move |owner, database| {
            assert!(owner.is_ok());
            assert!(database.is_ok());
            push_fake_event_v2(&finish_owner, "finish_owner");
            Ok::<(), ()>(())
        },
        move |discord, owner_held| {
            assert!(discord.is_ok());
            assert!(owner_held.is_ok());
            push_fake_event_v2(&finish_all, "finish_all");
            Ok::<(), ()>(())
        },
    )
    .await
    .unwrap();
}

fn fake_observation_resource_v2(step: FakeObservationStepV2) -> FakeObservationProcessResourceV2 {
    FakeObservationProcessResourceV2 {
        checks: Arc::new(AtomicUsize::new(0)),
        calls: Arc::new(AtomicUsize::new(0)),
        pre_failure: None,
        post_failure: None,
        step,
    }
}

fn fake_events_v2(events: &Arc<Mutex<Vec<&'static str>>>) -> Vec<&'static str> {
    events
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[tokio::test]
async fn generic_process_flow_finalizes_and_shuts_down_both_typed_outcomes() {
    for kind in [
        FakeObservedProcessV2::Continue,
        FakeObservedProcessV2::FixedPoint,
    ] {
        let mut observation = fake_observation_resource_v2(FakeObservationStepV2::Complete(kind));
        let observed = observe_startup_recovery_process_step_v2(&mut observation, &())
            .await
            .unwrap();
        assert_eq!(observed, kind);
        assert_eq!(observation.calls.load(Ordering::Acquire), 1);
        assert_eq!(observation.checks.load(Ordering::Acquire), 2);

        let events = Arc::new(Mutex::new(Vec::new()));
        let finalize_calls = Arc::new(AtomicUsize::new(0));
        let outcome = finalize_fake_process_step_v2(
            FakeFinalizeResourceV2 {
                current: false,
                events: events.clone(),
                finalize_calls: finalize_calls.clone(),
            },
            observed,
            false,
            false,
        )
        .unwrap();
        assert_eq!(outcome.kind, kind);
        assert_eq!(finalize_calls.load(Ordering::Acquire), 1);

        run_fake_process_cleanup_v2(outcome.events).await;
        assert_eq!(fake_events_v2(&events), FAKE_PROCESS_CLEANUP_EVENTS_V2);
    }
}

#[tokio::test]
async fn generic_process_failures_retain_cleanup_for_deadline_observer_and_state_change() {
    let cases = [
        (
            RuntimeProcessStartupRecoveryObservationFailureV2::OperationDeadlineElapsed,
            true,
        ),
        (
            RuntimeProcessStartupRecoveryObservationFailureV2::OperationDeadlineElapsed,
            false,
        ),
        (
            RuntimeProcessStartupRecoveryObservationFailureV2::ObservationUnavailable,
            false,
        ),
        (
            RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            ),
            false,
        ),
    ];

    for (expected, preflight) in cases {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut resource = fake_observation_resource_v2(if preflight {
            FakeObservationStepV2::Complete(FakeObservedProcessV2::Continue)
        } else {
            FakeObservationStepV2::Failure(expected)
        });
        if preflight {
            resource.pre_failure = Some(expected);
        } else if matches!(
            expected,
            RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(_)
        ) {
            resource.step = FakeObservationStepV2::Complete(FakeObservedProcessV2::Continue);
            resource.post_failure = Some(expected);
        }

        let error = observe_startup_recovery_process_step_v2(&mut resource, &())
            .await
            .unwrap_err();

        assert_eq!(error, expected);
        assert_eq!(
            resource.calls.load(Ordering::Acquire),
            usize::from(!preflight)
        );
        run_fake_process_cleanup_v2(events.clone()).await;
        assert_eq!(fake_events_v2(&events), FAKE_PROCESS_CLEANUP_EVENTS_V2);
    }
}

#[tokio::test]
async fn generic_process_interrupts_drop_the_observer_and_preserve_cleanup() {
    for (owner, expected) in [
        (
            false,
            RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            ),
        ),
        (
            true,
            RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
        ),
    ] {
        let active = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut resource = fake_observation_resource_v2(FakeObservationStepV2::Interrupt {
            owner,
            active: active.clone(),
            dropped: dropped.clone(),
        });

        let error = observe_startup_recovery_process_step_v2(&mut resource, &())
            .await
            .unwrap_err();

        assert_eq!(error, expected);
        assert_eq!(resource.calls.load(Ordering::Acquire), 1);
        assert_eq!(resource.checks.load(Ordering::Acquire), 1);
        assert_eq!(active.load(Ordering::Acquire), 0);
        assert_eq!(dropped.load(Ordering::Acquire), 1);
        let events = Arc::new(Mutex::new(Vec::new()));
        run_fake_process_cleanup_v2(events.clone()).await;
        assert_eq!(fake_events_v2(&events), FAKE_PROCESS_CLEANUP_EVENTS_V2);
    }
}

#[tokio::test]
async fn generic_process_future_drop_preserves_resource_and_cleanup_authority() {
    let active = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut polled = fake_observation_resource_v2(FakeObservationStepV2::Pending {
        active: active.clone(),
        dropped: dropped.clone(),
    });
    let mut future = Box::pin(observe_startup_recovery_process_step_v2(&mut polled, &()));
    std::future::poll_fn(|context| {
        assert!(Future::poll(future.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;

    drop(future);

    assert_eq!(polled.calls.load(Ordering::Acquire), 1);
    assert_eq!(polled.checks.load(Ordering::Acquire), 1);
    assert_eq!(active.load(Ordering::Acquire), 0);
    assert_eq!(dropped.load(Ordering::Acquire), 1);
    let events = Arc::new(Mutex::new(Vec::new()));
    run_fake_process_cleanup_v2(events.clone()).await;
    assert_eq!(fake_events_v2(&events).last(), Some(&"finish_all"));

    let mut unpolled = fake_observation_resource_v2(FakeObservationStepV2::Complete(
        FakeObservedProcessV2::FixedPoint,
    ));
    let future = observe_startup_recovery_process_step_v2(&mut unpolled, &());
    drop(future);
    assert_eq!(unpolled.calls.load(Ordering::Acquire), 0);
    assert_eq!(unpolled.checks.load(Ordering::Acquire), 0);
    let observed = observe_startup_recovery_process_step_v2(&mut unpolled, &())
        .await
        .unwrap();
    assert_eq!(observed, FakeObservedProcessV2::FixedPoint);
    assert_eq!(unpolled.calls.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn generic_finalize_retains_each_authority_shape_for_ordered_cleanup() {
    let cases = [
        (
            true,
            false,
            false,
            FakeFinalizeCleanupKindV2::Ready,
            RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            ),
            0,
        ),
        (
            false,
            true,
            false,
            FakeFinalizeCleanupKindV2::Retained,
            RuntimeProcessStartupRecoveryObservationFailureV2::GatewayProtocolViolation,
            1,
        ),
        (
            false,
            false,
            true,
            FakeFinalizeCleanupKindV2::Outcome,
            RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
                RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
            1,
        ),
    ];

    for (
        resource_current,
        finalizer_failure,
        outcome_current,
        cleanup_kind,
        transition,
        expected_finalize_calls,
    ) in cases
    {
        let events = Arc::new(Mutex::new(Vec::new()));
        let finalize_calls = Arc::new(AtomicUsize::new(0));
        let failure = finalize_fake_process_step_v2(
            FakeFinalizeResourceV2 {
                current: resource_current,
                events: events.clone(),
                finalize_calls: finalize_calls.clone(),
            },
            FakeObservedProcessV2::Continue,
            finalizer_failure,
            outcome_current,
        )
        .unwrap_err();

        assert_eq!(failure.transition, transition);
        assert_eq!(failure.cleanup_kind, cleanup_kind);
        assert_eq!(
            finalize_calls.load(Ordering::Acquire),
            expected_finalize_calls
        );
        failure.cleanup().await;
        assert_eq!(fake_events_v2(&events), FAKE_FINALIZE_CLEANUP_EVENTS_V2);
    }
}

#[tokio::test]
async fn interrupt_race_prioritizes_discord_when_both_signals_are_ready() {
    let interrupt = await_startup_recovery_observation_interrupt_v2(
        ready(RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated),
        ready(()),
        pending(),
    )
    .await;

    assert!(matches!(
        interrupt,
        RuntimeStartupRecoveryObservationInterruptV2::Discord(
            RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated
        )
    ));
}

#[tokio::test]
async fn interrupt_race_observes_owner_termination_while_discord_is_live() {
    let interrupt = await_startup_recovery_observation_interrupt_v2(
        pending::<RuntimeProcessPausedConnectedTransitionFailureV1>(),
        ready(()),
        pending(),
    )
    .await;

    assert!(matches!(
        interrupt,
        RuntimeStartupRecoveryObservationInterruptV2::Owner
    ));
}

#[tokio::test]
async fn production_handoff_shutdown_wins_at_every_long_wait_boundary() {
    for _boundary in 0..3 {
        let stage_polls = Arc::new(AtomicUsize::new(0));
        let stage_polls_for_future = stage_polls.clone();
        let latch = crate::process_supervisor::create_runtime_process_shutdown_latch_v1();
        let trigger = latch.trigger();
        let mut shutdown = latch.observer();
        trigger.trip(crate::RuntimeShutdownCauseV1::Explicit);
        let output = await_production_handoff_stage_v2(
            async move {
                stage_polls_for_future.fetch_add(1, Ordering::AcqRel);
                ready::<()>(()).await
            },
            &mut shutdown,
        )
        .await;
        assert_eq!(output, None);
        assert_eq!(stage_polls.load(Ordering::Acquire), 0);
        assert_eq!(
            trigger.observed().unwrap().cause(),
            crate::RuntimeShutdownCauseV1::Explicit
        );
    }
}

#[tokio::test]
async fn production_handoff_shutdown_cancels_a_started_stage_without_losing_its_owner() {
    for _boundary in 0..3 {
        let active = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let stage_active = active.clone();
        let stage_dropped = dropped.clone();
        let latch = crate::process_supervisor::create_runtime_process_shutdown_latch_v1();
        let trigger = latch.trigger();
        let mut shutdown = latch.observer();
        let mut handoff = Box::pin(await_production_handoff_stage_v2(
            async move {
                stage_active.fetch_add(1, Ordering::AcqRel);
                let _guard = FakePendingObservationGuardV2 {
                    active: stage_active,
                    dropped: stage_dropped,
                };
                pending::<()>().await
            },
            &mut shutdown,
        ));
        std::future::poll_fn(|context| {
            assert!(Future::poll(handoff.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(active.load(Ordering::Acquire), 1);
        assert!(trigger
            .trip(crate::RuntimeShutdownCauseV1::Explicit)
            .first());
        assert_eq!(handoff.await, None);
        assert_eq!(active.load(Ordering::Acquire), 0);
        assert_eq!(dropped.load(Ordering::Acquire), 1);
    }
}

#[test]
fn production_handoff_final_revalidation_rejects_latched_root_shutdown() {
    let latch = crate::process_supervisor::create_runtime_process_shutdown_latch_v1();
    let shutdown = latch.observer();
    assert_eq!(production_handoff_shutdown_failure_v2(&shutdown), None);

    latch
        .trigger()
        .trip(crate::RuntimeShutdownCauseV1::Explicit);

    assert_eq!(
        production_handoff_shutdown_failure_v2(&shutdown),
        Some(RuntimeProcessProductionHandoffFailureV2::ProcessShutdown)
    );
}

#[test]
fn observation_failures_map_to_finite_public_classes() {
    let operation_cutoff = Instant::now() + Duration::from_secs(2);
    let owner_safety_deadline = Instant::now() + Duration::from_secs(1);
    let failures = [
        map_observation_failure_v2::<()>(
            RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed,
            operation_cutoff,
            owner_safety_deadline,
        ),
        map_observation_failure_v2(
            RuntimeClosedRecoveryStartupObservationErrorV2::Observer("private"),
            operation_cutoff,
            owner_safety_deadline,
        ),
        map_observation_failure_v2::<()>(
            RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::Gateway(
                    RuntimeGatewayReadyObservationErrorV1::Draining,
                ),
            ),
            operation_cutoff,
            owner_safety_deadline,
        ),
        map_observation_failure_v2::<()>(
            RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::Coordinator(
                    RuntimeGatewayClosedTransitionErrorV2::Shutdown,
                ),
            ),
            operation_cutoff,
            owner_safety_deadline,
        ),
        map_observation_failure_v2::<()>(
            RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(
                RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
            ),
            operation_cutoff,
            owner_safety_deadline,
        ),
        map_observation_failure_v2::<()>(
            RuntimeClosedRecoveryStartupObservationErrorV2::Registry(
                RuntimeRegistryRecoveryObservationErrorV1::FailedClosed,
            ),
            operation_cutoff,
            owner_safety_deadline,
        ),
        map_observation_failure_v2::<()>(
            RuntimeClosedRecoveryStartupObservationErrorV2::Owner(
                RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SupervisorUnavailable,
            ),
            operation_cutoff,
            owner_safety_deadline,
        ),
    ];

    assert_eq!(
        failures[0],
        RuntimeProcessStartupRecoveryObservationFailureV2::PausedConnection(
            RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
        )
    );
    assert_eq!(
        failures[1],
        RuntimeProcessStartupRecoveryObservationFailureV2::ObservationUnavailable
    );
    assert_eq!(
        failures[2],
        RuntimeProcessStartupRecoveryObservationFailureV2::GatewayObservation(
            RuntimeGatewayReadyObservationErrorV1::Draining,
        )
    );
    assert_eq!(
        failures[3],
        RuntimeProcessStartupRecoveryObservationFailureV2::GatewayCoordinator
    );
    assert_eq!(
        failures[4],
        RuntimeProcessStartupRecoveryObservationFailureV2::GatewayProtocolViolation
    );
    assert_eq!(
        failures[5],
        RuntimeProcessStartupRecoveryObservationFailureV2::Registry(
            RuntimeRegistryRecoveryObservationErrorV1::FailedClosed,
        )
    );
    assert_eq!(
        failures[6],
        RuntimeProcessStartupRecoveryObservationFailureV2::OwnerLifetime(
            RuntimeProcessGatewayOwnerCommitFailureV2::SupervisorUnavailable,
        )
    );
    for failure in failures {
        assert!(!failure.code().is_empty());
        assert_eq!(failure.context(), None);
    }
}

#[test]
fn operation_deadline_is_distinct_when_it_precedes_owner_safety() {
    let operation_cutoff = Instant::now() + Duration::from_secs(1);
    let owner_safety_deadline = Instant::now() + Duration::from_secs(2);

    assert_eq!(
        classify_observation_deadline_v2(operation_cutoff, owner_safety_deadline),
        RuntimeProcessStartupRecoveryObservationFailureV2::OperationDeadlineElapsed,
    );
}

#[test]
fn cleanup_failure_preserves_transition_class_without_exposing_sources() {
    let transition = RuntimeProcessStartupRecoveryObservationFailureV2::ObservationUnavailable;
    let clean = finish_observation_transition_v2(transition, Ok(()));
    let cleanup = finish_observation_transition_v2(
        transition,
        Err(
            RuntimePausedConnectedProcessShutdownErrorV1::DiscordAndOwnerHeld {
                discord: RuntimeDiscordGatewayShutdownFailureV1::UnexpectedExit,
                owner_held: RuntimeOwnerHeldProcessShutdownErrorV1::GatewayOwnerAndDatabase {
                    owner: crate::RuntimeGatewayOwnerShutdownFailureV1::DeadlineElapsed,
                    database: RuntimeDatabasePoolShutdownErrorV1::TimedOut,
                },
            },
        ),
    );

    assert_eq!(
        clean,
        RuntimeProcessStartupRecoveryObservationErrorV2::Transition(transition)
    );
    assert!(matches!(
        cleanup,
        RuntimeProcessStartupRecoveryObservationErrorV2::CleanupAfterTransition {
            transition: RuntimeProcessStartupRecoveryObservationFailureV2::ObservationUnavailable,
            ..
        }
    ));
    assert_eq!(
        cleanup.code(),
        "runtime_process_startup_recovery_observation_transition_cleanup"
    );
    assert_eq!(cleanup.context(), None);
    assert_eq!(
        format!("{cleanup:?}"),
        "RuntimeProcessStartupRecoveryObservationErrorV2(<redacted>)"
    );
    assert!(std::error::Error::source(&cleanup).is_none());
}

#[test]
fn production_handoff_cleanup_reports_the_redacted_transition_code() {
    let transition = RuntimeProcessProductionHandoffFailureV2::Gateway;
    let clean = finish_production_handoff_transition_v2(transition, Ok(()));
    let cleanup = finish_production_handoff_transition_v2(
        transition,
        Err(RuntimeClosedRecoveryProcessCleanupFailureV2::OwnerHeld(
            RuntimeOwnerHeldProcessShutdownErrorV1::Database(
                RuntimeDatabasePoolShutdownErrorV1::TimedOut,
            ),
        )),
    );

    assert_eq!(
        clean,
        RuntimeProcessProductionHandoffErrorV2::Transition(transition)
    );
    assert_eq!(clean.context(), None);
    assert_eq!(
        cleanup.code(),
        "runtime_process_production_handoff_transition_cleanup"
    );
    assert_eq!(
        cleanup.context(),
        Some("runtime_process_production_handoff_gateway")
    );
    assert_eq!(
        format!("{cleanup:?}"),
        "RuntimeProcessProductionHandoffErrorV2(<redacted>)"
    );
    assert!(std::error::Error::source(&cleanup).is_none());
}

#[test]
fn observation_outer_shutdown_error_forces_failed_closed_terminal_total() {
    let (recorder, observer) =
        crate::lifecycle_timing::RuntimeLifecycleTimingRecorderV2::create_v2();
    let terminal = crate::lifecycle_timing::RuntimeLifecycleTimingTerminalReporterV2::new_v2(
        recorder,
        observer.clone(),
    );
    let result = finish_observation_shutdown_timing_v2(
        terminal,
        Err::<(), _>(RuntimeClosedRecoveryProcessCleanupFailureV2::Discord(
            RuntimeDiscordGatewayShutdownFailureV1::UnexpectedExit,
        )),
    );
    assert!(result.is_err());
    assert_eq!(
        observer
            .snapshot_v2()
            .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownTotal)
            .unwrap()
            .outcome(),
        RuntimeLifecycleTimingOutcomeV2::FailedClosed
    );
    assert_eq!(observer.terminal_emission_count_v2(), 1);
}

#[tokio::test]
async fn observation_phase_wrapper_preserves_discord_and_owner_deadlines() {
    let (recorder, observer) =
        crate::lifecycle_timing::RuntimeLifecycleTimingRecorderV2::create_v2();
    let discord = ready(Err::<
        (),
        crate::discord::RuntimeDiscordGatewayShutdownErrorV1,
    >(
        crate::discord::RuntimeDiscordGatewayShutdownErrorV1::CloseDeadlineElapsed,
    ));
    let _ = time_shutdown_result_v2(
        &recorder,
        RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin,
        discord,
        discord_shutdown_timing_outcome_v2,
    )
    .await;
    let owner = ready(Err::<
        RuntimeGatewayOwnerStartupWatchdogExitV1,
        crate::gateway_owner_startup_watchdog::
            RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1,
    >(
        crate::gateway_owner_startup_watchdog::
            RuntimeGatewayOwnerStartupWatchdogShutdownErrorV1::DeadlineElapsed,
    ));
    let _ = time_shutdown_result_v2(
        &recorder,
        RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin,
        owner,
        owner_shutdown_timing_outcome_v2,
    )
    .await;
    let snapshot = observer.snapshot_v2();
    assert_eq!(
        snapshot
            .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownGatewayDrainJoin)
            .unwrap()
            .outcome(),
        RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
    );
    assert_eq!(
        snapshot
            .sample_v2(RuntimeLifecycleTimingMetricV2::ShutdownOwnerJoin)
            .unwrap()
            .outcome(),
        RuntimeLifecycleTimingOutcomeV2::DeadlineElapsed
    );
}
