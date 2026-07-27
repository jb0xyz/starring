use std::num::NonZeroU64;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2,
    RuntimeRecoveryIdV2, RuntimeStartupRecoveryObservationReceiptV2,
    RuntimeStartupRecoveryObservationRequestV2, RuntimeStartupRecoveryStateV2,
    RuntimeStartupServingStateV2,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use super::{
    RuntimeGatewayClosedLifecycleV2, RuntimeGatewayClosedSnapshotV2,
    RuntimeGatewayClosedTransitionErrorV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimeGatewayEmergencyCauseV2, RuntimeGatewayInvalidationCauseV2,
};
use crate::{
    accept_runtime_registry_recovery_empty_observation_v2, RuntimeAcceptedStartupRecoveryOutcomeV2,
    RuntimeAuthorizedStartupRecoveryIterationV2, RuntimeAuthorizedStartupRecoveryObservationV2,
    RuntimeCapabilityReadinessKindV2, RuntimeCapabilityReadinessReceiptV2,
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimePausedGatewayObservationV2, RuntimePausedGatewaySequenceV2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryObservationInputV2,
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryContinuationV2,
    RuntimeStartupRecoveryObservationAcceptanceErrorV2,
};

pub(super) fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

pub(super) fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn readiness(
    database_identity: &str,
    database_name: &str,
    roles: [&str; 5],
    checked_at: i64,
) -> RuntimeCapabilityReadinessSetV2 {
    readiness_with_checks(
        database_identity,
        database_name,
        roles,
        [
            checked_at,
            checked_at + 1,
            checked_at + 2,
            checked_at + 3,
            checked_at + 4,
        ],
    )
}

fn readiness_with_checks(
    database_identity: &str,
    database_name: &str,
    roles: [&str; 5],
    checked_at: [i64; 5],
) -> RuntimeCapabilityReadinessSetV2 {
    let receipt = |kind, role, checked_at| {
        RuntimeCapabilityReadinessReceiptV2::new(
            kind,
            database_identity,
            database_name,
            role,
            at(checked_at),
        )
        .unwrap()
    };
    RuntimeCapabilityReadinessSetV2::new(
        receipt(
            RuntimeCapabilityReadinessKindV2::Convergence,
            roles[0],
            checked_at[0],
        ),
        receipt(
            RuntimeCapabilityReadinessKindV2::ExactTarget,
            roles[1],
            checked_at[1],
        ),
        receipt(
            RuntimeCapabilityReadinessKindV2::Panel,
            roles[2],
            checked_at[2],
        ),
        receipt(
            RuntimeCapabilityReadinessKindV2::Serving,
            roles[3],
            checked_at[3],
        ),
        receipt(
            RuntimeCapabilityReadinessKindV2::Interaction,
            roles[4],
            checked_at[4],
        ),
    )
    .unwrap()
}

pub(super) fn current_readiness(checked_at: i64) -> RuntimeCapabilityReadinessSetV2 {
    readiness(
        "01234567-89ab-cdef-8123-456789abcdef",
        "starring",
        ["role_a", "role_b", "role_c", "role_d", "role_e"],
        checked_at,
    )
}

fn recovery_input(
    generation: RuntimeGatewayCoordinatorGenerationV2,
) -> RuntimeClosedRecoveryInputV2 {
    let process = ProcessInstanceId::parse("process:1").unwrap();
    let paused = RuntimePausedGatewayObservationV2::new(
        generation,
        process.clone(),
        non_zero(2),
        RuntimeGatewayReadyKindV2::Ready,
        non_zero(3),
        RuntimePausedGatewaySequenceV2::new(
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(5)),
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(4)),
            None,
        )
        .unwrap(),
    );
    let registry = accept_runtime_registry_recovery_empty_observation_v2(
        process.clone(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(non_zero(6)),
            retained_slot_count: 0,
            retained_empty_tombstone_count: 0,
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
    RuntimeClosedRecoveryInputV2::new(
        RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
        RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: process,
                lease_epoch: non_zero(7),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            owner_revision: non_zero(8),
            database_now: at(100),
            expires_at: at(200),
        },
        current_readiness(100),
        paused,
        RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry),
    )
}

