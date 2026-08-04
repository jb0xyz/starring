use std::collections::VecDeque;
use std::future::{ready, Future};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeBuildRevisionV1, RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1,
    RuntimeDrainIntentIdV2, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseObservationV1, RuntimeGatewayOwnerLeaseReceiptV1,
    RuntimeGatewayReadyKindV2, RuntimeLiveAttestationDigestV2, RuntimeObserveGatewayOwnerLeaseV1,
    RuntimeObservedGatewayOwnerLeaseV1, RuntimeRecoveryIdV2,
    RuntimeReleaseGatewayOwnerLeaseOutcomeV1, RuntimeReleaseGatewayOwnerLeaseV1,
    RuntimeRenewGatewayOwnerLeaseOutcomeV1, RuntimeRenewGatewayOwnerLeaseV1,
    RuntimeServingIdentityV2, RuntimeServingReceiptV2, RuntimeServingSlotV2,
    RuntimeStartupRecoveryObservationReceiptV2, RuntimeStartupRecoveryStateV2,
    RuntimeStartupServingStateV2,
};
use automation_runtime_convergence::{
    DeploymentId, InstallationId, ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeGeneration,
    RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_execution_postgres::RuntimeExecutionPersistenceErrorV1 as PendingDrainPersistenceErrorV1;
use automation_runtime_serving_postgres::{
    RuntimePendingDrainServingLookupV1, RuntimePendingDrainServingObservationV1,
    RuntimePendingDrainServingSourceEvidenceV1, RuntimeServingPersistenceErrorV1,
};
use automation_runtime_worker::{
    accept_gateway_owner_acquire_v1, accept_runtime_registry_recovery_empty_observation_v2,
    RuntimeAcceptedGatewayOwnerAcquireV1, RuntimeAcceptedGatewayOwnerReceiptV1,
    RuntimeAcceptedPendingDrainSelectionV2, RuntimeAuthorizedPendingDrainAcknowledgementV2,
    RuntimeAuthorizedPendingDrainClaimV2, RuntimeAuthorizedPendingDrainSelectionV3,
    RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeAuthorizedStartupRecoveryObservationV2,
    RuntimeCapabilityReadinessKindV2, RuntimeCapabilityReadinessReceiptV2,
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimeCompletedStartupRecoveryExecutionV2, RuntimeCompletedStartupRecoveryObservationV2,
    RuntimeGatewayClosedLifecycleV2, RuntimeGatewayOwnerLeasePortV1,
    RuntimeGatewayOwnerMutationErrorV1, RuntimeGatewayOwnerObservationErrorClassV1,
    RuntimePausedGatewayObservationV2, RuntimePausedGatewaySequenceV2,
    RuntimePendingDrainAcknowledgementReceiptV2, RuntimePendingDrainCandidateV2,
    RuntimePendingDrainClaimReceiptV2, RuntimePendingDrainNoCandidateReceiptV2,
    RuntimePendingDrainPreviousOwnerClaimedCandidateV3, RuntimePendingDrainRegistrySealWitnessV2,
    RuntimePendingDrainSelectionOutcomeV2, RuntimePendingDrainSelectionOutcomeV3,
    RuntimePendingDrainSelectionReceiptV2, RuntimePendingDrainSelectionReceiptV3,
    RuntimePendingDrainStateDigestV2, RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryObservationInputV2,
    RuntimeSelectedPendingDrainNoCandidateV2, RuntimeStartupRecoveryClassV2,
    RuntimeStartupRecoveryContinuationV2, RuntimeStartupRecoveryExecutionActionIdentityV2,
    RuntimeStartupRecoveryExecutionReceiptOutcomeV2, RuntimeStartupRecoveryExecutionReceiptV2,
    RuntimeStartupRecoveryExecutionTerminalDigestV2, RuntimeStartupRecoveryObservationPortV2,
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
    OutcomeUnknown,
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
    release_gate: Mutex<Option<Arc<Notify>>>,
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
                release_gate: Mutex::new(None),
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

struct StartupObservationFixturePortV2 {
    calls: Arc<AtomicUsize>,
    outcome: StartupObservationFixtureOutcomeV2,
}

enum StartupObservationFixtureOutcomeV2 {
    Complete(RuntimeStartupRecoveryStateV2),
    Error(FakeErrorV1),
    Pending {
        active: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    },
}

struct PendingStartupObservationGuardV2 {
    active: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
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

impl Drop for PendingStartupObservationGuardV2 {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
        self.dropped.fetch_add(1, Ordering::AcqRel);
    }
}

impl RuntimeStartupRecoveryObservationPortV2 for StartupObservationFixturePortV2 {
    type Error = FakeErrorV1;

    fn observe_startup_recovery(
        &self,
        authorization: RuntimeAuthorizedStartupRecoveryObservationV2,
        _operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimeCompletedStartupRecoveryObservationV2, Self::Error>> + Send
    {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let outcome = match &self.outcome {
            StartupObservationFixtureOutcomeV2::Complete(state) => {
                StartupObservationFixtureOutcomeV2::Complete(state.clone())
            }
            StartupObservationFixtureOutcomeV2::Error(error) => {
                StartupObservationFixtureOutcomeV2::Error(*error)
            }
            StartupObservationFixtureOutcomeV2::Pending { active, dropped } => {
                StartupObservationFixtureOutcomeV2::Pending {
                    active: active.clone(),
                    dropped: dropped.clone(),
                }
            }
        };
        async move {
            match outcome {
                StartupObservationFixtureOutcomeV2::Complete(state) => {
                    Ok(complete_startup_recovery_observation_v2(
                        authorization,
                        at_millis(1_000_100),
                        state,
                    ))
                }
                StartupObservationFixtureOutcomeV2::Error(error) => Err(error),
                StartupObservationFixtureOutcomeV2::Pending { active, dropped } => {
                    active.fetch_add(1, Ordering::AcqRel);
                    let _guard = PendingStartupObservationGuardV2 { active, dropped };
                    std::future::pending().await
                }
            }
        }
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
                FakeRenewStepV1::OutcomeUnknown => {
                    Err(RuntimeGatewayOwnerMutationErrorV1::OutcomeUnknown {
                        source: FakeErrorV1::Retryable,
                    })
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
        let state = self.state.clone();
        async move {
            state.release_calls.fetch_add(1, Ordering::AcqRel);
            let _guard = FakeOperationGuardV1::begin(state.clone());
            let gate = state
                .release_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(gate) = gate {
                gate.notified().await;
            }
            Ok(RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released {
                lease_id: request.lease_id,
                database_now: at_millis(1_000_002),
            })
        }
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
    receipt_with_epoch(lease_for, NonZeroU64::new(1).unwrap())
}

fn receipt_with_epoch(
    lease_for: Duration,
    lease_epoch: NonZeroU64,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    let database_now = at_millis(1_000_000);
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: RuntimeGatewayOwnerLeaseIdV1 {
            lease_epoch,
            ..lease_id()
        },
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

fn empty_startup_recovery_state_v2() -> RuntimeStartupRecoveryStateV2 {
    RuntimeStartupRecoveryStateV2 {
        serving: RuntimeStartupServingStateV2::Empty,
        recoverable_awaiting_certification_count: 0,
        suspended_local_effect_count: 0,
        pending_runtime_drain_intent_count: 0,
        acknowledged_product_handoff_count: 0,
    }
}

fn complete_startup_recovery_observation_v2(
    authorization: RuntimeAuthorizedStartupRecoveryObservationV2,
    database_now: DateTime<Utc>,
    state: RuntimeStartupRecoveryStateV2,
) -> RuntimeCompletedStartupRecoveryObservationV2 {
    let request = authorization.request().clone();
    authorization.complete(RuntimeStartupRecoveryObservationReceiptV2 {
        correlation: request.correlation,
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: request.gateway_owner_lease_id,
            owner_revision: request.expected_owner_revision,
            database_now,
            expires_at: request.expected_owner_expires_at,
        },
        state,
    })
}

fn complete_startup_recovery_execution_v2(
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    database_now: DateTime<Utc>,
) -> RuntimeCompletedStartupRecoveryExecutionV2 {
    let request = authorization.request();
    let receipt = RuntimeStartupRecoveryExecutionReceiptV2 {
        correlation: request.correlation().clone(),
        class: request.class(),
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: request.gateway_owner_lease_id().clone(),
            owner_revision: request.expected_owner_revision(),
            database_now,
            expires_at: request.expected_owner_expires_at(),
        },
        outcome: RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
            action_identity: request.action_identity().clone(),
            terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2::new([31; 32])
                .unwrap(),
        },
    };
    authorization.complete(receipt)
}

fn pending_drain_candidate_v2() -> RuntimePendingDrainCandidateV2 {
    let target: RuntimeDeploymentTargetV1 = serde_json::from_value(serde_json::json!({
        "guild_id": "42",
        "ruleset_key": "studyroom",
        "version": 1,
        "content_hash": "2".repeat(64),
        "binding_revision": 1,
        "binding_fingerprint": "3".repeat(64)
    }))
    .unwrap();
    RuntimePendingDrainCandidateV2::new(
        RuntimeDrainIntentIdV2::parse("07".repeat(16)).unwrap(),
        RuntimeServingSlotV2::from_target(&target),
        target,
        NonZeroU64::new(1).unwrap(),
        RuntimePendingDrainStateDigestV2::new([41; 32]).unwrap(),
    )
    .unwrap()
}

fn pending_drain_serving_source_evidence_v1(
    candidate: &RuntimePendingDrainCandidateV2,
) -> RuntimePendingDrainServingSourceEvidenceV1 {
    RuntimePendingDrainServingSourceEvidenceV1::from(
        &RuntimePendingDrainServingLookupV1::new(
            candidate.intent_id().clone(),
            candidate.source_intent_revision(),
            *candidate.source_state_digest().as_bytes(),
        )
        .unwrap(),
    )
}

fn pending_drain_test_serving_receipt_v1(
    target: RuntimeDeploymentTargetV1,
    revision: u64,
    observed_at: DateTime<Utc>,
    connected: bool,
) -> RuntimeServingReceiptV2 {
    let expires_at = observed_at - TimeDelta::milliseconds(1);
    let last_heartbeat_at = if connected {
        expires_at - TimeDelta::milliseconds(1)
    } else {
        expires_at
    };
    RuntimeServingReceiptV2 {
        identity: RuntimeServingIdentityV2 {
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("tenant:pending-drain").unwrap(),
                installation_id: InstallationId::parse("installation:pending-drain").unwrap(),
                deployment_id: DeploymentId::parse("deployment:pending-drain").unwrap(),
            },
            operation_id: RuntimeCertificationOperationIdV2::parse(
                "00112233445566778899aabbccddeeff",
            )
            .unwrap(),
            attestation_digest: RuntimeLiveAttestationDigestV2::parse("a".repeat(64)).unwrap(),
            process_identity: RuntimeProcessIdentityV1 {
                target,
                runtime_generation: RuntimeGeneration::new(4).unwrap(),
                process_instance_id: ProcessInstanceId::parse("process:pending-drain-source")
                    .unwrap(),
            },
            lease_epoch: NonZeroU64::new(5).unwrap(),
            revision: NonZeroU64::new(revision).unwrap(),
        },
        acquired_at: last_heartbeat_at - TimeDelta::milliseconds(1),
        last_heartbeat_at,
        expires_at,
        connected,
        serving: connected,
    }
}

fn pending_drain_owner_receipt_v2(
    request: &automation_runtime_worker::RuntimeStartupRecoveryExecutionRequestV2,
    database_now: DateTime<Utc>,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: request.gateway_owner_lease_id().clone(),
        owner_revision: request.expected_owner_revision(),
        database_now,
        expires_at: request.expected_owner_expires_at(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingDrainTestStageV2 {
    Selection,
    NoCandidate,
    Claim,
    Acknowledgement,
    Succession,
}

#[derive(Clone, Copy)]
enum PendingDrainTestSelectionV2 {
    Unclaimed,
    NoCandidate,
    FreshPreviousOwner,
    ExpiredPreviousOwner,
}

#[derive(Clone, Copy)]
enum PendingDrainTestAwaitFailureV2 {
    Transition(crate::process::RuntimeProcessStartupRecoveryLoopFailureV2),
    Database(PendingDrainPersistenceErrorV1),
}

#[derive(PartialEq, Eq)]
enum PendingDrainTestMutationFingerprintV2 {
    NoCandidate {
        authorization_address: usize,
        request_address: usize,
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    },
    Claim {
        authorization_address: usize,
        request_address: usize,
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        candidate: RuntimePendingDrainCandidateV2,
        seal: RuntimePendingDrainRegistrySealWitnessV2,
        minimum_database_now: DateTime<Utc>,
    },
    Acknowledgement {
        authorization_address: usize,
        request_address: usize,
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        claim_action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        candidate: RuntimePendingDrainCandidateV2,
        seal: RuntimePendingDrainRegistrySealWitnessV2,
        claim_terminal_digest: [u8; 32],
        claimed_intent_revision: NonZeroU64,
        claimed_state_digest: [u8; 32],
        minimum_database_now: DateTime<Utc>,
    },
    Succession {
        authorization_address: usize,
        request_address: usize,
        action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
        candidate: Box<RuntimePendingDrainPreviousOwnerClaimedCandidateV3>,
        seal: RuntimePendingDrainRegistrySealWitnessV2,
        minimum_database_now: DateTime<Utc>,
    },
}

impl PendingDrainTestMutationFingerprintV2 {
    fn stage(&self) -> PendingDrainTestStageV2 {
        match self {
            Self::NoCandidate { .. } => PendingDrainTestStageV2::NoCandidate,
            Self::Claim { .. } => PendingDrainTestStageV2::Claim,
            Self::Acknowledgement { .. } => PendingDrainTestStageV2::Acknowledgement,
            Self::Succession { .. } => PendingDrainTestStageV2::Succession,
        }
    }
}

struct FakePendingDrainRecoveryEnvironmentV2 {
    selection: PendingDrainTestSelectionV2,
    failures: VecDeque<(PendingDrainTestStageV2, PendingDrainTestAwaitFailureV2)>,
    transition_after: Option<(
        PendingDrainTestStageV2,
        usize,
        crate::process::RuntimeProcessStartupRecoveryLoopFailureV2,
    )>,
    session_invalidation_after: Option<(PendingDrainTestStageV2, usize)>,
    succession_revision_delta: u64,
    events: Vec<PendingDrainTestStageV2>,
    mutation_fingerprints: Vec<PendingDrainTestMutationFingerprintV2>,
    serving_observations:
        VecDeque<Result<RuntimePendingDrainServingObservationV1, RuntimeServingPersistenceErrorV1>>,
    serving_disconnects:
        VecDeque<Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1>>,
    current_serving: Option<RuntimeServingReceiptV2>,
    serving_observation_calls: usize,
    serving_disconnect_calls: usize,
    selection_database_now: Option<DateTime<Utc>>,
}

impl FakePendingDrainRecoveryEnvironmentV2 {
    fn new(selection: PendingDrainTestSelectionV2) -> Self {
        Self {
            selection,
            failures: VecDeque::new(),
            transition_after: None,
            session_invalidation_after: None,
            succession_revision_delta: 1,
            events: Vec::new(),
            mutation_fingerprints: Vec::new(),
            serving_observations: VecDeque::new(),
            serving_disconnects: VecDeque::new(),
            current_serving: None,
            serving_observation_calls: 0,
            serving_disconnect_calls: 0,
            selection_database_now: None,
        }
    }

    fn candidate() -> Self {
        Self::new(PendingDrainTestSelectionV2::Unclaimed)
    }

    fn no_candidate() -> Self {
        Self::new(PendingDrainTestSelectionV2::NoCandidate)
    }

    fn fresh_previous_owner() -> Self {
        Self::new(PendingDrainTestSelectionV2::FreshPreviousOwner)
    }

    fn expired_previous_owner() -> Self {
        Self::new(PendingDrainTestSelectionV2::ExpiredPreviousOwner)
    }

    fn failing(stage: PendingDrainTestStageV2, failure: PendingDrainTestAwaitFailureV2) -> Self {
        Self::failing_sequence([(stage, failure)])
    }

    fn failing_sequence(
        failures: impl IntoIterator<Item = (PendingDrainTestStageV2, PendingDrainTestAwaitFailureV2)>,
    ) -> Self {
        let failures: VecDeque<_> = failures.into_iter().collect();
        let selection = if failures
            .iter()
            .any(|(stage, _)| *stage == PendingDrainTestStageV2::NoCandidate)
        {
            PendingDrainTestSelectionV2::NoCandidate
        } else if failures
            .iter()
            .any(|(stage, _)| *stage == PendingDrainTestStageV2::Succession)
        {
            PendingDrainTestSelectionV2::ExpiredPreviousOwner
        } else {
            PendingDrainTestSelectionV2::Unclaimed
        };
        let mut environment = Self::new(selection);
        environment.failures = failures;
        environment
    }

    fn with_transition_after(
        mut self,
        stage: PendingDrainTestStageV2,
        invocation_count: usize,
        transition: crate::process::RuntimeProcessStartupRecoveryLoopFailureV2,
    ) -> Self {
        self.transition_after = Some((stage, invocation_count, transition));
        self
    }

    fn with_session_invalidation_after(
        mut self,
        stage: PendingDrainTestStageV2,
        invocation_count: usize,
    ) -> Self {
        self.session_invalidation_after = Some((stage, invocation_count));
        self
    }

    fn with_succession_revision_delta(mut self, delta: u64) -> Self {
        self.succession_revision_delta = delta;
        self
    }

    fn with_serving_observations(
        mut self,
        observations: impl IntoIterator<
            Item = Result<
                RuntimePendingDrainServingObservationV1,
                RuntimeServingPersistenceErrorV1,
            >,
        >,
    ) -> Self {
        self.serving_observations = observations.into_iter().collect();
        self
    }

    fn with_serving_disconnects(
        mut self,
        disconnects: impl IntoIterator<
            Item = Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1>,
        >,
    ) -> Self {
        self.serving_disconnects = disconnects.into_iter().collect();
        self
    }

    fn finish_stage_v2<T>(
        &mut self,
        stage: PendingDrainTestStageV2,
        value: T,
    ) -> Result<
        T,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        self.events.push(stage);
        let failure = self
            .failures
            .front()
            .copied()
            .filter(|(failed_stage, _)| *failed_stage == stage)
            .map(|(_, failure)| {
                self.failures.pop_front();
                failure
            });
        match failure {
            Some(PendingDrainTestAwaitFailureV2::Transition(error)) => Err(
                crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2::Transition(error),
            ),
            Some(PendingDrainTestAwaitFailureV2::Database(error)) => {
                Err(crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(error))
            }
            _ => Ok(value),
        }
    }
}

fn pending_drain_test_database_now_v2(
    request: &automation_runtime_worker::RuntimeStartupRecoveryExecutionRequestV2,
    offset_millis: i64,
) -> DateTime<Utc> {
    request.minimum_database_now() + TimeDelta::milliseconds(offset_millis)
}

impl crate::process::RuntimePendingDrainRecoveryEnvironmentV2
    for FakePendingDrainRecoveryEnvironmentV2
{
    fn current_transition_v2(
        &self,
        session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
    ) -> Option<crate::process::RuntimeProcessStartupRecoveryLoopFailureV2> {
        if self
            .session_invalidation_after
            .is_some_and(|(stage, invocation_count)| {
                self.events.iter().filter(|event| **event == stage).count() >= invocation_count
            })
        {
            session.invalidate_startup_recovery_execution_v2();
        }
        self.transition_after
            .filter(|(stage, invocation_count, _)| {
                self.events.iter().filter(|event| **event == *stage).count() >= *invocation_count
            })
            .map(|(_, _, transition)| transition)
    }

    async fn select_pending_drain_v3(
        &mut self,
        _session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV3,
    ) -> Result<
        RuntimePendingDrainSelectionReceiptV3,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        let outcome = match self.selection {
            PendingDrainTestSelectionV2::Unclaimed => {
                RuntimePendingDrainSelectionOutcomeV3::Unclaimed(pending_drain_candidate_v2())
            }
            PendingDrainTestSelectionV2::NoCandidate => {
                RuntimePendingDrainSelectionOutcomeV3::NoCandidate
            }
            PendingDrainTestSelectionV2::FreshPreviousOwner => {
                RuntimePendingDrainSelectionOutcomeV3::FreshPreviousOwner(
                    crate::registry::succession_tests::candidate_with_claim_expiry(
                        1,
                        pending_drain_test_database_now_v2(authorization.request(), 500),
                    ),
                )
            }
            PendingDrainTestSelectionV2::ExpiredPreviousOwner => {
                RuntimePendingDrainSelectionOutcomeV3::ExpiredPreviousOwner(
                    crate::registry::succession_tests::candidate(1),
                )
            }
        };
        let database_now = pending_drain_test_database_now_v2(authorization.request(), 1);
        self.selection_database_now = Some(database_now);
        let receipt = RuntimePendingDrainSelectionReceiptV3::new(
            authorization.request().correlation().clone(),
            pending_drain_owner_receipt_v2(authorization.request(), database_now),
            outcome,
        );
        self.finish_stage_v2(PendingDrainTestStageV2::Selection, receipt)
    }

    async fn observe_pending_drain_source_serving_v1(
        &mut self,
        _session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        candidate: &RuntimePendingDrainCandidateV2,
    ) -> Result<
        RuntimePendingDrainServingObservationV1,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            RuntimeServingPersistenceErrorV1,
        >,
    > {
        self.serving_observation_calls += 1;
        let result = self.serving_observations.pop_front().unwrap_or_else(|| {
            let observed_at = self.selection_database_now.unwrap() + TimeDelta::milliseconds(1);
            let serving = pending_drain_test_serving_receipt_v1(
                candidate.expected_target().clone(),
                7,
                observed_at,
                false,
            );
            Ok(RuntimePendingDrainServingObservationV1::Disconnected {
                source: pending_drain_serving_source_evidence_v1(candidate),
                serving: Box::new(serving),
                observed_at,
            })
        });
        match result {
            Ok(observation) => {
                self.current_serving = match &observation {
                    RuntimePendingDrainServingObservationV1::Fresh { serving, .. }
                    | RuntimePendingDrainServingObservationV1::Expired { serving, .. }
                    | RuntimePendingDrainServingObservationV1::Disconnected { serving, .. } => {
                        Some((**serving).clone())
                    }
                    RuntimePendingDrainServingObservationV1::Absent { .. }
                    | RuntimePendingDrainServingObservationV1::Diverged { .. } => None,
                };
                Ok(observation)
            }
            Err(error) => {
                Err(crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(error))
            }
        }
    }

    async fn disconnect_pending_drain_source_serving_v1(
        &mut self,
        _session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        candidate: &RuntimePendingDrainCandidateV2,
        identity: &RuntimeServingIdentityV2,
    ) -> Result<
        RuntimeServingReceiptV2,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            RuntimeServingPersistenceErrorV1,
        >,
    > {
        self.serving_disconnect_calls += 1;
        assert_eq!(
            candidate.expected_target(),
            &identity.process_identity.target
        );
        assert_eq!(
            candidate.slot(),
            &automation_runtime_controller::RuntimeServingSlotV2::from_target(
                &identity.process_identity.target,
            )
        );
        let result = self.serving_disconnects.pop_front().unwrap_or_else(|| {
            let mut serving = self.current_serving.clone().unwrap();
            assert_eq!(&serving.identity, identity);
            serving.identity.revision = NonZeroU64::new(identity.revision.get() + 1).unwrap();
            serving.connected = false;
            serving.serving = false;
            serving.last_heartbeat_at = serving.expires_at;
            Ok(serving)
        });
        match result {
            Ok(serving) => {
                self.current_serving = Some(serving.clone());
                Ok(serving)
            }
            Err(error) => {
                Err(crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2::Database(error))
            }
        }
    }

    async fn record_pending_drain_no_candidate_v2(
        &mut self,
        _session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        selection: &RuntimeSelectedPendingDrainNoCandidateV2,
    ) -> Result<
        RuntimePendingDrainNoCandidateReceiptV2,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        self.mutation_fingerprints
            .push(PendingDrainTestMutationFingerprintV2::NoCandidate {
                authorization_address: std::ptr::from_ref(selection).addr(),
                request_address: std::ptr::from_ref(selection.request()).addr(),
                action_identity: selection.request().action_identity().clone(),
            });
        let receipt = RuntimePendingDrainNoCandidateReceiptV2::new(
            selection.request().action_identity().clone(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([46; 32]).unwrap(),
            pending_drain_owner_receipt_v2(
                selection.request(),
                pending_drain_test_database_now_v2(selection.request(), 2),
            ),
        );
        self.finish_stage_v2(PendingDrainTestStageV2::NoCandidate, receipt)
    }

    async fn execute_pending_drain_claim_v2(
        &mut self,
        _session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainClaimV2,
    ) -> Result<
        RuntimePendingDrainClaimReceiptV2,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        self.mutation_fingerprints
            .push(PendingDrainTestMutationFingerprintV2::Claim {
                authorization_address: std::ptr::from_ref(authorization).addr(),
                request_address: std::ptr::from_ref(authorization.request()).addr(),
                action_identity: authorization.action_identity().clone(),
                candidate: authorization.candidate().clone(),
                seal: authorization.seal().clone(),
                minimum_database_now: authorization.minimum_database_now(),
            });
        let receipt = RuntimePendingDrainClaimReceiptV2::new(
            authorization.action_identity().clone(),
            authorization.candidate().clone(),
            authorization.seal().clone(),
            NonZeroU64::new(2).unwrap(),
            RuntimePendingDrainStateDigestV2::new([42; 32]).unwrap(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([43; 32]).unwrap(),
            pending_drain_owner_receipt_v2(
                authorization.request(),
                pending_drain_test_database_now_v2(authorization.request(), 2),
            ),
        );
        self.finish_stage_v2(PendingDrainTestStageV2::Claim, receipt)
    }

    async fn execute_pending_drain_acknowledgement_v2(
        &mut self,
        _session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainAcknowledgementV2,
    ) -> Result<
        RuntimePendingDrainAcknowledgementReceiptV2,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        self.mutation_fingerprints
            .push(PendingDrainTestMutationFingerprintV2::Acknowledgement {
                authorization_address: std::ptr::from_ref(authorization).addr(),
                request_address: std::ptr::from_ref(authorization.request()).addr(),
                action_identity: authorization.action_identity().clone(),
                claim_action_identity: authorization.request().action_identity().clone(),
                candidate: authorization.candidate().clone(),
                seal: authorization.seal().clone(),
                claim_terminal_digest: *authorization.claim_terminal_digest().as_bytes(),
                claimed_intent_revision: authorization.claimed_intent_revision(),
                claimed_state_digest: *authorization.claimed_state_digest().as_bytes(),
                minimum_database_now: authorization.minimum_database_now(),
            });
        let receipt = RuntimePendingDrainAcknowledgementReceiptV2::new(
            authorization.action_identity().clone(),
            authorization.request().action_identity().clone(),
            authorization.candidate().clone(),
            authorization.seal().clone(),
            authorization.claimed_intent_revision(),
            authorization.claimed_state_digest().clone(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([43; 32]).unwrap(),
            NonZeroU64::new(3).unwrap(),
            RuntimePendingDrainStateDigestV2::new([44; 32]).unwrap(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([45; 32]).unwrap(),
            pending_drain_owner_receipt_v2(
                authorization.request(),
                pending_drain_test_database_now_v2(authorization.request(), 3),
            ),
        );
        self.finish_stage_v2(PendingDrainTestStageV2::Acknowledgement, receipt)
    }

    async fn execute_pending_drain_succession_v3(
        &mut self,
        _session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    ) -> Result<
        RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        self.mutation_fingerprints
            .push(PendingDrainTestMutationFingerprintV2::Succession {
                authorization_address: std::ptr::from_ref(authorization).addr(),
                request_address: std::ptr::from_ref(authorization.request()).addr(),
                action_identity: authorization.action_identity().clone(),
                candidate: Box::new(authorization.candidate().clone()),
                seal: authorization.seal().clone(),
                minimum_database_now: authorization.minimum_database_now(),
            });
        let receipt = RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
            authorization.action_identity().clone(),
            authorization.candidate().clone(),
            authorization.seal().clone(),
            NonZeroU64::new(
                authorization.candidate().source_intent_revision().get()
                    + self.succession_revision_delta,
            )
            .unwrap(),
            RuntimePendingDrainStateDigestV2::new([47; 32]).unwrap(),
            RuntimeStartupRecoveryExecutionTerminalDigestV2::new([48; 32]).unwrap(),
            pending_drain_owner_receipt_v2(
                authorization.request(),
                pending_drain_test_database_now_v2(authorization.request(), 2),
            ),
        );
        self.finish_stage_v2(PendingDrainTestStageV2::Succession, receipt)
    }
}

impl crate::process::RuntimePendingDrainMutationEnvironmentV3
    for FakePendingDrainRecoveryEnvironmentV2
{
    fn current_transition_v3(
        &self,
        session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
    ) -> Option<crate::process::RuntimeProcessStartupRecoveryLoopFailureV2> {
        crate::process::RuntimePendingDrainRecoveryEnvironmentV2::current_transition_v2(
            self, session,
        )
    }

    async fn record_no_candidate_v3(
        &mut self,
        session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        selection: &RuntimeSelectedPendingDrainNoCandidateV2,
    ) -> Result<
        RuntimePendingDrainNoCandidateReceiptV2,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        crate::process::RuntimePendingDrainRecoveryEnvironmentV2::record_pending_drain_no_candidate_v2(
            self,
            session,
            selection,
        )
        .await
    }

    async fn execute_claim_v3(
        &mut self,
        session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainClaimV2,
    ) -> Result<
        RuntimePendingDrainClaimReceiptV2,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        crate::process::RuntimePendingDrainRecoveryEnvironmentV2::execute_pending_drain_claim_v2(
            self,
            session,
            authorization,
        )
        .await
    }

    async fn execute_acknowledgement_v3(
        &mut self,
        session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainAcknowledgementV2,
    ) -> Result<
        RuntimePendingDrainAcknowledgementReceiptV2,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        crate::process::RuntimePendingDrainRecoveryEnvironmentV2::execute_pending_drain_acknowledgement_v2(
            self,
            session,
            authorization,
        )
        .await
    }

    async fn execute_succession_v3(
        &mut self,
        session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
    ) -> Result<
        RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            PendingDrainPersistenceErrorV1,
        >,
    > {
        crate::process::RuntimePendingDrainRecoveryEnvironmentV2::execute_pending_drain_succession_v3(
            self,
            session,
            authorization,
        )
        .await
    }

    async fn observe_pending_drain_source_serving_v3(
        &mut self,
        session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        candidate: &RuntimePendingDrainCandidateV2,
    ) -> Result<
        RuntimePendingDrainServingObservationV1,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            RuntimeServingPersistenceErrorV1,
        >,
    > {
        crate::process::RuntimePendingDrainRecoveryEnvironmentV2::observe_pending_drain_source_serving_v1(
            self,
            session,
            candidate,
        )
        .await
    }

    async fn disconnect_pending_drain_source_serving_v3(
        &mut self,
        session: &crate::closed_recovery::RuntimeClosedRecoverySessionV2,
        candidate: &RuntimePendingDrainCandidateV2,
        identity: &RuntimeServingIdentityV2,
    ) -> Result<
        RuntimeServingReceiptV2,
        crate::process::RuntimeStartupRecoveryExecutionAwaitFailureV2<
            RuntimeServingPersistenceErrorV1,
        >,
    > {
        crate::process::RuntimePendingDrainRecoveryEnvironmentV2::disconnect_pending_drain_source_serving_v1(
            self,
            session,
            candidate,
            identity,
        )
        .await
    }
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
    fixture_with_startup_cleanup_deadline(lease_for, renew_before, safety_margin, renew_steps, None)
}

