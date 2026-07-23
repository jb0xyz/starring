use std::collections::VecDeque;
use std::future::{ready, Future};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeBuildRevisionV1, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseObservationV1, RuntimeGatewayOwnerLeaseReceiptV1,
    RuntimeGatewayReadyKindV2, RuntimeObserveGatewayOwnerLeaseV1,
    RuntimeObservedGatewayOwnerLeaseV1, RuntimeRecoveryIdV2,
    RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeReleaseGatewayOwnerLeaseV1,
    RuntimeRenewGatewayOwnerLeaseOutcomeV1, RuntimeRenewGatewayOwnerLeaseV1,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    accept_gateway_owner_acquire_v1, accept_runtime_registry_recovery_empty_observation_v2,
    RuntimeAcceptedGatewayOwnerAcquireV1, RuntimeAcceptedGatewayOwnerReceiptV1,
    RuntimeCapabilityReadinessKindV2, RuntimeCapabilityReadinessReceiptV2,
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimeGatewayClosedLifecycleV2, RuntimeGatewayOwnerLeasePortV1,
    RuntimeGatewayOwnerMutationErrorV1, RuntimeGatewayOwnerObservationErrorClassV1,
    RuntimePausedGatewayObservationV2, RuntimePausedGatewaySequenceV2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryObservationInputV2,
};
use chrono::{DateTime, TimeDelta, Utc};
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FakeErrorV1 {
    Retryable,
    OwnershipLost,
    ProtocolViolation,
}

#[derive(Clone)]
enum FakeObservationStepV1 {
    Error(FakeErrorV1),
    Unowned,
    Blocked(Arc<Notify>),
}

#[derive(Clone)]
enum FakeRenewStepV1 {
    Renewed,
    OwnershipLost,
    BlockedDefinitelyNotApplied(Arc<Notify>),
    Blocked(Arc<Notify>),
}

#[derive(Clone)]
struct FakePortV1 {
    state: Arc<FakePortStateV1>,
}

struct FakePortStateV1 {
    receipt: Mutex<RuntimeGatewayOwnerLeaseReceiptV1>,
    observation_steps: Mutex<VecDeque<FakeObservationStepV1>>,
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
                observation_steps: Mutex::new(VecDeque::new()),
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

    fn block_next_observation(&self, gate: Arc<Notify>) {
        *self
            .state
            .observation_steps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            VecDeque::from([FakeObservationStepV1::Blocked(gate)]);
    }

    fn push_observation_step(&self, step: FakeObservationStepV1) {
        self.state
            .observation_steps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(step);
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
        match error {
            FakeErrorV1::Retryable => RuntimeGatewayOwnerObservationErrorClassV1::Retryable,
            FakeErrorV1::OwnershipLost => RuntimeGatewayOwnerObservationErrorClassV1::OwnershipLost,
            FakeErrorV1::ProtocolViolation => {
                RuntimeGatewayOwnerObservationErrorClassV1::ProtocolViolation
            }
        }
    }

