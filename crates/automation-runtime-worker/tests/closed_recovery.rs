use std::num::NonZeroU64;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2,
    RuntimeRecoveryIdV2,
};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_worker::{
    accept_runtime_registry_recovery_empty_observation_v2, RuntimeCapabilityReadinessKindV2,
    RuntimeCapabilityReadinessReceiptV2, RuntimeCapabilityReadinessSetV2,
    RuntimeClosedRecoveryAuthorityRevisionV2, RuntimeClosedRecoveryInputV2,
    RuntimeClosedRecoveryRegistryEvidenceV2, RuntimeGatewayClosedLifecycleV2,
    RuntimeGatewayClosedSnapshotV2, RuntimeGatewayClosedTransitionErrorV2,
    RuntimeGatewayCoordinatorGenerationV2, RuntimeGatewayEmergencyCauseV2,
    RuntimeGatewayInvalidationCauseV2, RuntimePausedGatewayObservationV2,
    RuntimePausedGatewaySequenceV2, RuntimeRegistryGlobalObservationSequenceV2,
    RuntimeRegistryRecoveryObservationInputV2,
};
use chrono::{DateTime, Utc};

fn generation(snapshot: &RuntimeGatewayClosedSnapshotV2) -> RuntimeGatewayCoordinatorGenerationV2 {
    snapshot.generation()
}

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn process(value: &str) -> ProcessInstanceId {
    ProcessInstanceId::parse(value).unwrap()
}

fn owner_receipt(process_instance_id: ProcessInstanceId) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    owner_receipt_with_sequences(process_instance_id, 7, 11)
}

fn owner_receipt_with_sequences(
    process_instance_id: ProcessInstanceId,
    lease_epoch: u64,
    owner_revision: u64,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id,
            lease_epoch: non_zero(lease_epoch),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        },
        owner_revision: non_zero(owner_revision),
        database_now: at(100),
        expires_at: at(200),
    }
}

fn readiness() -> RuntimeCapabilityReadinessSetV2 {
    let receipt = |kind, role, checked_at| {
        RuntimeCapabilityReadinessReceiptV2::new(
            kind,
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring",
            role,
            at(checked_at),
        )
        .unwrap()
    };
    RuntimeCapabilityReadinessSetV2::new(
        receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a", 101),
        receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_b", 102),
        receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c", 103),
        receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d", 104),
        receipt(RuntimeCapabilityReadinessKindV2::Interaction, "role_e", 105),
    )
    .unwrap()
}

fn paused_gateway(
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    process_instance_id: ProcessInstanceId,
) -> RuntimePausedGatewayObservationV2 {
    paused_gateway_shape(
        coordinator_generation,
        process_instance_id,
        RuntimeGatewayReadyKindV2::Ready,
        None,
    )
}

fn paused_gateway_shape(
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    process_instance_id: ProcessInstanceId,
    kind: RuntimeGatewayReadyKindV2,
    resumed_at: Option<u64>,
) -> RuntimePausedGatewayObservationV2 {
    paused_gateway_with_sequences(
        coordinator_generation,
        process_instance_id,
        PausedGatewayEvidenceSequences {
            kind,
            connection_epoch: 13,
            admission_revision: 17,
            transition_sequence: 20,
            connected_event_sequence: 18,
            resume_sequence: resumed_at,
        },
    )
}

#[derive(Clone, Copy)]
struct PausedGatewayEvidenceSequences {
    kind: RuntimeGatewayReadyKindV2,
    connection_epoch: u64,
    admission_revision: u64,
    transition_sequence: u64,
    connected_event_sequence: u64,
    resume_sequence: Option<u64>,
}