pub(super) fn begin_recovery() -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
) {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let generation = lifecycle.snapshot().generation();
    let (_, permit) = lifecycle
        .begin_recovery(generation, recovery_input(generation))
        .unwrap();
    (lifecycle, permit)
}

fn refresh_startup_iteration(
    lifecycle: &mut RuntimeGatewayClosedLifecycleV2,
    permit: &mut RuntimeClosedDrainRecoveryPermitV2,
    checked_at: i64,
) -> RuntimeAuthorizedStartupRecoveryIterationV2 {
    lifecycle
        .refresh_recovery_readiness(permit, current_readiness(checked_at))
        .unwrap()
}

pub(super) fn begin_startup_observation(
    lifecycle: &mut RuntimeGatewayClosedLifecycleV2,
    permit: &mut RuntimeClosedDrainRecoveryPermitV2,
    checked_at: i64,
) -> RuntimeAuthorizedStartupRecoveryObservationV2 {
    let iteration = refresh_startup_iteration(lifecycle, permit, checked_at);
    lifecycle
        .begin_startup_recovery_observation(permit, iteration)
        .unwrap()
}

pub(super) fn empty_startup_state() -> RuntimeStartupRecoveryStateV2 {
    RuntimeStartupRecoveryStateV2 {
        serving: RuntimeStartupServingStateV2::Empty,
        recoverable_awaiting_certification_count: 0,
        suspended_local_effect_count: 0,
        pending_runtime_drain_intent_count: 0,
        acknowledged_product_handoff_count: 0,
    }
}

pub(super) fn startup_observation_receipt(
    request: &RuntimeStartupRecoveryObservationRequestV2,
    database_now: DateTime<Utc>,
    state: RuntimeStartupRecoveryStateV2,
) -> RuntimeStartupRecoveryObservationReceiptV2 {
    RuntimeStartupRecoveryObservationReceiptV2 {
        correlation: request.correlation.clone(),
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: request.gateway_owner_lease_id.clone(),
            owner_revision: request.expected_owner_revision,
            database_now,
            expires_at: request.expected_owner_expires_at,
        },
        state,
    }
}

pub(super) fn complete_startup_observation(
    authorization: RuntimeAuthorizedStartupRecoveryObservationV2,
    database_now: DateTime<Utc>,
    state: RuntimeStartupRecoveryStateV2,
) -> crate::RuntimeCompletedStartupRecoveryObservationV2 {
    let receipt = startup_observation_receipt(authorization.request(), database_now, state);
    authorization.complete(receipt)
}

fn assert_startup_observation_rejected(
    mutate: impl FnOnce(&mut RuntimeStartupRecoveryObservationReceiptV2),
    expected: RuntimeStartupRecoveryObservationAcceptanceErrorV2,
    cause: RuntimeGatewayEmergencyCauseV2,
) {
    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    let authorization = begin_startup_observation(&mut lifecycle, &mut permit, 200);
    let mut receipt =
        startup_observation_receipt(authorization.request(), at(101), empty_startup_state());
    mutate(&mut receipt);
    let completed = authorization.complete(receipt);

    assert_eq!(
        lifecycle.complete_startup_recovery_observation(&mut permit, completed),
        Err(RuntimeGatewayClosedTransitionErrorV2::StartupRecoveryObservation(expected))
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: RuntimeGatewayCoordinatorGenerationV2::new(non_zero(generation.get() + 1)),
            cause,
        }
    );
    assert_eq!(permit.authority_revision().get(), 2);
    assert_eq!(
        lifecycle.validate_recovery_permit(&permit),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit)
    );
}

fn observe_startup_decision(
    state: RuntimeStartupRecoveryStateV2,
) -> RuntimeAcceptedStartupRecoveryOutcomeV2 {
    let (mut lifecycle, mut permit) = begin_recovery();
    let authorization = begin_startup_observation(&mut lifecycle, &mut permit, 200);
    let completed = complete_startup_observation(authorization, at(101), state);
    lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
}