    fn observe_gateway_owner(
        &self,
        request: RuntimeObserveGatewayOwnerLeaseV1,
    ) -> impl Future<Output = Result<RuntimeGatewayOwnerLeaseObservationV1, Self::Error>> + Send
    {
        let state = self.state.clone();
        async move {
            state.observe_calls.fetch_add(1, Ordering::AcqRel);
            let _guard = FakeOperationGuardV1::begin(state.clone());
            let step = state
                .observation_steps
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front();
            match step {
                Some(FakeObservationStepV1::Error(error)) => Err(error),
                Some(FakeObservationStepV1::Unowned) => {
                    Ok(RuntimeGatewayOwnerLeaseObservationV1::Unowned {
                        gateway_shard_id: request.gateway_shard_id,
                        database_now: at_millis(1_000_001),
                    })
                }
                Some(FakeObservationStepV1::Blocked(gate)) => {
                    gate.notified().await;
                    Ok(current_observation(&state))
                }
                None => Ok(current_observation(&state)),
            }
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
                FakeRenewStepV1::BlockedDefinitelyNotApplied(gate) => {
                    gate.notified().await;
                    Err(RuntimeGatewayOwnerMutationErrorV1::DefinitelyNotApplied {
                        source: FakeErrorV1::Retryable,
                    })
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

fn closed_recovery_readiness() -> RuntimeCapabilityReadinessSetV2 {
    let readiness_receipt = |kind, role, checked_at| {
        RuntimeCapabilityReadinessReceiptV2::new(
            kind,
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring",
            role,
            at_millis(checked_at),
        )
        .unwrap()
    };
    RuntimeCapabilityReadinessSetV2::new(
        readiness_receipt(
            RuntimeCapabilityReadinessKindV2::Convergence,
            "role_a",
            1_000_001,
        ),
        readiness_receipt(
            RuntimeCapabilityReadinessKindV2::ExactTarget,
            "role_b",
            1_000_002,
        ),
        readiness_receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c", 1_000_003),
        readiness_receipt(
            RuntimeCapabilityReadinessKindV2::Serving,
            "role_d",
            1_000_004,
        ),
        readiness_receipt(
            RuntimeCapabilityReadinessKindV2::Interaction,
            "role_e",
            1_000_005,
        ),
    )
    .unwrap()
}

fn closed_recovery_permit(
    owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
) -> RuntimeClosedDrainRecoveryPermitV2 {
    let process_instance_id = owner_receipt.lease_id.process_instance_id.clone();
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let emergency_generation = lifecycle.snapshot().generation();
    let readiness = closed_recovery_readiness();
    let paused_gateway = RuntimePausedGatewayObservationV2::new(
        emergency_generation,
        process_instance_id.clone(),
        NonZeroU64::new(13).unwrap(),
        RuntimeGatewayReadyKindV2::Ready,
        NonZeroU64::new(17).unwrap(),
        RuntimePausedGatewaySequenceV2::new(
            RuntimeGatewayAdmissionSequenceV2::new(NonZeroU64::new(20).unwrap()),
            RuntimeGatewayAdmissionSequenceV2::new(NonZeroU64::new(18).unwrap()),
            None,
        )
        .unwrap(),
    );
    let registry = accept_runtime_registry_recovery_empty_observation_v2(
        process_instance_id,
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                NonZeroU64::new(23).unwrap(),
            ),
            retained_slot_count: 2,
            retained_empty_tombstone_count: 2,
            staged_route_count: 0,
            serving_route_count: 0,
            draining_route_count: 0,
            sealed_slot_count: 0,
            active_interaction_count: 0,
            failed_closed_slot_count: 0,
            registry_failed_closed: false,
        },
    )
    .unwrap();
    let input = RuntimeClosedRecoveryInputV2::new(
        RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
        owner_receipt,
        readiness,
        paused_gateway,
        RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry),
    );
    lifecycle
        .begin_recovery(emergency_generation, input)
        .unwrap()
        .1
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
        invalidated.clone(),
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

#[tokio::test]
async fn prepared_owner_is_bound_to_one_gateway_and_snapshot_guard_blocks_publication() {
    let process_instance_id = ProcessInstanceId::parse("process:handoff").unwrap();
    let mut owner_gateway = crate::gateway::compose_runtime_gateway_section_test_bootstrap_v2(
        process_instance_id.clone(),
    );
    let mut foreign_gateway =
        crate::gateway::compose_runtime_gateway_section_test_bootstrap_v2(process_instance_id);
    owner_gateway.connect_ready_for_gateway_section_test_v2();
    foreign_gateway.connect_ready_for_gateway_section_test_v2();

    let lease_for = Duration::from_secs(2);
    let owner_receipt = receipt(lease_for);
    let port = FakePortV1::new(owner_receipt.clone(), []);
    let config = RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
        lease_for,
        Duration::from_millis(1_500),
        Duration::from_millis(200),
        Duration::from_millis(20),
        Duration::from_millis(500),
    )
    .unwrap();
    let started = Instant::now();
    let handle = owner_gateway
        .start_gateway_owner_startup_watchdog_v1(
            port.clone(),
            accepted_receipt(owner_receipt),
            started,
            started,
            config,
        )
        .unwrap();
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();

    assert!(matches!(
        foreign_gateway.initial_emergency_gateway_section_v2(&prepared),
        Err(crate::RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
    ));
    owner_gateway
        .held_initial_section_blocks_repeated_pause_test_v2(&prepared)
        .await
        .unwrap();

    let registry = crate::compose_runtime_registry_bootstrap_v1(
        ProcessInstanceId::parse("process:handoff").unwrap(),
        crate::GatewayResourceConfigV1::default(),
    )
    .unwrap();
    let readiness = crate::database::runtime_database_readiness_for_test_v1();
    let mut pending = crate::closed_recovery::begin_initial_empty_recovery_v2(
        &owner_gateway,
        &registry,
        prepared,
        RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
        &readiness,
    )
    .unwrap();

    assert!(matches!(
        owner_gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id,
            ..
        } if generation.get() == 2
            && recovery_id.as_str() == "0123456789abcdef0123456789abcdef"
    ));
    assert_eq!(
        format!("{pending:?}"),
        "RuntimeClosedRecoveryPendingPhaseV2(<redacted>)"
    );
    pending
        .stale_predecessor_drop_preserves_successor_v2()
        .unwrap();
    assert!(matches!(
        owner_gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id,
            ..
        } if generation.get() == 4
            && recovery_id.as_str() == "fedcba9876543210fedcba9876543210"
    ));
    drop(pending);
    assert!(matches!(
        owner_gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 5
    ));
    wait_for(|| port.release_calls() == 1).await;
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

#[test]
fn closed_recovery_prepare_ack_requires_strict_positive_monotonic_safety() {
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
            accept_closed_recovery_prepare_observation_v2(observation, observed_at),
            Err(RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SafetyElapsed)
        );
    }
    let observation = RuntimeGatewayOwnerCurrentObservationV1 {
        receipt: receipt(Duration::from_secs(5)),
        safety_deadline: observed_at.checked_add(Duration::from_nanos(1)).unwrap(),
    };
    assert!(accept_closed_recovery_prepare_observation_v2(observation, observed_at).is_ok());
}

