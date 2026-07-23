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
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimePausedGatewayObservationV2, RuntimePausedGatewaySequenceV2,
    RuntimeRegistryGlobalObservationSequenceV2, RuntimeRegistryRecoveryObservationInputV2,
};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn recovery_input(
    generation: RuntimeGatewayCoordinatorGenerationV2,
) -> RuntimeClosedRecoveryInputV2 {
    let process = ProcessInstanceId::parse("process:1").unwrap();
    let receipt = |kind, role| {
        RuntimeCapabilityReadinessReceiptV2::new(
            kind,
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring",
            role,
            at(100),
        )
        .unwrap()
    };
    let readiness = RuntimeCapabilityReadinessSetV2::new(
        receipt(RuntimeCapabilityReadinessKindV2::Convergence, "role_a"),
        receipt(RuntimeCapabilityReadinessKindV2::ExactTarget, "role_b"),
        receipt(RuntimeCapabilityReadinessKindV2::Panel, "role_c"),
        receipt(RuntimeCapabilityReadinessKindV2::Serving, "role_d"),
        receipt(RuntimeCapabilityReadinessKindV2::Interaction, "role_e"),
    )
    .unwrap();
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
        readiness,
        paused,
        RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry),
    )
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