#[test]
fn startup_observation_advances_exact_authority_and_preserves_closed_evidence() {
    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    let recovery_id = permit.recovery_id().clone();
    let owner = permit.owner_receipt().clone();
    let paused = permit.paused_gateway().clone();
    let registry_sequence = permit
        .registry_evidence()
        .empty_observation()
        .observation_sequence();
    let registry_retained = permit
        .registry_evidence()
        .empty_observation()
        .retained_slot_count();
    let iteration = refresh_startup_iteration(&mut lifecycle, &mut permit, 200);
    assert_eq!(
        format!("{iteration:?}"),
        "RuntimeAuthorizedStartupRecoveryIterationV2(<redacted>)"
    );
    let readiness = permit.readiness().clone();
    let authorization = lifecycle
        .begin_startup_recovery_observation(&mut permit, iteration)
        .unwrap();

    assert_eq!(authorization.request().correlation.recovery_id, recovery_id);
    assert_eq!(
        authorization
            .request()
            .correlation
            .originating_emergency_generation,
        non_zero(1)
    );
    assert_eq!(
        authorization.request().correlation.coordinator_generation,
        non_zero(generation.get())
    );
    assert_eq!(
        authorization.request().correlation.authority_revision,
        non_zero(2)
    );
    assert_eq!(
        authorization.request().gateway_owner_lease_id,
        owner.lease_id
    );
    assert_eq!(
        authorization.request().expected_owner_revision,
        owner.owner_revision
    );
    assert_eq!(
        format!("{authorization:?}"),
        "RuntimeAuthorizedStartupRecoveryObservationV2(<redacted>)"
    );
    let mut state = empty_startup_state();
    state.acknowledged_product_handoff_count = 7;
    let completed = complete_startup_observation(authorization, at(101), state);
    assert_eq!(
        format!("{completed:?}"),
        "RuntimeCompletedStartupRecoveryObservationV2(<redacted>)"
    );

    let RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(fixed_point) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("expected startup recovery fixed point")
    };

    assert_eq!(fixed_point.acknowledged_product_handoff_count(), 7);
    assert_eq!(fixed_point.successor_authority_revision().get(), 3);
    assert_eq!(
        format!("{fixed_point:?}"),
        "RuntimeStartupRecoveryFixedPointProofV2(<redacted>)"
    );
    assert_eq!(permit.authority_revision().get(), 3);
    assert_eq!(permit.owner_receipt(), &owner);
    assert_eq!(permit.readiness(), &readiness);
    assert_eq!(permit.paused_gateway(), &paused);
    assert_eq!(
        permit
            .registry_evidence()
            .empty_observation()
            .observation_sequence(),
        registry_sequence
    );
    assert_eq!(
        permit
            .registry_evidence()
            .empty_observation()
            .retained_slot_count(),
        registry_retained
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id,
            authority_revision: permit.authority_revision(),
        }
    );
    assert_eq!(lifecycle.validate_recovery_permit(&permit), Ok(()));
    assert_eq!(
        lifecycle.validate_startup_recovery_fixed_point(&permit, &fixed_point),
        Ok(())
    );
}

#[test]
fn fixed_point_proof_escrows_the_iteration_authority_until_closed_handoff() {
    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    let authorization = begin_startup_observation(&mut lifecycle, &mut permit, 200);
    let completed = complete_startup_observation(authorization, at(101), empty_startup_state());
    let RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(fixed_point) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("expected startup recovery fixed point")
    };

    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut permit, current_readiness(300)),
        Err(RuntimeGatewayClosedTransitionErrorV2::RecoveryOperationInFlight)
    );
    let emergency = RuntimeGatewayClosedSnapshotV2::Emergency {
        generation: RuntimeGatewayCoordinatorGenerationV2::new(non_zero(generation.get() + 1)),
        cause: RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    };
    assert_eq!(lifecycle.snapshot(), emergency);
    assert_eq!(
        lifecycle.validate_startup_recovery_fixed_point(&permit, &fixed_point),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit)
    );
    assert_eq!(lifecycle.snapshot(), emergency);
}