fn paused_gateway_with_sequences(
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    process_instance_id: ProcessInstanceId,
    evidence: PausedGatewayEvidenceSequences,
) -> RuntimePausedGatewayObservationV2 {
    RuntimePausedGatewayObservationV2::new(
        coordinator_generation,
        process_instance_id,
        non_zero(evidence.connection_epoch),
        evidence.kind,
        non_zero(evidence.admission_revision),
        RuntimePausedGatewaySequenceV2::new(
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(evidence.transition_sequence)),
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(evidence.connected_event_sequence)),
            evidence
                .resume_sequence
                .map(|sequence| RuntimeGatewayAdmissionSequenceV2::new(non_zero(sequence))),
        )
        .unwrap(),
    )
}

fn empty_registry(
    process_instance_id: ProcessInstanceId,
) -> automation_runtime_worker::RuntimeRegistryRecoveryEmptyObservationV2 {
    empty_registry_with_sequence(process_instance_id, 23)
}

fn empty_registry_with_sequence(
    process_instance_id: ProcessInstanceId,
    observation_sequence: u64,
) -> automation_runtime_worker::RuntimeRegistryRecoveryEmptyObservationV2 {
    accept_runtime_registry_recovery_empty_observation_v2(
        process_instance_id,
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(non_zero(
                observation_sequence,
            )),
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
    .unwrap()
}

fn recovery_input(
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    owner_process: &str,
    paused_process: &str,
    registry_process: &str,
) -> RuntimeClosedRecoveryInputV2 {
    RuntimeClosedRecoveryInputV2::new(
        RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
        owner_receipt(process(owner_process)),
        readiness(),
        paused_gateway(coordinator_generation, process(paused_process)),
        RuntimeClosedRecoveryRegistryEvidenceV2::Empty(empty_registry(process(registry_process))),
    )
}

#[test]
fn emergency_to_recovery_pending_binds_exact_evidence_once() {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let emergency_generation = generation(&lifecycle.snapshot());
    let (snapshot, permit) = lifecycle
        .begin_recovery(
            emergency_generation,
            recovery_input(emergency_generation, "process:1", "process:1", "process:1"),
        )
        .unwrap();

    let RuntimeGatewayClosedSnapshotV2::RecoveryPending {
        generation,
        recovery_id,
        authority_revision,
    } = &snapshot
    else {
        panic!("expected recovery pending")
    };
    assert_eq!(generation.get(), emergency_generation.get() + 1);
    assert_eq!(recovery_id.as_str(), "0123456789abcdef0123456789abcdef");
    assert_eq!(
        *authority_revision,
        RuntimeClosedRecoveryAuthorityRevisionV2::FIRST
    );
    assert_eq!(
        permit.originating_emergency_generation(),
        emergency_generation
    );
    assert_eq!(permit.coordinator_generation(), *generation);
    assert_eq!(permit.recovery_id(), recovery_id);
    assert_eq!(permit.authority_revision().get(), 1);
    assert_eq!(
        permit.owner_receipt().lease_id.process_instance_id.as_str(),
        "process:1"
    );
    assert_eq!(
        permit.paused_gateway().process_instance_id().as_str(),
        "process:1"
    );
    assert_eq!(
        permit.registry_evidence().process_instance_id().as_str(),
        "process:1"
    );
    assert_eq!(permit.readiness().checked_at_bounds(), (at(101), at(105)));
    assert_eq!(
        format!("{permit:?}"),
        "RuntimeClosedDrainRecoveryPermitV2(<redacted>)"
    );
    assert_eq!(lifecycle.snapshot(), snapshot);
    assert_eq!(lifecycle.validate_recovery_permit(&permit), Ok(()));
}

#[test]
fn recovery_start_rejects_noncurrent_owner_receipt_without_advancing() {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let first = generation(&lifecycle.snapshot());
    let input = RuntimeClosedRecoveryInputV2::new(
        RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
        RuntimeGatewayOwnerLeaseReceiptV1 {
            expires_at: at(100),
            ..owner_receipt(process("process:1"))
        },
        readiness(),
        paused_gateway(first, process("process:1")),
        RuntimeClosedRecoveryRegistryEvidenceV2::Empty(empty_registry(process("process:1"))),
    );

    assert_eq!(
        lifecycle.begin_recovery(first, input),
        Err(RuntimeGatewayClosedTransitionErrorV2::OwnerReceiptNotCurrent)
    );
    assert_eq!(generation(&lifecycle.snapshot()), first);
}

#[test]
fn recovery_start_preserves_gateway_kind_independently_from_prior_resume() {
    for (kind, resumed_at) in [
        (RuntimeGatewayReadyKindV2::Ready, Some(19)),
        (RuntimeGatewayReadyKindV2::Resumed, None),
    ] {
        let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
        let first = generation(&lifecycle.snapshot());
        let input = RuntimeClosedRecoveryInputV2::new(
            RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
            owner_receipt(process("process:1")),
            readiness(),
            paused_gateway_shape(first, process("process:1"), kind, resumed_at),
            RuntimeClosedRecoveryRegistryEvidenceV2::Empty(empty_registry(process("process:1"))),
        );

        let (_, permit) = lifecycle.begin_recovery(first, input).unwrap();
        assert_eq!(permit.paused_gateway().kind(), kind);
        assert_eq!(
            permit
                .paused_gateway()
                .last_resume_sequence()
                .map(|sequence| sequence.get()),
            resumed_at
        );
    }
}

#[test]
fn recovery_start_rejects_out_of_range_evidence_without_advancing() {
    let invalid = i64::MAX as u64 + 1;
    for (lease_epoch, owner_revision, connection_epoch, admission_revision, transition_sequence) in [
        (invalid, 11, 13, 17, 20),
        (7, invalid, 13, 17, 20),
        (7, 11, invalid, 17, 20),
        (7, 11, 13, invalid, 20),
        (7, 11, 13, 17, invalid),
    ] {
        let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
        let first = generation(&lifecycle.snapshot());
        let input = RuntimeClosedRecoveryInputV2::new(
            RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
            owner_receipt_with_sequences(process("process:1"), lease_epoch, owner_revision),
            readiness(),
            paused_gateway_with_sequences(
                first,
                process("process:1"),
                PausedGatewayEvidenceSequences {
                    kind: RuntimeGatewayReadyKindV2::Ready,
                    connection_epoch,
                    admission_revision,
                    transition_sequence,
                    connected_event_sequence: 18,
                    resume_sequence: None,
                },
            ),
            RuntimeClosedRecoveryRegistryEvidenceV2::Empty(empty_registry(process("process:1"))),
        );

        assert_eq!(
            lifecycle.begin_recovery(first, input),
            Err(RuntimeGatewayClosedTransitionErrorV2::EvidenceSequenceOutOfRange)
        );
        assert_eq!(generation(&lifecycle.snapshot()), first);
    }
}

#[test]
fn recovery_start_accepts_every_persistence_sequence_at_i64_max() {
    let maximum = i64::MAX as u64;
    for (connected_event_sequence, resume_sequence) in
        [(maximum, None), (maximum - 1, Some(maximum))]
    {
        let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
        let first = generation(&lifecycle.snapshot());
        let input = RuntimeClosedRecoveryInputV2::new(
            RuntimeRecoveryIdV2::parse("0123456789abcdef0123456789abcdef").unwrap(),
            owner_receipt_with_sequences(process("process:1"), maximum, maximum),
            readiness(),
            paused_gateway_with_sequences(
                first,
                process("process:1"),
                PausedGatewayEvidenceSequences {
                    kind: RuntimeGatewayReadyKindV2::Ready,
                    connection_epoch: maximum,
                    admission_revision: maximum,
                    transition_sequence: maximum,
                    connected_event_sequence,
                    resume_sequence,
                },
            ),
            RuntimeClosedRecoveryRegistryEvidenceV2::Empty(empty_registry_with_sequence(
                process("process:1"),
                maximum,
            )),
        );

        let (_, permit) = lifecycle.begin_recovery(first, input).unwrap();
        assert_eq!(permit.owner_receipt().lease_id.lease_epoch.get(), maximum);
        assert_eq!(permit.owner_receipt().owner_revision.get(), maximum);
        assert_eq!(permit.paused_gateway().connection_epoch().get(), maximum);
        assert_eq!(permit.paused_gateway().admission_revision().get(), maximum);
        assert_eq!(permit.paused_gateway().transition_sequence().get(), maximum);
        assert_eq!(
            permit.paused_gateway().connected_event_sequence().get(),
            connected_event_sequence
        );
        assert_eq!(
            permit
                .paused_gateway()
                .last_resume_sequence()
                .map(|sequence| sequence.get()),
            resume_sequence
        );
        assert_eq!(
            permit
                .registry_evidence()
                .empty_observation()
                .observation_sequence()
                .get(),
            maximum
        );
    }
}

#[test]
fn recovery_start_rejects_every_generation_and_process_mismatch_without_advancing() {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let first = generation(&lifecycle.snapshot());
    let second = RuntimeGatewayCoordinatorGenerationV2::new(non_zero(first.get() + 1));

    assert_eq!(
        lifecycle.begin_recovery(
            second,
            recovery_input(first, "process:1", "process:1", "process:1"),
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration)
    );
    assert_eq!(
        lifecycle.begin_recovery(
            first,
            recovery_input(second, "process:1", "process:1", "process:1"),
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::PausedGatewayGenerationMismatch)
    );
    assert_eq!(
        lifecycle.begin_recovery(
            first,
            recovery_input(first, "process:1", "process:2", "process:1"),
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::ProcessInstanceMismatch)
    );
    assert_eq!(
        lifecycle.begin_recovery(
            first,
            recovery_input(first, "process:1", "process:1", "process:2"),
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::ProcessInstanceMismatch)
    );
    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: first,
            cause: RuntimeGatewayEmergencyCauseV2::Starting,
        }
    );
}

