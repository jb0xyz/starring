use std::num::NonZeroU64;
use std::time::Duration;

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2, RuntimeRecoveryIdV2,
    RuntimeStartupRecoveryStateV2, RuntimeStartupServingStateV2,
};

use super::tests::{
    at, begin_recovery, begin_startup_observation, complete_startup_observation, current_readiness,
    empty_startup_state, non_zero,
};
use super::{
    RuntimeGatewayClosedLifecycleV2, RuntimeGatewayClosedSnapshotV2,
    RuntimeGatewayClosedTransitionErrorV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimeGatewayEmergencyCauseV2, RuntimeGatewayInvalidationCauseV2,
};
use crate::{
    accept_runtime_registry_recovery_empty_observation_v2, RuntimeAcceptedStartupRecoveryOutcomeV2,
    RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryRegistryEvidenceV2, RuntimeCompletedStartupRecoveryExecutionV2,
    RuntimePausedGatewayObservationV2, RuntimePausedGatewaySequenceV2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryObservationInputV2,
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryContinuationV2,
    RuntimeStartupRecoveryExecutionAcceptanceErrorV2, RuntimeStartupRecoveryExecutionDigestErrorV2,
    RuntimeStartupRecoveryExecutionReceiptOutcomeV2, RuntimeStartupRecoveryExecutionReceiptV2,
    RuntimeStartupRecoveryExecutionTerminalDigestV2,
};

enum TestExecutionOutcome {
    Progressed,
    NoCandidate,
    RetryAfter(Duration),
}

fn state_for_class(class: RuntimeStartupRecoveryClassV2) -> RuntimeStartupRecoveryStateV2 {
    let mut state = empty_startup_state();
    match class {
        RuntimeStartupRecoveryClassV2::StaleLive => {
            state.serving = RuntimeStartupServingStateV2::RecoverableStale { count: 1 };
        }
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification => {
            state.recoverable_awaiting_certification_count = 1;
        }
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect => {
            state.suspended_local_effect_count = 1;
        }
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent => {
            state.pending_runtime_drain_intent_count = 1;
        }
    }
    state
}

fn observe_pending_class(
    lifecycle: &mut RuntimeGatewayClosedLifecycleV2,
    permit: &mut RuntimeClosedDrainRecoveryPermitV2,
    class: RuntimeStartupRecoveryClassV2,
) -> RuntimeStartupRecoveryContinuationV2 {
    let authorization = begin_startup_observation(lifecycle, permit, 200);
    let completed = complete_startup_observation(authorization, at(101), state_for_class(class));
    let RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) = lifecycle
        .complete_startup_recovery_observation(permit, completed)
        .unwrap()
    else {
        panic!("expected startup recovery continuation")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(class)
    );
    continuation
}

fn begin_execution(
    class: RuntimeStartupRecoveryClassV2,
) -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    RuntimeStartupRecoveryContinuationV2,
    RuntimeAuthorizedStartupRecoveryExecutionV2,
) {
    let (mut lifecycle, mut permit) = begin_recovery();
    let continuation = observe_pending_class(&mut lifecycle, &mut permit, class);
    let authorization = lifecycle
        .begin_startup_recovery_execution(&mut permit, continuation)
        .unwrap();
    (lifecycle, permit, continuation, authorization)
}

fn execution_receipt(
    authorization: &RuntimeAuthorizedStartupRecoveryExecutionV2,
    database_now: i64,
    outcome: TestExecutionOutcome,
) -> RuntimeStartupRecoveryExecutionReceiptV2 {
    let request = authorization.request();
    let outcome = match outcome {
        TestExecutionOutcome::Progressed => {
            RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
                action_identity: request.action_identity().clone(),
                terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2::new([7; 32])
                    .unwrap(),
            }
        }
        TestExecutionOutcome::NoCandidate => {
            RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate
        }
        TestExecutionOutcome::RetryAfter(retry_after) => {
            RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter { retry_after }
        }
    };
    RuntimeStartupRecoveryExecutionReceiptV2 {
        correlation: request.correlation().clone(),
        class: request.class(),
        owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1 {
            lease_id: request.gateway_owner_lease_id().clone(),
            owner_revision: request.expected_owner_revision(),
            database_now: at(database_now),
            expires_at: request.expected_owner_expires_at(),
        },
        outcome,
    }
}