#[test]
fn startup_observation_forwards_every_planner_decision_without_minting_resume() {
    let mut stale = empty_startup_state();
    stale.serving = RuntimeStartupServingStateV2::RecoverableStale { count: 2 };
    assert_eq!(
        observe_startup_decision(stale),
        RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(
            RuntimeStartupRecoveryContinuationV2::Recover(RuntimeStartupRecoveryClassV2::StaleLive)
        )
    );

    let mut awaiting = empty_startup_state();
    awaiting.recoverable_awaiting_certification_count = 1;
    assert_eq!(
        observe_startup_decision(awaiting),
        RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(
            RuntimeStartupRecoveryContinuationV2::Recover(
                RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification
            )
        )
    );

    let mut suspended = empty_startup_state();
    suspended.suspended_local_effect_count = 1;
    assert_eq!(
        observe_startup_decision(suspended),
        RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(
            RuntimeStartupRecoveryContinuationV2::Recover(
                RuntimeStartupRecoveryClassV2::SuspendedLocalEffect
            )
        )
    );

    let mut drain = empty_startup_state();
    drain.pending_runtime_drain_intent_count = 1;
    assert_eq!(
        observe_startup_decision(drain),
        RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(
            RuntimeStartupRecoveryContinuationV2::Recover(
                RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent
            )
        )
    );

    let mut foreign = empty_startup_state();
    foreign.serving = RuntimeStartupServingStateV2::ForeignFresh {
        count: 2,
        database_now: at(101),
        earliest_expiry: at(110),
        retry_after: std::time::Duration::from_secs(5),
    };
    assert_eq!(
        observe_startup_decision(foreign),
        RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(
            RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh {
                retry_after: std::time::Duration::from_secs(5),
            }
        )
    );

    assert!(matches!(
        observe_startup_decision(empty_startup_state()),
        RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(_)
    ));
}

#[test]
fn startup_observation_token_blocks_overlap_and_is_lost_on_drop() {
    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    let iteration = refresh_startup_iteration(&mut lifecycle, &mut permit, 200);

    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut permit, current_readiness(300)),
        Err(RuntimeGatewayClosedTransitionErrorV2::RecoveryOperationInFlight)
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: RuntimeGatewayCoordinatorGenerationV2::new(non_zero(generation.get() + 1)),
            cause: RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        }
    );
    drop(iteration);

    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    let authorization = begin_startup_observation(&mut lifecycle, &mut permit, 200);
    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut permit, current_readiness(200)),
        Err(RuntimeGatewayClosedTransitionErrorV2::RecoveryOperationInFlight)
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: RuntimeGatewayCoordinatorGenerationV2::new(non_zero(generation.get() + 1)),
            cause: RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        }
    );
    drop(authorization);

    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    let iteration = refresh_startup_iteration(&mut lifecycle, &mut permit, 200);
    drop(iteration);
    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut permit, current_readiness(300)),
        Err(RuntimeGatewayClosedTransitionErrorV2::RecoveryOperationInFlight)
    );
    assert_eq!(
        lifecycle.snapshot().generation(),
        RuntimeGatewayCoordinatorGenerationV2::new(non_zero(generation.get() + 1))
    );
}