fn fixture_with_startup_cleanup_deadline(
    lease_for: Duration,
    renew_before: Duration,
    safety_margin: Duration,
    renew_steps: impl IntoIterator<Item = FakeRenewStepV1>,
    startup_cleanup_deadline: Option<Instant>,
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
        config,
        RuntimeGatewayOwnerStartupWatchdogStartContextV1::new(
            started,
            started,
            startup_cleanup_deadline,
        ),
    )
    .unwrap();
    (handle, port, invalidated)
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

async fn initial_pending_recovery_fixture_v2(
    recovery_id: &str,
) -> (
    crate::RuntimeGatewayBootstrapV1,
    crate::RuntimeRegistryBootstrapV1,
    crate::closed_recovery::RuntimeClosedRecoveryPendingPhaseV2,
    FakePortV1,
) {
    initial_pending_recovery_fixture_until_with_registry_v2(
        recovery_id,
        Instant::now() + Duration::from_secs(4),
        InitialRegistryFixtureV2::RetainedTombstone,
    )
    .await
}

async fn initial_pending_recovery_fixture_until_v2(
    recovery_id: &str,
    operation_cutoff: Instant,
) -> (
    crate::RuntimeGatewayBootstrapV1,
    crate::RuntimeRegistryBootstrapV1,
    crate::closed_recovery::RuntimeClosedRecoveryPendingPhaseV2,
    FakePortV1,
) {
    initial_pending_recovery_fixture_until_with_registry_v2(
        recovery_id,
        operation_cutoff,
        InitialRegistryFixtureV2::RetainedTombstone,
    )
    .await
}

#[derive(Clone, Copy)]
enum InitialRegistryFixtureV2 {
    Fresh,
    RetainedTombstone,
}

async fn initial_pending_recovery_fixture_until_with_registry_v2(
    recovery_id: &str,
    operation_cutoff: Instant,
    registry_fixture: InitialRegistryFixtureV2,
) -> (
    crate::RuntimeGatewayBootstrapV1,
    crate::RuntimeRegistryBootstrapV1,
    crate::closed_recovery::RuntimeClosedRecoveryPendingPhaseV2,
    FakePortV1,
) {
    initial_pending_recovery_fixture_until_with_registry_and_owner_v2(
        recovery_id,
        operation_cutoff,
        registry_fixture,
        NonZeroU64::new(1).unwrap(),
    )
    .await
}

async fn initial_pending_recovery_fixture_until_with_registry_and_owner_v2(
    recovery_id: &str,
    operation_cutoff: Instant,
    registry_fixture: InitialRegistryFixtureV2,
    owner_epoch: NonZeroU64,
) -> (
    crate::RuntimeGatewayBootstrapV1,
    crate::RuntimeRegistryBootstrapV1,
    crate::closed_recovery::RuntimeClosedRecoveryPendingPhaseV2,
    FakePortV1,
) {
    let process_instance_id = ProcessInstanceId::parse("process:handoff").unwrap();
    let mut gateway = crate::compose_runtime_gateway_bootstrap_v1(
        process_instance_id.clone(),
        crate::GatewayResourceConfigV1::default(),
    )
    .unwrap();
    gateway.connect_ready_for_gateway_section_test_v2();
    let lease_for = Duration::from_secs(5);
    let owner_receipt = receipt_with_epoch(lease_for, owner_epoch);
    let port = FakePortV1::new(owner_receipt.clone(), []);
    let config = RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
        lease_for,
        Duration::from_secs(4),
        Duration::from_millis(500),
        Duration::from_millis(20),
        Duration::from_millis(500),
    )
    .unwrap();
    let started = Instant::now();
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(
            port.clone(),
            accepted_receipt(owner_receipt),
            started,
            started,
            config,
        )
        .unwrap();
    let owner = handle.prepare_closed_recovery_v2().await.unwrap();
    let registry = crate::compose_runtime_registry_bootstrap_v1(
        process_instance_id,
        crate::GatewayResourceConfigV1::default(),
    )
    .unwrap();
    if matches!(
        registry_fixture,
        InitialRegistryFixtureV2::RetainedTombstone
    ) {
        registry.advance_empty_sequence_for_test_v2();
    }
    let readiness = crate::database::runtime_database_readiness_for_test_v1();
    let paused_gateway = gateway.observe_paused_connected_gateway_v2().unwrap();
    let pending = crate::closed_recovery::begin_initial_empty_recovery_v2(
        &mut gateway,
        &registry,
        owner,
        RuntimeRecoveryIdV2::parse(recovery_id).unwrap(),
        &readiness,
        &paused_gateway,
        operation_cutoff,
    )
    .unwrap();
    (gateway, registry, pending, port)
}

async fn initial_committed_recovery_fixture_v2(
    recovery_id: &str,
) -> (
    crate::RuntimeGatewayBootstrapV1,
    crate::RuntimeRegistryBootstrapV1,
    crate::closed_recovery::RuntimeClosedRecoverySessionV2,
    FakePortV1,
) {
    let (gateway, registry, pending, port) = initial_pending_recovery_fixture_v2(recovery_id).await;
    let session = pending.commit_owner_v2().await.unwrap();
    (gateway, registry, session, port)
}

async fn initial_committed_fresh_registry_recovery_with_owner_epoch_v2(
    recovery_id: &str,
    owner_epoch: NonZeroU64,
) -> (
    crate::RuntimeGatewayBootstrapV1,
    crate::RuntimeRegistryBootstrapV1,
    crate::closed_recovery::RuntimeClosedRecoverySessionV2,
    FakePortV1,
) {
    let (gateway, registry, pending, port) =
        initial_pending_recovery_fixture_until_with_registry_and_owner_v2(
            recovery_id,
            Instant::now() + Duration::from_secs(4),
            InitialRegistryFixtureV2::Fresh,
            owner_epoch,
        )
        .await;
    let session = pending.commit_owner_v2().await.unwrap();
    (gateway, registry, session, port)
}

async fn initial_pending_drain_continue_fresh_registry_fixture_v2(
    recovery_id: &str,
) -> (
    crate::RuntimeGatewayBootstrapV1,
    crate::RuntimeRegistryBootstrapV1,
    crate::closed_recovery::RuntimeClosedRecoverySessionV2,
    FakePortV1,
) {
    let (gateway, registry, session, port) =
        initial_committed_fresh_registry_recovery_with_owner_epoch_v2(
            recovery_id,
            NonZeroU64::new(7).unwrap(),
        )
        .await;
    let mut pending = empty_startup_recovery_state_v2();
    pending.pending_runtime_drain_intent_count = 1;
    let outcome = session
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    pending,
                ),
            ))
        })
        .await
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
        session,
        continuation,
    } = outcome
    else {
        panic!("expected pending drain continuation")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(
            RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
        )
    );
    (gateway, registry, session, port)
}

async fn initial_committed_recovery_fixture_until_v2(
    recovery_id: &str,
    operation_cutoff: Instant,
) -> (
    crate::RuntimeGatewayBootstrapV1,
    crate::RuntimeRegistryBootstrapV1,
    crate::closed_recovery::RuntimeClosedRecoverySessionV2,
    FakePortV1,
) {
    let (gateway, registry, pending, port) =
        initial_pending_recovery_fixture_until_v2(recovery_id, operation_cutoff).await;
    let session = pending.commit_owner_v2().await.unwrap();
    (gateway, registry, session, port)
}

async fn ready_recovery_iteration_fixture_v2(
    recovery_id: &str,
    operation_cutoff: Instant,
) -> (
    crate::RuntimeGatewayBootstrapV1,
    crate::RuntimeRegistryBootstrapV1,
    crate::closed_recovery::RuntimeClosedRecoveryReadyIterationV2,
    FakePortV1,
) {
    let (gateway, registry, session, port) =
        initial_committed_recovery_fixture_until_v2(recovery_id, operation_cutoff).await;
    let ready = session
        .refresh_iteration_readiness_with_test_verifier_v2(|_| {
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_for_test_v2(),
            ))
        })
        .await
        .unwrap();
    (gateway, registry, ready, port)
}

#[tokio::test]
async fn in_place_startup_observation_finalizes_continue_once() {
    let (gateway, _registry, mut iteration, owner_port) = ready_recovery_iteration_fixture_v2(
        "41414141414141414141414141414141",
        Instant::now() + Duration::from_secs(2),
    )
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut state = empty_startup_recovery_state_v2();
    state.serving = RuntimeStartupServingStateV2::RecoverableStale { count: 1 };
    let observer = StartupObservationFixturePortV2 {
        calls: calls.clone(),
        outcome: StartupObservationFixtureOutcomeV2::Complete(state),
    };

    let completion = iteration
        .observe_startup_recovery_interruptible_in_place_v2(&observer, std::future::pending::<()>())
        .await
        .unwrap();
    let outcome = iteration
        .into_startup_recovery_observation_outcome_v2(completion)
        .unwrap();

    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
        session,
        continuation,
    } = outcome
    else {
        panic!("expected startup continuation")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(RuntimeStartupRecoveryClassV2::StaleLive)
    );
    assert_eq!(calls.load(Ordering::Acquire), 1);
    let _ = session
        .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
        .await;
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
    ));
    wait_for(|| owner_port.release_calls() == 1).await;
}

#[tokio::test]
async fn in_place_startup_observation_finalizes_fixed_point_once() {
    let (gateway, _registry, mut iteration, owner_port) = ready_recovery_iteration_fixture_v2(
        "42424242424242424242424242424242",
        Instant::now() + Duration::from_secs(2),
    )
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let observer = StartupObservationFixturePortV2 {
        calls: calls.clone(),
        outcome: StartupObservationFixtureOutcomeV2::Complete(empty_startup_recovery_state_v2()),
    };

    let completion = iteration
        .observe_startup_recovery_interruptible_in_place_v2(&observer, std::future::pending::<()>())
        .await
        .unwrap();
    let outcome = iteration
        .into_startup_recovery_observation_outcome_v2(completion)
        .unwrap();

    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(
        fixed_point,
    ) = outcome
    else {
        panic!("expected startup fixed point")
    };
    assert_eq!(calls.load(Ordering::Acquire), 1);
    let _ = fixed_point
        .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
        .await;
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
    ));
    wait_for(|| owner_port.release_calls() == 1).await;
}

#[tokio::test]
async fn production_resume_advances_exact_successor_and_reaches_admission_acknowledging() {
    let process_instance_id = ProcessInstanceId::parse("process:handoff").unwrap();
    let mut gateway = crate::compose_runtime_gateway_bootstrap_v1(
        process_instance_id.clone(),
        crate::GatewayResourceConfigV1::default(),
    )
    .unwrap();
    let (signals, driver, _polls, _closes, _drops) = crate::discord::test_support::driver();
    let discord = gateway
        .start_discord_gateway_with_driver_v1(
            driver,
            Instant::now() + Duration::from_secs(4),
            Instant::now() + Duration::from_secs(6),
        )
        .await
        .unwrap();
    signals
        .send(crate::discord::RuntimeDiscordGatewaySignalV1::Ready)
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if gateway.observe_paused_connected_gateway_v2().is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let lease_for = Duration::from_secs(5);
    let owner_receipt = receipt_with_epoch(lease_for, NonZeroU64::MIN);
    let owner_port = FakePortV1::new(owner_receipt.clone(), []);
    let owner_config = RuntimeGatewayOwnerStartupWatchdogConfigV1::new(
        lease_for,
        Duration::from_secs(4),
        Duration::from_millis(500),
        Duration::from_millis(20),
        Duration::from_millis(500),
    )
    .unwrap();
    let started = Instant::now();
    let owner = gateway
        .start_gateway_owner_startup_watchdog_v1(
            owner_port.clone(),
            accepted_receipt(owner_receipt),
            started,
            started,
            owner_config,
        )
        .unwrap()
        .prepare_closed_recovery_v2()
        .await
        .unwrap();
    let registry = crate::compose_runtime_registry_bootstrap_v1(
        process_instance_id,
        crate::GatewayResourceConfigV1::default(),
    )
    .unwrap();
    registry.advance_empty_sequence_for_test_v2();
    let readiness = crate::database::runtime_database_readiness_for_test_v1();
    let paused_gateway = gateway.observe_paused_connected_gateway_v2().unwrap();
    let pending = crate::closed_recovery::begin_initial_empty_recovery_v2(
        &mut gateway,
        &registry,
        owner,
        RuntimeRecoveryIdV2::parse("53535353535353535353535353535353").unwrap(),
        &readiness,
        &paused_gateway,
        Instant::now() + Duration::from_secs(4),
    )
    .unwrap();
    let session = pending.commit_owner_v2().await.unwrap();
    let outcome = session
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    empty_startup_recovery_state_v2(),
                ),
            ))
        })
        .await
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(
        fixed_point,
    ) = outcome
    else {
        panic!("expected startup fixed point")
    };
    owner_port
        .state
        .receipt
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .database_now = at_millis(1_000_100);
    let mut fixed_point = match fixed_point.into_worker_fixed_point_v2() {
        Ok(fixed_point) => fixed_point,
        Err(_) => panic!("expected worker fixed point"),
    };
    fixed_point
        .enter_admission_frozen_in_place_v2()
        .await
        .unwrap();
    let mut lifecycle = match fixed_point.try_into_admission_frozen_v2() {
        Ok(lifecycle) => lifecycle,
        Err(_) => panic!("expected admission-frozen lifecycle"),
    };
    let process_generation = NonZeroU64::MIN;
    let discord = match discord.handoff_to_process_v2(process_generation).await {
        crate::discord::RuntimeDiscordProcessHandoffV2::Process(discord) => discord,
        outcome => {
            drop(outcome);
            panic!("expected Discord process handoff")
        }
    };
    lifecycle
        .activate_process_owner_in_place_v2(process_generation)
        .await
        .unwrap();
    let lifecycle = match lifecycle.try_into_process_frozen_v2() {
        Ok(lifecycle) => lifecycle,
        Err(_) => panic!("expected process-frozen lifecycle"),
    };
    let finalizer_generation =
        automation_runtime_worker::RuntimeMutationFinalizerGenerationV1::new(NonZeroU64::MIN)
            .unwrap();
    let lifecycle = match lifecycle
        .into_production_handoff_v2(finalizer_generation)
        .await
    {
        Ok(lifecycle) => lifecycle,
        Err(_) => panic!("expected production-handoff lifecycle"),
    };
    let predecessor = lifecycle.coordinator_generation_v2();
    let mut discord = discord;
    let stage = crate::process::execute_recovery_resume_gateway_stage_v2(
        &mut discord,
        &lifecycle,
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    let lifecycle = match lifecycle
        .into_admission_acknowledging_v2(
            crate::closed_recovery::RuntimeClosedRecoveryResumeObservationV2 {
                owner_receipt: stage.owner_receipt,
                readiness: crate::database::runtime_database_readiness_refresh_for_test_v2()
                    .into_exact_capability_receipts(),
                gateway_ready: stage.gateway_ready,
                writer_fence_generation:
                    automation_runtime_controller::RuntimeWriterFenceGenerationV1::new(
                        NonZeroU64::MIN,
                    ),
                maintenance_gate_generation:
                    automation_runtime_worker::RuntimeMaintenanceGateGenerationV2::new(
                        NonZeroU64::MIN,
                    )
                    .unwrap(),
            },
        )
        .await
    {
        Ok(lifecycle) => lifecycle,
        Err(failure) => panic!(
            "expected admission-acknowledging lifecycle: {:?}",
            failure.error_v2()
        ),
    };

    assert_eq!(
        lifecycle.coordinator_generation_v2().get(),
        predecessor.get() + 1
    );
    assert_eq!(lifecycle.process_generation_v2(), process_generation);
    assert_eq!(
        discord
            .shutdown_until(
                gateway.begin_discord_drain_v1(),
                process_generation,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap()
            .exit(),
        crate::discord::RuntimeDiscordGatewayExitV1::Commanded
    );
    assert_eq!(
        lifecycle
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(owner_port.release_calls(), 1);
}

#[tokio::test]
async fn in_place_startup_observer_error_retains_bounded_cleanup_authority() {
    let (gateway, _registry, mut iteration, owner_port) = ready_recovery_iteration_fixture_v2(
        "43434343434343434343434343434343",
        Instant::now() + Duration::from_secs(2),
    )
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let observer = StartupObservationFixturePortV2 {
        calls: calls.clone(),
        outcome: StartupObservationFixtureOutcomeV2::Error(FakeErrorV1::Retryable),
    };

    let error = iteration
        .observe_startup_recovery_interruptible_in_place_v2(&observer, std::future::pending::<()>())
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
            crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::Observer(
                FakeErrorV1::Retryable
            )
        )
    ));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    let _ = iteration
        .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
        .await;
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
    ));
    wait_for(|| owner_port.release_calls() == 1).await;
}