fn complete_execution(
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    database_now: i64,
    outcome: TestExecutionOutcome,
) -> RuntimeCompletedStartupRecoveryExecutionV2 {
    let receipt = execution_receipt(&authorization, database_now, outcome);
    authorization.complete(receipt)
}

fn expected_emergency(
    generation: RuntimeGatewayCoordinatorGenerationV2,
    cause: RuntimeGatewayEmergencyCauseV2,
) -> RuntimeGatewayClosedSnapshotV2 {
    RuntimeGatewayClosedSnapshotV2::Emergency {
        generation: RuntimeGatewayCoordinatorGenerationV2::new(non_zero(generation.get() + 1)),
        cause,
    }
}

fn assert_receipt_rejected(
    mutate: impl FnOnce(
        &RuntimeAuthorizedStartupRecoveryExecutionV2,
        &mut RuntimeStartupRecoveryExecutionReceiptV2,
    ),
    expected: RuntimeStartupRecoveryExecutionAcceptanceErrorV2,
    cause: RuntimeGatewayEmergencyCauseV2,
) {
    let (mut lifecycle, mut permit, _, authorization) =
        begin_execution(RuntimeStartupRecoveryClassV2::StaleLive);
    let generation = permit.coordinator_generation();
    let mut receipt = execution_receipt(&authorization, 102, TestExecutionOutcome::NoCandidate);
    mutate(&authorization, &mut receipt);
    let completed = authorization.complete(receipt);

    assert_eq!(
        lifecycle
            .complete_startup_recovery_execution(&mut permit, completed)
            .unwrap_err(),
        RuntimeGatewayClosedTransitionErrorV2::StartupRecoveryExecution(expected)
    );
    assert_eq!(lifecycle.snapshot(), expected_emergency(generation, cause));
    assert_eq!(permit.authority_revision().get(), 3);
}

fn assert_request_binding_rejected(
    mutate: impl FnOnce(&mut RuntimeClosedDrainRecoveryPermitV2),
    expected: RuntimeStartupRecoveryExecutionAcceptanceErrorV2,
    cause: RuntimeGatewayEmergencyCauseV2,
) {
    let (mut lifecycle, mut permit, _, authorization) =
        begin_execution(RuntimeStartupRecoveryClassV2::StaleLive);
    let generation = permit.coordinator_generation();
    let completed = complete_execution(authorization, 102, TestExecutionOutcome::NoCandidate);
    mutate(&mut permit);

    assert_eq!(
        lifecycle
            .complete_startup_recovery_execution(&mut permit, completed)
            .unwrap_err(),
        RuntimeGatewayClosedTransitionErrorV2::StartupRecoveryExecution(expected)
    );
    assert_eq!(lifecycle.snapshot(), expected_emergency(generation, cause));
    assert_eq!(permit.authority_revision().get(), 3);
}