#[test]
fn startup_observation_rejects_every_correlation_and_owner_mismatch() {
    assert_startup_observation_rejected(
        |receipt| {
            receipt.correlation.recovery_id =
                RuntimeRecoveryIdV2::parse("fedcba9876543210fedcba9876543210").unwrap();
        },
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::CorrelationMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_startup_observation_rejected(
        |receipt| receipt.correlation.originating_emergency_generation = non_zero(9),
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::CorrelationMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_startup_observation_rejected(
        |receipt| receipt.correlation.coordinator_generation = non_zero(9),
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::CorrelationMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_startup_observation_rejected(
        |receipt| receipt.correlation.authority_revision = non_zero(9),
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::CorrelationMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_startup_observation_rejected(
        |receipt| {
            receipt.owner_receipt.lease_id.gateway_shard_id =
                GatewayShardIdV1::parse("shard:1").unwrap();
        },
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerMismatch,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
    assert_startup_observation_rejected(
        |receipt| {
            receipt.owner_receipt.lease_id.process_instance_id =
                ProcessInstanceId::parse("process:2").unwrap();
        },
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerMismatch,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
    assert_startup_observation_rejected(
        |receipt| receipt.owner_receipt.lease_id.lease_epoch = non_zero(9),
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerMismatch,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
    assert_startup_observation_rejected(
        |receipt| {
            receipt.owner_receipt.lease_id.expected_build_revision =
                RuntimeBuildRevisionV1::parse("build:2").unwrap();
        },
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerMismatch,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
    assert_startup_observation_rejected(
        |receipt| receipt.owner_receipt.owner_revision = non_zero(9),
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerMismatch,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
    assert_startup_observation_rejected(
        |receipt| receipt.owner_receipt.expires_at = at(201),
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerMismatch,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
}

#[test]
fn startup_observation_rejects_clock_state_and_foreign_time_failures() {
    assert_startup_observation_rejected(
        |receipt| receipt.owner_receipt.database_now = at(99),
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::DatabaseClockRegressed,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_startup_observation_rejected(
        |receipt| receipt.owner_receipt.database_now = at(200),
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::OwnerNotCurrent,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
    assert_startup_observation_rejected(
        |receipt| {
            receipt.state.serving = RuntimeStartupServingStateV2::ForeignFresh {
                count: 1,
                database_now: at(102),
                earliest_expiry: at(110),
                retry_after: std::time::Duration::from_secs(5),
            };
        },
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::DatabaseTimeMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_startup_observation_rejected(
        |receipt| receipt.state.serving = RuntimeStartupServingStateV2::Ambiguous,
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::Ambiguous,
        RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
    );
    assert_startup_observation_rejected(
        |receipt| {
            receipt.state.serving = RuntimeStartupServingStateV2::RecoverableStale { count: 0 };
        },
        RuntimeStartupRecoveryObservationAcceptanceErrorV2::InvalidObservation,
        RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
    );
}

#[test]
fn startup_observation_database_clock_is_monotonic_across_successors() {
    let (mut lifecycle, mut permit) = begin_recovery();
    for (checked_at, database_now, expected_revision) in [(200, 101, 3), (300, 102, 5)] {
        let authorization = begin_startup_observation(&mut lifecycle, &mut permit, checked_at);
        let mut state = empty_startup_state();
        state.serving = RuntimeStartupServingStateV2::ForeignFresh {
            count: 1,
            database_now: at(database_now),
            earliest_expiry: at(150),
            retry_after: std::time::Duration::from_secs(1),
        };
        let completed = complete_startup_observation(authorization, at(database_now), state);
        let outcome = lifecycle
            .complete_startup_recovery_observation(&mut permit, completed)
            .unwrap();
        assert_eq!(
            outcome,
            RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(
                RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh {
                    retry_after: std::time::Duration::from_secs(1),
                },
            )
        );
        assert_eq!(permit.authority_revision().get(), expected_revision);
    }
    let authorization = begin_startup_observation(&mut lifecycle, &mut permit, 400);
    let completed = complete_startup_observation(authorization, at(100), empty_startup_state());
    assert_eq!(
        lifecycle.complete_startup_recovery_observation(&mut permit, completed),
        Err(
            RuntimeGatewayClosedTransitionErrorV2::StartupRecoveryObservation(
                RuntimeStartupRecoveryObservationAcceptanceErrorV2::DatabaseClockRegressed
            )
        )
    );
}

#[test]
fn stale_startup_completion_cannot_disturb_a_newer_closed_state() {
    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    let authorization = begin_startup_observation(&mut lifecycle, &mut permit, 200);
    let completed = complete_startup_observation(authorization, at(101), empty_startup_state());
    lifecycle
        .invalidate(
            generation,
            RuntimeGatewayInvalidationCauseV2::TransportDisconnected,
        )
        .unwrap();
    let newer = lifecycle.snapshot();

    assert_eq!(
        lifecycle.complete_startup_recovery_observation(&mut permit, completed),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit)
    );
    assert_eq!(lifecycle.snapshot(), newer);
}

#[test]
fn startup_observation_authority_overflow_is_terminal() {
    let (mut lifecycle, mut permit) = begin_recovery();
    permit.prepare_authority_revision_overflow_for_test();
    let generation = permit.coordinator_generation();
    let recovery_id = permit.recovery_id().clone();
    let authority_revision = permit.authority_revision();
    lifecycle.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending {
        generation,
        recovery_id,
        authority_revision,
    };
    let iteration = lifecycle
        .refresh_recovery_readiness(&mut permit, current_readiness(200))
        .unwrap();
    let authority_revision = permit.authority_revision();
    assert_eq!(authority_revision.get(), i64::MAX as u64);
    let authorization = lifecycle
        .begin_startup_recovery_observation(&mut permit, iteration)
        .unwrap();
    let completed = complete_startup_observation(authorization, at(101), empty_startup_state());

    assert_eq!(
        lifecycle.complete_startup_recovery_observation(&mut permit, completed),
        Err(RuntimeGatewayClosedTransitionErrorV2::AuthorityRevisionOverflow)
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Shutdown { generation }
    );
    assert_eq!(permit.authority_revision(), authority_revision);
}

#[test]
fn overflow_becomes_terminally_closed() {
    let maximum = RuntimeGatewayCoordinatorGenerationV2::new(NonZeroU64::MAX);
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2 {
        snapshot: RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: maximum,
            cause: RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        },
    };

    assert_eq!(
        lifecycle.invalidate(
            maximum,
            RuntimeGatewayInvalidationCauseV2::CapabilityNotReady,
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow)
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Shutdown {
            generation: maximum,
        }
    );
    assert_eq!(
        lifecycle.invalidate(maximum, RuntimeGatewayInvalidationCauseV2::ControlOrphaned),
        Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown)
    );

    let mut shutdown = RuntimeGatewayClosedLifecycleV2 {
        snapshot: RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: maximum,
            cause: RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
        },
    };
    assert_eq!(
        shutdown.shutdown(maximum),
        Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow)
    );
    assert_eq!(
        shutdown.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Shutdown {
            generation: maximum,
        }
    );
}

#[test]
fn recovery_successor_is_bounded_and_generation_overflow_is_terminal() {
    let persistence_predecessor =
        RuntimeGatewayCoordinatorGenerationV2::new(non_zero(i64::MAX as u64 - 1));
    let mut persistence_boundary = RuntimeGatewayClosedLifecycleV2 {
        snapshot: RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: persistence_predecessor,
            cause: RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        },
    };
    let (boundary, _) = persistence_boundary
        .begin_recovery(
            persistence_predecessor,
            recovery_input(persistence_predecessor),
        )
        .unwrap();
    assert_eq!(boundary.generation().get(), i64::MAX as u64);

    let persistence_max = RuntimeGatewayCoordinatorGenerationV2::new(non_zero(i64::MAX as u64));
    let mut persistence_bounded = RuntimeGatewayClosedLifecycleV2 {
        snapshot: RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: persistence_max,
            cause: RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        },
    };

    assert_eq!(
        persistence_bounded.begin_recovery(persistence_max, recovery_input(persistence_max)),
        Err(RuntimeGatewayClosedTransitionErrorV2::EvidenceSequenceOutOfRange)
    );
    assert_eq!(
        persistence_bounded.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: persistence_max,
            cause: RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        }
    );

    let maximum = RuntimeGatewayCoordinatorGenerationV2::new(NonZeroU64::MAX);
    let mut exhausted = RuntimeGatewayClosedLifecycleV2 {
        snapshot: RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: maximum,
            cause: RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        },
    };
    assert_eq!(
        exhausted.begin_recovery(maximum, recovery_input(maximum)),
        Err(RuntimeGatewayClosedTransitionErrorV2::GenerationOverflow)
    );
    assert_eq!(
        exhausted.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Shutdown {
            generation: maximum,
        }
    );
}

#[test]
fn readiness_refresh_advances_once_and_preserves_all_other_evidence() {
    let (mut lifecycle, mut permit) = begin_recovery();
    let (_, predecessor) = begin_recovery();
    let originating_generation = permit.originating_emergency_generation();
    let coordinator_generation = permit.coordinator_generation();
    let recovery_id = permit.recovery_id().clone();
    let owner_receipt = permit.owner_receipt().clone();
    let paused_gateway = permit.paused_gateway().clone();
    let registry = permit.registry_evidence().empty_observation();
    let registry_process = registry.process_instance_id().clone();
    let registry_sequence = registry.observation_sequence();
    let retained_slot_count = registry.retained_slot_count();
    let retained_tombstone_count = registry.retained_empty_tombstone_count();

    let iteration = lifecycle
        .refresh_recovery_readiness(&mut permit, current_readiness(200))
        .unwrap();
    assert_eq!(
        format!("{iteration:?}"),
        "RuntimeAuthorizedStartupRecoveryIterationV2(<redacted>)"
    );
    assert_eq!(permit.authority_revision().get(), 2);
    assert_eq!(permit.readiness().checked_at_bounds(), (at(200), at(204)));
    assert_eq!(
        permit.originating_emergency_generation(),
        originating_generation
    );
    assert_eq!(permit.coordinator_generation(), coordinator_generation);
    assert_eq!(permit.recovery_id(), &recovery_id);
    assert_eq!(permit.owner_receipt(), &owner_receipt);
    assert_eq!(permit.paused_gateway(), &paused_gateway);
    assert_eq!(
        permit
            .registry_evidence()
            .empty_observation()
            .process_instance_id(),
        &registry_process
    );
    assert_eq!(
        permit
            .registry_evidence()
            .empty_observation()
            .observation_sequence(),
        registry_sequence
    );
    assert_eq!(
        permit
            .registry_evidence()
            .empty_observation()
            .retained_slot_count(),
        retained_slot_count
    );
    assert_eq!(
        permit
            .registry_evidence()
            .empty_observation()
            .retained_empty_tombstone_count(),
        retained_tombstone_count
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation: coordinator_generation,
            recovery_id: recovery_id.clone(),
            authority_revision: permit.authority_revision(),
        }
    );
    assert_eq!(
        lifecycle.validate_recovery_permit(&predecessor),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit)
    );

    assert_eq!(lifecycle.validate_recovery_permit(&permit), Ok(()));
    drop(iteration);
}

#[test]
fn stale_readiness_refresh_cannot_disturb_current_successor() {
    let (mut lifecycle, mut current) = begin_recovery();
    let (_, mut predecessor) = begin_recovery();
    lifecycle
        .refresh_recovery_readiness(&mut current, current_readiness(200))
        .unwrap();
    let successor_snapshot = lifecycle.snapshot();
    let predecessor_bounds = predecessor.readiness().checked_at_bounds();

    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut predecessor, current_readiness(300)),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit)
    );
    assert_eq!(lifecycle.snapshot(), successor_snapshot);
    assert_eq!(lifecycle.validate_recovery_permit(&current), Ok(()));
    assert_eq!(predecessor.authority_revision().get(), 1);
    assert_eq!(
        predecessor.readiness().checked_at_bounds(),
        predecessor_bounds
    );
}