#[tokio::test]
async fn in_place_startup_deadline_skips_observer_and_retains_cleanup() {
    let (gateway, _registry, iteration, owner_port) = ready_recovery_iteration_fixture_v2(
        "44444444444444444444444444444444",
        Instant::now() + Duration::from_secs(2),
    )
    .await;
    let mut iteration = iteration.with_operation_cutoff_for_test_v2(Instant::now());
    let calls = Arc::new(AtomicUsize::new(0));
    let observer = StartupObservationFixturePortV2 {
        calls: calls.clone(),
        outcome: StartupObservationFixtureOutcomeV2::Complete(empty_startup_recovery_state_v2()),
    };

    let error = iteration
        .observe_startup_recovery_interruptible_in_place_v2(&observer, std::future::pending::<()>())
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Observation(
            crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::DeadlineElapsed
        )
    ));
    assert_eq!(calls.load(Ordering::Acquire), 0);
    let _ = iteration
        .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
        .await;
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
    ));
    wait_for(|| owner_port.release_calls() == 1).await;
}

#[tokio::test]
async fn in_place_startup_interrupt_drops_losing_observer_and_retains_cleanup() {
    let (gateway, _registry, mut iteration, owner_port) = ready_recovery_iteration_fixture_v2(
        "45454545454545454545454545454545",
        Instant::now() + Duration::from_secs(2),
    )
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let observer = StartupObservationFixturePortV2 {
        calls: calls.clone(),
        outcome: StartupObservationFixtureOutcomeV2::Pending {
            active: active.clone(),
            dropped: dropped.clone(),
        },
    };
    let interrupt_active = active.clone();
    let interrupt = async move {
        while interrupt_active.load(Ordering::Acquire) == 0 {
            tokio::task::yield_now().await;
        }
        "discord"
    };

    let error = iteration
        .observe_startup_recovery_interruptible_in_place_v2(&observer, interrupt)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationAttemptErrorV2::Interrupted(
            "discord"
        )
    ));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(active.load(Ordering::Acquire), 0);
    assert_eq!(dropped.load(Ordering::Acquire), 1);
    let _ = iteration
        .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
        .await;
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
    ));
    wait_for(|| owner_port.release_calls() == 1).await;
}

#[tokio::test]
async fn dropping_polled_in_place_observation_preserves_cleanup_authority() {
    let (gateway, _registry, mut iteration, owner_port) = ready_recovery_iteration_fixture_v2(
        "46464646464646464646464646464646",
        Instant::now() + Duration::from_secs(2),
    )
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let observer = StartupObservationFixturePortV2 {
        calls: calls.clone(),
        outcome: StartupObservationFixtureOutcomeV2::Pending {
            active: active.clone(),
            dropped: dropped.clone(),
        },
    };
    let mut observation = Box::pin(
        iteration.observe_startup_recovery_interruptible_in_place_v2(
            &observer,
            std::future::pending::<()>(),
        ),
    );
    std::future::poll_fn(|context| {
        assert!(observation.as_mut().poll(context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;

    drop(observation);

    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(active.load(Ordering::Acquire), 0);
    assert_eq!(dropped.load(Ordering::Acquire), 1);
    let _ = iteration
        .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
        .await;
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
    ));
    wait_for(|| owner_port.release_calls() == 1).await;
    sleep(Duration::from_millis(20)).await;
    assert_eq!(owner_port.release_calls(), 1);
}

#[tokio::test]
async fn dropping_unpolled_in_place_observation_preserves_the_single_call() {
    let (_gateway, _registry, mut iteration, owner_port) = ready_recovery_iteration_fixture_v2(
        "47474747474747474747474747474747",
        Instant::now() + Duration::from_secs(2),
    )
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let observer = StartupObservationFixturePortV2 {
        calls: calls.clone(),
        outcome: StartupObservationFixtureOutcomeV2::Complete(empty_startup_recovery_state_v2()),
    };
    let observation = iteration.observe_startup_recovery_interruptible_in_place_v2(
        &observer,
        std::future::pending::<()>(),
    );

    drop(observation);

    assert_eq!(calls.load(Ordering::Acquire), 0);
    let completion = iteration
        .observe_startup_recovery_interruptible_in_place_v2(&observer, std::future::pending::<()>())
        .await
        .unwrap();
    let outcome = iteration
        .into_startup_recovery_observation_outcome_v2(completion)
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(
        fixed_point,
    ) = outcome
    else {
        panic!("expected startup fixed point")
    };
    assert_eq!(calls.load(Ordering::Acquire), 1);
    let _ = fixed_point
        .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
        .await;
    wait_for(|| owner_port.release_calls() == 1).await;
}

#[tokio::test]
async fn committed_startup_observation_refreshes_then_advances_and_remains_closed() {
    let operation_cutoff = Instant::now() + Duration::from_secs(2);
    let (gateway, _registry, session, port) = initial_committed_recovery_fixture_until_v2(
        "15151515151515151515151515151515",
        operation_cutoff,
    )
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_cutoff = Arc::new(Mutex::new(None));
    let observer_calls = calls.clone();
    let observer_cutoff = observed_cutoff.clone();
    let mut state = empty_startup_recovery_state_v2();
    state.acknowledged_product_handoff_count = 7;

    let outcome = session
        .observe_startup_recovery_with_test_observer_v2(move |authorization, cutoff| {
            observer_calls.fetch_add(1, Ordering::AcqRel);
            *observer_cutoff
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cutoff);
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    state,
                ),
            ))
        })
        .await
        .unwrap();

    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(
        fixed_point,
    ) = outcome
    else {
        panic!("expected startup fixed point")
    };
    assert_eq!(fixed_point.acknowledged_product_handoff_count_v2(), 7);
    assert_eq!(
        format!("{fixed_point:?}"),
        "RuntimeClosedRecoveryFixedPointV2(<redacted>)"
    );
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(
        *observed_cutoff
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        Some(operation_cutoff)
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id,
            authority_revision,
        } if generation.get() == 2
            && recovery_id.as_str() == "15151515151515151515151515151515"
            && authority_revision.get() == 3
    ));
    drop(fixed_point);
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn owner_safety_deadline_is_the_startup_observer_cutoff_minimum() {
    let operation_cutoff = Instant::now() + Duration::from_secs(10);
    let (gateway, _registry, session, port) = initial_committed_recovery_fixture_until_v2(
        "16161616161616161616161616161616",
        operation_cutoff,
    )
    .await;
    let observed_cutoff = Arc::new(Mutex::new(None));
    let observer_cutoff = observed_cutoff.clone();

    let outcome = session
        .observe_startup_recovery_with_test_observer_v2(move |authorization, cutoff| {
            *observer_cutoff
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cutoff);
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    empty_startup_recovery_state_v2(),
                ),
            ))
        })
        .await
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(
        fixed_point,
    ) = outcome
    else {
        panic!("expected startup fixed point")
    };

    let cutoff = observed_cutoff
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .unwrap();
    assert!(cutoff < operation_cutoff);
    assert!(cutoff > Instant::now());
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 3
    ));
    drop(fixed_point);
    wait_for(|| port.release_calls() == 1).await;
}

struct PendingDrainTestRunV2 {
    outcome: Result<(), crate::process::RuntimeProcessStartupRecoveryLoopFailureV2>,
    source: automation_runtime_worker::RuntimeRegistryRecoveryEmptyObservationV2,
    successor: Result<
        automation_runtime_worker::RuntimeRegistryRecoveryEmptyObservationV2,
        crate::RuntimeRegistryRecoveryObservationErrorV1,
    >,
    environment: FakePendingDrainRecoveryEnvironmentV2,
}

impl PendingDrainTestRunV2 {
    fn assert_success(&self) {
        assert!(self.outcome.is_ok());
    }

    fn assert_error(&self, expected: crate::process::RuntimeProcessStartupRecoveryLoopFailureV2) {
        assert_eq!(self.outcome, Err(expected));
    }

    fn assert_events(&self, expected: &[PendingDrainTestStageV2]) {
        assert_eq!(self.environment.events, expected);
    }

    fn assert_event_count(&self, stage: PendingDrainTestStageV2, expected: usize) {
        assert_eq!(
            self.environment
                .events
                .iter()
                .filter(|event| **event == stage)
                .count(),
            expected
        );
    }

    fn assert_exact_mutation_fingerprints(&self, stage: PendingDrainTestStageV2, expected: usize) {
        let fingerprints: Vec<_> = self
            .environment
            .mutation_fingerprints
            .iter()
            .filter(|fingerprint| fingerprint.stage() == stage)
            .collect();
        assert_eq!(fingerprints.len(), expected);
        assert!(fingerprints.windows(2).all(|pair| pair[0] == pair[1]));
    }

    fn assert_registry_sealed(&self, sealed: bool) {
        if sealed {
            assert_eq!(
                self.successor,
                Err(crate::RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
            );
        } else {
            assert!(self.successor.is_ok());
        }
    }

    fn assert_no_candidate_completed(&self) {
        self.assert_registry_sealed(false);
        assert_eq!(self.successor.as_ref().unwrap(), &self.source);
    }

    fn assert_candidate_completed(&self) {
        let successor = self.successor.as_ref().unwrap();
        assert_eq!(
            successor.observation_sequence().get(),
            self.source.observation_sequence().get() + 2
        );
        assert_eq!(successor.retained_slot_count(), 1);
        assert_eq!(successor.retained_empty_tombstone_count(), 1);
    }
}

async fn run_pending_drain_test_v2(
    recovery_id: &str,
    mut environment: FakePendingDrainRecoveryEnvironmentV2,
) -> PendingDrainTestRunV2 {
    let (gateway, registry, mut session, port) =
        initial_pending_drain_continue_fresh_registry_fixture_v2(recovery_id).await;
    let source = registry.observe_recovery_empty_projection_v2().unwrap();
    let outcome = crate::process::execute_pending_drain_recovery_with_environment_v2(
        &mut session,
        &mut environment,
    )
    .await
    .map(|_| ());
    let successor = registry.observe_recovery_empty_projection_v2();
    let gateway = gateway.closed_snapshot();
    if outcome.is_ok() {
        assert!(matches!(
            gateway,
            automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending { .. }
        ));
    } else {
        assert!(matches!(
            gateway,
            automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
                cause:
                    automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
                ..
            }
        ));
    }
    drop(session);
    wait_for(|| port.release_calls() == 1).await;
    PendingDrainTestRunV2 {
        outcome,
        source,
        successor,
        environment,
    }
}

fn pending_drain_finalizer_supervisor_v3() -> crate::RuntimeMutationFinalizerSupervisorV1<
    crate::process::RuntimePendingDrainFinalizerPortV3<FakePendingDrainRecoveryEnvironmentV2>,
> {
    crate::RuntimeMutationFinalizerSupervisorV1::start(
        crate::RuntimeMutationFinalizerConfigV1::new(1).unwrap(),
        automation_runtime_worker::RuntimeMutationFinalizerGenerationV1::new(
            NonZeroU64::new(9).unwrap(),
        )
        .unwrap(),
        crate::process::RuntimePendingDrainFinalizerPortV3::new(),
    )
    .unwrap()
}

async fn select_pending_drain_for_finalizer_v3(
    session: &mut crate::closed_recovery::RuntimeClosedRecoverySessionV2,
    environment: &mut FakePendingDrainRecoveryEnvironmentV2,
) -> automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV3 {
    let authorization = session
        .begin_startup_recovery_execution_v2(RuntimeStartupRecoveryContinuationV2::Recover(
            RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
        ))
        .unwrap()
        .into_pending_drain_selection_v3()
        .unwrap();
    let receipt =
        match crate::process::RuntimePendingDrainRecoveryEnvironmentV2::select_pending_drain_v3(
            environment,
            session,
            &authorization,
        )
        .await
        {
            Ok(receipt) => receipt,
            Err(_) => panic!("pending-drain selection"),
        };
    authorization.accept_selection(receipt).unwrap()
}

#[tokio::test]
async fn root_finalizer_owns_each_pending_drain_mutation_and_exactly_finalizes_once() {
    let (gateway, registry, mut session, port) =
        initial_pending_drain_continue_fresh_registry_fixture_v2(
            "d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1",
        )
        .await;
    let mut environment = FakePendingDrainRecoveryEnvironmentV2::failing_sequence([(
        PendingDrainTestStageV2::NoCandidate,
        PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate),
    )]);
    let selected = select_pending_drain_for_finalizer_v3(&mut session, &mut environment).await;
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV3::NoCandidate(selected) =
        selected
    else {
        panic!("no-candidate selection")
    };
    let job = crate::process::RuntimePendingDrainFinalizerJobV3::new(
        session,
        environment,
        crate::process::RuntimePendingDrainMutationStageV3::NoCandidate(selected),
    );
    let mut supervisor = pending_drain_finalizer_supervisor_v3();
    let settled = crate::process::register_and_complete_pending_drain_job_v3(&mut supervisor, job)
        .await
        .unwrap();
    let (session, environment, output) = settled.into_parts();
    assert!(matches!(
        output,
        crate::process::RuntimePendingDrainMutationOutputV3::Completed(_)
    ));
    assert_eq!(
        environment.events,
        [
            PendingDrainTestStageV2::Selection,
            PendingDrainTestStageV2::NoCandidate,
            PendingDrainTestStageV2::NoCandidate,
        ]
    );
    let fingerprints: Vec<_> = environment
        .mutation_fingerprints
        .iter()
        .filter(|fingerprint| fingerprint.stage() == PendingDrainTestStageV2::NoCandidate)
        .collect();
    assert_eq!(fingerprints.len(), 2);
    assert!(fingerprints[0] == fingerprints[1]);
    assert!(registry.observe_recovery_empty_projection_v2().is_ok());
    drop(session);
    drop(gateway);
    let report = supervisor.join().await;
    assert_eq!(report.exit(), crate::RuntimeSupervisorExitV1::Commanded);
    wait_for(|| port.release_calls() == 1).await;

    let (gateway, registry, mut session, port) =
        initial_pending_drain_continue_fresh_registry_fixture_v2(
            "d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2",
        )
        .await;
    let mut environment = FakePendingDrainRecoveryEnvironmentV2::failing_sequence([
        (
            PendingDrainTestStageV2::Claim,
            PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate),
        ),
        (
            PendingDrainTestStageV2::Acknowledgement,
            PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate),
        ),
    ]);
    let selected = select_pending_drain_for_finalizer_v3(&mut session, &mut environment).await;
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selected) =
        selected
    else {
        panic!("unclaimed selection")
    };
    let seal = session
        .seal_pending_drain_candidate_v2(selected.candidate())
        .unwrap();
    let claim = selected.bind_registry_seal(seal).unwrap();
    let mut supervisor = pending_drain_finalizer_supervisor_v3();
    let job = crate::process::RuntimePendingDrainFinalizerJobV3::new(
        session,
        environment,
        crate::process::RuntimePendingDrainMutationStageV3::Claim(Box::new(claim)),
    );
    let settled = crate::process::register_and_complete_pending_drain_job_v3(&mut supervisor, job)
        .await
        .unwrap();
    let (session, environment, output) = settled.into_parts();
    let crate::process::RuntimePendingDrainMutationOutputV3::ClaimAccepted(acknowledgement) =
        output
    else {
        panic!("claim acknowledgement")
    };
    let job = crate::process::RuntimePendingDrainFinalizerJobV3::new(
        session,
        environment,
        crate::process::RuntimePendingDrainMutationStageV3::Acknowledgement(Box::new(
            acknowledgement,
        )),
    );
    let settled = crate::process::register_and_complete_pending_drain_job_v3(&mut supervisor, job)
        .await
        .unwrap();
    let (session, environment, output) = settled.into_parts();
    assert!(matches!(
        output,
        crate::process::RuntimePendingDrainMutationOutputV3::Completed(_)
    ));
    assert_eq!(
        environment.events,
        [
            PendingDrainTestStageV2::Selection,
            PendingDrainTestStageV2::Claim,
            PendingDrainTestStageV2::Claim,
            PendingDrainTestStageV2::Acknowledgement,
            PendingDrainTestStageV2::Acknowledgement,
        ]
    );
    for stage in [
        PendingDrainTestStageV2::Claim,
        PendingDrainTestStageV2::Acknowledgement,
    ] {
        let fingerprints: Vec<_> = environment
            .mutation_fingerprints
            .iter()
            .filter(|fingerprint| fingerprint.stage() == stage)
            .collect();
        assert_eq!(fingerprints.len(), 2);
        assert!(fingerprints[0] == fingerprints[1]);
    }
    assert!(registry.observe_recovery_empty_projection_v2().is_ok());
    drop(session);
    drop(gateway);
    let report = supervisor.join().await;
    assert_eq!(report.exit(), crate::RuntimeSupervisorExitV1::Commanded);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn root_finalizer_owns_expired_serving_disconnect_and_one_indeterminate_reobservation() {
    for (recovery_id, indeterminate) in [
        ("d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4", false),
        ("d5d5d5d5d5d5d5d5d5d5d5d5d5d5d5d5", true),
    ] {
        let (gateway, registry, mut session, port) =
            initial_pending_drain_continue_fresh_registry_fixture_v2(recovery_id).await;
        let mut environment = FakePendingDrainRecoveryEnvironmentV2::candidate();
        let selected = select_pending_drain_for_finalizer_v3(&mut session, &mut environment).await;
        let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selected) =
            selected
        else {
            panic!("unclaimed selection")
        };
        let observed_at = environment.selection_database_now.unwrap() + TimeDelta::milliseconds(1);
        let source = pending_drain_test_serving_receipt_v1(
            selected.candidate().expected_target().clone(),
            7,
            observed_at,
            true,
        );
        environment.current_serving = Some(source.clone());
        if indeterminate {
            let mut disconnected = source.clone();
            disconnected.identity.revision = NonZeroU64::new(8).unwrap();
            disconnected.connected = false;
            disconnected.serving = false;
            disconnected.last_heartbeat_at = disconnected.expires_at;
            environment = environment
                .with_serving_disconnects([Err(RuntimeServingPersistenceErrorV1::Indeterminate)])
                .with_serving_observations([Ok(
                    RuntimePendingDrainServingObservationV1::Disconnected {
                        source: pending_drain_serving_source_evidence_v1(selected.candidate()),
                        serving: Box::new(disconnected),
                        observed_at: observed_at + TimeDelta::milliseconds(1),
                    },
                )]);
        }
        let source_evidence = pending_drain_serving_source_evidence_v1(selected.candidate());
        let stage = crate::process::pending_drain_serving_disconnect_stage_for_test_v1(
            selected,
            RuntimePendingDrainServingObservationV1::Expired {
                source: source_evidence,
                serving: Box::new(source),
                observed_at,
            },
        )
        .unwrap();
        let job =
            crate::process::RuntimePendingDrainFinalizerJobV3::new(session, environment, stage);
        let mut supervisor = pending_drain_finalizer_supervisor_v3();
        let settled =
            crate::process::register_and_complete_pending_drain_job_v3(&mut supervisor, job)
                .await
                .unwrap();
        let (session, environment, output) = settled.into_parts();
        assert!(matches!(
            output,
            crate::process::RuntimePendingDrainMutationOutputV3::ServingResolved(_)
        ));
        assert_eq!(environment.serving_disconnect_calls, 1);
        assert_eq!(
            environment.serving_observation_calls,
            usize::from(indeterminate)
        );
        assert!(registry.observe_recovery_empty_projection_v2().is_ok());
        drop(session);
        drop(gateway);
        let report = supervisor.join().await;
        assert_eq!(report.exit(), crate::RuntimeSupervisorExitV1::Commanded);
        wait_for(|| port.release_calls() == 1).await;
    }
}

