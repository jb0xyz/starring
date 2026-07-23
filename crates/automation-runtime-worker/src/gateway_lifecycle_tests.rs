use std::num::NonZeroU64;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBuildRevisionV1, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1, RuntimeGatewayReadyKindV2,
    RuntimeRecoveryIdV2,
};
use automation_runtime_convergence::ProcessInstanceId;
use chrono::{DateTime, Utc};

use super::{
    RuntimeGatewayClosedLifecycleV2, RuntimeGatewayClosedSnapshotV2,
    RuntimeGatewayClosedTransitionErrorV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimeGatewayEmergencyCauseV2, RuntimeGatewayInvalidationCauseV2,
};
use crate::{
    accept_runtime_registry_recovery_empty_observation_v2, RuntimeCapabilityReadinessKindV2,
    RuntimeCapabilityReadinessReceiptV2, RuntimeCapabilityReadinessSetV2,
    RuntimeClosedDrainRecoveryPermitV2, RuntimeClosedRecoveryInputV2,
    RuntimeClosedRecoveryRegistryEvidenceV2, RuntimePausedGatewayObservationV2,
    RuntimePausedGatewaySequenceV2, RuntimeRegistryGlobalObservationSequenceV2,
    RuntimeRegistryRecoveryObservationInputV2,
};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
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

fn current_readiness(checked_at: i64) -> RuntimeCapabilityReadinessSetV2 {
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

fn begin_recovery() -> (
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

    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut permit, current_readiness(200)),
        Ok(())
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

    assert_eq!(
        lifecycle.refresh_recovery_readiness(&mut permit, current_readiness(300)),
        Ok(())
    );
    assert_eq!(permit.authority_revision().get(), 3);
    assert_eq!(permit.readiness().checked_at_bounds(), (at(300), at(304)));
    assert_eq!(lifecycle.validate_recovery_permit(&permit), Ok(()));
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
