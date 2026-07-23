use std::collections::VecDeque;
use std::future::{ready, Future};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeBuildRevisionV1, RuntimeGatewayOwnerLeaseDurationV1, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseObservationV1, RuntimeGatewayOwnerLeaseReceiptV1,
    RuntimeObserveGatewayOwnerLeaseV1, RuntimeObservedGatewayOwnerLeaseV1,
    RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeReleaseGatewayOwnerLeaseV1,
    RuntimeRenewGatewayOwnerLeaseOutcomeV1, RuntimeRenewGatewayOwnerLeaseV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    accept_gateway_owner_acquire_v1, RuntimeAcceptedGatewayOwnerAcquireV1,
    RuntimeAcceptedGatewayOwnerReceiptV1, RuntimeGatewayEmergencyCauseV2,
    RuntimeGatewayOwnerLeasePortV1, RuntimeGatewayOwnerMutationErrorV1,
};
use chrono::{DateTime, TimeDelta, Utc};
use starring_runtime::{
    compose_runtime_gateway_bootstrap_v1, GatewayResourceConfigV1, RuntimeGatewayBootstrapV1,
    RuntimeGatewayOwnerStartupWatchdogConfigErrorV1, RuntimeGatewayOwnerStartupWatchdogConfigV1,
    RuntimeGatewayOwnerStartupWatchdogExitV1, RuntimeGatewayOwnerStartupWatchdogStartErrorV1,
};
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FakePortErrorV1 {
    Transport,
}

#[derive(Clone)]
enum FakeRenewStepV1 {
    Renewed,
    DefinitelyNotApplied,
    OutcomeUnknown,
    OutcomeUnknownApplied,
    Blocked(Arc<Notify>),
}

#[derive(Clone)]
enum FakeReleaseStepV1 {
    Released,
    DefinitelyNotApplied,
    OutcomeUnknown,
    ProtocolViolation,
}

#[derive(Clone)]
struct FakeGatewayOwnerPortV1 {
    state: Arc<FakeGatewayOwnerPortStateV1>,
}

struct FakeGatewayOwnerPortStateV1 {
    receipt: Mutex<RuntimeGatewayOwnerLeaseReceiptV1>,
    renew_steps: Mutex<VecDeque<FakeRenewStepV1>>,
    renew_calls: AtomicUsize,
    active_renewals: AtomicUsize,
    maximum_active_renewals: AtomicUsize,
    release_calls: AtomicUsize,
    release_steps: Mutex<VecDeque<FakeReleaseStepV1>>,
    release_gate: Mutex<Option<Arc<Notify>>>,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl FakeGatewayOwnerPortV1 {
    fn new(
        receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        steps: impl IntoIterator<Item = FakeRenewStepV1>,
        events: Arc<Mutex<Vec<&'static str>>>,
    ) -> Self {
        Self {
            state: Arc::new(FakeGatewayOwnerPortStateV1 {
                receipt: Mutex::new(receipt),
                renew_steps: Mutex::new(steps.into_iter().collect()),
                renew_calls: AtomicUsize::new(0),
                active_renewals: AtomicUsize::new(0),
                maximum_active_renewals: AtomicUsize::new(0),
                release_calls: AtomicUsize::new(0),
                release_steps: Mutex::new(VecDeque::new()),
                release_gate: Mutex::new(None),
                events,
            }),
        }
    }

    fn renew_calls(&self) -> usize {
        self.state.renew_calls.load(Ordering::Acquire)
    }

    fn maximum_active_renewals(&self) -> usize {
        self.state.maximum_active_renewals.load(Ordering::Acquire)
    }

    fn release_calls(&self) -> usize {
        self.state.release_calls.load(Ordering::Acquire)
    }

    fn block_release(&self, gate: Arc<Notify>) {
        *self
            .state
            .release_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(gate);
    }

    fn set_release_steps(&self, steps: impl IntoIterator<Item = FakeReleaseStepV1>) {
        *self
            .state
            .release_steps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = steps.into_iter().collect();
    }

    fn events(&self) -> Arc<Mutex<Vec<&'static str>>> {
        self.state.events.clone()
    }
}

