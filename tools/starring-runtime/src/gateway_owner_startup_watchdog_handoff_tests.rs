use std::collections::VecDeque;
use std::future::{ready, Future};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeBuildRevisionV1, RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseObservationV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeObserveGatewayOwnerLeaseV1,
    RuntimeObservedGatewayOwnerLeaseV1, RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
    RuntimeReleaseGatewayOwnerLeaseV1, RuntimeRenewGatewayOwnerLeaseOutcomeV1,
    RuntimeRenewGatewayOwnerLeaseV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    accept_gateway_owner_acquire_v1, RuntimeAcceptedGatewayOwnerAcquireV1,
    RuntimeAcceptedGatewayOwnerReceiptV1, RuntimeGatewayOwnerLeasePortV1,
    RuntimeGatewayOwnerMutationErrorV1, RuntimeGatewayOwnerObservationErrorClassV1,
};
use chrono::{DateTime, TimeDelta, Utc};
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FakeErrorV1 {}

#[derive(Clone)]
enum FakeRenewStepV1 {
    Renewed,
    OwnershipLost,
    Blocked(Arc<Notify>),
}

#[derive(Clone)]
struct FakePortV1 {
    state: Arc<FakePortStateV1>,
}

struct FakePortStateV1 {
    receipt: Mutex<RuntimeGatewayOwnerLeaseReceiptV1>,
    renew_steps: Mutex<VecDeque<FakeRenewStepV1>>,
    acquire_calls: AtomicUsize,
    observe_calls: AtomicUsize,
    renew_calls: AtomicUsize,
    release_calls: AtomicUsize,
    active_operations: AtomicUsize,
    maximum_active_operations: AtomicUsize,
}

impl FakePortV1 {
    fn new(
        receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        renew_steps: impl IntoIterator<Item = FakeRenewStepV1>,
    ) -> Self {
        Self {
            state: Arc::new(FakePortStateV1 {
                receipt: Mutex::new(receipt),
                renew_steps: Mutex::new(renew_steps.into_iter().collect()),
                acquire_calls: AtomicUsize::new(0),
                observe_calls: AtomicUsize::new(0),
                renew_calls: AtomicUsize::new(0),
                release_calls: AtomicUsize::new(0),
                active_operations: AtomicUsize::new(0),
                maximum_active_operations: AtomicUsize::new(0),
            }),
        }
    }

    fn acquire_calls(&self) -> usize {
        self.state.acquire_calls.load(Ordering::Acquire)
    }

    fn observe_calls(&self) -> usize {
        self.state.observe_calls.load(Ordering::Acquire)
    }

    fn renew_calls(&self) -> usize {
        self.state.renew_calls.load(Ordering::Acquire)
    }

    fn release_calls(&self) -> usize {
        self.state.release_calls.load(Ordering::Acquire)
    }

    fn maximum_active_operations(&self) -> usize {
        self.state.maximum_active_operations.load(Ordering::Acquire)
    }
}

struct FakeOperationGuardV1 {
    state: Arc<FakePortStateV1>,
}

impl FakeOperationGuardV1 {
    fn begin(state: Arc<FakePortStateV1>) -> Self {
        let active = state.active_operations.fetch_add(1, Ordering::AcqRel) + 1;
        state
            .maximum_active_operations
            .fetch_max(active, Ordering::AcqRel);
        Self { state }
    }
}

impl Drop for FakeOperationGuardV1 {
    fn drop(&mut self) {
        self.state.active_operations.fetch_sub(1, Ordering::AcqRel);
    }
}

impl RuntimeGatewayOwnerLeasePortV1 for FakePortV1 {
    type Error = FakeErrorV1;

    fn classify_observation_error(
        error: &Self::Error,
    ) -> RuntimeGatewayOwnerObservationErrorClassV1 {
        match *error {}
    }