#[test]
fn execution_authorization_binds_every_closed_recovery_evidence_and_progresses_once() {
    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    let recovery_id = permit.recovery_id().clone();
    let paused = permit.paused_gateway().clone();
    let registry = permit.registry_evidence().empty_observation();
    let registry_process = registry.process_instance_id().clone();
    let registry_sequence = registry.observation_sequence();
    let continuation = observe_pending_class(
        &mut lifecycle,
        &mut permit,
        RuntimeStartupRecoveryClassV2::StaleLive,
    );
    assert_eq!(permit.authority_revision().get(), 3);
    let readiness = permit.readiness().clone();

    let authorization = lifecycle
        .begin_startup_recovery_execution(&mut permit, continuation)
        .unwrap();
    let request = authorization.request();
    assert_eq!(request.correlation().recovery_id(), &recovery_id);
    assert_eq!(
        request.correlation().originating_emergency_generation(),
        non_zero(1)
    );
    assert_eq!(
        request.correlation().coordinator_generation(),
        non_zero(generation.get())
    );
    assert_eq!(request.correlation().authority_revision(), non_zero(3));
    assert_eq!(
        request.correlation().selection_authority_revision(),
        non_zero(2)
    );
    assert_eq!(request.class(), RuntimeStartupRecoveryClassV2::StaleLive);
    assert_eq!(
        request.action_identity().correlation(),
        request.correlation()
    );
    assert_eq!(request.action_identity().class(), request.class());
    assert_eq!(
        request.gateway_owner_lease_id(),
        &permit.owner_receipt().lease_id
    );
    assert_eq!(
        request.expected_owner_revision(),
        permit.owner_receipt().owner_revision
    );
    assert_eq!(
        request.expected_owner_expires_at(),
        permit.owner_receipt().expires_at
    );
    assert_eq!(request.minimum_database_now(), at(101));
    assert_eq!(request.readiness(), &readiness);
    assert_eq!(request.paused_gateway(), &paused);
    assert_eq!(request.registry_process_instance_id(), &registry_process);
    assert_eq!(request.registry_observation_sequence(), registry_sequence);
    assert_eq!(request.registry_retained_slot_count(), 0);
    assert_eq!(request.registry_retained_empty_tombstone_count(), 0);
    assert_eq!(
        format!("{:?}", request.correlation()),
        "RuntimeStartupRecoveryExecutionCorrelationV2(<redacted>)"
    );
    assert_eq!(
        format!("{:?}", request.action_identity()),
        "RuntimeStartupRecoveryExecutionActionIdentityV2(<redacted>)"
    );
    assert_eq!(
        format!("{request:?}"),
        "RuntimeStartupRecoveryExecutionRequestV2(<redacted>)"
    );
    assert_eq!(
        format!("{authorization:?}"),
        "RuntimeAuthorizedStartupRecoveryExecutionV2(<redacted>)"
    );

    let completed = complete_execution(authorization, 102, TestExecutionOutcome::Progressed);
    assert_eq!(
        format!("{completed:?}"),
        "RuntimeCompletedStartupRecoveryExecutionV2(<redacted>)"
    );
    let accepted = lifecycle
        .complete_startup_recovery_execution(&mut permit, completed)
        .unwrap();

    assert_eq!(accepted.class(), RuntimeStartupRecoveryClassV2::StaleLive);
    let RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
        action_identity,
        terminal_digest,
    } = accepted.outcome()
    else {
        panic!("expected progress proof")
    };
    assert_eq!(
        action_identity.class(),
        RuntimeStartupRecoveryClassV2::StaleLive
    );
    assert_eq!(terminal_digest.as_bytes(), &[7; 32]);
    assert_eq!(
        format!("{terminal_digest:?}"),
        "RuntimeStartupRecoveryExecutionTerminalDigestV2(<redacted>)"
    );
    assert_eq!(accepted.successor_authority_revision().get(), 4);
    assert_eq!(accepted.owner_receipt().database_now, at(102));
    assert_eq!(
        format!("{accepted:?}"),
        "RuntimeAcceptedStartupRecoveryExecutionOutcomeV2(<redacted>)"
    );
    assert_eq!(permit.authority_revision().get(), 4);
    assert!(permit.pending_startup_recovery_execution().is_none());
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::RecoveryPending {
            generation,
            recovery_id,
            authority_revision: permit.authority_revision(),
        }
    );

    let iteration = lifecycle
        .refresh_recovery_readiness(&mut permit, current_readiness(300))
        .unwrap();
    assert_eq!(permit.authority_revision().get(), 5);
    drop(iteration);
}