impl RuntimeGatewayOwnerLeasePortV1 for FakeGatewayOwnerPortV1 {
    type Error = FakePortErrorV1;

    fn observe_gateway_owner(
        &self,
        _request: RuntimeObserveGatewayOwnerLeaseV1,
    ) -> impl Future<Output = Result<RuntimeGatewayOwnerLeaseObservationV1, Self::Error>> + Send
    {
        let receipt = self
            .state
            .receipt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        ready(Ok(RuntimeGatewayOwnerLeaseObservationV1::Owned(
            RuntimeObservedGatewayOwnerLeaseV1 {
                lease_id: receipt.lease_id,
                owner_revision: receipt.owner_revision,
                observed_database_now: receipt.database_now,
                expires_at: receipt.expires_at,
            },
        )))
    }

    fn acquire_gateway_owner(
        &self,
        _request: RuntimeAcquireGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeAcquireGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send {
        let receipt = self
            .state
            .receipt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        ready(Ok(RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(
            receipt,
        )))
    }

    fn renew_gateway_owner(
        &self,
        request: RuntimeRenewGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeRenewGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send {
        let state = self.state.clone();
        async move {
            state.renew_calls.fetch_add(1, Ordering::AcqRel);
            let active = state.active_renewals.fetch_add(1, Ordering::AcqRel) + 1;
            state
                .maximum_active_renewals
                .fetch_max(active, Ordering::AcqRel);
            let step = state
                .renew_steps
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
                .unwrap_or(FakeRenewStepV1::Renewed);
            let result = match step {
                FakeRenewStepV1::Renewed => Ok(renewed_outcome(&state, &request)),
                FakeRenewStepV1::DefinitelyNotApplied => {
                    Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied {
                        source: FakePortErrorV1::Transport,
                    })
                }
                FakeRenewStepV1::OutcomeUnknown => {
                    Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown {
                        source: FakePortErrorV1::Transport,
                    })
                }
                FakeRenewStepV1::OutcomeUnknownApplied => {
                    let _outcome = renewed_outcome(&state, &request);
                    Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown {
                        source: FakePortErrorV1::Transport,
                    })
                }
                FakeRenewStepV1::Blocked(gate) => {
                    gate.notified().await;
                    Ok(renewed_outcome(&state, &request))
                }
            };
            state.active_renewals.fetch_sub(1, Ordering::AcqRel);
            result
        }
    }

    fn release_gateway_owner(
        &self,
        request: RuntimeReleaseGatewayOwnerLeaseV1,
    ) -> impl Future<
        Output = Result<
            RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
            RuntimeGatewayOwnerMutationErrorV1<Self::Error>,
        >,
    > + Send {
        let state = self.state.clone();
        async move {
            state.release_calls.fetch_add(1, Ordering::AcqRel);
            state
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push("release");
            let gate = state
                .release_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(gate) = gate {
                gate.notified().await;
            }
            let step = state
                .release_steps
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
                .unwrap_or(FakeReleaseStepV1::Released);
            match step {
                FakeReleaseStepV1::Released => {
                    Ok(RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released {
                        lease_id: request.lease_id,
                        database_now: at_millis(1_000_000),
                    })
                }
                FakeReleaseStepV1::DefinitelyNotApplied => {
                    Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied {
                        source: FakePortErrorV1::Transport,
                    })
                }
                FakeReleaseStepV1::OutcomeUnknown => {
                    Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown {
                        source: FakePortErrorV1::Transport,
                    })
                }
                FakeReleaseStepV1::ProtocolViolation => {
                    let mut wrong = request.lease_id;
                    wrong.lease_epoch = non_zero(wrong.lease_epoch.get() + 1);
                    Ok(RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released {
                        lease_id: wrong,
                        database_now: at_millis(1_000_000),
                    })
                }
            }
        }
    }
}