#[test]
fn recovery_pending_cannot_reenter_and_stale_permit_fails_after_invalidation() {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let first = generation(&lifecycle.snapshot());
    let (_, permit) = lifecycle
        .begin_recovery(
            first,
            recovery_input(first, "process:1", "process:1", "process:1"),
        )
        .unwrap();
    let recovery_generation = permit.coordinator_generation();

    assert_eq!(
        lifecycle.begin_recovery(
            recovery_generation,
            recovery_input(recovery_generation, "process:1", "process:1", "process:1",),
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::NotEmergency)
    );
    let emergency = lifecycle
        .invalidate(
            recovery_generation,
            RuntimeGatewayInvalidationCauseV2::OwnershipUncertain,
        )
        .unwrap();
    assert_eq!(
        lifecycle.validate_recovery_permit(&permit),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleRecoveryPermit)
    );
    assert!(matches!(
        emergency,
        RuntimeGatewayClosedSnapshotV2::Emergency {
            cause: RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
            ..
        }
    ));
}

#[test]
fn shutdown_wins_terminally_before_or_after_recovery_start() {
    let mut before = RuntimeGatewayClosedLifecycleV2::starting();
    let before_generation = generation(&before.snapshot());
    let shutdown = before.shutdown(before_generation).unwrap();
    let shutdown_generation = generation(&shutdown);
    assert_eq!(
        before.begin_recovery(
            shutdown_generation,
            recovery_input(shutdown_generation, "process:1", "process:1", "process:1",),
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown)
    );

    let mut after = RuntimeGatewayClosedLifecycleV2::starting();
    let first = generation(&after.snapshot());
    let (_, permit) = after
        .begin_recovery(
            first,
            recovery_input(first, "process:1", "process:1", "process:1"),
        )
        .unwrap();
    let recovery_generation = permit.coordinator_generation();
    let shutdown = after.shutdown(recovery_generation).unwrap();
    assert!(matches!(
        shutdown,
        RuntimeGatewayClosedSnapshotV2::Shutdown { .. }
    ));
    assert_eq!(
        after.validate_recovery_permit(&permit),
        Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown)
    );
    assert_eq!(
        after.invalidate(
            generation(&after.snapshot()),
            RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown)
    );
}