#[tokio::test]
async fn root_finalizer_keeps_seal_on_second_uncertainty_and_returns_session_to_mailbox() {
    let (gateway, registry, mut session, port) =
        initial_pending_drain_continue_fresh_registry_fixture_v2(
            "d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3",
        )
        .await;
    let indeterminate =
        PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate);
    let mut environment = FakePendingDrainRecoveryEnvironmentV2::failing_sequence([
        (PendingDrainTestStageV2::Claim, indeterminate),
        (PendingDrainTestStageV2::Claim, indeterminate),
    ]);
    let selected = select_pending_drain_for_finalizer_v3(&mut session, &mut environment).await;
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV3::Unclaimed(selected) =
        selected
    else {
        panic!("unclaimed selection")
    };
    let seal = session
        .seal_pending_drain_candidate_v2(selected.candidate())
        .unwrap();
    let claim = selected.bind_registry_seal(seal).unwrap();
    let job = crate::process::RuntimePendingDrainFinalizerJobV3::new(
        session,
        environment,
        crate::process::RuntimePendingDrainMutationStageV3::Claim(Box::new(claim)),
    );
    let mut supervisor = pending_drain_finalizer_supervisor_v3();
    let failed = crate::process::register_and_complete_pending_drain_job_v3(&mut supervisor, job)
        .await
        .unwrap_err();
    let crate::process::RuntimePendingDrainFinalizerDispatchFailureV3::Failed(failed) = failed
    else {
        panic!("terminal mutation failure")
    };
    let (session, environment, failure) = (*failed).into_parts();
    assert_eq!(
        failure,
        crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecution(
            PendingDrainPersistenceErrorV1::Indeterminate,
        )
    );
    assert_eq!(
        environment.events,
        [
            PendingDrainTestStageV2::Selection,
            PendingDrainTestStageV2::Claim,
            PendingDrainTestStageV2::Claim,
        ]
    );
    assert_eq!(
        registry.observe_recovery_empty_projection_v2(),
        Err(crate::RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
    );
    session.invalidate_startup_recovery_execution_v2();
    drop(session);
    drop(gateway);
    let report = supervisor.join().await;
    assert_eq!(report.exit(), crate::RuntimeSupervisorExitV1::Commanded);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn root_finalizer_preserves_expired_owner_succession_identity() {
    let (gateway, registry, mut session, port) =
        initial_pending_drain_continue_fresh_registry_fixture_v2(
            "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4",
        )
        .await;
    let mut environment = FakePendingDrainRecoveryEnvironmentV2::failing_sequence([(
        PendingDrainTestStageV2::Succession,
        PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate),
    )]);
    let selected = select_pending_drain_for_finalizer_v3(&mut session, &mut environment).await;
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV3::ExpiredPreviousOwner(
        selected,
    ) = selected
    else {
        panic!("expired previous owner selection")
    };
    let seal = session
        .seal_pending_drain_succession_candidate_v3(selected.candidate())
        .unwrap();
    let succession = selected.bind_registry_seal(seal).unwrap();
    let job = crate::process::RuntimePendingDrainFinalizerJobV3::new(
        session,
        environment,
        crate::process::RuntimePendingDrainMutationStageV3::Succession(Box::new(succession)),
    );
    let mut supervisor = pending_drain_finalizer_supervisor_v3();
    let settled = crate::process::register_and_complete_pending_drain_job_v3(&mut supervisor, job)
        .await
        .unwrap();
    let (session, environment, output) = settled.into_parts();
    assert!(matches!(
        output,
        crate::process::RuntimePendingDrainMutationOutputV3::Completed(_)
    ));
    assert_eq!(
        environment.events,
        [
            PendingDrainTestStageV2::Selection,
            PendingDrainTestStageV2::Succession,
            PendingDrainTestStageV2::Succession,
        ]
    );
    let fingerprints: Vec<_> = environment
        .mutation_fingerprints
        .iter()
        .filter(|fingerprint| fingerprint.stage() == PendingDrainTestStageV2::Succession)
        .collect();
    assert_eq!(fingerprints.len(), 2);
    assert!(fingerprints[0] == fingerprints[1]);
    assert!(registry.observe_recovery_empty_projection_v2().is_ok());
    drop(session);
    drop(gateway);
    let report = supervisor.join().await;
    assert_eq!(report.exit(), crate::RuntimeSupervisorExitV1::Commanded);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn startup_recovery_continue_requires_a_fresh_ready_iteration_before_fixed_point() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("31313131313131313131313131313131").await;
    let mut stale = empty_startup_recovery_state_v2();
    stale.serving = RuntimeStartupServingStateV2::RecoverableStale { count: 1 };

    let outcome = session
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    stale,
                ),
            ))
        })
        .await
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
        session,
        continuation,
    } = outcome
    else {
        panic!("expected startup continuation")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(RuntimeStartupRecoveryClassV2::StaleLive)
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 3
    ));

    let (session, execution) = session
        .execute_startup_recovery_with_test_executor_v2(continuation, |authorization| {
            complete_startup_recovery_execution_v2(authorization, at_millis(1_000_150))
        })
        .unwrap();
    assert_eq!(execution.class(), RuntimeStartupRecoveryClassV2::StaleLive);
    assert!(matches!(
        execution.outcome(),
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed { .. }
    ));
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 4
    ));

    let ready_iteration = session
        .refresh_iteration_readiness_after_test_hook_v2(
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_at_for_test_v2(3_000_000),
            )),
            || {},
        )
        .await
        .unwrap();
    assert_eq!(
        format!("{ready_iteration:?}"),
        "RuntimeClosedRecoveryReadyIterationV2(<redacted>)"
    );
    let outcome = ready_iteration
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_200),
                    empty_startup_recovery_state_v2(),
                ),
            ))
        })
        .await
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(
        fixed_point,
    ) = outcome
    else {
        panic!("expected startup fixed point")
    };
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 6
    ));
    drop(fixed_point);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn production_pending_drain_driver_defers_fresh_previous_owner_without_sealing() {
    let run = run_pending_drain_test_v2(
        "c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1",
        FakePendingDrainRecoveryEnvironmentV2::fresh_previous_owner(),
    )
    .await;
    run.assert_success();
    run.assert_events(&[PendingDrainTestStageV2::Selection]);
    assert!(run.environment.mutation_fingerprints.is_empty());
    assert_eq!(run.successor.as_ref().unwrap(), &run.source);
}

#[tokio::test]
async fn production_pending_drain_driver_defers_fresh_unclaimed_source_without_mutation() {
    let candidate = pending_drain_candidate_v2();
    let observed_at = at_millis(1_000_102);
    let mut serving = pending_drain_test_serving_receipt_v1(
        candidate.expected_target().clone(),
        7,
        observed_at,
        true,
    );
    serving.expires_at = observed_at + TimeDelta::milliseconds(500);
    let run = run_pending_drain_test_v2(
        "cececececececececececececececece",
        FakePendingDrainRecoveryEnvironmentV2::candidate().with_serving_observations([Ok(
            RuntimePendingDrainServingObservationV1::Fresh {
                source: pending_drain_serving_source_evidence_v1(&candidate),
                serving: Box::new(serving),
                observed_at,
            },
        )]),
    )
    .await;
    run.assert_success();
    run.assert_events(&[PendingDrainTestStageV2::Selection]);
    assert_eq!(run.environment.serving_observation_calls, 1);
    assert_eq!(run.environment.serving_disconnect_calls, 0);
    assert!(run.environment.mutation_fingerprints.is_empty());
    assert_eq!(run.successor.as_ref().unwrap(), &run.source);
}

#[tokio::test]
async fn production_pending_drain_driver_disconnects_expired_source_before_s1() {
    let candidate = pending_drain_candidate_v2();
    let observed_at = at_millis(1_000_102);
    let serving = pending_drain_test_serving_receipt_v1(
        candidate.expected_target().clone(),
        7,
        observed_at,
        true,
    );
    let run = run_pending_drain_test_v2(
        "cfcfcfcfcfcfcfcfcfcfcfcfcfcfcfcf",
        FakePendingDrainRecoveryEnvironmentV2::candidate().with_serving_observations([Ok(
            RuntimePendingDrainServingObservationV1::Expired {
                source: pending_drain_serving_source_evidence_v1(&candidate),
                serving: Box::new(serving),
                observed_at,
            },
        )]),
    )
    .await;
    run.assert_success();
    run.assert_events(&[
        PendingDrainTestStageV2::Selection,
        PendingDrainTestStageV2::Claim,
        PendingDrainTestStageV2::Acknowledgement,
    ]);
    assert_eq!(run.environment.serving_observation_calls, 1);
    assert_eq!(run.environment.serving_disconnect_calls, 1);
    run.assert_candidate_completed();
}

#[tokio::test]
async fn production_pending_drain_driver_rejects_mismatched_determinate_disconnect_before_s1() {
    let candidate = pending_drain_candidate_v2();
    let observed_at = at_millis(1_000_102);
    let source = pending_drain_test_serving_receipt_v1(
        candidate.expected_target().clone(),
        7,
        observed_at,
        true,
    );
    let mut scope_mismatch = source.clone();
    scope_mismatch.identity.revision = NonZeroU64::new(8).unwrap();
    scope_mismatch.identity.scope.tenant_id = TenantId::parse("tenant:detached").unwrap();
    scope_mismatch.connected = false;
    scope_mismatch.serving = false;
    scope_mismatch.last_heartbeat_at = scope_mismatch.expires_at;
    let mut clock_regression = source.clone();
    clock_regression.identity.revision = NonZeroU64::new(8).unwrap();
    clock_regression.connected = false;
    clock_regression.serving = false;
    clock_regression.expires_at = source.expires_at - TimeDelta::milliseconds(1);
    clock_regression.last_heartbeat_at = clock_regression.expires_at;
    for (recovery_id, mismatched) in [
        ("c7c7c7c7c7c7c7c7c7c7c7c7c7c7c7c7", scope_mismatch),
        ("c6c6c6c6c6c6c6c6c6c6c6c6c6c6c6c6", clock_regression),
    ] {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::candidate()
                .with_serving_observations([Ok(RuntimePendingDrainServingObservationV1::Expired {
                    source: pending_drain_serving_source_evidence_v1(&candidate),
                    serving: Box::new(source.clone()),
                    observed_at,
                })])
                .with_serving_disconnects([Ok(mismatched)]),
        )
        .await;
        run.assert_error(
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainCompound,
        );
        run.assert_events(&[PendingDrainTestStageV2::Selection]);
        assert_eq!(run.environment.serving_observation_calls, 1);
        assert_eq!(run.environment.serving_disconnect_calls, 1);
        assert!(run.environment.mutation_fingerprints.is_empty());
        run.assert_registry_sealed(false);
    }
}

#[tokio::test]
async fn production_pending_drain_driver_reobserves_once_after_indeterminate_disconnect() {
    let candidate = pending_drain_candidate_v2();
    let observed_at = at_millis(1_000_102);
    let source = pending_drain_test_serving_receipt_v1(
        candidate.expected_target().clone(),
        7,
        observed_at,
        true,
    );
    let mut disconnected = source.clone();
    disconnected.identity.revision = NonZeroU64::new(8).unwrap();
    disconnected.connected = false;
    disconnected.serving = false;
    disconnected.last_heartbeat_at = disconnected.expires_at;
    let run = run_pending_drain_test_v2(
        "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        FakePendingDrainRecoveryEnvironmentV2::candidate()
            .with_serving_observations([
                Ok(RuntimePendingDrainServingObservationV1::Expired {
                    source: pending_drain_serving_source_evidence_v1(&candidate),
                    serving: Box::new(source),
                    observed_at,
                }),
                Ok(RuntimePendingDrainServingObservationV1::Disconnected {
                    source: pending_drain_serving_source_evidence_v1(&candidate),
                    serving: Box::new(disconnected),
                    observed_at: observed_at + TimeDelta::milliseconds(1),
                }),
            ])
            .with_serving_disconnects([Err(RuntimeServingPersistenceErrorV1::Indeterminate)]),
    )
    .await;
    run.assert_success();
    assert_eq!(run.environment.serving_observation_calls, 2);
    assert_eq!(run.environment.serving_disconnect_calls, 1);
    run.assert_exact_mutation_fingerprints(PendingDrainTestStageV2::Claim, 1);
    run.assert_candidate_completed();
}

#[tokio::test]
async fn production_pending_drain_driver_fails_closed_on_noncurrent_source_evidence() {
    for (recovery_id, observation) in [
        (
            "cacacacacacacacacacacacacacacaca",
            RuntimePendingDrainServingObservationV1::Absent {
                source: pending_drain_serving_source_evidence_v1(&pending_drain_candidate_v2()),
                observed_at: at_millis(1_000_102),
            },
        ),
        (
            "c9c9c9c9c9c9c9c9c9c9c9c9c9c9c9c9",
            RuntimePendingDrainServingObservationV1::Diverged {
                observed_at: at_millis(1_000_102),
            },
        ),
    ] {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::candidate()
                .with_serving_observations([Ok(observation)]),
        )
        .await;
        run.assert_error(
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainCompound,
        );
        assert_eq!(run.environment.serving_disconnect_calls, 0);
        assert!(run.environment.mutation_fingerprints.is_empty());
        run.assert_registry_sealed(false);
    }
}

#[tokio::test]
async fn production_pending_drain_driver_fails_closed_after_indeterminate_identity_drift() {
    let candidate = pending_drain_candidate_v2();
    let observed_at = at_millis(1_000_102);
    let source = pending_drain_test_serving_receipt_v1(
        candidate.expected_target().clone(),
        7,
        observed_at,
        true,
    );
    let run = run_pending_drain_test_v2(
        "c8c8c8c8c8c8c8c8c8c8c8c8c8c8c8c8",
        FakePendingDrainRecoveryEnvironmentV2::candidate()
            .with_serving_observations([
                Ok(RuntimePendingDrainServingObservationV1::Expired {
                    source: pending_drain_serving_source_evidence_v1(&candidate),
                    serving: Box::new(source),
                    observed_at,
                }),
                Ok(RuntimePendingDrainServingObservationV1::Diverged {
                    observed_at: observed_at + TimeDelta::milliseconds(1),
                }),
            ])
            .with_serving_disconnects([Err(RuntimeServingPersistenceErrorV1::Indeterminate)]),
    )
    .await;
    run.assert_error(
        crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainCompound,
    );
    assert_eq!(run.environment.serving_observation_calls, 2);
    assert_eq!(run.environment.serving_disconnect_calls, 1);
    assert!(run.environment.mutation_fingerprints.is_empty());
    run.assert_registry_sealed(false);
}

#[tokio::test]
async fn production_pending_drain_driver_rolls_expired_previous_owner_s0_s1_s2() {
    let run = run_pending_drain_test_v2(
        "c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2",
        FakePendingDrainRecoveryEnvironmentV2::expired_previous_owner(),
    )
    .await;
    run.assert_success();
    run.assert_events(&[
        PendingDrainTestStageV2::Selection,
        PendingDrainTestStageV2::Succession,
    ]);
    run.assert_exact_mutation_fingerprints(PendingDrainTestStageV2::Succession, 1);
    run.assert_candidate_completed();
}

#[tokio::test]
async fn production_pending_drain_driver_finalizes_succession_indeterminate_once() {
    let run = run_pending_drain_test_v2(
        "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3",
        FakePendingDrainRecoveryEnvironmentV2::failing_sequence([(
            PendingDrainTestStageV2::Succession,
            PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate),
        )]),
    )
    .await;
    run.assert_success();
    run.assert_events(&[
        PendingDrainTestStageV2::Selection,
        PendingDrainTestStageV2::Succession,
        PendingDrainTestStageV2::Succession,
    ]);
    run.assert_exact_mutation_fingerprints(PendingDrainTestStageV2::Succession, 2);
    run.assert_candidate_completed();
}

#[tokio::test]
async fn production_pending_drain_driver_keeps_succession_s1_on_terminal_failures() {
    for (recovery_id, failure) in [
        (
            "c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4",
            PendingDrainPersistenceErrorV1::Timeout,
        ),
        (
            "c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5c5",
            PendingDrainPersistenceErrorV1::OwnershipLost,
        ),
        (
            "c6c6c6c6c6c6c6c6c6c6c6c6c6c6c6c6",
            PendingDrainPersistenceErrorV1::PersistenceCorrupt,
        ),
    ] {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::failing(
                PendingDrainTestStageV2::Succession,
                PendingDrainTestAwaitFailureV2::Database(failure),
            ),
        )
        .await;
        run.assert_error(
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecution(
                failure,
            ),
        );
        run.assert_event_count(PendingDrainTestStageV2::Succession, 1);
        run.assert_registry_sealed(true);
    }
}

#[tokio::test]
async fn production_pending_drain_driver_keeps_succession_s1_on_invalid_durable_receipt() {
    let run = run_pending_drain_test_v2(
        "cbcbcbcbcbcbcbcbcbcbcbcbcbcbcbcb",
        FakePendingDrainRecoveryEnvironmentV2::expired_previous_owner()
            .with_succession_revision_delta(2),
    )
    .await;
    run.assert_error(
        crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainCompound,
    );
    run.assert_event_count(PendingDrainTestStageV2::Succession, 1);
    run.assert_registry_sealed(true);
}

#[tokio::test]
async fn production_pending_drain_driver_stops_after_second_succession_indeterminate() {
    let indeterminate =
        PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate);
    let run = run_pending_drain_test_v2(
        "c7c7c7c7c7c7c7c7c7c7c7c7c7c7c7c7",
        FakePendingDrainRecoveryEnvironmentV2::failing_sequence([
            (PendingDrainTestStageV2::Succession, indeterminate),
            (PendingDrainTestStageV2::Succession, indeterminate),
        ]),
    )
    .await;
    run.assert_error(
        crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecution(
            PendingDrainPersistenceErrorV1::Indeterminate,
        ),
    );
    run.assert_event_count(PendingDrainTestStageV2::Succession, 2);
    run.assert_exact_mutation_fingerprints(PendingDrainTestStageV2::Succession, 2);
    run.assert_registry_sealed(true);
}

#[tokio::test]
async fn production_pending_drain_driver_prefers_transition_before_succession_replay() {
    for (recovery_id, transition) in [
        (
            "c8c8c8c8c8c8c8c8c8c8c8c8c8c8c8c8",
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed,
        ),
        (
            "c9c9c9c9c9c9c9c9c9c9c9c9c9c9c9c9",
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                crate::RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            ),
        ),
        (
            "cacacacacacacacacacacacacacacaca",
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                crate::RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
        ),
    ] {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::failing(
                PendingDrainTestStageV2::Succession,
                PendingDrainTestAwaitFailureV2::Database(
                    PendingDrainPersistenceErrorV1::Indeterminate,
                ),
            )
            .with_transition_after(PendingDrainTestStageV2::Succession, 1, transition),
        )
        .await;
        run.assert_error(transition);
        run.assert_event_count(PendingDrainTestStageV2::Succession, 1);
        run.assert_registry_sealed(true);
    }
}

#[tokio::test]
async fn production_pending_drain_driver_rolls_fresh_slot_s0_s1_s2() {
    let run = run_pending_drain_test_v2(
        "36363636363636363636363636363636",
        FakePendingDrainRecoveryEnvironmentV2::candidate(),
    )
    .await;
    run.assert_success();
    assert_eq!(run.source.retained_slot_count(), 0);
    assert_eq!(run.source.retained_empty_tombstone_count(), 0);
    run.assert_events(&[
        PendingDrainTestStageV2::Selection,
        PendingDrainTestStageV2::Claim,
        PendingDrainTestStageV2::Acknowledgement,
    ]);
    run.assert_candidate_completed();
}

#[tokio::test]
async fn production_pending_drain_driver_records_no_candidate_without_sealing() {
    let run = run_pending_drain_test_v2(
        "35353535353535353535353535353535",
        FakePendingDrainRecoveryEnvironmentV2::no_candidate(),
    )
    .await;
    run.assert_success();
    run.assert_events(&[
        PendingDrainTestStageV2::Selection,
        PendingDrainTestStageV2::NoCandidate,
    ]);
    run.assert_no_candidate_completed();
}

#[tokio::test]
async fn production_pending_drain_driver_finalizes_each_indeterminate_mutation_exactly_once() {
    let indeterminate =
        PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate);
    for (recovery_id, failures, events, no_candidate) in [
        (
            "29292929292929292929292929292929",
            vec![(PendingDrainTestStageV2::NoCandidate, indeterminate)],
            vec![
                PendingDrainTestStageV2::Selection,
                PendingDrainTestStageV2::NoCandidate,
                PendingDrainTestStageV2::NoCandidate,
            ],
            true,
        ),
        (
            "28282828282828282828282828282828",
            vec![(PendingDrainTestStageV2::Claim, indeterminate)],
            vec![
                PendingDrainTestStageV2::Selection,
                PendingDrainTestStageV2::Claim,
                PendingDrainTestStageV2::Claim,
                PendingDrainTestStageV2::Acknowledgement,
            ],
            false,
        ),
        (
            "27272727272727272727272727272727",
            vec![(PendingDrainTestStageV2::Acknowledgement, indeterminate)],
            vec![
                PendingDrainTestStageV2::Selection,
                PendingDrainTestStageV2::Claim,
                PendingDrainTestStageV2::Acknowledgement,
                PendingDrainTestStageV2::Acknowledgement,
            ],
            false,
        ),
        (
            "26262626262626262626262626262626",
            vec![
                (PendingDrainTestStageV2::Claim, indeterminate),
                (PendingDrainTestStageV2::Acknowledgement, indeterminate),
            ],
            vec![
                PendingDrainTestStageV2::Selection,
                PendingDrainTestStageV2::Claim,
                PendingDrainTestStageV2::Claim,
                PendingDrainTestStageV2::Acknowledgement,
                PendingDrainTestStageV2::Acknowledgement,
            ],
            false,
        ),
    ] {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::failing_sequence(failures),
        )
        .await;
        run.assert_success();
        run.assert_events(&events);
        for stage in [
            PendingDrainTestStageV2::NoCandidate,
            PendingDrainTestStageV2::Claim,
            PendingDrainTestStageV2::Acknowledgement,
        ] {
            let expected = events.iter().filter(|event| **event == stage).count();
            if expected > 0 {
                run.assert_exact_mutation_fingerprints(stage, expected);
            }
        }
        if no_candidate {
            run.assert_no_candidate_completed();
        } else {
            run.assert_candidate_completed();
        }
    }
}

#[tokio::test]
async fn production_pending_drain_driver_prefers_transition_around_mutation_finalization() {
    for (recovery_id, transition) in [
        (
            "abababababababababababababababab",
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed,
        ),
        (
            "acacacacacacacacacacacacacacacac",
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                crate::RuntimeProcessPausedConnectedTransitionFailureV1::DiscordTerminated,
            ),
        ),
        (
            "adadadadadadadadadadadadadadadad",
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PausedConnection(
                crate::RuntimeProcessPausedConnectedTransitionFailureV1::GatewayOwnerTerminated,
            ),
        ),
        (
            "aeaeaeaeaeaeaeaeaeaeaeaeaeaeaeae",
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::ProtocolViolation,
        ),
    ] {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::failing_sequence([(
                PendingDrainTestStageV2::Claim,
                PendingDrainTestAwaitFailureV2::Database(
                    PendingDrainPersistenceErrorV1::Indeterminate,
                ),
            )])
            .with_transition_after(PendingDrainTestStageV2::Claim, 1, transition),
        )
        .await;
        run.assert_error(transition);
        run.assert_event_count(PendingDrainTestStageV2::Claim, 1);
        assert!(!run
            .environment
            .events
            .contains(&PendingDrainTestStageV2::Acknowledgement));
        run.assert_exact_mutation_fingerprints(PendingDrainTestStageV2::Claim, 1);
        run.assert_registry_sealed(true);
    }

    let transition =
        crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed;
    for (recovery_id, stage, leaves_sealed) in [
        (
            "afafafafafafafafafafafafafafafaf",
            PendingDrainTestStageV2::NoCandidate,
            false,
        ),
        (
            "b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0",
            PendingDrainTestStageV2::Claim,
            true,
        ),
        (
            "b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1",
            PendingDrainTestStageV2::Acknowledgement,
            true,
        ),
    ] {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::failing_sequence([(
                stage,
                PendingDrainTestAwaitFailureV2::Database(
                    PendingDrainPersistenceErrorV1::Indeterminate,
                ),
            )])
            .with_transition_after(stage, 2, transition),
        )
        .await;
        run.assert_error(transition);
        run.assert_event_count(stage, 2);
        run.assert_registry_sealed(leaves_sealed);
    }

    let run = run_pending_drain_test_v2(
        "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2",
        FakePendingDrainRecoveryEnvironmentV2::failing_sequence([(
            PendingDrainTestStageV2::Claim,
            PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate),
        )])
        .with_session_invalidation_after(PendingDrainTestStageV2::Claim, 1),
    )
    .await;
    run.assert_error(
        crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecutionRejected(
            crate::RuntimeProcessClosedRecoveryCommitFailureV2::GatewayObservation(
                crate::RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain,
            ),
        ),
    );
    run.assert_event_count(PendingDrainTestStageV2::Claim, 1);
    run.assert_registry_sealed(true);
}