fn renewed_outcome(
    state: &FakeGatewayOwnerPortStateV1,
    request: &RuntimeRenewGatewayOwnerLeaseV1,
) -> RuntimeRenewGatewayOwnerLeaseOutcomeV1 {
    let database_now = at_millis(1_000_000);
    let expires_at = database_now + TimeDelta::from_std(request.lease_for.get()).unwrap();
    let receipt = RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: request.lease_id.clone(),
        owner_revision: NonZeroU64::new(request.expected_owner_revision.get() + 1).unwrap(),
        database_now,
        expires_at,
    };
    *state
        .receipt
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = receipt.clone();
    RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt)
}

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at_millis(value: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(value).unwrap()
}

fn lease_id() -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
        lease_epoch: non_zero(7),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
    }
}

fn receipt(duration: Duration) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    let database_now = at_millis(100_000);
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: lease_id(),
        owner_revision: non_zero(3),
        database_now,
        expires_at: database_now + TimeDelta::from_std(duration).unwrap(),
    }
}

fn accepted_receipt(
    receipt: RuntimeGatewayOwnerLeaseReceiptV1,
) -> RuntimeAcceptedGatewayOwnerReceiptV1 {
    let request = RuntimeAcquireGatewayOwnerLeaseV1 {
        gateway_shard_id: receipt.lease_id.gateway_shard_id.clone(),
        process_instance_id: receipt.lease_id.process_instance_id.clone(),
        expected_build_revision: receipt.lease_id.expected_build_revision.clone(),
        lease_for: RuntimeGatewayOwnerLeaseDurationV1::new(Duration::from_secs(1)).unwrap(),
    };
    let accepted = accept_gateway_owner_acquire_v1(
        &request,
        RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(receipt),
    )
    .unwrap();
    let RuntimeAcceptedGatewayOwnerAcquireV1::Acquired(receipt) = accepted else {
        panic!("expected accepted owner receipt")
    };
    receipt
}

fn fixture(
    lease_for: Duration,
    renew_before: Duration,
    safety_margin: Duration,
    retry_delay: Duration,
    steps: impl IntoIterator<Item = FakeRenewStepV1>,
) -> (
    FakeGatewayOwnerPortV1,
    RuntimeGatewayBootstrapV1,
    RuntimeAcceptedGatewayOwnerReceiptV1,
    RuntimeGatewayOwnerStartupWatchdogConfigV1,
) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let receipt = receipt(lease_for);
    let port = FakeGatewayOwnerPortV1::new(receipt.clone(), steps, events.clone());
    let gateway = compose_runtime_gateway_bootstrap_v1(
        ProcessInstanceId::parse("process:1").unwrap(),
        GatewayResourceConfigV1::default(),
    )
    .unwrap();
    let config = RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
        lease_for,
        renew_before,
        safety_margin,
        retry_delay,
        Duration::from_millis(500),
    )
    .unwrap();
    (port, gateway, accepted_receipt(receipt), config)
}

fn ownership_is_uncertain(gateway: &RuntimeGatewayBootstrapV1) -> bool {
    matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            cause: RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
            ..
        }
    )
}

async fn wait_for(mut condition: impl FnMut() -> bool) {
    timeout(Duration::from_secs(2), async {
        while !condition() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn one_actor_dispatches_only_one_renewal_while_the_call_is_inflight() {
    let renewal_gate = Arc::new(Notify::new());
    let (port, mut gateway, first_receipt, config) = fixture(
        Duration::from_millis(800),
        Duration::from_millis(600),
        Duration::from_millis(100),
        Duration::from_millis(20),
        [FakeRenewStepV1::Blocked(renewal_gate.clone())],
    );
    let inspection = port.clone();
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, first_receipt, started, started, config)
        .unwrap();

    wait_for(|| inspection.renew_calls() == 1).await;
    sleep(Duration::from_millis(50)).await;
    assert_eq!(inspection.renew_calls(), 1);
    assert_eq!(inspection.maximum_active_renewals(), 1);
    renewal_gate.notify_one();
    assert_eq!(
        handle.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(inspection.maximum_active_renewals(), 1);
    assert_eq!(inspection.release_calls(), 1);
}

#[tokio::test]
async fn safety_deadline_invalidates_before_a_late_renewal_is_joined() {
    let renewal_gate = Arc::new(Notify::new());
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_millis(400),
        Duration::from_millis(300),
        Duration::from_millis(100),
        Duration::from_millis(20),
        [FakeRenewStepV1::Blocked(renewal_gate.clone())],
    );
    let started = Instant::now();
    let mut handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    wait_for(|| ownership_is_uncertain(&gateway)).await;
    assert_eq!(handle.terminal_status(), None);
    renewal_gate.notify_one();
    assert_eq!(
        handle.wait_terminal().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed
    );
}

#[tokio::test]
async fn unknown_renewal_invalidates_before_exact_release_cleanup() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_millis(400),
        Duration::from_millis(300),
        Duration::from_millis(100),
        Duration::from_millis(20),
        [FakeRenewStepV1::OutcomeUnknown],
    );
    let events = port.events();
    let started = Instant::now();
    let mut handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    assert_eq!(
        timeout(Duration::from_secs(2), handle.wait_terminal())
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
    );
    assert_eq!(
        *events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        ["release"]
    );
    assert!(ownership_is_uncertain(&gateway));
}