    fn observe_gateway_owner(
        &self,
        _request: RuntimeObserveGatewayOwnerLeaseV1,
    ) -> impl Future<Output = Result<RuntimeGatewayOwnerLeaseObservationV1, Self::Error>> + Send
    {
        let state = self.state.clone();
        async move {
            state.observe_calls.fetch_add(1, Ordering::AcqRel);
            let _guard = FakeOperationGuardV1::begin(state.clone());
            Ok(current_observation(&state))
        }
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
        self.state.acquire_calls.fetch_add(1, Ordering::AcqRel);
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
            let _guard = FakeOperationGuardV1::begin(state.clone());
            let step = state
                .renew_steps
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
                .unwrap_or(FakeRenewStepV1::Renewed);
            match step {
                FakeRenewStepV1::Renewed => Ok(renewed_outcome(&state, request)),
                FakeRenewStepV1::OwnershipLost => {
                    Ok(RuntimeRenewGatewayOwnerLeaseOutcomeV1::NotCurrent(
                        RuntimeGatewayOwnerLeaseObservationV1::Unowned {
                            gateway_shard_id: request.lease_id.gateway_shard_id,
                            database_now: at_millis(1_000_001),
                        },
                    ))
                }
                FakeRenewStepV1::Blocked(gate) => {
                    gate.notified().await;
                    Ok(renewed_outcome(&state, request))
                }
            }
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
        self.state.release_calls.fetch_add(1, Ordering::AcqRel);
        ready(Ok(RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released {
            lease_id: request.lease_id,
            database_now: at_millis(1_000_002),
        }))
    }
}

#[derive(Clone)]
struct FakeInvalidatorV1 {
    invalidated: Arc<AtomicBool>,
}

impl RuntimeGatewayOwnerEmergencyInvalidatorV1 for FakeInvalidatorV1 {
    fn invalidate_gateway_ownership(&self) {
        self.invalidated.store(true, Ordering::Release);
    }
}

fn current_observation(state: &FakePortStateV1) -> RuntimeGatewayOwnerLeaseObservationV1 {
    let receipt = state
        .receipt
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    RuntimeGatewayOwnerLeaseObservationV1::Owned(RuntimeObservedGatewayOwnerLeaseV1 {
        lease_id: receipt.lease_id,
        owner_revision: receipt.owner_revision,
        observed_database_now: receipt.database_now,
        expires_at: receipt.expires_at,
    })
}

fn renewed_outcome(
    state: &FakePortStateV1,
    request: RuntimeRenewGatewayOwnerLeaseV1,
) -> RuntimeRenewGatewayOwnerLeaseOutcomeV1 {
    let database_now = state
        .receipt
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .database_now
        + TimeDelta::milliseconds(1);
    let receipt = RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: request.lease_id,
        owner_revision: NonZeroU64::new(request.expected_owner_revision.get() + 1).unwrap(),
        database_now,
        expires_at: database_now + TimeDelta::from_std(request.lease_for.get()).unwrap(),
    };
    *state
        .receipt
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = receipt.clone();
    RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt)
}

fn at_millis(value: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(value).unwrap()
}

fn lease_id() -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:handoff").unwrap(),
        lease_epoch: NonZeroU64::new(1).unwrap(),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:handoff").unwrap(),
    }
}

fn receipt(lease_for: Duration) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    let database_now = at_millis(1_000_000);
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: lease_id(),
        owner_revision: NonZeroU64::new(3).unwrap(),
        database_now,
        expires_at: database_now + TimeDelta::from_std(lease_for).unwrap(),
    }
}

fn accepted_receipt(
    receipt: RuntimeGatewayOwnerLeaseReceiptV1,
) -> RuntimeAcceptedGatewayOwnerReceiptV1 {
    let request = RuntimeAcquireGatewayOwnerLeaseV1 {
        gateway_shard_id: receipt.lease_id.gateway_shard_id.clone(),
        process_instance_id: receipt.lease_id.process_instance_id.clone(),
        expected_build_revision: receipt.lease_id.expected_build_revision.clone(),
        lease_for: automation_runtime_controller::RuntimeGatewayOwnerLeaseDurationV1::new(
            Duration::from_secs(1),
        )
        .unwrap(),
    };
    let accepted = accept_gateway_owner_acquire_v1(
        &request,
        RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(receipt),
    )
    .unwrap();
    let RuntimeAcceptedGatewayOwnerAcquireV1::Acquired(receipt) = accepted else {
        panic!("expected accepted receipt")
    };
    receipt
}