#[tokio::test]
async fn production_pending_drain_driver_stops_after_two_mutation_failures() {
    let indeterminate =
        PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Indeterminate);
    let cases = [
        (
            "25252525252525252525252525252525",
            PendingDrainTestStageV2::NoCandidate,
            indeterminate,
            false,
        ),
        (
            "24242424242424242424242424242424",
            PendingDrainTestStageV2::Claim,
            indeterminate,
            true,
        ),
        (
            "23232323232323232323232323232323",
            PendingDrainTestStageV2::Acknowledgement,
            indeterminate,
            true,
        ),
        (
            "22222222222222222222222222222222",
            PendingDrainTestStageV2::Claim,
            PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::Timeout),
            true,
        ),
        (
            "21212121212121212121212121212121",
            PendingDrainTestStageV2::Acknowledgement,
            PendingDrainTestAwaitFailureV2::Database(PendingDrainPersistenceErrorV1::OwnershipLost),
            true,
        ),
        (
            "20202020202020202020202020202020",
            PendingDrainTestStageV2::NoCandidate,
            PendingDrainTestAwaitFailureV2::Database(
                PendingDrainPersistenceErrorV1::PersistenceCorrupt,
            ),
            false,
        ),
    ];

    for (recovery_id, stage, terminal_failure, leaves_sealed) in cases {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::failing_sequence([
                (stage, indeterminate),
                (stage, terminal_failure),
            ]),
        )
        .await;
        let PendingDrainTestAwaitFailureV2::Database(expected) = terminal_failure else {
            unreachable!()
        };
        run.assert_error(
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecution(
                expected,
            ),
        );
        run.assert_event_count(stage, 2);
        run.assert_registry_sealed(leaves_sealed);
    }
}

#[tokio::test]
async fn production_pending_drain_driver_never_finalizes_selection_or_terminal_mutation_failure() {
    let cases = [
        (
            "19191919191919191919191919191919",
            PendingDrainTestStageV2::Selection,
            PendingDrainPersistenceErrorV1::Indeterminate,
            false,
        ),
        (
            "18181818181818181818181818181818",
            PendingDrainTestStageV2::NoCandidate,
            PendingDrainPersistenceErrorV1::Timeout,
            false,
        ),
        (
            "17171717171717171717171717171717",
            PendingDrainTestStageV2::Claim,
            PendingDrainPersistenceErrorV1::OwnershipLost,
            true,
        ),
        (
            "16161616161616161616161616161616",
            PendingDrainTestStageV2::Acknowledgement,
            PendingDrainPersistenceErrorV1::PersistenceCorrupt,
            true,
        ),
    ];

    for (recovery_id, stage, failure, leaves_sealed) in cases {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::failing(
                stage,
                PendingDrainTestAwaitFailureV2::Database(failure),
            ),
        )
        .await;
        run.assert_error(
            crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecution(
                failure,
            ),
        );
        run.assert_event_count(stage, 1);
        run.assert_registry_sealed(leaves_sealed);
    }
}

#[tokio::test]
async fn production_pending_drain_driver_invalidates_and_preserves_seal_on_every_await_failure() {
    let cases = [
        (
            "34343434343434343434343434343434",
            PendingDrainTestStageV2::Selection,
            PendingDrainTestAwaitFailureV2::Database(
                PendingDrainPersistenceErrorV1::Timeout,
            ),
            false,
        ),
        (
            "33333333333333333333333333333333",
            PendingDrainTestStageV2::NoCandidate,
            PendingDrainTestAwaitFailureV2::Database(
                PendingDrainPersistenceErrorV1::Unavailable,
            ),
            false,
        ),
        (
            "32323232323232323232323232323232",
            PendingDrainTestStageV2::Claim,
            PendingDrainTestAwaitFailureV2::Database(
                PendingDrainPersistenceErrorV1::Timeout,
            ),
            true,
        ),
        (
            "31313131313131313131313131313131",
            PendingDrainTestStageV2::Acknowledgement,
            PendingDrainTestAwaitFailureV2::Database(
                PendingDrainPersistenceErrorV1::Concurrency,
            ),
            true,
        ),
        (
            "30303030303030303030303030303030",
            PendingDrainTestStageV2::Claim,
            PendingDrainTestAwaitFailureV2::Transition(
                crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::OperationDeadlineElapsed,
            ),
            true,
        ),
    ];

    for (recovery_id, stage, failure, leaves_sealed) in cases {
        let run = run_pending_drain_test_v2(
            recovery_id,
            FakePendingDrainRecoveryEnvironmentV2::failing(stage, failure),
        )
        .await;
        match failure {
            PendingDrainTestAwaitFailureV2::Transition(expected) => {
                run.assert_error(expected);
            }
            PendingDrainTestAwaitFailureV2::Database(expected) => {
                run.assert_error(
                    crate::process::RuntimeProcessStartupRecoveryLoopFailureV2::PendingRuntimeDrainExecution(
                        expected,
                    ),
                );
            }
        }
        assert_eq!(run.environment.events.last(), Some(&stage));
        run.assert_registry_sealed(leaves_sealed);
    }
}