#[tokio::test]
async fn definite_failure_retries_serially_without_extending_the_deadline() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_millis(800),
        Duration::from_millis(600),
        Duration::from_millis(100),
        Duration::from_millis(20),
        [
            FakeRenewStepV1::DefinitelyNotApplied,
            FakeRenewStepV1::Renewed,
        ],
    );
    let inspection = port.clone();
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    wait_for(|| inspection.renew_calls() >= 2).await;
    assert_eq!(inspection.maximum_active_renewals(), 1);
    assert_eq!(
        handle.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
}

#[tokio::test]
async fn shutdown_acknowledges_only_after_release_cleanup_finishes() {
    let release_gate = Arc::new(Notify::new());
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    port.block_release(release_gate.clone());
    let inspection = port.clone();
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();
    let shutdown = tokio::spawn(handle.shutdown());

    wait_for(|| inspection.release_calls() == 1).await;
    assert!(!shutdown.is_finished());
    release_gate.notify_one();
    assert_eq!(
        shutdown.await.unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
}

#[tokio::test]
async fn shutdown_reports_unconfirmed_when_release_exhausts_the_cleanup_deadline() {
    let release_gate = Arc::new(Notify::new());
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    port.block_release(release_gate);
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    assert_eq!(
        timeout(Duration::from_secs(1), handle.shutdown())
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
    );
}

#[tokio::test]
async fn safety_cleanup_is_bounded_when_renewal_and_release_never_settle() {
    let renewal_gate = Arc::new(Notify::new());
    let release_gate = Arc::new(Notify::new());
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_millis(400),
        Duration::from_millis(300),
        Duration::from_millis(100),
        Duration::from_millis(20),
        [FakeRenewStepV1::Blocked(renewal_gate)],
    );
    port.block_release(release_gate);
    let started = Instant::now();
    let mut handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    assert_eq!(
        timeout(Duration::from_secs(1), handle.wait_terminal())
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
    );
    assert!(ownership_is_uncertain(&gateway));
}

#[tokio::test]
async fn repeated_unknown_release_that_still_observes_the_same_lease_is_unconfirmed() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    port.set_release_steps([
        FakeReleaseStepV1::OutcomeUnknown,
        FakeReleaseStepV1::OutcomeUnknown,
    ]);
    let inspection = port.clone();
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    assert_eq!(
        handle.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
    );
    assert_eq!(inspection.release_calls(), 2);
}

#[tokio::test]
async fn definitely_not_applied_release_exhaustion_is_unconfirmed() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    port.set_release_steps([
        FakeReleaseStepV1::DefinitelyNotApplied,
        FakeReleaseStepV1::DefinitelyNotApplied,
    ]);
    let inspection = port.clone();
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    assert_eq!(
        handle.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
    );
    assert_eq!(inspection.release_calls(), 2);
}

#[tokio::test]
async fn release_response_for_a_different_stable_lease_is_a_protocol_violation() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    port.set_release_steps([FakeReleaseStepV1::ProtocolViolation]);
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    assert_eq!(
        handle.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
}