fn fixture(
    lease_for: Duration,
    renew_before: Duration,
    safety_margin: Duration,
    renew_steps: impl IntoIterator<Item = FakeRenewStepV1>,
) -> (
    RuntimeGatewayOwnerStartupWatchdogHandleV1,
    FakePortV1,
    Arc<AtomicBool>,
) {
    let receipt = receipt(lease_for);
    let port = FakePortV1::new(receipt.clone(), renew_steps);
    let invalidated = Arc::new(AtomicBool::new(false));
    let invalidator = FakeInvalidatorV1 {
        invalidated: invalidated.clone(),
    };
    let config = RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
        lease_for,
        renew_before,
        safety_margin,
        Duration::from_millis(20),
        Duration::from_millis(500),
    )
    .unwrap();
    let started = Instant::now();
    let handle = start_runtime_gateway_owner_startup_watchdog_v1(
        port.clone(),
        invalidator,
        accepted_receipt(receipt),
        started,
        started,
        config,
    )
    .unwrap();
    (handle, port, invalidated)
}

fn handoff_proof() -> RuntimeGatewayOwnerProductionHandoffProofV1 {
    RuntimeGatewayOwnerProductionHandoffProofV1 { _private: () }
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

#[test]
fn handoff_observation_requires_strict_positive_monotonic_safety() {
    let observed_at = Instant::now();
    for safety_deadline in [
        observed_at.checked_sub(Duration::from_nanos(1)).unwrap(),
        observed_at,
    ] {
        let observation = RuntimeGatewayOwnerCurrentObservationV1 {
            receipt: receipt(Duration::from_secs(5)),
            safety_deadline,
        };
        assert_eq!(
            accept_production_handoff_observation_v1(observation, observed_at),
            Err(RuntimeGatewayOwnerProductionHandoffErrorV1::SafetyElapsed)
        );
    }
    let observation = RuntimeGatewayOwnerCurrentObservationV1 {
        receipt: receipt(Duration::from_secs(5)),
        safety_deadline: observed_at.checked_add(Duration::from_nanos(1)).unwrap(),
    };
    assert!(accept_production_handoff_observation_v1(observation, observed_at).is_ok());
}

#[tokio::test]
async fn production_handoff_keeps_one_actor_and_contiguous_owner_revision() {
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(700),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );

    let production = handle.into_production_v1(handoff_proof()).await.unwrap();

    assert_eq!(
        production.handoff_observation().receipt().owner_revision,
        NonZeroU64::new(3).unwrap()
    );
    assert_eq!(port.acquire_calls(), 0);
    assert_eq!(port.observe_calls(), 0);
    assert_eq!(port.release_calls(), 0);
    assert_eq!(production.terminal_status(), None);
    wait_for(|| port.renew_calls() == 1).await;
    let observation = production.observe_current_gateway_owner_v1().await.unwrap();
    assert_eq!(
        observation.receipt().owner_revision,
        NonZeroU64::new(4).unwrap()
    );
    assert_eq!(port.maximum_active_operations(), 1);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        production.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn handoff_queued_behind_renewal_receives_the_exact_successor() {
    let gate = Arc::new(Notify::new());
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(800),
        Duration::from_millis(600),
        Duration::from_millis(100),
        [FakeRenewStepV1::Blocked(gate.clone())],
    );
    wait_for(|| port.renew_calls() == 1).await;
    let mut handoff = Box::pin(handle.into_production_v1(handoff_proof()));

    tokio::select! {
        _ = &mut handoff => panic!("handoff completed before renewal"),
        _ = sleep(Duration::from_millis(20)) => {}
    }
    gate.notify_one();
    let production = handoff.await.unwrap();

    assert_eq!(
        production.handoff_observation().receipt().owner_revision,
        NonZeroU64::new(4).unwrap()
    );
    assert_eq!(port.acquire_calls(), 0);
    assert_eq!(port.observe_calls(), 0);
    assert_eq!(port.release_calls(), 0);
    assert_eq!(port.maximum_active_operations(), 1);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        production.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn canceled_handoff_invalidates_synchronously_and_releases_once() {
    let gate = Arc::new(Notify::new());
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(800),
        Duration::from_millis(600),
        Duration::from_millis(100),
        [FakeRenewStepV1::Blocked(gate.clone())],
    );
    wait_for(|| port.renew_calls() == 1).await;
    let mut handoff = Box::pin(handle.into_production_v1(handoff_proof()));
    tokio::select! {
        _ = &mut handoff => panic!("blocked handoff completed"),
        _ = sleep(Duration::from_millis(20)) => {}
    }

    drop(handoff);

    assert!(invalidated.load(Ordering::Acquire));
    gate.notify_one();
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.acquire_calls(), 0);
    assert_eq!(port.maximum_active_operations(), 1);
}