#[tokio::test]
async fn pending_drain_compound_rolls_tombstone_s0_s1_s2_then_reobserves() {
    let (gateway, registry, session, port) =
        initial_committed_recovery_fixture_v2("39393939393939393939393939393939").await;
    let mut pending = empty_startup_recovery_state_v2();
    pending.pending_runtime_drain_intent_count = 1;
    let outcome = session
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    pending,
                ),
            ))
        })
        .await
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
        mut session,
        continuation,
    } = outcome
    else {
        panic!("expected pending drain continuation")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(
            RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
        )
    );
    let source = registry.observe_recovery_empty_projection_v2().unwrap();
    let selection = session
        .begin_startup_recovery_execution_v2(continuation)
        .unwrap()
        .into_pending_drain_selection()
        .unwrap();
    let selection_receipt = RuntimePendingDrainSelectionReceiptV2::new(
        selection.request().correlation().clone(),
        pending_drain_owner_receipt_v2(selection.request(), at_millis(1_000_150)),
        RuntimePendingDrainSelectionOutcomeV2::Candidate(pending_drain_candidate_v2()),
    );
    let RuntimeAcceptedPendingDrainSelectionV2::Candidate(selected) =
        selection.accept_selection(selection_receipt).unwrap()
    else {
        panic!("expected candidate")
    };
    let seal = session
        .seal_pending_drain_candidate_v2(selected.candidate())
        .unwrap();
    assert_eq!(
        seal.pre_slot_observation()
            .unwrap()
            .admission_generation
            .get(),
        2
    );
    assert_eq!(
        registry.observe_recovery_empty_projection_v2(),
        Err(crate::RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
    );
    let claim = selected.bind_registry_seal(seal).unwrap();
    let claim_receipt = RuntimePendingDrainClaimReceiptV2::new(
        claim.action_identity().clone(),
        claim.candidate().clone(),
        claim.seal().clone(),
        NonZeroU64::new(2).unwrap(),
        RuntimePendingDrainStateDigestV2::new([42; 32]).unwrap(),
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([43; 32]).unwrap(),
        pending_drain_owner_receipt_v2(claim.request(), at_millis(1_000_160)),
    );
    let acknowledgement = claim.complete(claim_receipt).unwrap();
    let acknowledgement_receipt = RuntimePendingDrainAcknowledgementReceiptV2::new(
        acknowledgement.action_identity().clone(),
        acknowledgement.request().action_identity().clone(),
        acknowledgement.candidate().clone(),
        acknowledgement.seal().clone(),
        acknowledgement.claimed_intent_revision(),
        acknowledgement.claimed_state_digest().clone(),
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([43; 32]).unwrap(),
        NonZeroU64::new(3).unwrap(),
        RuntimePendingDrainStateDigestV2::new([44; 32]).unwrap(),
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([45; 32]).unwrap(),
        pending_drain_owner_receipt_v2(acknowledgement.request(), at_millis(1_000_170)),
    );
    let durable = acknowledgement.complete(acknowledgement_receipt).unwrap();
    let unseal = session
        .unseal_pending_drain_after_durable_ack_v2(&durable)
        .unwrap();
    let completed = durable.complete_registry_rollover(unseal).unwrap();
    let accepted = session
        .complete_startup_recovery_execution_v2(completed)
        .unwrap();
    assert!(matches!(
        accepted.outcome(),
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed { .. }
    ));
    let successor = registry.observe_recovery_empty_projection_v2().unwrap();
    assert_eq!(
        successor.observation_sequence().get(),
        source.observation_sequence().get() + 2
    );
    assert_eq!(successor.retained_slot_count(), 1);
    assert_eq!(successor.retained_empty_tombstone_count(), 1);

    let ready_iteration = session
        .refresh_iteration_readiness_after_test_hook_v2(
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_at_for_test_v2(3_000_000),
            )),
            || {},
        )
        .await
        .unwrap();
    let outcome = ready_iteration
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_200),
                    empty_startup_recovery_state_v2(),
                ),
            ))
        })
        .await
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::FixedPoint(
        fixed_point,
    ) = outcome
    else {
        panic!("expected startup fixed point")
    };
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 6
    ));
    drop(fixed_point);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn pending_drain_no_candidate_records_without_registry_seal() {
    let (_gateway, registry, session, port) =
        initial_committed_recovery_fixture_v2("38383838383838383838383838383838").await;
    let mut pending = empty_startup_recovery_state_v2();
    pending.pending_runtime_drain_intent_count = 1;
    let outcome = session
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    pending,
                ),
            ))
        })
        .await
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
        mut session,
        continuation,
    } = outcome
    else {
        panic!("expected pending drain continuation")
    };
    let source = registry.observe_recovery_empty_projection_v2().unwrap();
    let selection = session
        .begin_startup_recovery_execution_v2(continuation)
        .unwrap()
        .into_pending_drain_selection()
        .unwrap();
    let selection_receipt = RuntimePendingDrainSelectionReceiptV2::new(
        selection.request().correlation().clone(),
        pending_drain_owner_receipt_v2(selection.request(), at_millis(1_000_150)),
        RuntimePendingDrainSelectionOutcomeV2::NoCandidate,
    );
    let RuntimeAcceptedPendingDrainSelectionV2::NoCandidate(selected) =
        selection.accept_selection(selection_receipt).unwrap()
    else {
        panic!("expected no candidate")
    };
    let receipt = RuntimePendingDrainNoCandidateReceiptV2::new(
        selected.request().action_identity().clone(),
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([46; 32]).unwrap(),
        pending_drain_owner_receipt_v2(selected.request(), at_millis(1_000_160)),
    );
    let completed = selected.complete(receipt).unwrap();
    let accepted = session
        .complete_startup_recovery_execution_v2(completed)
        .unwrap();

    assert!(matches!(
        accepted.outcome(),
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate
    ));
    assert_eq!(
        registry.observe_recovery_empty_projection_v2().unwrap(),
        source
    );
    drop(session);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn pending_drain_transition_failure_after_seal_leaves_registry_sealed() {
    let (_gateway, registry, session, port) =
        initial_committed_recovery_fixture_v2("37373737373737373737373737373737").await;
    let mut pending = empty_startup_recovery_state_v2();
    pending.pending_runtime_drain_intent_count = 1;
    let outcome = session
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    pending,
                ),
            ))
        })
        .await
        .unwrap();
    let crate::closed_recovery::RuntimeClosedRecoveryStartupIterationOutcomeV2::Continue {
        mut session,
        continuation,
    } = outcome
    else {
        panic!("expected pending drain continuation")
    };
    let selection = session
        .begin_startup_recovery_execution_v2(continuation)
        .unwrap()
        .into_pending_drain_selection()
        .unwrap();
    let receipt = RuntimePendingDrainSelectionReceiptV2::new(
        selection.request().correlation().clone(),
        pending_drain_owner_receipt_v2(selection.request(), at_millis(1_000_150)),
        RuntimePendingDrainSelectionOutcomeV2::Candidate(pending_drain_candidate_v2()),
    );
    let RuntimeAcceptedPendingDrainSelectionV2::Candidate(selected) =
        selection.accept_selection(receipt).unwrap()
    else {
        panic!("expected candidate")
    };
    let seal = session
        .seal_pending_drain_candidate_v2(selected.candidate())
        .unwrap();
    let _claim = selected.bind_registry_seal(seal).unwrap();

    session.invalidate_startup_recovery_execution_v2();
    drop(session);

    assert_eq!(
        registry.observe_recovery_empty_projection_v2(),
        Err(crate::RuntimeRegistryRecoveryObservationErrorV1::NotEmpty)
    );
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn startup_observer_failure_invalidates_capability_authority_once() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("17171717171717171717171717171717").await;
    let calls = Arc::new(AtomicUsize::new(0));
    let observer_calls = calls.clone();

    let error = session
        .observe_startup_recovery_with_test_observer_v2(move |_authorization, _cutoff| {
            observer_calls.fetch_add(1, Ordering::AcqRel);
            ready(Err::<RuntimeCompletedStartupRecoveryObservationV2, _>(
                FakeErrorV1::Retryable,
            ))
        })
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::Observer(
            FakeErrorV1::Retryable,
        )
    );
    assert_eq!(
        format!("{error:?}"),
        "RuntimeClosedRecoveryStartupObservationErrorV2(<redacted>)"
    );
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn elapsed_startup_observation_cutoff_never_polls_the_observer() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("18181818181818181818181818181818").await;
    let session = session.with_operation_cutoff_for_test_v2(
        Instant::now().checked_sub(Duration::from_nanos(1)).unwrap(),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let observer_calls = calls.clone();

    let error = session
        .observe_startup_recovery_with_test_observer_v2(move |authorization, _cutoff| {
            observer_calls.fetch_add(1, Ordering::AcqRel);
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    empty_startup_recovery_state_v2(),
                ),
            ))
        })
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::<
            FakeErrorV1,
        >::DeadlineElapsed
    );
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn late_startup_observation_success_cannot_advance_authority() {
    let operation_cutoff = Instant::now() + Duration::from_millis(50);
    let (gateway, _registry, session, port) = initial_committed_recovery_fixture_until_v2(
        "19191919191919191919191919191919",
        operation_cutoff,
    )
    .await;

    let error = session
        .observe_startup_recovery_with_test_observer_v2(|authorization, cutoff| async move {
            tokio::time::sleep_until(tokio::time::Instant::from_std(
                cutoff + Duration::from_millis(10),
            ))
            .await;
            Ok::<_, FakeErrorV1>(complete_startup_recovery_observation_v2(
                authorization,
                at_millis(1_000_100),
                empty_startup_recovery_state_v2(),
            ))
        })
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::<
            FakeErrorV1,
        >::DeadlineElapsed
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            ..
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn late_startup_observation_failure_is_classified_as_deadline_elapsed() {
    let operation_cutoff = Instant::now() + Duration::from_millis(50);
    let (gateway, _registry, session, port) = initial_committed_recovery_fixture_until_v2(
        "25252525252525252525252525252525",
        operation_cutoff,
    )
    .await;

    let error = session
        .observe_startup_recovery_with_test_observer_v2(|_authorization, cutoff| async move {
            tokio::time::sleep_until(tokio::time::Instant::from_std(
                cutoff + Duration::from_millis(10),
            ))
            .await;
            Err::<RuntimeCompletedStartupRecoveryObservationV2, _>(FakeErrorV1::Retryable)
        })
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::<
            FakeErrorV1,
        >::DeadlineElapsed
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            ..
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test(flavor = "current_thread")]
async fn startup_observation_successor_cannot_escape_after_cutoff() {
    let operation_cutoff = Instant::now() + Duration::from_millis(50);
    let (gateway, _registry, session, port) = initial_committed_recovery_fixture_until_v2(
        "20202020202020202020202020202020",
        operation_cutoff,
    )
    .await;

    let error = session
        .observe_startup_recovery_after_test_hook_v2(
            |authorization, _cutoff| {
                ready(Ok::<_, FakeErrorV1>(
                    complete_startup_recovery_observation_v2(
                        authorization,
                        at_millis(1_000_100),
                        empty_startup_recovery_state_v2(),
                    ),
                ))
            },
            || {
                std::thread::sleep(
                    operation_cutoff.saturating_duration_since(Instant::now())
                        + Duration::from_millis(1),
                );
            },
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::<
            FakeErrorV1,
        >::DeadlineElapsed
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            ..
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test(flavor = "current_thread")]
async fn startup_observation_cannot_escape_when_final_revalidation_crosses_cutoff() {
    let operation_cutoff = Instant::now() + Duration::from_millis(50);
    let (gateway, _registry, session, port) = initial_committed_recovery_fixture_until_v2(
        "28282828282828282828282828282828",
        operation_cutoff,
    )
    .await;

    let error = session
        .observe_startup_recovery_after_final_revalidation_test_hook_v2(
            |authorization, _cutoff| {
                ready(Ok::<_, FakeErrorV1>(
                    complete_startup_recovery_observation_v2(
                        authorization,
                        at_millis(1_000_100),
                        empty_startup_recovery_state_v2(),
                    ),
                ))
            },
            || {
                std::thread::sleep(
                    operation_cutoff.saturating_duration_since(Instant::now())
                        + Duration::from_millis(1),
                );
            },
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::<
            FakeErrorV1,
        >::DeadlineElapsed
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            ..
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn unpolled_and_pending_startup_observations_cancel_closed() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("21212121212121212121212121212121").await;
    let observation =
        session.observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    empty_startup_recovery_state_v2(),
                ),
            ))
        });
    drop(observation);
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            ..
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;

    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("22222222222222222222222222222222").await;
    let mut observation =
        Box::pin(session.observe_startup_recovery_with_test_observer_v2(
            |_authorization, _cutoff| {
                std::future::pending::<
                    Result<RuntimeCompletedStartupRecoveryObservationV2, FakeErrorV1>,
                >()
            },
        ));
    std::future::poll_fn(|context| {
        assert!(observation.as_mut().poll(context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(observation);
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            ..
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn registry_aba_around_startup_observation_refuses_both_predecessor_and_successor() {
    let (gateway, registry, session, port) =
        initial_committed_recovery_fixture_v2("23232323232323232323232323232323").await;
    let error = session
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            registry.advance_empty_sequence_for_test_v2();
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    empty_startup_recovery_state_v2(),
                ),
            ))
        })
        .await
        .unwrap_err();
    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::Registry(
            crate::RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
    ));
    wait_for(|| port.release_calls() == 1).await;

    let (gateway, registry, session, port) =
        initial_committed_recovery_fixture_v2("24242424242424242424242424242424").await;
    let error = session
        .observe_startup_recovery_after_test_hook_v2(
            |authorization, _cutoff| {
                ready(Ok::<_, FakeErrorV1>(
                    complete_startup_recovery_observation_v2(
                        authorization,
                        at_millis(1_000_100),
                        empty_startup_recovery_state_v2(),
                    ),
                ))
            },
            || registry.advance_empty_sequence_for_test_v2(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::Registry(
            crate::RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn disconnect_during_startup_observation_prevents_authority_advance() {
    let (mut gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("26262626262626262626262626262626").await;

    let error = session
        .observe_startup_recovery_with_test_observer_v2(|authorization, _cutoff| {
            gateway.disconnect_for_gateway_section_test_v2();
            ready(Ok::<_, FakeErrorV1>(
                complete_startup_recovery_observation_v2(
                    authorization,
                    at_millis(1_000_100),
                    empty_startup_recovery_state_v2(),
                ),
            ))
        })
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(
            crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Gateway(
                crate::RuntimeGatewayReadyObservationErrorV1::Stopped,
            ),
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn disconnect_after_startup_observation_refuses_the_successor_session() {
    let (mut gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("27272727272727272727272727272727").await;

    let error = session
        .observe_startup_recovery_after_test_hook_v2(
            |authorization, _cutoff| {
                ready(Ok::<_, FakeErrorV1>(
                    complete_startup_recovery_observation_v2(
                        authorization,
                        at_millis(1_000_100),
                        empty_startup_recovery_state_v2(),
                    ),
                ))
            },
            || gateway.disconnect_for_gateway_section_test_v2(),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryStartupObservationErrorV2::Gateway(
            crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Gateway(
                crate::RuntimeGatewayReadyObservationErrorV1::Stopped,
            ),
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
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
    let owner_paused_gateway = owner_gateway.observe_paused_connected_gateway_v2().unwrap();

    assert!(matches!(
        foreign_gateway.initial_emergency_gateway_section_v2(&prepared, &owner_paused_gateway),
        Err(crate::RuntimeGatewayReadyObservationErrorV1::OwnershipUncertain)
    ));
    owner_gateway
        .held_initial_section_blocks_repeated_pause_test_v2(&prepared)
        .await
        .unwrap();
    let owner_paused_gateway = owner_gateway.observe_paused_connected_gateway_v2().unwrap();

    let registry = crate::compose_runtime_registry_bootstrap_v1(
        ProcessInstanceId::parse("process:handoff").unwrap(),
        crate::GatewayResourceConfigV1::default(),
    )
    .unwrap();
    let readiness = crate::database::runtime_database_readiness_for_test_v1();
    let mut pending = crate::closed_recovery::begin_initial_empty_recovery_v2(
        &mut owner_gateway,
        &registry,
        prepared,
        RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
        &readiness,
        &owner_paused_gateway,
        Instant::now() + Duration::from_secs(4),
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
    pending.revalidate_v2().unwrap();
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
    let session = pending.commit_owner_v2().await.unwrap();
    assert_eq!(
        format!("{session:?}"),
        "RuntimeClosedRecoverySessionV2(<redacted>)"
    );
    assert!(matches!(
        owner_gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id,
            ..
        } if generation.get() == 4
            && recovery_id.as_str() == "fedcba9876543210fedcba9876543210"
    ));
    assert_eq!(port.observe_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
    assert_eq!(port.acquire_calls(), 0);
    assert_eq!(port.release_calls(), 0);
    assert_eq!(port.maximum_active_operations(), 1);
    drop(session);
    assert!(matches!(
        owner_gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 5
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn initial_recovery_rejects_a_replaced_paused_connection_epoch() {
    let process_instance_id = ProcessInstanceId::parse("process:handoff").unwrap();
    let mut gateway = crate::gateway::compose_runtime_gateway_section_test_bootstrap_v2(
        process_instance_id.clone(),
    );
    gateway.connect_ready_for_gateway_section_test_v2();
    let stale_paused_gateway = gateway.observe_paused_connected_gateway_v2().unwrap();
    gateway.disconnect_for_gateway_section_test_v2();
    gateway.connect_ready_for_gateway_section_test_v2();
    let current_paused_gateway = gateway.observe_paused_connected_gateway_v2().unwrap();
    assert_ne!(stale_paused_gateway, current_paused_gateway);

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
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(
            port.clone(),
            accepted_receipt(owner_receipt),
            started,
            started,
            config,
        )
        .unwrap();
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let registry = crate::compose_runtime_registry_bootstrap_v1(
        process_instance_id,
        crate::GatewayResourceConfigV1::default(),
    )
    .unwrap();
    let readiness = crate::database::runtime_database_readiness_for_test_v1();
    let failure = match crate::closed_recovery::begin_initial_empty_recovery_retained_v2(
        &mut gateway,
        &registry,
        prepared,
        RuntimeRecoveryIdV2::parse("cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd").unwrap(),
        &readiness,
        &stale_paused_gateway,
        Instant::now() + Duration::from_secs(1),
    ) {
        Ok(_) => panic!("replaced epoch advanced recovery"),
        Err(failure) => failure,
    };
    let (prepared, error) = failure.into_parts();

    assert!(matches!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryBeginErrorV2::Gateway(
            crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Gateway(
                crate::RuntimeGatewayReadyObservationErrorV1::StaleAdmissionSnapshot
            )
        )
    ));
    assert_eq!(
        gateway.observe_paused_connected_gateway_v2().unwrap(),
        current_paused_gateway
    );
    assert_eq!(
        prepared
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn retained_recovery_begin_deadline_returns_owner_for_bounded_cleanup() {
    let process_instance_id = ProcessInstanceId::parse("process:handoff").unwrap();
    let mut gateway = crate::compose_runtime_gateway_bootstrap_v1(
        process_instance_id.clone(),
        crate::GatewayResourceConfigV1::default(),
    )
    .unwrap();
    gateway.connect_ready_for_gateway_section_test_v2();
    let paused_gateway = gateway.observe_paused_connected_gateway_v2().unwrap();
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
    let handle = gateway
        .start_gateway_owner_startup_watchdog_v1(
            port.clone(),
            accepted_receipt(owner_receipt),
            started,
            started,
            config,
        )
        .unwrap();
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let registry = crate::compose_runtime_registry_bootstrap_v1(
        process_instance_id,
        crate::GatewayResourceConfigV1::default(),
    )
    .unwrap();
    let readiness = crate::database::runtime_database_readiness_for_test_v1();
    let failure = match crate::closed_recovery::begin_initial_empty_recovery_retained_v2(
        &mut gateway,
        &registry,
        prepared,
        RuntimeRecoveryIdV2::parse("abababababababababababababababab").unwrap(),
        &readiness,
        &paused_gateway,
        Instant::now().checked_sub(Duration::from_nanos(1)).unwrap(),
    ) {
        Ok(_) => panic!("elapsed recovery begin advanced"),
        Err(failure) => failure,
    };
    let (prepared, error) = failure.into_parts();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryBeginErrorV2::DeadlineElapsed
    );
    assert_eq!(
        prepared
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn in_place_owner_commit_converts_once_and_supports_bounded_shutdown() {
    let (gateway, _registry, mut pending, port) =
        initial_pending_recovery_fixture_v2("71717171717171717171717171717171").await;

    pending.commit_owner_in_place_v2().await.unwrap();
    let session = pending.try_into_committed_session_v2().unwrap();
    session.revalidate_v2().unwrap();
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 1
    ));
    assert_eq!(
        session
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn elapsed_in_place_owner_commit_retains_bounded_cleanup_authority() {
    let (_gateway, _registry, pending, port) =
        initial_pending_recovery_fixture_v2("72727272727272727272727272727272").await;
    let mut pending = pending.with_operation_cutoff_for_test_v2(Instant::now());

    assert_eq!(
        pending.commit_owner_in_place_v2().await,
        Err(crate::closed_recovery::RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed)
    );
    assert_eq!(
        pending
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn canceled_in_place_owner_commit_retains_bounded_cleanup_authority() {
    let (_gateway, _registry, mut pending, port) =
        initial_pending_recovery_fixture_v2("75757575757575757575757575757575").await;
    let mut commit = Box::pin(pending.commit_owner_in_place_v2());
    let mut polled = false;
    std::future::poll_fn(|context| {
        if !polled {
            polled = true;
            assert!(commit.as_mut().poll(context).is_pending());
        }
        std::task::Poll::Ready(())
    })
    .await;

    drop(commit);

    assert_eq!(
        pending
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn duplicate_in_place_owner_commit_invalidates_and_releases_once() {
    let (_gateway, _registry, mut pending, port) =
        initial_pending_recovery_fixture_v2("73737373737373737373737373737373").await;

    pending.commit_owner_in_place_v2().await.unwrap();
    assert_eq!(
        pending.commit_owner_in_place_v2().await,
        Err(
            crate::closed_recovery::RuntimeClosedRecoveryCommitErrorV2::Owner(
                RuntimeGatewayOwnerClosedRecoveryCommitErrorV2::ProtocolViolation,
            )
        )
    );
    let pending = match pending.try_into_committed_session_v2() {
        Ok(_) => panic!("duplicate commit converted"),
        Err(pending) => *pending,
    };
    assert_eq!(
        pending
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn post_commit_registry_failure_retains_committed_bounded_cleanup() {
    let (_gateway, registry, mut pending, port) =
        initial_pending_recovery_fixture_v2("74747474747474747474747474747474").await;

    pending.commit_owner_in_place_v2().await.unwrap();
    let session = pending.try_into_committed_session_v2().unwrap();
    registry.advance_empty_sequence_for_test_v2();
    assert_eq!(
        session.revalidate_v2(),
        Err(
            crate::closed_recovery::RuntimeClosedRecoveryCommitErrorV2::Registry(
                crate::RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
            )
        )
    );
    assert_eq!(
        session
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn in_place_readiness_refresh_converts_once_and_supports_bounded_shutdown() {
    let (gateway, _registry, mut session, port) =
        initial_committed_recovery_fixture_v2("76767676767676767676767676767676").await;

    session
        .refresh_iteration_readiness_in_place_after_test_hook_v2(
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_for_test_v2(),
            )),
            || {},
        )
        .await
        .unwrap();
    let iteration = session.try_into_ready_iteration_v2().unwrap();

    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 2
    ));
    assert_eq!(
        iteration
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn canceled_in_place_readiness_refresh_retains_bounded_cleanup_authority() {
    let (_gateway, _registry, mut session, port) =
        initial_committed_recovery_fixture_v2("78787878787878787878787878787878").await;
    let verification = std::future::pending::<
        Result<
            crate::database::RuntimeDatabaseReadinessRefreshV2,
            crate::RuntimeDatabaseCompositionErrorV1,
        >,
    >();
    let mut refresh = Box::pin(
        session.refresh_iteration_readiness_in_place_after_test_hook_v2(verification, || {}),
    );
    let mut polled = false;
    std::future::poll_fn(|context| {
        if !polled {
            polled = true;
            assert!(refresh.as_mut().poll(context).is_pending());
        }
        std::task::Poll::Ready(())
    })
    .await;

    drop(refresh);

    let session = match session.try_into_ready_iteration_v2() {
        Ok(_) => panic!("canceled readiness refresh converted"),
        Err(session) => *session,
    };
    assert_eq!(
        session
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn duplicate_in_place_readiness_refresh_invalidates_and_releases_once() {
    let (_gateway, _registry, mut session, port) =
        initial_committed_recovery_fixture_v2("79797979797979797979797979797979").await;

    session
        .refresh_iteration_readiness_in_place_after_test_hook_v2(
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_for_test_v2(),
            )),
            || {},
        )
        .await
        .unwrap();
    assert_eq!(
        session
            .refresh_iteration_readiness_in_place_after_test_hook_v2(
                ready(Ok(
                    crate::database::runtime_database_readiness_refresh_for_test_v2(),
                )),
                || {},
            )
            .await,
        Err(
            crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway(
                crate::gateway::RuntimeGatewayRecoverySectionErrorV2::ProtocolViolation,
            )
        )
    );
    let session = match session.try_into_ready_iteration_v2() {
        Ok(_) => panic!("duplicate readiness refresh converted"),
        Err(session) => *session,
    };
    assert_eq!(
        session
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn post_refresh_registry_failure_retains_bounded_cleanup_authority() {
    let (_gateway, registry, mut session, port) =
        initial_committed_recovery_fixture_v2("80808080808080808080808080808080").await;

    assert_eq!(
        session
            .refresh_iteration_readiness_in_place_after_test_hook_v2(
                ready(Ok(
                    crate::database::runtime_database_readiness_refresh_for_test_v2(),
                )),
                || registry.advance_empty_sequence_for_test_v2(),
            )
            .await,
        Err(
            crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::Registry(
                crate::RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
            )
        )
    );
    let session = match session.try_into_ready_iteration_v2() {
        Ok(_) => panic!("failed readiness refresh converted"),
        Err(session) => *session,
    };
    assert_eq!(
        session
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn in_place_readiness_database_failure_retains_explicit_bounded_cleanup() {
    let (gateway, _registry, mut session, port) =
        initial_committed_recovery_fixture_v2("81818181818181818181818181818181").await;

    assert_eq!(
        session
            .refresh_iteration_readiness_in_place_after_test_hook_v2(
                ready(Err(
                    crate::RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut,
                )),
                || {},
            )
            .await,
        Err(
            crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::Database(
                crate::RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut,
            )
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
        } if generation.get() == 3
    ));
    let session = match session.try_into_ready_iteration_v2() {
        Ok(_) => panic!("failed database readiness refresh converted"),
        Err(session) => *session,
    };
    let _exit = session
        .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
        .await
        .unwrap();
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn ready_iteration_final_registry_failure_retains_bounded_cleanup() {
    let (gateway, registry, mut session, port) =
        initial_committed_recovery_fixture_v2("82828282828282828282828282828282").await;

    session
        .refresh_iteration_readiness_in_place_after_test_hook_v2(
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_for_test_v2(),
            )),
            || {},
        )
        .await
        .unwrap();
    let iteration = session.try_into_ready_iteration_v2().unwrap();
    registry.advance_empty_sequence_for_test_v2();
    assert_eq!(
        iteration.revalidate_v2(),
        Err(
            crate::closed_recovery::RuntimeClosedRecoveryCommitErrorV2::Registry(
                crate::RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
            )
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 2
    ));
    assert_eq!(
        iteration
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_millis(500))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency { .. }
    ));
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn unpolled_compound_owner_commit_cancels_closed_and_releases_once() {
    let (gateway, _registry, pending, port) =
        initial_pending_recovery_fixture_v2("11111111111111111111111111111111").await;

    let commit = pending.commit_owner_v2();
    drop(commit);

    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn compound_owner_commit_ack_wait_cancels_closed_and_releases_once() {
    let (gateway, _registry, pending, port) =
        initial_pending_recovery_fixture_v2("44444444444444444444444444444444").await;
    let mut commit = Box::pin(pending.commit_owner_v2());
    let mut polled = false;
    std::future::poll_fn(|context| {
        if !polled {
            polled = true;
            assert!(commit.as_mut().poll(context).is_pending());
        }
        std::task::Poll::Ready(())
    })
    .await;

    drop(commit);

    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn compound_owner_commit_cannot_cross_the_operation_cutoff() {
    let (gateway, _registry, pending, port) =
        initial_pending_recovery_fixture_v2("14141414141414141414141414141414").await;
    let operation_cutoff = Instant::now() + Duration::from_millis(100);
    let pending = pending.with_operation_cutoff_for_test_v2(operation_cutoff);
    let mut commit = Box::pin(pending.commit_owner_v2());

    std::future::poll_fn(|context| {
        assert!(commit.as_mut().poll(context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    std::thread::sleep(
        operation_cutoff.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
    );
    tokio::task::yield_now().await;
    let error = commit.await.unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryCommitErrorV2::DeadlineElapsed
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn post_commit_registry_aba_refuses_the_compound_session() {
    let (gateway, registry, pending, port) =
        initial_pending_recovery_fixture_v2("22222222222222222222222222222222").await;
    let before = registry.observe_recovery_empty_projection_v2().unwrap();

    let error = pending
        .commit_owner_after_test_hook_v2(|| registry.advance_empty_sequence_for_test_v2())
        .await
        .unwrap_err();
    let after = registry.observe_recovery_empty_projection_v2().unwrap();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryCommitErrorV2::Registry(
            crate::RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
        )
    );
    assert_eq!(before.retained_slot_count(), after.retained_slot_count());
    assert_eq!(
        before.retained_empty_tombstone_count(),
        after.retained_empty_tombstone_count()
    );
    assert_ne!(before.observation_sequence(), after.observation_sequence());
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn post_commit_disconnect_refuses_the_compound_session() {
    let (mut gateway, _registry, pending, port) =
        initial_pending_recovery_fixture_v2("33333333333333333333333333333333").await;

    let error = pending
        .commit_owner_after_test_hook_v2(|| gateway.disconnect_for_gateway_section_test_v2())
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryCommitErrorV2::Gateway(
            crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Gateway(
                crate::RuntimeGatewayReadyObservationErrorV1::Stopped,
            ),
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn committed_recovery_readiness_refresh_advances_one_linear_revision() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("55555555555555555555555555555555").await;

    let session = session
        .refresh_iteration_readiness_after_test_hook_v2(
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_for_test_v2(),
            )),
            || {},
        )
        .await
        .unwrap();

    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id,
            authority_revision,
        } if generation.get() == 2
            && recovery_id.as_str() == "55555555555555555555555555555555"
            && authority_revision.get() == 2
    ));
    assert_eq!(port.renew_calls(), 0);
    drop(session);
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn readiness_verifier_receives_the_session_bound_operation_cutoff() {
    let operation_cutoff = Instant::now() + Duration::from_secs(2);
    let (gateway, _registry, session, port) = initial_committed_recovery_fixture_until_v2(
        "12121212121212121212121212121212",
        operation_cutoff,
    )
    .await;
    let observed = Arc::new(Mutex::new(None));
    let verification_observed = observed.clone();

    let session = session
        .refresh_iteration_readiness_with_test_verifier_v2(move |cutoff| {
            *verification_observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cutoff);
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_for_test_v2(),
            ))
        })
        .await
        .unwrap();

    assert_eq!(
        *observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        Some(operation_cutoff)
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 2
    ));
    drop(session);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn owner_safety_deadline_limits_readiness_verification_before_session_cutoff() {
    let operation_cutoff = Instant::now() + Duration::from_secs(10);
    let (gateway, _registry, session, port) = initial_committed_recovery_fixture_until_v2(
        "13131313131313131313131313131313",
        operation_cutoff,
    )
    .await;
    let observed = Arc::new(Mutex::new(None));
    let verification_observed = observed.clone();

    let session = session
        .refresh_iteration_readiness_with_test_verifier_v2(move |cutoff| {
            *verification_observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cutoff);
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_for_test_v2(),
            ))
        })
        .await
        .unwrap();

    let verification_cutoff = observed
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .unwrap();
    assert!(verification_cutoff < operation_cutoff);
    assert!(verification_cutoff > Instant::now());
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            authority_revision,
            ..
        } if authority_revision.get() == 2
    ));
    drop(session);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn readiness_verification_failure_invalidates_capability_authority() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("66666666666666666666666666666666").await;

    let error = session
        .refresh_iteration_readiness_after_test_hook_v2(
            ready(Err(
                crate::RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut,
            )),
            || {},
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::Database(
            crate::RuntimeDatabaseCompositionErrorV1::ReadinessTimedOut,
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn elapsed_readiness_cutoff_never_polls_database_verification() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("dddddddddddddddddddddddddddddddd").await;
    let session = session.with_operation_cutoff_for_test_v2(
        Instant::now().checked_sub(Duration::from_nanos(1)).unwrap(),
    );
    let polls = Arc::new(AtomicUsize::new(0));
    let verification_polls = polls.clone();
    let verification = std::future::poll_fn(move |_| {
        verification_polls.fetch_add(1, Ordering::AcqRel);
        std::task::Poll::Ready(Ok(
            crate::database::runtime_database_readiness_refresh_for_test_v2(),
        ))
    });

    let error = session
        .refresh_iteration_readiness_after_test_hook_v2(verification, || {})
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::DeadlineElapsed
    );
    assert_eq!(polls.load(Ordering::Acquire), 0);
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn successful_readiness_result_after_cutoff_cannot_advance_authority() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee").await;
    let operation_cutoff = Instant::now() + Duration::from_millis(50);
    let session = session.with_operation_cutoff_for_test_v2(operation_cutoff);
    let verification = async move {
        tokio::time::sleep_until(tokio::time::Instant::from_std(
            operation_cutoff + Duration::from_millis(10),
        ))
        .await;
        Ok(crate::database::runtime_database_readiness_refresh_for_test_v2())
    };

    let error = session
        .refresh_iteration_readiness_after_test_hook_v2(verification, || {})
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::DeadlineElapsed
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn replayed_readiness_evidence_invalidates_capability_authority() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").await;

    let error = session
        .refresh_iteration_readiness_after_test_hook_v2(
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_at_for_test_v2(1_000_000),
            )),
            || {},
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway(
            crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Coordinator(
                automation_runtime_worker::RuntimeGatewayClosedTransitionErrorV2::CapabilityReadinessNotSuccessor,
            ),
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn unpolled_readiness_refresh_cancels_the_committed_session_closed() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("77777777777777777777777777777777").await;

    let refresh = session.refresh_iteration_readiness_after_test_hook_v2(
        ready(Ok(
            crate::database::runtime_database_readiness_refresh_for_test_v2(),
        )),
        || {},
    );
    drop(refresh);

    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn pending_readiness_refresh_cancellation_cannot_leave_current_authority() {
    let (gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("88888888888888888888888888888888").await;
    let verification = std::future::pending::<
        Result<
            crate::database::RuntimeDatabaseReadinessRefreshV2,
            crate::RuntimeDatabaseCompositionErrorV1,
        >,
    >();
    let mut refresh =
        Box::pin(session.refresh_iteration_readiness_after_test_hook_v2(verification, || {}));
    let mut polled = false;
    std::future::poll_fn(|context| {
        if !polled {
            polled = true;
            assert!(refresh.as_mut().poll(context).is_pending());
        }
        std::task::Poll::Ready(())
    })
    .await;
    drop(refresh);

    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn registry_aba_during_readiness_wait_prevents_authority_advance() {
    let (gateway, registry, session, port) =
        initial_committed_recovery_fixture_v2("99999999999999999999999999999999").await;
    let verification = async {
        registry.advance_empty_sequence_for_test_v2();
        Ok(crate::database::runtime_database_readiness_refresh_for_test_v2())
    };

    let error = session
        .refresh_iteration_readiness_after_test_hook_v2(verification, || {})
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::Registry(
            crate::RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn disconnect_during_readiness_wait_prevents_authority_advance() {
    let (mut gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("ffffffffffffffffffffffffffffffff").await;
    let verification = async {
        gateway.disconnect_for_gateway_section_test_v2();
        Ok(crate::database::runtime_database_readiness_refresh_for_test_v2())
    };

    let error = session
        .refresh_iteration_readiness_after_test_hook_v2(verification, || {})
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway(
            crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Gateway(
                crate::RuntimeGatewayReadyObservationErrorV1::Stopped,
            ),
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn registry_aba_after_readiness_advance_refuses_the_successor_session() {
    let (gateway, registry, session, port) =
        initial_committed_recovery_fixture_v2("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").await;

    let error = session
        .refresh_iteration_readiness_after_test_hook_v2(
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_for_test_v2(),
            )),
            || registry.advance_empty_sequence_for_test_v2(),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::Registry(
            crate::RuntimeRegistryRecoveryObservationErrorV1::StaleEmptyBinding,
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn disconnect_after_readiness_advance_refuses_the_successor_session() {
    let (mut gateway, _registry, session, port) =
        initial_committed_recovery_fixture_v2("cccccccccccccccccccccccccccccccc").await;

    let error = session
        .refresh_iteration_readiness_after_test_hook_v2(
            ready(Ok(
                crate::database::runtime_database_readiness_refresh_for_test_v2(),
            )),
            || gateway.disconnect_for_gateway_section_test_v2(),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::closed_recovery::RuntimeClosedRecoveryReadinessRefreshErrorV2::Gateway(
            crate::gateway::RuntimeGatewayRecoverySectionErrorV2::Gateway(
                crate::RuntimeGatewayReadyObservationErrorV1::Stopped,
            ),
        )
    );
    assert!(matches!(
        gateway.closed_snapshot(),
        automation_runtime_worker::RuntimeGatewayClosedSnapshotV2::Emergency {
            generation,
            cause: automation_runtime_worker::RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
        } if generation.get() == 3
    ));
    wait_for(|| port.release_calls() == 1).await;
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
async fn in_place_closed_recovery_prepare_is_linear_and_self_bound() {
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    let mut handle = match handle.try_into_prepared_closed_recovery_v2() {
        Ok(_) => panic!("unprepared handle advanced"),
        Err(handle) => *handle,
    };
    assert_eq!(port.observe_calls(), 0);

    handle.prepare_closed_recovery_in_place_v2().await.unwrap();
    let prepared = match handle.try_into_prepared_closed_recovery_v2() {
        Ok(prepared) => prepared,
        Err(_) => panic!("prepared handle did not advance"),
    };

    assert_eq!(
        prepared.observation().receipt(),
        &receipt(Duration::from_secs(5))
    );
    assert_eq!(port.observe_calls(), 1);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        prepared.abort_and_shutdown_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn duplicate_in_place_closed_recovery_prepare_invalidates_and_releases_once() {
    let (mut handle, port, invalidated) = fixture(
        Duration::from_secs(5),
        Duration::from_secs(2),
        Duration::from_millis(500),
        [],
    );
    handle.prepare_closed_recovery_in_place_v2().await.unwrap();

    assert_eq!(
        handle.prepare_closed_recovery_in_place_v2().await,
        Err(RuntimeGatewayOwnerClosedRecoveryPrepareErrorV2::ProtocolViolation)
    );
    assert!(invalidated.load(Ordering::Acquire));
    let handle = match handle.try_into_prepared_closed_recovery_v2() {
        Ok(_) => panic!("invalidated duplicate prepare advanced"),
        Err(handle) => *handle,
    };
    drop(handle);
    wait_for(|| port.release_calls() == 1).await;
    assert_eq!(port.observe_calls(), 1);
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
async fn committed_closed_recovery_freezes_the_exact_owner_without_renewal() {
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(700),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let permit = closed_recovery_permit(prepared.observation().receipt().clone());
    let mut closed = prepared.commit_closed_recovery_v2(&permit).await.unwrap();
    let committed = closed.observation().receipt().clone();
    let authority =
        crate::closed_recovery::RuntimeGatewayOwnerAdmissionFrozenAuthorityV2::for_test_v2(
            committed.clone(),
            Instant::now() + Duration::from_millis(300),
        );

    closed
        .enter_admission_frozen_in_place_v2(authority)
        .await
        .unwrap();
    let frozen = closed.try_into_admission_frozen_v2().unwrap();

    assert_eq!(frozen.handoff_observation_v2().receipt(), &committed);
    assert_eq!(port.observe_calls(), 2);
    assert_eq!(port.renew_calls(), 0);
    assert_eq!(port.acquire_calls(), 0);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        frozen
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn admission_frozen_rejects_a_strict_owner_revision_successor() {
    let (handle, port, invalidated) = fixture(
        Duration::from_millis(700),
        Duration::from_millis(500),
        Duration::from_millis(100),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let permit = closed_recovery_permit(prepared.observation().receipt().clone());
    let mut closed = prepared.commit_closed_recovery_v2(&permit).await.unwrap();
    let committed = closed.observation().receipt().clone();
    {
        let mut receipt = port
            .state
            .receipt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        receipt.owner_revision =
            NonZeroU64::new(receipt.owner_revision.get().saturating_add(1)).unwrap();
        receipt.expires_at += TimeDelta::milliseconds(100);
    }
    let authority =
        crate::closed_recovery::RuntimeGatewayOwnerAdmissionFrozenAuthorityV2::for_test_v2(
            committed,
            Instant::now() + Duration::from_millis(300),
        );

    assert_eq!(
        closed.enter_admission_frozen_in_place_v2(authority).await,
        Err(RuntimeGatewayOwnerAdmissionFrozenHandoffErrorV2::ProtocolViolation)
    );
    assert_eq!(
        closed.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert_eq!(port.renew_calls(), 0);
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn admission_frozen_keeps_the_startup_cutoff_and_cancellation_fails_closed() {
    let (handle, port, invalidated) = fixture(
        Duration::from_secs(2),
        Duration::from_millis(900),
        Duration::from_millis(100),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let permit = closed_recovery_permit(prepared.observation().receipt().clone());
    let mut closed = prepared.commit_closed_recovery_v2(&permit).await.unwrap();
    let committed = closed.observation().receipt().clone();
    let gate = Arc::new(Notify::new());
    port.block_next_observation(gate.clone());
    let authority =
        crate::closed_recovery::RuntimeGatewayOwnerAdmissionFrozenAuthorityV2::for_test_v2(
            committed,
            Instant::now() + Duration::from_millis(400),
        );
    let mut handoff = Box::pin(closed.enter_admission_frozen_in_place_v2(authority));
    tokio::select! {
        biased;
        result = &mut handoff => panic!("unexpected admission-frozen result: {result:?}"),
        _ = sleep(Duration::from_millis(20)) => {}
    }
    wait_for(|| port.observe_calls() == 2).await;
    drop(handoff);
    gate.notify_waiters();

    assert_eq!(
        closed.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.renew_calls(), 0);
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);

    let (handle, port, invalidated) = fixture(
        Duration::from_secs(2),
        Duration::from_millis(900),
        Duration::from_millis(100),
        [],
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let permit = closed_recovery_permit(prepared.observation().receipt().clone());
    let mut closed = prepared.commit_closed_recovery_v2(&permit).await.unwrap();
    let committed = closed.observation().receipt().clone();
    let cutoff = Instant::now() + Duration::from_millis(80);
    let authority =
        crate::closed_recovery::RuntimeGatewayOwnerAdmissionFrozenAuthorityV2::for_test_v2(
            committed, cutoff,
        );
    closed
        .enter_admission_frozen_in_place_v2(authority)
        .await
        .unwrap();
    let frozen = closed.try_into_admission_frozen_v2().unwrap();
    assert_eq!(frozen.handoff_cutoff_v2(), cutoff);
    assert_eq!(
        frozen.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed
    );
    assert_eq!(port.renew_calls(), 0);
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn admission_frozen_cleanup_remains_bounded_by_the_original_startup_cap() {
    let startup_cleanup_deadline = Instant::now() + Duration::from_millis(250);
    let (handle, port, invalidated) = fixture_with_startup_cleanup_deadline(
        Duration::from_secs(2),
        Duration::from_millis(900),
        Duration::from_millis(100),
        [],
        Some(startup_cleanup_deadline),
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let permit = closed_recovery_permit(prepared.observation().receipt().clone());
    let mut closed = prepared.commit_closed_recovery_v2(&permit).await.unwrap();
    let authority =
        crate::closed_recovery::RuntimeGatewayOwnerAdmissionFrozenAuthorityV2::for_test_v2(
            closed.observation().receipt().clone(),
            Instant::now() + Duration::from_secs(1),
        );
    closed
        .enter_admission_frozen_in_place_v2(authority)
        .await
        .unwrap();
    let frozen = closed.try_into_admission_frozen_v2().unwrap();
    *port
        .state
        .release_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(Notify::new()));

    let exit = timeout(
        Duration::from_millis(700),
        frozen.abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(2)),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(
        exit,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ReleaseUnconfirmed
    );
    assert!(Instant::now() >= startup_cleanup_deadline);
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

async fn process_activation_fixture(
    lease_for: Duration,
    renew_before: Duration,
    safety_margin: Duration,
    startup_cleanup_deadline: Option<Instant>,
    handoff_cutoff: Instant,
) -> (
    RuntimeGatewayOwnerAdmissionFrozenSupervisorV2,
    FakePortV1,
    Arc<AtomicBool>,
    RuntimeGatewayOwnerLeaseReceiptV1,
) {
    process_activation_fixture_with_renew_steps(
        lease_for,
        renew_before,
        safety_margin,
        startup_cleanup_deadline,
        handoff_cutoff,
        [],
    )
    .await
}

async fn process_activation_fixture_with_renew_steps(
    lease_for: Duration,
    renew_before: Duration,
    safety_margin: Duration,
    startup_cleanup_deadline: Option<Instant>,
    handoff_cutoff: Instant,
    renew_steps: impl IntoIterator<Item = FakeRenewStepV1>,
) -> (
    RuntimeGatewayOwnerAdmissionFrozenSupervisorV2,
    FakePortV1,
    Arc<AtomicBool>,
    RuntimeGatewayOwnerLeaseReceiptV1,
) {
    let (handle, port, invalidated) = fixture_with_startup_cleanup_deadline(
        lease_for,
        renew_before,
        safety_margin,
        renew_steps,
        startup_cleanup_deadline,
    );
    let prepared = handle.prepare_closed_recovery_v2().await.unwrap();
    let permit = closed_recovery_permit(prepared.observation().receipt().clone());
    let mut closed = prepared.commit_closed_recovery_v2(&permit).await.unwrap();
    let receipt = closed.observation().receipt().clone();
    let authority =
        crate::closed_recovery::RuntimeGatewayOwnerAdmissionFrozenAuthorityV2::for_test_v2(
            receipt.clone(),
            handoff_cutoff,
        );
    closed
        .enter_admission_frozen_in_place_v2(authority)
        .await
        .unwrap();
    (
        closed.try_into_admission_frozen_v2().unwrap(),
        port,
        invalidated,
        receipt,
    )
}

async fn certification_production_fixture(
    renew_steps: impl IntoIterator<Item = FakeRenewStepV1>,
    process_generation: NonZeroU64,
) -> (
    RuntimeGatewayOwnerProductionSupervisorV2,
    FakePortV1,
    Arc<AtomicBool>,
    RuntimeGatewayOwnerLeaseReceiptV1,
) {
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture_with_renew_steps(
        Duration::from_secs(2),
        Duration::from_millis(1_200),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
        renew_steps,
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(process_generation)
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();
    process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();
    (
        process.try_into_production_v2().unwrap(),
        port,
        invalidated,
        receipt,
    )
}

#[tokio::test]
async fn exact_process_activation_preserves_the_frozen_receipt_and_generation() {
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    let process_generation = NonZeroU64::new(7).unwrap();

    let activated = frozen
        .activate_process_ownership_in_place_v2(process_generation)
        .await
        .unwrap();
    let process = frozen.try_into_process_frozen_v2().unwrap();

    assert_eq!(activated.receipt(), &receipt);
    assert_eq!(process.activation_observation_v2().receipt(), &receipt);
    assert_eq!(process.process_generation_v2(), process_generation);
    assert_eq!(
        process.observe_current_v2().await.unwrap().receipt(),
        &receipt
    );
    assert_eq!(port.renew_calls(), 0);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        process
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn lost_process_activation_acknowledgement_exact_observes_the_applied_generation() {
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    let process_generation = NonZeroU64::new(11).unwrap();
    frozen
        .activate_process_ownership_in_place_v2(process_generation)
        .await
        .unwrap();
    let (sender, acknowledgement) = oneshot::channel();
    drop(sender);

    let recovered = frozen
        .inner()
        .accept_process_activation_acknowledgement_v2(process_generation, acknowledgement)
        .await
        .unwrap();

    assert_eq!(recovered.receipt(), &receipt);
    assert_eq!(port.renew_calls(), 0);
    assert!(!invalidated.load(Ordering::Acquire));
    let process = frozen.try_into_process_frozen_v2().unwrap();
    assert_eq!(
        process
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
}

#[tokio::test]
async fn stale_process_activation_receipt_is_rejected_and_invalidates() {
    let (frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    let mut stale = receipt;
    stale.owner_revision = NonZeroU64::new(stale.owner_revision.get() + 1).unwrap();

    assert_eq!(
        frozen
            .inner()
            .activate_process_ownership_v2(stale, NonZeroU64::new(3).unwrap())
            .await,
        Err(RuntimeGatewayOwnerProcessActivationErrorV2::OwnerReceiptMismatch)
    );
    assert_eq!(
        frozen.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.renew_calls(), 0);
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn stale_process_generation_cannot_replace_an_activated_generation() {
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(NonZeroU64::new(5).unwrap())
        .await
        .unwrap();

    assert_eq!(
        frozen
            .inner()
            .activate_process_ownership_v2(receipt, NonZeroU64::new(4).unwrap())
            .await,
        Err(RuntimeGatewayOwnerProcessActivationErrorV2::ProtocolViolation)
    );
    assert_eq!(
        frozen.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.renew_calls(), 0);
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn process_frozen_survives_the_expired_startup_handoff_cutoff() {
    let cutoff = Instant::now() + Duration::from_millis(80);
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_millis(900),
        Duration::from_millis(500),
        Duration::from_millis(100),
        None,
        cutoff,
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(NonZeroU64::new(9).unwrap())
        .await
        .unwrap();
    let process = frozen.try_into_process_frozen_v2().unwrap();

    sleep(Duration::from_millis(140)).await;

    assert!(Instant::now() >= cutoff);
    assert_eq!(
        process.observe_current_v2().await.unwrap().receipt(),
        &receipt
    );
    assert_eq!(process.terminal_status_v2(), None);
    assert_eq!(port.renew_calls(), 0);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        process
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
}

#[tokio::test]
async fn process_activation_clears_the_original_startup_cleanup_cap() {
    let startup_cleanup_deadline = Instant::now() + Duration::from_millis(250);
    let (mut frozen, port, invalidated, _) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        Some(startup_cleanup_deadline),
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(NonZeroU64::new(13).unwrap())
        .await
        .unwrap();
    let process = frozen.try_into_process_frozen_v2().unwrap();
    let release_gate = Arc::new(Notify::new());
    *port
        .state
        .release_gate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(release_gate.clone());
    let release = tokio::spawn(async move {
        sleep(Duration::from_millis(350)).await;
        release_gate.notify_one();
    });

    let exit = process
        .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
        .await
        .unwrap();

    release.await.unwrap();
    assert_eq!(exit, RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown);
    assert!(Instant::now() >= startup_cleanup_deadline);
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
    assert_eq!(port.renew_calls(), 0);
}

#[tokio::test]
async fn process_frozen_does_not_renew_at_the_production_renewal_boundary() {
    let (mut frozen, port, invalidated, _) = process_activation_fixture(
        Duration::from_millis(1_200),
        Duration::from_millis(900),
        Duration::from_millis(100),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(NonZeroU64::new(17).unwrap())
        .await
        .unwrap();
    let process = frozen.try_into_process_frozen_v2().unwrap();

    sleep(Duration::from_millis(450)).await;

    assert_eq!(port.renew_calls(), 0);
    assert_eq!(process.terminal_status_v2(), None);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        process
            .abort_and_shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn exact_process_renewal_start_preserves_identity_and_enters_production() {
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    let process_generation = NonZeroU64::new(19).unwrap();
    frozen
        .activate_process_ownership_in_place_v2(process_generation)
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();

    let started = process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();
    let production = process.try_into_production_v2().unwrap();

    assert_eq!(started.receipt(), &receipt);
    assert_eq!(production.start_observation_v2().receipt(), &receipt);
    assert_eq!(production.process_generation_v2(), process_generation);
    assert_eq!(
        format!("{production:?}"),
        "RuntimeGatewayOwnerProductionSupervisorV2(<redacted>)"
    );
    assert_eq!(
        production
            .observe_current_v2()
            .await
            .unwrap()
            .receipt()
            .lease_id,
        receipt.lease_id
    );
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        production
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn lost_process_renewal_start_acknowledgement_reconciles_without_reactivation() {
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    let process_generation = NonZeroU64::new(23).unwrap();
    frozen
        .activate_process_ownership_in_place_v2(process_generation)
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();
    process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();
    let (sender, acknowledgement) = oneshot::channel();
    drop(sender);

    let recovered = process
        .inner()
        .accept_process_renewal_acknowledgement_v2(process_generation, acknowledgement)
        .await
        .unwrap();

    assert_eq!(recovered.receipt().lease_id, receipt.lease_id);
    assert!(recovered.receipt().owner_revision >= receipt.owner_revision);
    assert_eq!(
        process
            .inner()
            .production_generation
            .load(Ordering::Acquire),
        process_generation.get()
    );
    assert!(!invalidated.load(Ordering::Acquire));
    let production = process.try_into_production_v2().unwrap();
    assert_eq!(
        production
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn stale_process_renewal_receipt_is_terminal_and_fail_closed() {
    let (mut frozen, port, invalidated, mut receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    let process_generation = NonZeroU64::new(29).unwrap();
    frozen
        .activate_process_ownership_in_place_v2(process_generation)
        .await
        .unwrap();
    let process = frozen.try_into_process_frozen_v2().unwrap();
    receipt.owner_revision = NonZeroU64::new(receipt.owner_revision.get() + 1).unwrap();

    assert_eq!(
        process
            .inner()
            .start_process_renewal_v2(receipt, process_generation)
            .await,
        Err(RuntimeGatewayOwnerProcessRenewalStartErrorV2::OwnerReceiptMismatch)
    );
    assert_eq!(
        process.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.renew_calls(), 0);
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn stale_process_renewal_generation_is_terminal_and_fail_closed() {
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    let process_generation = NonZeroU64::new(31).unwrap();
    frozen
        .activate_process_ownership_in_place_v2(process_generation)
        .await
        .unwrap();
    let process = frozen.try_into_process_frozen_v2().unwrap();

    assert_eq!(
        process
            .inner()
            .start_process_renewal_v2(receipt, NonZeroU64::new(30).unwrap())
            .await,
        Err(RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProcessGenerationMismatch)
    );
    assert_eq!(
        process.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.renew_calls(), 0);
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn duplicate_process_renewal_start_is_terminal_and_fail_closed() {
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    let process_generation = NonZeroU64::new(37).unwrap();
    frozen
        .activate_process_ownership_in_place_v2(process_generation)
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();
    process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();

    assert_eq!(
        process
            .inner()
            .start_process_renewal_v2(receipt, process_generation)
            .await,
        Err(RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProtocolViolation)
    );
    assert_eq!(
        process.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn process_renewal_start_in_another_role_is_terminal_and_fail_closed() {
    let (frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_secs(2),
        Duration::from_secs(1),
        Duration::from_millis(200),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;

    assert_eq!(
        frozen
            .inner()
            .start_process_renewal_v2(receipt, NonZeroU64::new(41).unwrap())
            .await,
        Err(RuntimeGatewayOwnerProcessRenewalStartErrorV2::ProtocolViolation)
    );
    assert_eq!(
        frozen.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.renew_calls(), 0);
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn production_immediately_renews_and_publishes_a_strict_successor_without_polling() {
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_millis(900),
        Duration::from_millis(600),
        Duration::from_millis(100),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(NonZeroU64::new(43).unwrap())
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();
    process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();
    let production = process.try_into_production_v2().unwrap();
    let observation_calls = port.observe_calls();

    let successor = production
        .wait_for_strict_successor_v2(
            receipt.owner_revision,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();

    assert_eq!(
        successor.receipt().owner_revision.get(),
        receipt.owner_revision.get() + 1
    );
    assert_eq!(port.renew_calls(), 1);
    assert_eq!(port.observe_calls(), observation_calls);
    assert_eq!(production.terminal_status_v2(), None);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        production
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
}

#[tokio::test]
async fn canceling_a_strict_successor_wait_does_not_cancel_the_production_renewal() {
    let gate = Arc::new(Notify::new());
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture_with_renew_steps(
        Duration::from_millis(900),
        Duration::from_millis(600),
        Duration::from_millis(100),
        None,
        Instant::now() + Duration::from_secs(1),
        [FakeRenewStepV1::Blocked(gate.clone())],
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(NonZeroU64::new(46).unwrap())
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();
    process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();
    let production = process.try_into_production_v2().unwrap();
    wait_for(|| port.renew_calls() == 1).await;
    let mut successor = Box::pin(production.wait_for_strict_successor_v2(
        receipt.owner_revision,
        Instant::now() + Duration::from_secs(1),
    ));
    tokio::select! {
        biased;
        result = &mut successor => panic!("unexpected strict successor result: {result:?}"),
        _ = sleep(Duration::from_millis(20)) => {}
    }
    drop(successor);

    gate.notify_one();
    let observed = production
        .wait_for_strict_successor_v2(
            receipt.owner_revision,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();

    assert!(observed.receipt().owner_revision > receipt.owner_revision);
    assert_eq!(port.renew_calls(), 1);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        production
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
}

#[tokio::test]
async fn production_renewal_continues_past_the_startup_handoff_cutoff() {
    let cutoff = Instant::now() + Duration::from_millis(150);
    let (mut frozen, port, invalidated, _) = process_activation_fixture(
        Duration::from_millis(600),
        Duration::from_millis(400),
        Duration::from_millis(100),
        None,
        cutoff,
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(NonZeroU64::new(47).unwrap())
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();
    let started = process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();
    let production = process.try_into_production_v2().unwrap();
    let first = production
        .wait_for_strict_successor_v2(
            started.receipt().owner_revision,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();

    let second = production
        .wait_for_strict_successor_v2(
            first.receipt().owner_revision,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();

    assert!(Instant::now() >= cutoff);
    assert!(second.receipt().owner_revision > first.receipt().owner_revision);
    assert!(port.renew_calls() >= 2);
    assert_eq!(production.terminal_status_v2(), None);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        production
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
}

#[tokio::test]
async fn production_renewal_ownership_loss_is_terminal_and_fail_closed() {
    let (mut frozen, port, invalidated, _) = process_activation_fixture_with_renew_steps(
        Duration::from_millis(900),
        Duration::from_millis(600),
        Duration::from_millis(100),
        None,
        Instant::now() + Duration::from_secs(1),
        [FakeRenewStepV1::OwnershipLost],
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(NonZeroU64::new(53).unwrap())
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();
    let started = process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();
    let production = process.try_into_production_v2().unwrap();

    assert_eq!(
        production
            .wait_for_strict_successor_v2(
                started.receipt().owner_revision,
                Instant::now() + Duration::from_secs(1),
            )
            .await,
        Err(RuntimeGatewayOwnerStrictSuccessorErrorV2::OwnershipLost)
    );
    assert_eq!(
        production.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.renew_calls(), 1);
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn production_renewal_unknown_is_terminal_and_fail_closed() {
    let (mut frozen, port, invalidated, _) = process_activation_fixture_with_renew_steps(
        Duration::from_millis(900),
        Duration::from_millis(600),
        Duration::from_millis(100),
        None,
        Instant::now() + Duration::from_secs(1),
        [FakeRenewStepV1::OutcomeUnknown],
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(NonZeroU64::new(59).unwrap())
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();
    let started = process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();
    let production = process.try_into_production_v2().unwrap();

    assert_eq!(
        production
            .wait_for_strict_successor_v2(
                started.receipt().owner_revision,
                Instant::now() + Duration::from_secs(1),
            )
            .await,
        Err(RuntimeGatewayOwnerStrictSuccessorErrorV2::SupervisorUnavailable)
    );
    assert_eq!(
        production.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::RenewalUnknown
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.renew_calls(), 1);
    assert_eq!(port.release_calls(), 1);
}

#[test]
fn certification_freeze_observation_acceptance_is_exact_and_monotonic() {
    let now = Instant::now();
    let cutoff = now + Duration::from_secs(10);
    let expected_safety_deadline = now + Duration::from_secs(30);
    let expected = RuntimeGatewayOwnerCurrentObservationV1 {
        receipt: receipt(Duration::from_secs(60)),
        safety_deadline: expected_safety_deadline,
    };

    assert!(accept_certification_freeze_observation_v2(
        &expected, &expected, cutoff
    ));

    let mut refined = expected.clone();
    refined.receipt.database_now += TimeDelta::milliseconds(1);
    assert!(accept_certification_freeze_observation_v2(
        &expected, &refined, cutoff
    ));

    let mut tightened = refined.clone();
    tightened.safety_deadline -= Duration::from_millis(1);
    assert!(accept_certification_freeze_observation_v2(
        &expected, &tightened, cutoff
    ));

    let mut regressed_database_time = expected.clone();
    regressed_database_time.receipt.database_now -= TimeDelta::milliseconds(1);
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &regressed_database_time,
        cutoff
    ));

    let mut drifted_expiry = refined.clone();
    drifted_expiry.receipt.expires_at += TimeDelta::milliseconds(1);
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &drifted_expiry,
        cutoff
    ));

    let mut extended_safety = refined.clone();
    extended_safety.safety_deadline += Duration::from_millis(1);
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &extended_safety,
        cutoff
    ));

    let mut foreign_lease = refined.clone();
    foreign_lease.receipt.lease_id.lease_epoch = NonZeroU64::new(2).unwrap();
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &foreign_lease,
        cutoff
    ));

    let mut successor = expected.clone();
    successor.receipt.owner_revision = NonZeroU64::new(4).unwrap();
    successor.receipt.database_now += TimeDelta::milliseconds(1);
    successor.receipt.expires_at += TimeDelta::milliseconds(1);
    successor.safety_deadline += Duration::from_millis(1);
    assert!(accept_certification_freeze_observation_v2(
        &expected, &successor, cutoff
    ));

    let mut revision_skip = successor.clone();
    revision_skip.receipt.owner_revision = NonZeroU64::new(5).unwrap();
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &revision_skip,
        cutoff
    ));

    let mut successor_without_database_advance = successor.clone();
    successor_without_database_advance.receipt.database_now = expected.receipt.database_now;
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &successor_without_database_advance,
        cutoff
    ));

    let mut successor_without_expiry_advance = successor.clone();
    successor_without_expiry_advance.receipt.expires_at = expected.receipt.expires_at;
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &successor_without_expiry_advance,
        cutoff
    ));

    let mut successor_without_safety_advance = successor.clone();
    successor_without_safety_advance.safety_deadline = expected.safety_deadline;
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &successor_without_safety_advance,
        cutoff
    ));

    let mut stale_current = refined.clone();
    stale_current.receipt.database_now = stale_current.receipt.expires_at;
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &stale_current,
        cutoff
    ));

    let mut unsafe_expected = expected.clone();
    unsafe_expected.safety_deadline = cutoff;
    assert!(!accept_certification_freeze_observation_v2(
        &unsafe_expected,
        &successor,
        cutoff
    ));

    let mut unsafe_current = refined.clone();
    unsafe_current.safety_deadline = cutoff;
    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &unsafe_current,
        cutoff
    ));

    let mut stale_expected = expected.clone();
    stale_expected.receipt.database_now = stale_expected.receipt.expires_at;
    assert!(!accept_certification_freeze_observation_v2(
        &stale_expected,
        &successor,
        cutoff
    ));

    let mut maximum_revision = expected.clone();
    maximum_revision.receipt.owner_revision = NonZeroU64::new(u64::MAX).unwrap();
    let mut wrapped_successor = successor;
    wrapped_successor.receipt.owner_revision = NonZeroU64::new(1).unwrap();
    assert!(!accept_certification_freeze_observation_v2(
        &maximum_revision,
        &wrapped_successor,
        cutoff
    ));

    assert!(!accept_certification_freeze_observation_v2(
        &expected,
        &refined,
        Instant::now()
    ));
}

#[tokio::test]
async fn certification_freeze_uses_latest_published_same_revision_observation() {
    let process_generation = NonZeroU64::new(60).unwrap();
    let (production, port, invalidated, receipt) =
        certification_production_fixture([], process_generation).await;
    let renewed = production
        .wait_for_strict_successor_v2(
            receipt.owner_revision,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    {
        let mut current = port
            .state
            .receipt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        current.database_now += TimeDelta::milliseconds(1);
    }
    let refreshed = production.observe_current_v2().await.unwrap();

    assert_eq!(
        refreshed.receipt().owner_revision,
        renewed.receipt().owner_revision
    );
    assert_eq!(refreshed.receipt().expires_at, renewed.receipt().expires_at);
    assert!(refreshed.receipt().database_now > renewed.receipt().database_now);
    assert!(refreshed.safety_deadline() <= renewed.safety_deadline());

    let authority = production
        .prepare_certification_freeze_v2(Instant::now() + Duration::from_millis(500))
        .unwrap();
    assert_eq!(authority.expected_observation_v2(), &refreshed);
    let (frozen, observation) = authority.freeze_v2().await.unwrap();

    assert_eq!(observation.observation_v2(), &refreshed);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        frozen
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn certification_freeze_accepts_a_queued_same_revision_observation_refinement() {
    let process_generation = NonZeroU64::new(69).unwrap();
    let (production, port, invalidated, receipt) =
        certification_production_fixture([], process_generation).await;
    let renewed = production
        .wait_for_strict_successor_v2(
            receipt.owner_revision,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    let renew_calls = port.renew_calls();
    let gate = Arc::new(Notify::new());
    port.block_next_observation(gate.clone());
    let observe_calls = port.observe_calls();
    let commands = production.inner().supervisor_commands.clone();
    let (response, acknowledgement) = oneshot::channel();
    commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Observe { response })
        .await
        .unwrap();
    wait_for(|| port.observe_calls() > observe_calls).await;

    let cutoff = Instant::now() + Duration::from_millis(500);
    let authority = production.prepare_certification_freeze_v2(cutoff).unwrap();
    assert_eq!(authority.expected_observation_v2(), &renewed);
    {
        let mut current = port
            .state
            .receipt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        current.database_now += TimeDelta::milliseconds(1);
    }
    let mut freezing = Box::pin(authority.freeze_v2());
    tokio::select! {
        biased;
        result = &mut freezing => panic!("unexpected certification freeze result: {result:?}"),
        _ = sleep(Duration::from_millis(20)) => {}
    }

    gate.notify_one();
    let refined = acknowledgement.await.unwrap().unwrap();
    let (frozen, observation) = freezing.await.unwrap();

    assert_eq!(
        refined.receipt().owner_revision,
        renewed.receipt().owner_revision
    );
    assert_eq!(refined.receipt().expires_at, renewed.receipt().expires_at);
    assert!(refined.receipt().database_now > renewed.receipt().database_now);
    assert!(refined.safety_deadline() <= renewed.safety_deadline());
    assert_eq!(observation.observation_v2(), &refined);
    assert_eq!(port.renew_calls(), renew_calls);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        frozen
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn certification_freeze_queues_behind_inflight_renewal_and_stops_automatic_renewal() {
    let gate = Arc::new(Notify::new());
    let process_generation = NonZeroU64::new(61).unwrap();
    let (production, port, invalidated, receipt) = certification_production_fixture(
        [FakeRenewStepV1::Blocked(gate.clone())],
        process_generation,
    )
    .await;
    wait_for(|| port.renew_calls() == 1).await;
    let cutoff = Instant::now() + Duration::from_millis(1_350);
    let authority = production.prepare_certification_freeze_v2(cutoff).unwrap();
    let expected_revision = authority.expected_observation_v2().receipt().owner_revision;

    assert_eq!(authority.process_generation_v2(), process_generation);
    assert_eq!(authority.cutoff_v2(), cutoff);
    assert_eq!(
        format!("{authority:?}"),
        "RuntimeGatewayOwnerCertificationFreezeAuthorityV2(<redacted>)"
    );
    let mut freezing = Box::pin(authority.freeze_v2());
    tokio::select! {
        biased;
        result = &mut freezing => panic!("unexpected certification freeze result: {result:?}"),
        _ = sleep(Duration::from_millis(20)) => {}
    }

    gate.notify_one();
    let (frozen, observation) = freezing.await.unwrap();

    assert_eq!(
        observation.observation_v2().receipt().lease_id,
        receipt.lease_id
    );
    assert_eq!(
        observation.observation_v2().receipt().owner_revision.get(),
        expected_revision.get() + 1
    );
    assert_eq!(observation.process_generation_v2(), process_generation);
    assert_eq!(observation.cutoff_v2(), cutoff);
    assert_eq!(frozen.frozen_observation_v2(), observation.observation_v2());
    assert_eq!(frozen.process_generation_v2(), process_generation);
    assert_eq!(frozen.cutoff_v2(), cutoff);
    assert_eq!(
        format!("{observation:?}"),
        "RuntimeGatewayOwnerCertificationFrozenObservationV2(<redacted>)"
    );
    assert_eq!(
        format!("{frozen:?}"),
        "RuntimeGatewayOwnerCertificationFrozenSupervisorV2(<redacted>)"
    );
    sleep(Duration::from_millis(900)).await;
    assert_eq!(port.renew_calls(), 1);
    assert_eq!(
        frozen.observe_current_v2().await.unwrap(),
        *observation.observation_v2()
    );
    assert_eq!(frozen.terminal_status_v2(), None);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        frozen
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn certification_freeze_send_queue_stall_is_bounded_by_the_cutoff() {
    let gate = Arc::new(Notify::new());
    let process_generation = NonZeroU64::new(62).unwrap();
    let (production, port, invalidated, _) = certification_production_fixture(
        [FakeRenewStepV1::Blocked(gate.clone())],
        process_generation,
    )
    .await;
    wait_for(|| port.renew_calls() == 1).await;
    let (response, observation_acknowledgement) = oneshot::channel();
    production
        .inner()
        .supervisor_commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::Observe { response })
        .await
        .unwrap();
    let cutoff = Instant::now() + Duration::from_millis(120);
    let authority = production.prepare_certification_freeze_v2(cutoff).unwrap();
    let started_at = Instant::now();

    assert!(matches!(
        authority.freeze_v2().await,
        Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed)
    ));
    assert!(Instant::now() >= cutoff);
    assert!(started_at.elapsed() < Duration::from_millis(400));
    assert!(invalidated.load(Ordering::Acquire));

    gate.notify_one();
    drop(observation_acknowledgement);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn certification_freeze_ack_stall_is_bounded_by_the_cutoff() {
    let gate = Arc::new(Notify::new());
    let process_generation = NonZeroU64::new(63).unwrap();
    let (production, port, invalidated, _) = certification_production_fixture(
        [FakeRenewStepV1::Blocked(gate.clone())],
        process_generation,
    )
    .await;
    wait_for(|| port.renew_calls() == 1).await;
    let cutoff = Instant::now() + Duration::from_millis(120);
    let authority = production.prepare_certification_freeze_v2(cutoff).unwrap();
    let started_at = Instant::now();

    assert!(matches!(
        authority.freeze_v2().await,
        Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::DeadlineElapsed)
    ));
    assert!(Instant::now() >= cutoff);
    assert!(started_at.elapsed() < Duration::from_millis(400));
    assert!(invalidated.load(Ordering::Acquire));

    gate.notify_one();
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn stale_certification_freeze_process_generation_is_terminal_and_fail_closed() {
    let process_generation = NonZeroU64::new(65).unwrap();
    let (production, port, invalidated, _) =
        certification_production_fixture([], process_generation).await;
    let expected_observation = production.inner().current_observation.borrow().clone();

    assert_eq!(
        production
            .inner()
            .freeze_certification_v2(
                expected_observation,
                NonZeroU64::new(64).unwrap(),
                Instant::now() + Duration::from_secs(1),
            )
            .await,
        Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::ProcessGenerationMismatch)
    );
    assert_eq!(
        production.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn stale_certification_freeze_receipt_after_queued_renewal_is_terminal() {
    let gate = Arc::new(Notify::new());
    let process_generation = NonZeroU64::new(66).unwrap();
    let (production, port, invalidated, _) = certification_production_fixture(
        [FakeRenewStepV1::Blocked(gate.clone())],
        process_generation,
    )
    .await;
    wait_for(|| port.renew_calls() == 1).await;
    let mut expected_observation = production.inner().current_observation.borrow().clone();
    expected_observation.receipt.owner_revision = NonZeroU64::new(
        expected_observation
            .receipt
            .owner_revision
            .get()
            .checked_add(2)
            .unwrap(),
    )
    .unwrap();
    let release = tokio::spawn(async move {
        sleep(Duration::from_millis(20)).await;
        gate.notify_one();
    });

    assert_eq!(
        production
            .inner()
            .freeze_certification_v2(
                expected_observation,
                process_generation,
                Instant::now() + Duration::from_secs(1),
            )
            .await,
        Err(RuntimeGatewayOwnerCertificationFreezeErrorV2::OwnerReceiptMismatch)
    );
    release.await.unwrap();
    assert_eq!(
        production.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.renew_calls(), 1);
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn certification_frozen_cutoff_is_terminal_without_an_automatic_renewal() {
    let process_generation = NonZeroU64::new(67).unwrap();
    let (production, port, invalidated, _) =
        certification_production_fixture([], process_generation).await;
    let cutoff = Instant::now() + Duration::from_millis(150);
    let authority = production.prepare_certification_freeze_v2(cutoff).unwrap();
    let (frozen, _observation) = authority.freeze_v2().await.unwrap();
    let renew_calls = port.renew_calls();

    assert_eq!(
        frozen.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed
    );
    assert_eq!(port.renew_calls(), renew_calls);
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn exact_certification_thaw_resumes_with_a_strict_successor() {
    let gate = Arc::new(Notify::new());
    let process_generation = NonZeroU64::new(71).unwrap();
    let (production, port, invalidated, _) = certification_production_fixture(
        [FakeRenewStepV1::Blocked(gate.clone())],
        process_generation,
    )
    .await;
    wait_for(|| port.renew_calls() == 1).await;
    let authority = production
        .prepare_certification_freeze_v2(Instant::now() + Duration::from_millis(1_350))
        .unwrap();
    let mut freezing = Box::pin(authority.freeze_v2());
    tokio::select! {
        biased;
        result = &mut freezing => panic!("unexpected certification freeze result: {result:?}"),
        _ = sleep(Duration::from_millis(20)) => {}
    }
    gate.notify_one();
    let (frozen, observation) = freezing.await.unwrap();
    let frozen_revision = observation.observation_v2().receipt().owner_revision;

    let (production, successor) = frozen
        .thaw_v2(observation, Instant::now() + Duration::from_millis(800))
        .await
        .unwrap();

    assert_eq!(
        successor.receipt().owner_revision.get(),
        frozen_revision.get() + 1
    );
    assert_eq!(production.process_generation_v2(), process_generation);
    assert_eq!(port.renew_calls(), 2);
    assert_eq!(production.terminal_status_v2(), None);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        production
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn production_observation_crossing_renewal_boundary_retries_after_successor() {
    let process_generation = NonZeroU64::new(72).unwrap();
    let (mut frozen, port, invalidated, receipt) = process_activation_fixture(
        Duration::from_millis(700),
        Duration::from_millis(450),
        Duration::from_millis(100),
        None,
        Instant::now() + Duration::from_secs(1),
    )
    .await;
    frozen
        .activate_process_ownership_in_place_v2(process_generation)
        .await
        .unwrap();
    let mut process = frozen.try_into_process_frozen_v2().unwrap();
    process
        .start_production_renewal_in_place_v2()
        .await
        .unwrap();
    let production = process.try_into_production_v2().unwrap();
    let first = production
        .wait_for_strict_successor_v2(
            receipt.owner_revision,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    let authority = production
        .prepare_certification_freeze_v2(Instant::now() + Duration::from_millis(500))
        .unwrap();
    let (frozen, observation) = authority.freeze_v2().await.unwrap();
    let (production, thawed) = frozen
        .thaw_v2(observation, Instant::now() + Duration::from_millis(450))
        .await
        .unwrap();
    assert!(thawed.receipt().owner_revision > first.receipt().owner_revision);

    port.block_next_observation(Arc::new(Notify::new()));
    let renew_calls = port.renew_calls();
    let observe_calls = port.observe_calls();
    let observed = timeout(Duration::from_secs(1), production.observe_current_v2())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        observed.receipt().owner_revision.get(),
        thawed.receipt().owner_revision.get() + 1
    );
    assert_eq!(port.renew_calls(), renew_calls + 1);
    assert_eq!(port.observe_calls(), observe_calls + 2);
    assert_eq!(production.terminal_status_v2(), None);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        production
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn production_observation_recovery_is_bounded_to_one_retry() {
    let process_generation = NonZeroU64::new(77).unwrap();
    let (production, port, invalidated, receipt) =
        certification_production_fixture([], process_generation).await;
    production
        .wait_for_strict_successor_v2(
            receipt.owner_revision,
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    port.push_observation_step(FakeObservationStepV1::Error(FakeErrorV1::Retryable));
    port.push_observation_step(FakeObservationStepV1::Error(FakeErrorV1::Retryable));
    let observe_calls = port.observe_calls();

    assert_eq!(
        production.observe_current_v2().await,
        Err(RuntimeGatewayOwnerCurrentObservationErrorV1::Retryable)
    );
    assert_eq!(port.observe_calls(), observe_calls + 2);
    assert_eq!(production.terminal_status_v2(), None);
    assert!(!invalidated.load(Ordering::Acquire));
    assert_eq!(
        production
            .shutdown_until_v2(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap(),
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn production_observation_terminal_failures_are_never_retried() {
    let cases = [
        (
            FakeErrorV1::OwnershipLost,
            RuntimeGatewayOwnerCurrentObservationErrorV1::OwnershipLost,
            RuntimeGatewayOwnerStartupWatchdogExitV1::OwnershipLost,
        ),
        (
            FakeErrorV1::ProtocolViolation,
            RuntimeGatewayOwnerCurrentObservationErrorV1::ProtocolViolation,
            RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation,
        ),
    ];
    for (injected, expected_error, expected_exit) in cases {
        let process_generation = NonZeroU64::new(78).unwrap();
        let (production, port, invalidated, receipt) =
            certification_production_fixture([], process_generation).await;
        production
            .wait_for_strict_successor_v2(
                receipt.owner_revision,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .unwrap();
        port.push_observation_step(FakeObservationStepV1::Error(injected));
        let observe_calls = port.observe_calls();

        assert_eq!(production.observe_current_v2().await, Err(expected_error));
        assert_eq!(port.observe_calls(), observe_calls + 1);
        assert_eq!(production.terminal_observation_v2().await, expected_exit);
        assert!(invalidated.load(Ordering::Acquire));
        assert_eq!(port.release_calls(), 1);
    }
}

#[tokio::test]
async fn certification_thaw_send_queue_stall_is_bounded_by_the_authority_cutoff() {
    let process_generation = NonZeroU64::new(72).unwrap();
    let (production, port, invalidated, _) =
        certification_production_fixture([], process_generation).await;
    let cutoff = Instant::now() + Duration::from_millis(300);
    let authority = production.prepare_certification_freeze_v2(cutoff).unwrap();
    let (frozen, observation) = authority.freeze_v2().await.unwrap();
    let renew_calls = port.renew_calls();
    let sender = frozen.inner().supervisor_commands.clone();
    let permit = sender.reserve_owned().await.unwrap();
    let started_at = Instant::now();

    assert!(matches!(
        frozen
            .thaw_v2(observation, Instant::now() + Duration::from_secs(1))
            .await,
        Err(RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed)
    ));
    assert!(Instant::now() >= cutoff);
    assert!(started_at.elapsed() < Duration::from_millis(600));
    assert_eq!(port.renew_calls(), renew_calls);
    assert!(invalidated.load(Ordering::Acquire));

    drop(permit);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn certification_thaw_send_queue_uses_the_earlier_successor_deadline() {
    let process_generation = NonZeroU64::new(75).unwrap();
    let (production, port, invalidated, _) =
        certification_production_fixture([], process_generation).await;
    let cutoff = Instant::now() + Duration::from_millis(600);
    let authority = production.prepare_certification_freeze_v2(cutoff).unwrap();
    let (frozen, observation) = authority.freeze_v2().await.unwrap();
    let renew_calls = port.renew_calls();
    let sender = frozen.inner().supervisor_commands.clone();
    let permit = sender.reserve_owned().await.unwrap();
    let successor_deadline = Instant::now() + Duration::from_millis(100);
    let started_at = Instant::now();

    assert!(matches!(
        frozen.thaw_v2(observation, successor_deadline).await,
        Err(RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed)
    ));
    assert!(Instant::now() >= successor_deadline);
    assert!(Instant::now() < cutoff);
    assert!(started_at.elapsed() < Duration::from_millis(350));
    assert_eq!(port.renew_calls(), renew_calls);
    assert!(invalidated.load(Ordering::Acquire));

    drop(permit);
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn late_certification_thaw_command_never_restores_production_generation() {
    let process_generation = NonZeroU64::new(76).unwrap();
    let (production, port, invalidated, _) =
        certification_production_fixture([], process_generation).await;
    let cutoff = Instant::now() + Duration::from_millis(600);
    let authority = production.prepare_certification_freeze_v2(cutoff).unwrap();
    let (frozen, observation) = authority.freeze_v2().await.unwrap();
    let renew_calls = port.renew_calls();
    let completion_deadline = Instant::now() + Duration::from_millis(80);
    sleep_until(TokioInstant::from_std(completion_deadline)).await;
    let (response, acknowledgement) = oneshot::channel();
    frozen
        .inner()
        .supervisor_commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::ThawCertification {
            authority: observation,
            completion_deadline,
            response,
        })
        .await
        .unwrap();

    assert_eq!(
        acknowledgement.await.unwrap(),
        Err(RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed)
    );
    assert_eq!(
        frozen.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::SafetyElapsed
    );
    assert_eq!(
        frozen.inner().production_generation.load(Ordering::Acquire),
        0
    );
    assert_eq!(port.renew_calls(), renew_calls);
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn certification_thaw_strict_successor_stall_is_bounded_by_the_authority_cutoff() {
    let process_generation = NonZeroU64::new(74).unwrap();
    let (production, port, invalidated, _) =
        certification_production_fixture([], process_generation).await;
    let cutoff = Instant::now() + Duration::from_millis(300);
    let authority = production.prepare_certification_freeze_v2(cutoff).unwrap();
    let (frozen, observation) = authority.freeze_v2().await.unwrap();
    let gate = Arc::new(Notify::new());
    port.state
        .renew_steps
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push_back(FakeRenewStepV1::Blocked(gate.clone()));
    let started_at = Instant::now();

    assert!(matches!(
        frozen
            .thaw_v2(observation, Instant::now() + Duration::from_secs(1))
            .await,
        Err(RuntimeGatewayOwnerCertificationThawErrorV2::DeadlineElapsed)
    ));
    assert!(Instant::now() >= cutoff);
    assert!(started_at.elapsed() < Duration::from_millis(600));
    assert!(invalidated.load(Ordering::Acquire));

    gate.notify_one();
    wait_for(|| port.release_calls() == 1).await;
}

#[tokio::test]
async fn stale_certification_thaw_authority_is_fail_closed() {
    let process_generation = NonZeroU64::new(73).unwrap();
    let (production, port, invalidated, _) =
        certification_production_fixture([], process_generation).await;
    let authority = production
        .prepare_certification_freeze_v2(Instant::now() + Duration::from_secs(1))
        .unwrap();
    let (frozen, mut observation) = authority.freeze_v2().await.unwrap();
    observation.observation.receipt.owner_revision = NonZeroU64::new(
        observation
            .observation
            .receipt
            .owner_revision
            .get()
            .checked_add(1)
            .unwrap(),
    )
    .unwrap();
    let terminal = frozen.terminal_observation_v2();

    assert!(matches!(
        frozen
            .thaw_v2(observation, Instant::now() + Duration::from_millis(500),)
            .await,
        Err(RuntimeGatewayOwnerCertificationThawErrorV2::StaleAuthority)
    ));
    assert_eq!(
        terminal.await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::Shutdown
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn lost_certification_freeze_acknowledgement_is_terminal_and_fail_closed() {
    let gate = Arc::new(Notify::new());
    let process_generation = NonZeroU64::new(79).unwrap();
    let (production, port, invalidated, _) = certification_production_fixture(
        [FakeRenewStepV1::Blocked(gate.clone())],
        process_generation,
    )
    .await;
    wait_for(|| port.renew_calls() == 1).await;
    let expected_observation = production.inner().current_observation.borrow().clone();
    let cutoff = Instant::now() + Duration::from_secs(1);
    let (response, acknowledgement) = oneshot::channel();
    drop(acknowledgement);
    production
        .inner()
        .supervisor_commands
        .send(
            RuntimeGatewayOwnerSupervisorCommandV1::FreezeCertification {
                expected_observation,
                process_generation,
                cutoff,
                response,
            },
        )
        .await
        .unwrap();
    let terminal = production.terminal_observation_v2();

    gate.notify_one();

    assert_eq!(
        terminal.await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert!(invalidated.load(Ordering::Acquire));
    assert_eq!(port.renew_calls(), 1);
    assert_eq!(port.release_calls(), 1);
}

#[tokio::test]
async fn lost_certification_thaw_acknowledgement_is_terminal_before_renewal() {
    let process_generation = NonZeroU64::new(83).unwrap();
    let (production, port, invalidated, _) =
        certification_production_fixture([], process_generation).await;
    let authority = production
        .prepare_certification_freeze_v2(Instant::now() + Duration::from_secs(1))
        .unwrap();
    let (frozen, observation) = authority.freeze_v2().await.unwrap();
    let completion_deadline = observation.cutoff_v2();
    let renew_calls = port.renew_calls();
    let (response, acknowledgement) = oneshot::channel();
    drop(acknowledgement);
    frozen
        .inner()
        .supervisor_commands
        .send(RuntimeGatewayOwnerSupervisorCommandV1::ThawCertification {
            authority: observation,
            completion_deadline,
            response,
        })
        .await
        .unwrap();

    assert_eq!(
        frozen.terminal_observation_v2().await,
        RuntimeGatewayOwnerStartupWatchdogExitV1::ProtocolViolation
    );
    assert_eq!(port.renew_calls(), renew_calls);
    assert!(invalidated.load(Ordering::Acquire));
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