#[test]
fn replayed_or_regressed_readiness_fails_closed_without_replacing_evidence() {
    let identity = "01234567-89ab-cdef-8123-456789abcdef";
    let roles = ["role_a", "role_b", "role_c", "role_d", "role_e"];
    let rejected = [
        current_readiness(100),
        readiness_with_checks(identity, "starring", roles, [200, 201, 101, 203, 204]),
    ];

    for readiness in rejected {
        let (mut lifecycle, mut permit) = begin_recovery();
        let recovery_generation = permit.coordinator_generation();
        let readiness_bounds = permit.readiness().checked_at_bounds();

        assert_eq!(
            lifecycle.refresh_recovery_readiness(&mut permit, readiness),
            Err(RuntimeGatewayClosedTransitionErrorV2::CapabilityReadinessNotSuccessor)
        );
        assert_eq!(
            lifecycle.snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                generation: RuntimeGatewayCoordinatorGenerationV2::new(non_zero(
                    recovery_generation.get() + 1,
                )),
                cause: RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            }
        );
        assert_eq!(permit.authority_revision().get(), 1);
        assert_eq!(permit.readiness().checked_at_bounds(), readiness_bounds);
        assert_eq!(
            lifecycle.validate_recovery_permit(&permit),
            Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit)
        );
    }
}