#[test]
fn every_recovery_class_and_nonprogress_outcome_restores_one_successor() {
    let classes = [
        RuntimeStartupRecoveryClassV2::StaleLive,
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification,
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
        RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent,
    ];
    for class in classes {
        let (mut lifecycle, mut permit, _, authorization) = begin_execution(class);
        let completed = complete_execution(authorization, 102, TestExecutionOutcome::NoCandidate);
        let accepted = lifecycle
            .complete_startup_recovery_execution(&mut permit, completed)
            .unwrap();
        assert_eq!(accepted.class(), class);
        assert_eq!(
            accepted.outcome(),
            &RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate
        );
        assert_eq!(accepted.successor_authority_revision().get(), 4);
        assert_eq!(permit.authority_revision().get(), 4);
        assert!(permit.pending_startup_recovery_execution().is_none());
    }

    let (mut lifecycle, mut permit, _, authorization) =
        begin_execution(RuntimeStartupRecoveryClassV2::SuspendedLocalEffect);
    let completed = complete_execution(
        authorization,
        102,
        TestExecutionOutcome::RetryAfter(Duration::from_secs(5)),
    );
    let accepted = lifecycle
        .complete_startup_recovery_execution(&mut permit, completed)
        .unwrap();
    assert_eq!(
        accepted.outcome(),
        &RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter {
            retry_after: Duration::from_secs(5),
        }
    );
    assert_eq!(permit.authority_revision().get(), 4);
}

#[test]
fn pending_execution_cannot_be_skipped_or_replaced_by_another_continuation() {
    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    observe_pending_class(
        &mut lifecycle,
        &mut permit,
        RuntimeStartupRecoveryClassV2::StaleLive,
    );
    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut permit, current_readiness(300)),
        Err(RuntimeGatewayClosedTransitionErrorV2::StartupRecoveryExecutionPending)
    );
    assert_eq!(
        lifecycle.snapshot(),
        expected_emergency(
            generation,
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation
        )
    );

    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    observe_pending_class(
        &mut lifecycle,
        &mut permit,
        RuntimeStartupRecoveryClassV2::StaleLive,
    );
    assert_eq!(
        lifecycle
            .begin_startup_recovery_execution(
                &mut permit,
                RuntimeStartupRecoveryContinuationV2::Recover(
                    RuntimeStartupRecoveryClassV2::SuspendedLocalEffect,
                ),
            )
            .unwrap_err(),
        RuntimeGatewayClosedTransitionErrorV2::StartupRecoveryExecutionClassMismatch
    );
    assert_eq!(
        lifecycle.snapshot(),
        expected_emergency(
            generation,
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation
        )
    );

    let (mut lifecycle, mut permit) = begin_recovery();
    let generation = permit.coordinator_generation();
    observe_pending_class(
        &mut lifecycle,
        &mut permit,
        RuntimeStartupRecoveryClassV2::StaleLive,
    );
    assert_eq!(
        lifecycle
            .begin_startup_recovery_execution(
                &mut permit,
                RuntimeStartupRecoveryContinuationV2::WaitForForeignFresh {
                    retry_after: Duration::from_secs(1),
                },
            )
            .unwrap_err(),
        RuntimeGatewayClosedTransitionErrorV2::StartupRecoveryExecutionNotPending
    );
    assert_eq!(
        lifecycle.snapshot(),
        expected_emergency(
            generation,
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation
        )
    );
}

#[test]
fn execution_authority_is_lost_on_overlap_or_drop() {
    let (mut lifecycle, mut permit, continuation, authorization) =
        begin_execution(RuntimeStartupRecoveryClassV2::StaleLive);
    let generation = permit.coordinator_generation();
    assert_eq!(
        lifecycle
            .begin_startup_recovery_execution(&mut permit, continuation)
            .unwrap_err(),
        RuntimeGatewayClosedTransitionErrorV2::RecoveryOperationInFlight
    );
    assert_eq!(
        lifecycle.snapshot(),
        expected_emergency(
            generation,
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation
        )
    );
    drop(authorization);

    let (mut lifecycle, mut permit, _, authorization) =
        begin_execution(RuntimeStartupRecoveryClassV2::StaleLive);
    let generation = permit.coordinator_generation();
    drop(authorization);
    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut permit, current_readiness(300)),
        Err(RuntimeGatewayClosedTransitionErrorV2::RecoveryOperationInFlight)
    );
    assert_eq!(
        lifecycle.snapshot(),
        expected_emergency(
            generation,
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation
        )
    );
}