#[test]
fn closed_recovery_commit_ack_classifies_elapsed_before_protocol_mismatch() {
    let observed_at = Instant::now();
    let prepared = RuntimeGatewayOwnerCurrentObservationV1 {
        receipt: receipt(Duration::from_secs(5)),
        safety_deadline: observed_at,
    };
    let mut mismatched_receipt = prepared.receipt().clone();
    mismatched_receipt.owner_revision =
        NonZeroU64::new(mismatched_receipt.owner_revision.get() + 1).unwrap();

    assert_eq!(
        accept_closed_recovery_commit_observation_v2(
            prepared.clone(),
            &prepared,
            &mismatched_receipt,
            observed_at,
        ),
        Err(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::SafetyElapsed)
    );
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
async fn closed_recovery_prepare_joins_one_renewal_exact_observes_and_freezes() {
    let gate = Arc::new(Notify::new());
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(900),
        Duration::from_millis(700),
        Duration::from_millis(100),
        [FakeRenewStepV1::Blocked(gate.clone())],
    );
    wait_for(|| port.renew_calls() == 1).await;
    let mut prepare = Box::pin(handle.prepare_closed_recovery_v2());

    tokio::select! {
        _ = &mut prepare => panic!("prepare completed before renewal"),
        _ = sleep(Duration::from_millis(20)) => {}
    }
    gate.notify_one();
    let prepared = prepare.await.unwrap();

    assert_eq!(
        prepared.observation().receipt().owner_revision,
        NonZeroU64::new(4).unwrap()
    );
    assert_eq!(port.observe_calls(), 1);
    assert_eq!(port.renew_calls(), 1);
    assert_eq!(port.acquire_calls(), 0);
    assert_eq!(port.release_calls(), 0);
    assert_eq!(port.maximum_active_operations(), 1);
    sleep(Duration::from_millis(100)).await;
    assert_eq!(port.renew_calls(), 1);
    assert!(!invalidated.load(Ordering::Acquire));

    assert_eq!(
        prepared.abort_and_shutdown_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn queued_closed_recovery_prepare_beats_second_renewal_after_definite_non_apply() {
    let gate = Arc::new(Notify::new());
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(900),
        Duration::from_millis(700),
        Duration::from_millis(100),
        [
            FakeRenewStepV1::BlockedDefinitelyNotApplied(gate.clone()),
            FakeRenewStepV1::Renewed,
        ],
    );
    wait_for(|| port.renew_calls() == 1).await;
    let mut prepare = Box::pin(handle.prepare_closed_recovery_v2());
    tokio::select! {
        _ = &mut prepare => panic!("prepare completed while renewal was blocked"),
        _ = sleep(Duration::from_millis(20)) => {}
    }
    gate.notify_one();

    let prepared = prepare.await.unwrap();

    assert_eq!(port.renew_calls(), 1);
    assert_eq!(port.observe_calls(), 1);
    assert_eq!(
        prepared.observation().receipt().owner_revision,
        NonZeroU64::new(3).unwrap()
    );
    assert_eq!(port.maximum_active_operations(), 1);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        prepared.abort_and_shutdown_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn canceled_observation_cannot_hide_prepare_behind_definite_non_apply() {
    let gate = Arc::new(Notify::new());
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(900),
        Duration::from_millis(700),
        Duration::from_millis(100),
        [
            FakeRenewStepV1::BlockedDefinitelyNotApplied(gate.clone()),
            FakeRenewStepV1::Renewed,
        ],
    );
    wait_for(|| port.renew_calls() == 1).await;
    let mut observation = Box::pin(handle.observe_current_gateway_owner_v1());
    tokio::select! {
        _ = &mut observation => panic!("observation completed while renewal was blocked"),
        _ = sleep(Duration::from_millis(20)) => {}
    }
    drop(observation);
    let mut prepare = Box::pin(handle.prepare_closed_recovery_v2());
    tokio::select! {
        _ = &mut prepare => panic!("prepare completed while renewal was blocked"),
        _ = sleep(Duration::from_millis(20)) => {}
    }
    gate.notify_one();

    let prepared = prepare.await.unwrap();

    assert_eq!(port.renew_calls(), 1);
    assert_eq!(port.observe_calls(), 1);
    assert_eq!(
        prepared.observation().receipt().owner_revision,
        NonZeroU64::new(3).unwrap()
    );
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        prepared.abort_and_shutdown_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn queued_closed_recovery_prepare_beats_due_renewal_after_observation_timeout() {
    let gate = Arc::new(Notify::new());
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(2_000),
        Duration::from_millis(1_600),
        Duration::from_millis(300),
        [],
    );
    port.block_next_observation(gate);
    let commands = handle.inner().supervisor_commands.clone();
    let recovery_commands = handle.inner().closed_recovery_commands.clone();
    let (observation_response, observation_acknowledgement) = oneshot::channel();
    commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Observe {
            response: observation_response,
        })
        .await
        .unwrap();
    wait_for(|| port.observe_calls() == 1).await;
    let (prepare_response, prepare_acknowledgement) = oneshot::channel();
    recovery_commands
        .send(RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare {
            response: prepare_response,
        })
        .await
        .unwrap();

    assert_eq!(
        observation_acknowledgement.await.unwrap(),
        Err(RuntimeGatewayOwnerCurrentObservationErrorV1::Retryable)
    );
    let prepared_observation = prepare_acknowledgement.await.unwrap().unwrap();

    assert_eq!(
        prepared_observation.receipt().owner_revision,
        NonZeroU64::new(3).unwrap()
    );
    assert_eq!(port.observe_calls(), 2);
    assert_eq!(port.renew_calls(), 0);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        handle.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn closed_recovery_prepare_observation_failures_are_terminal_and_fail_closed() {
    let cases = [
        (
            FakeObservationStepV1::Error(FakeErrorV1::Retryable),
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ObservationUnavailable,
        ),
        (
            FakeObservationStepV1::Error(FakeErrorV1::OwnershipLost),
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::OwnershipLost,
        ),
        (
            FakeObservationStepV1::Error(FakeErrorV1::ProtocolViolation),
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation,
        ),
        (
            FakeObservationStepV1::Unowned,
            RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::OwnershipLost,
        ),
    ];
    for (step, expected) in cases {
        let (handle, port, invalidated) = fixture(
            Duration::from_secs(5),
            Duration::from_secs(2),
            Duration::from_millis(500),
            [],
        );
        port.push_observation_step(step);

        assert_eq!(
            handle.prepare_closed_recovery_v2().await.unwrap_err(),
            expected
        );
        assert!(invalidated.load(Ordering::Acquire));
        wait_for(|| port.release_calls() == 1).await;
        assert_eq!(port.observe_calls(), 1);
        assert_eq!(port.renew_calls(), 0);
    }
}

#[tokio::test]
async fn canceled_closed_recovery_prepare_during_exact_observation_is_fail_closed() {
    let gate = Arc::new(Notify::new());
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    port.block_next_observation(gate.clone());
    let mut prepare = Box::pin(handle.prepare_closed_recovery_v2());
    tokio::select! {
        _ = &mut prepare => panic!("prepare completed while exact observation was blocked"),
        _ = wait_for(|| port.observe_calls() == 1) => {}
    }

    drop(prepare);

    assert!(invalidated.load(Ordering::Acquire));
    gate.notify_one();
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn shutdown_during_closed_recovery_exact_observation_is_fail_closed() {
    let gate = Arc::new(Notify::new());
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    port.block_next_observation(gate);
    let commands = handle.inner().closed_recovery_commands.clone();
    let (response, acknowledgement) = oneshot::channel();
    commands
        .send(RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare { response })
        .await
        .unwrap();
    wait_for(|| port.observe_calls() == 1).await;

    assert_eq!(
        handle.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(
        acknowledgement.await.unwrap(),
        Err(RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SupervisorUnavailable)
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn safety_deadline_during_closed_recovery_exact_observation_is_fail_closed() {
    let gate = Arc::new(Notify::new());
    let (mut handle, port, invalidated) = fixture(
        Duration::from_millis(1_200),
        Duration::from_millis(900),
        Duration::from_millis(200),
        [],
    );
    port.block_next_observation(gate);
    let (response, acknowledgement) = oneshot::channel();
    handle
        .inner()
        .closed_recovery_commands
        .send(RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare { response })
        .await
        .unwrap();
    wait_for(|| port.observe_calls() == 1).await;

    assert_eq!(
        acknowledgement.await.unwrap(),
        Err(RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::SafetyElapsed)
    );
    assert_eq!(
        handle.wait_terminal().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn dropping_prepared_closed_recovery_invalidates_and_releases_once() {
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();

    drop(prepared);

    assert!(invalidated.load(Ordering::Acquire));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn canceled_closed_recovery_prepare_invalidates_and_releases_once() {
    let gate = Arc::new(Notify::new());
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(900),
        Duration::from_millis(700),
        Duration::from_millis(100),
        [FakeRenewStepV1::Blocked(gate.clone())],
    );
    wait_for(|| port.renew_calls() == 1).await;
    let mut prepare = Box::pin(handle.prepare_closed_recovery_v2());
    tokio::select! {
        _ = &mut prepare => panic!("prepare completed while renewal was blocked"),
        _ = sleep(Duration::from_millis(20)) => {}
    }

    drop(prepare);

    assert!(invalidated.load(Ordering::Acquire));
    gate.notify_one();
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.maximum_active_operations(), 1);
}

#[tokio::test]
async fn illegal_command_after_closed_recovery_prepare_is_terminal() {
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let inner = prepared.inner.as_ref().unwrap();
    let commands = inner.supervisor_commands.clone();
    let mut terminal = inner.terminal.clone();
    let (response, acknowledgement) = oneshot::channel();
    commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Observe { response })
        .await
        .unwrap();

    assert_eq!(
        acknowledgement.await.unwrap(),
        Err(RuntimeGatewayOwnerCurrentObservationErrorV1::ProtocolViolation)
    );
    loop {
        if let Some(exit) = *terminal.borrow() {
            assert_eq!(
                exit,
                RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
            );
            break;
        }
        terminal.changed().await.unwrap();
    }
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn closed_recovery_commit_rechecks_exact_permit_receipt_and_stays_frozen() {
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let permit = closed_recovery_permit(prepared.observation().receipt().clone());

    let closed = prepared.commit_closed_recovery_v2(&permit).await.unwrap();

    assert_eq!(closed.observation().receipt(), permit.owner_receipt());
    assert_eq!(port.observe_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
    assert_eq!(port.acquire_calls(), 0);
    assert_eq!(port.release_calls(), 0);
    assert_eq!(port.maximum_active_operations(), 1);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        format!("{closed:?}"),
        "RuntimeGatewayOwnerClosedRecoverySupervisorV2(<redacted>)"
    );
    assert_eq!(
        closed.shutdown().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn closed_recovery_commit_rejects_mismatched_permit_and_invalidates() {
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let mut mismatched_receipt = prepared.observation().receipt().clone();
    mismatched_receipt.owner_revision =
        NonZeroU64::new(mismatched_receipt.owner_revision.get() + 1).unwrap();
    let permit = closed_recovery_permit(mismatched_receipt);

    assert!(matches!(
        prepared.commit_closed_recovery_v2(&permit).await,
        Err(RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::OwnerReceiptMismatch)
    ));
    assert!(invalidated.load(Ordering::Acquire));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn lost_closed_recovery_prepare_acknowledgement_is_fail_closed() {
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
        .closed_recovery_commands
        .send(RuntimeGatewayOwnerClosedRecoveryCommandV2::Prepare { response })
        .await
        .unwrap();

    assert_eq!(
        handle.wait_terminal().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn lost_closed_recovery_commit_acknowledgement_is_fail_closed() {
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let inner = prepared.inner.as_ref().unwrap();
    let commands = inner.closed_recovery_commands.clone();
    let mut terminal = inner.terminal.clone();
    let (response, acknowledgement) = oneshot::channel();
    drop(acknowledgement);
    commands
        .send(RuntimeGatewayOwnerClosedRecoveryCommandV2::Commit {
            expected_receipt: prepared.observation().receipt().clone(),
            response,
        })
        .await
        .unwrap();

    loop {
        if let Some(exit) = *terminal.borrow() {
            assert_eq!(exit, RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown);
            break;
        }
        terminal.changed().await.unwrap();
    }
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn prepared_closed_recovery_never_renews_and_expires_fail_closed() {
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(1_500),
        Duration::from_millis(1_000),
        Duration::from_millis(300),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();

    wait_for(|| invalidated.load(Ordering::Acquire)).await;
    wait_for(|| port.release_calls() == 1).await;

    assert_eq!(port.renew_calls(), 0);
    drop(prepared);
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn committed_closed_recovery_never_renews_and_expires_fail_closed() {
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(1_500),
        Duration::from_millis(1_000),
        Duration::from_millis(300),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let permit = closed_recovery_permit(prepared.observation().receipt().clone());
    let closed = prepared.commit_closed_recovery_v2(&permit).await.unwrap();

    sleep(Duration::from_millis(700)).await;
    assert_eq!(port.renew_calls(), 0);
    assert!(!invalidated.load(Ordering::Acquire));
    wait_for(|| invalidated.load(Ordering::Acquire)).await;
    wait_for(|| port.release_calls() == 1).await;

    assert_eq!(port.renew_calls(), 0);
    drop(closed);
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