#[test]
fn readiness_authority_mismatch_fails_closed_for_every_authority_field() {
    let identity = "01234567-89ab-cdef-8123-456789abcdef";
    let roles = ["role_a", "role_b", "role_c", "role_d", "role_e"];
    let mismatches = [
        readiness(
            "11234567-89ab-cdef-8123-456789abcdef",
            "starring",
            roles,
            200,
        ),
        readiness(identity, "starring_other", roles, 200),
        readiness(
            identity,
            "starring",
            ["role_f", "role_b", "role_c", "role_d", "role_e"],
            200,
        ),
        readiness(
            identity,
            "starring",
            ["role_a", "role_f", "role_c", "role_d", "role_e"],
            200,
        ),
        readiness(
            identity,
            "starring",
            ["role_a", "role_b", "role_f", "role_d", "role_e"],
            200,
        ),
        readiness(
            identity,
            "starring",
            ["role_a", "role_b", "role_c", "role_f", "role_e"],
            200,
        ),
        readiness(
            identity,
            "starring",
            ["role_a", "role_b", "role_c", "role_d", "role_f"],
            200,
        ),
    ];

    for mismatch in mismatches {
        let (mut lifecycle, mut permit) = begin_recovery();
        let recovery_generation = permit.coordinator_generation();

        assert_eq!(
            lifecycle.refresh_recovery_readiness(&mut permit, mismatch),
            Err(RuntimeGatewayClosedTransitionErrorV2::CapabilityReadinessAuthorityMismatch)
        );
        assert_eq!(
            lifecycle.snapshot(),
            RuntimeGatewayClosedSnapshotV2::Emergency {
                generation: RuntimeGatewayCoordinatorGenerationV2::new(non_zero(
                    recovery_generation.get() + 1,
                )),
                cause: RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            }
        );
        assert_eq!(
            lifecycle.validate_recovery_permit(&permit),
            Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit)
        );
        assert_eq!(permit.authority_revision().get(), 1);
        assert_eq!(permit.readiness().checked_at_bounds(), (at(100), at(104)));
    }
}

#[test]
fn readiness_authority_revision_overflow_is_terminal_and_nonmutating() {
    let (mut lifecycle, mut permit) = begin_recovery();
    permit.exhaust_authority_revision_for_test();
    let generation = permit.coordinator_generation();
    let recovery_id = permit.recovery_id().clone();
    let authority_revision = permit.authority_revision();
    assert_eq!(authority_revision.get(), i64::MAX as u64);
    lifecycle.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending {
        generation,
        recovery_id,
        authority_revision,
    };
    let readiness_bounds = permit.readiness().checked_at_bounds();

    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut permit, current_readiness(200)),
        Err(RuntimeGatewayClosedTransitionErrorV2::AuthorityRevisionOverflow)
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Shutdown { generation }
    );
    assert_eq!(permit.authority_revision(), authority_revision);
    assert_eq!(permit.readiness().checked_at_bounds(), readiness_bounds);
    assert_eq!(
        lifecycle.validate_recovery_permit(&permit),
        Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown)
    );
}