#[tokio::test]
async fn lost_handoff_acknowledgement_invalidates_and_stops_the_actor() {
    let (mut handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let (response, acknowledgement) = oneshot::channel();
    drop(acknowledgement);
    handle
        .inner()
        .supervisor_commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Promote { response })
        .await
        .unwrap();

    assert_eq!(
        handle.wait_terminal().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn dropping_startup_after_actor_ack_invalidates_the_promoted_actor() {
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let (response, acknowledgement) = oneshot::channel();
    handle
        .inner()
        .supervisor_commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Promote { response })
        .await
        .unwrap();
    assert_eq!(
        acknowledgement.await.unwrap().receipt().owner_revision,
        NonZeroU64::new(3).unwrap()
    );
    assert!(!invalidated.load(Ordering::Acquire));

    drop(handle);

    assert!(invalidated.load(Ordering::Acquire));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn duplicate_private_handoff_is_a_terminal_protocol_violation() {
    let (mut handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let commands = handle.inner().supervisor_commands.clone();
    let (first_response, first_acknowledgement) = oneshot::channel();
    commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Promote {
            response: first_response,
        })
        .await
        .unwrap();
    assert_eq!(
        first_acknowledgement
            .await
            .unwrap()
            .receipt()
            .owner_revision,
        NonZeroU64::new(3).unwrap()
    );
    let (second_response, second_acknowledgement) = oneshot::channel();
    commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Promote {
            response: second_response,
        })
        .await
        .unwrap();

    assert!(second_acknowledgement.await.is_err());
    assert_eq!(
        handle.wait_terminal().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn dropping_production_invalidates_synchronously_and_releases_once() {
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let production = handle.into_production_v1(handoff_proof()).await.unwrap();

    drop(production);

    assert!(invalidated.load(Ordering::Acquire));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn shutdown_queued_with_handoff_wins_before_production_transition() {
    let gate = Arc::new(Notify::new());
    let (mut handle, port, invalidated) = fixture(
        Duration::from_millis(800),
        Duration::from_millis(600),
        Duration::from_millis(100),
        [FakeRenewStepV1::Blocked(gate.clone())],
    );
    wait_for(|| port.renew_calls() == 1).await;
    let (promotion_response, promotion_acknowledgement) = oneshot::channel();
    handle
        .inner()
        .supervisor_commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Promote {
            response: promotion_response,
        })
        .await
        .unwrap();
    let (shutdown_response, shutdown_acknowledgement) = oneshot::channel();
    handle
        .inner()
        .shutdown_commands
        .send(RuntimeGatewayOwnerStartupShutdownCommandV1 {
            response: shutdown_response,
        })
        .await
        .unwrap();
    gate.notify_one();

    assert_eq!(
        shutdown_acknowledgement.await.unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert!(promotion_acknowledgement.await.is_err());
    assert_eq!(
        handle.wait_terminal().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn safety_deadline_wins_over_handoff_queued_behind_renewal() {
    let gate = Arc::new(Notify::new());
    let (mut handle, port, invalidated) = fixture(
        Duration::from_millis(400),
        Duration::from_millis(300),
        Duration::from_millis(100),
        [FakeRenewStepV1::Blocked(gate.clone())],
    );
    wait_for(|| port.renew_calls() == 1).await;
    let (response, acknowledgement) = oneshot::channel();
    handle
        .inner()
        .supervisor_commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Promote { response })
        .await
        .unwrap();
    wait_for(|| invalidated.load(Ordering::Acquire)).await;
    gate.notify_one();

    assert!(acknowledgement.await.is_err());
    assert_eq!(
        handle.wait_terminal().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn production_wait_reports_ownership_loss_from_the_same_actor() {
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(700),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [FakeRenewStepV1::OwnershipLost],
    );
    let mut production = handle.into_production_v1(handoff_proof()).await.unwrap();

    assert_eq!(
        production.wait_terminal().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}