#[test]
fn execution_receipt_rejects_correlation_class_owner_and_clock_mismatches() {
    assert_receipt_rejected(
        |_, receipt| {
            receipt
                .correlation
                .replace_authority_revision_for_test(non_zero(9));
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::CorrelationMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_receipt_rejected(
        |_, receipt| {
            receipt.class = RuntimeStartupRecoveryClassV2::SuspendedLocalEffect;
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::ClassMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_receipt_rejected(
        |_, receipt| {
            receipt.owner_receipt.owner_revision = non_zero(9);
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::OwnerMismatch,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
    assert_receipt_rejected(
        |_, receipt| {
            receipt.owner_receipt.lease_id.lease_epoch = non_zero(9);
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::OwnerMismatch,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
    assert_receipt_rejected(
        |_, receipt| {
            receipt.owner_receipt.expires_at = at(199);
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::OwnerMismatch,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
    assert_receipt_rejected(
        |_, receipt| {
            receipt.owner_receipt.database_now = at(100);
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::DatabaseClockRegressed,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_receipt_rejected(
        |_, receipt| {
            receipt.owner_receipt.database_now = at(200);
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::OwnerNotCurrent,
        RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
    );
}

#[test]
fn execution_receipt_requires_bounded_retry_and_exact_progress_proof() {
    for retry_after in [Duration::ZERO, Duration::from_secs(100)] {
        assert_receipt_rejected(
            |_, receipt| {
                receipt.outcome =
                    RuntimeStartupRecoveryExecutionReceiptOutcomeV2::RetryAfter { retry_after };
            },
            RuntimeStartupRecoveryExecutionAcceptanceErrorV2::InvalidRetryAfter,
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        );
    }

    let (mut foreign_lifecycle, mut foreign_permit) = begin_recovery();
    let foreign_recovery_id =
        RuntimeRecoveryIdV2::parse("fedcba9876543210fedcba9876543210").unwrap();
    foreign_permit.replace_recovery_id_for_test(foreign_recovery_id.clone());
    foreign_lifecycle.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending {
        generation: foreign_permit.coordinator_generation(),
        recovery_id: foreign_recovery_id,
        authority_revision: foreign_permit.authority_revision(),
    };
    let foreign_continuation = observe_pending_class(
        &mut foreign_lifecycle,
        &mut foreign_permit,
        RuntimeStartupRecoveryClassV2::StaleLive,
    );
    let foreign_authorization = foreign_lifecycle
        .begin_startup_recovery_execution(&mut foreign_permit, foreign_continuation)
        .unwrap();
    let foreign_action_identity = foreign_authorization.request().action_identity().clone();
    drop(foreign_authorization);
    assert_receipt_rejected(
        move |_, receipt| {
            receipt.outcome = RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
                action_identity: foreign_action_identity,
                terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2::new([9; 32])
                    .unwrap(),
            };
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::ProgressProofMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_eq!(
        RuntimeStartupRecoveryExecutionTerminalDigestV2::new([0; 32]),
        Err(RuntimeStartupRecoveryExecutionDigestErrorV2::Zero)
    );
}

#[test]
fn execution_completion_revalidates_readiness_paused_registry_and_clock_bindings() {
    assert_request_binding_rejected(
        |permit| permit.replace_readiness_for_test(current_readiness(300)),
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::CapabilityReadinessMismatch,
        RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
    );
    assert_request_binding_rejected(
        |permit| {
            let paused = permit.paused_gateway();
            permit.replace_paused_gateway_for_test(RuntimePausedGatewayObservationV2::new(
                paused.coordinator_generation(),
                paused.process_instance_id().clone(),
                paused.connection_epoch(),
                RuntimeGatewayReadyKindV2::Ready,
                non_zero(paused.admission_revision().get() + 1),
                RuntimePausedGatewaySequenceV2::new(
                    paused.transition_sequence(),
                    paused.connected_event_sequence(),
                    paused.last_resume_sequence(),
                )
                .unwrap(),
            ));
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::PausedGatewayMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_request_binding_rejected(
        |permit| {
            let process = permit
                .registry_evidence()
                .empty_observation()
                .process_instance_id()
                .clone();
            let registry = accept_runtime_registry_recovery_empty_observation_v2(
                process,
                RuntimeRegistryRecoveryObservationInputV2 {
                    observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                        non_zero(7),
                    ),
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
            permit.replace_registry_evidence_for_test(
                RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry),
            );
        },
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::RegistryMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_request_binding_rejected(
        |permit| permit.replace_last_database_now_for_test(at(102)),
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::DatabaseClockMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
    assert_request_binding_rejected(
        |permit| permit.replace_pending_selection_revision_for_test(non_zero(9)),
        RuntimeStartupRecoveryExecutionAcceptanceErrorV2::CorrelationMismatch,
        RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
    );
}

#[test]
fn stale_or_replayed_execution_cannot_advance_the_permit_again() {
    let (mut lifecycle, mut permit, continuation, authorization) =
        begin_execution(RuntimeStartupRecoveryClassV2::StaleLive);
    let completed = complete_execution(authorization, 102, TestExecutionOutcome::NoCandidate);
    lifecycle
        .complete_startup_recovery_execution(&mut permit, completed)
        .unwrap();
    assert_eq!(permit.authority_revision().get(), 4);
    let generation = permit.coordinator_generation();
    assert_eq!(
        lifecycle
            .begin_startup_recovery_execution(&mut permit, continuation)
            .unwrap_err(),
        RuntimeGatewayClosedTransitionErrorV2::StartupRecoveryExecutionNotPending
    );
    assert_eq!(permit.authority_revision().get(), 4);
    assert_eq!(
        lifecycle.snapshot(),
        expected_emergency(
            generation,
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation
        )
    );

    let (mut lifecycle, mut permit, _, authorization) =
        begin_execution(RuntimeStartupRecoveryClassV2::StaleLive);
    let completed = complete_execution(authorization, 102, TestExecutionOutcome::NoCandidate);
    let generation = permit.coordinator_generation();
    lifecycle
        .invalidate(
            generation,
            RuntimeGatewayInvalidationCauseV2::TransportDisconnected,
        )
        .unwrap();
    let newer = lifecycle.snapshot();
    assert_eq!(
        lifecycle
            .complete_startup_recovery_execution(&mut permit, completed)
            .unwrap_err(),
        RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit
    );
    assert_eq!(lifecycle.snapshot(), newer);
    assert_eq!(permit.authority_revision().get(), 3);
}

#[test]
fn execution_authority_overflow_is_terminal_and_nonrestoring() {
    let (mut lifecycle, mut permit) = begin_recovery();
    let continuation = observe_pending_class(
        &mut lifecycle,
        &mut permit,
        RuntimeStartupRecoveryClassV2::StaleLive,
    );
    permit.exhaust_authority_revision_for_test();
    permit
        .replace_pending_selection_revision_for_test(NonZeroU64::new(i64::MAX as u64 - 1).unwrap());
    let generation = permit.coordinator_generation();
    let recovery_id = permit.recovery_id().clone();
    lifecycle.snapshot = RuntimeGatewayClosedSnapshotV2::RecoveryPending {
        generation,
        recovery_id,
        authority_revision: permit.authority_revision(),
    };
    let authorization = lifecycle
        .begin_startup_recovery_execution(&mut permit, continuation)
        .unwrap();
    let completed = complete_execution(authorization, 102, TestExecutionOutcome::NoCandidate);

    assert_eq!(
        lifecycle
            .complete_startup_recovery_execution(&mut permit, completed)
            .unwrap_err(),
        RuntimeGatewayClosedTransitionErrorV2::AuthorityRevisionOverflow
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Shutdown { generation }
    );
    assert_eq!(permit.authority_revision().get(), i64::MAX as u64);
    assert!(permit.pending_startup_recovery_execution().is_some());
}