#[tokio::test]
async fn a_shutdown_queued_before_the_actor_runs_prevents_a_due_renewal() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_millis(800),
        Duration::from_millis(600),
        Duration::from_millis(100),
        Duration::from_millis(20),
        [],
    );
    let inspection = port.clone();
    let started = Instant::now() - Duration::from_millis(250);
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    assert_eq!(
        handle.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(inspection.renew_calls(), 0);
}

#[tokio::test]
async fn the_bootstrap_rejects_a_second_watchdog_without_dispatching_renewal() {
    let (port, mut gateway, first_receipt, config) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, first_receipt, started, started, config)
        .unwrap();
    let second_receipt = receipt(Duration::from_secs(5));
    let second_port =
        FakeGatewayOwnerPortV1::new(second_receipt.clone(), [], Arc::new(Mutex::new(Vec::new())));
    let inspection = second_port.clone();
    let result = gateway.start_gateway_owner_startup_watchdog_v1(
        second_port,
        accepted_receipt(second_receipt),
        started,
        started,
        config,
    );
    let failure = match result {
        Ok(_) => panic!("second startup watchdog must be rejected"),
        Err(failure) => failure,
    };

    assert_eq!(
        failure.reason(),
        RuntimeGatewayOwnerStartupWatchdogStartErrorV1::AlreadyStarted
    );
    assert_eq!(inspection.renew_calls(), 0);
    assert!(ownership_is_uncertain(&gateway));
    assert_eq!(
        handle.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
}

#[tokio::test]
async fn bootstrap_rejects_a_receipt_for_another_process_before_spawn() {
    let mut wrong_receipt = receipt(Duration::from_secs(5));
    wrong_receipt.lease_id.process_instance_id = ProcessInstanceId::parse("process:2").unwrap();
    let port =
        FakeGatewayOwnerPortV1::new(wrong_receipt.clone(), [], Arc::new(Mutex::new(Vec::new())));
    let inspection = port.clone();
    let mut gateway = compose_runtime_gateway_bootstrap_v1(
        ProcessInstanceId::parse("process:1").unwrap(),
        GatewayResourceConfigV1::default(),
    )
    .unwrap();
    let config = RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        Duration::from_millis(500),
    )
    .unwrap();
    let started = Instant::now();
    let result = gateway.start_gateway_owner_startup_watchdog_v1(
        port,
        accepted_receipt(wrong_receipt),
        started,
        started,
        config,
    );
    let failure = match result {
        Ok(_) => panic!("foreign process receipt must be rejected"),
        Err(failure) => failure,
    };

    assert_eq!(
        failure.reason(),
        RuntimeGatewayOwnerStartupWatchdogStartErrorV1::ProcessMismatch
    );
    assert_eq!(inspection.renew_calls(), 0);
    assert!(ownership_is_uncertain(&gateway));
    assert_eq!(
        failure.cleanup(Duration::from_millis(100)).await,
        starring_runtime::RuntimeGatewayOwnerReleaseStatusV1::Confirmed
    );
}

#[tokio::test]
async fn bootstrap_rejects_a_receipt_for_another_shard_before_spawn() {
    let mut wrong_receipt = receipt(Duration::from_secs(5));
    wrong_receipt.lease_id.gateway_shard_id = GatewayShardIdV1::parse("shard:1").unwrap();
    let port =
        FakeGatewayOwnerPortV1::new(wrong_receipt.clone(), [], Arc::new(Mutex::new(Vec::new())));
    let inspection = port.clone();
    let mut gateway = compose_runtime_gateway_bootstrap_v1(
        ProcessInstanceId::parse("process:1").unwrap(),
        GatewayResourceConfigV1::default(),
    )
    .unwrap();
    let config = RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        Duration::from_millis(500),
    )
    .unwrap();
    let started = Instant::now();
    let result = gateway.start_gateway_owner_startup_watchdog_v1(
        port,
        accepted_receipt(wrong_receipt),
        started,
        started,
        config,
    );
    let failure = match result {
        Ok(_) => panic!("foreign shard receipt must be rejected"),
        Err(failure) => failure,
    };

    assert_eq!(
        failure.reason(),
        RuntimeGatewayOwnerStartupWatchdogStartErrorV1::ShardMismatch
    );
    assert_eq!(inspection.renew_calls(), 0);
    assert!(ownership_is_uncertain(&gateway));
    assert_eq!(
        failure.cleanup(Duration::from_millis(100)).await,
        starring_runtime::RuntimeGatewayOwnerReleaseStatusV1::Confirmed
    );
}

#[tokio::test]
async fn start_rejects_an_already_elapsed_safety_deadline_before_spawn() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_millis(800),
        Duration::from_millis(600),
        Duration::from_millis(100),
        Duration::from_millis(20),
        [],
    );
    let inspection = port.clone();
    let started = Instant::now() - Duration::from_millis(750);
    let result =
        gateway.start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config);
    let failure = match result {
        Ok(_) => panic!("elapsed startup watchdog must be rejected"),
        Err(failure) => failure,
    };

    assert_eq!(
        failure.reason(),
        RuntimeGatewayOwnerStartupWatchdogStartErrorV1::SafetyElapsed
    );
    assert_eq!(inspection.renew_calls(), 0);
    assert!(ownership_is_uncertain(&gateway));
    assert_eq!(
        failure.cleanup(Duration::from_millis(100)).await,
        starring_runtime::RuntimeGatewayOwnerReleaseStatusV1::Confirmed
    );
}

#[tokio::test]
async fn an_unknown_renewal_that_applied_is_invalidated_before_stable_id_release() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_millis(400),
        Duration::from_millis(300),
        Duration::from_millis(100),
        Duration::from_millis(20),
        [FakeRenewStepV1::OutcomeUnknownApplied],
    );
    let events = port.events();
    let started = Instant::now();
    let mut handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    assert_eq!(
        handle.wait_terminal().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
    );
    assert_eq!(
        *events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        ["release"]
    );
    assert!(ownership_is_uncertain(&gateway));
}

#[tokio::test]
async fn dropping_the_only_handle_invalidates_synchronously_and_releases() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    let inspection = port.clone();
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config)
        .unwrap();

    drop(handle);
    assert!(ownership_is_uncertain(&gateway));
    wait_for(|| inspection.release_calls() == 1).await;
    assert!(ownership_is_uncertain(&gateway));
}

#[test]
fn start_without_a_runtime_invalidates_before_returning_the_error() {
    let (port, mut gateway, receipt, config) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    let started = Instant::now();
    let result =
        gateway.start_gateway_owner_startup_watchdog_v1(port, receipt, started, started, config);

    let failure = match result {
        Ok(_) => panic!("supervisor must not start without a runtime"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.reason(),
        RuntimeGatewayOwnerStartupWatchdogStartErrorV1::RuntimeUnavailable
    );
    assert_eq!(failure.lease_id(), &lease_id());
    assert!(ownership_is_uncertain(&gateway));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    assert_eq!(
        runtime.block_on(failure.cleanup(Duration::from_millis(100))),
        starring_runtime::RuntimeGatewayOwnerReleaseStatusV1::Confirmed
    );
}

#[test]
fn supervisor_configuration_rejects_zero_or_unusable_retry_windows() {
    assert_eq!(
        RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
            Duration::from_secs(30),
            Duration::from_secs(10),
            Duration::from_secs(3),
            Duration::ZERO,
            Duration::from_secs(10),
        ),
        Err(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidRetryDelay)
    );
    assert_eq!(
        RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
            Duration::from_secs(30),
            Duration::from_secs(10),
            Duration::from_secs(3),
            Duration::from_secs(7),
            Duration::from_secs(10),
        ),
        Err(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidRetryDelay)
    );
    assert_eq!(
        RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
            Duration::from_secs(30),
            Duration::from_secs(10),
            Duration::from_secs(3),
            Duration::from_millis(250),
            Duration::ZERO,
        ),
        Err(RuntimeGatewayOwnerStartupWatchdogConfigErrorV1::InvalidCleanupTimeout)
    );
}
