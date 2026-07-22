use automation_runtime_worker::{
    RuntimeGatewayClosedLifecycleV2, RuntimeGatewayClosedSnapshotV2,
    RuntimeGatewayClosedTransitionErrorV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimeGatewayEmergencyCauseV2, RuntimeGatewayInvalidationCauseV2,
};

fn generation(snapshot: RuntimeGatewayClosedSnapshotV2) -> RuntimeGatewayCoordinatorGenerationV2 {
    snapshot.generation()
}

#[test]
fn initial_state_is_closed_and_cannot_represent_open() {
    let lifecycle = RuntimeGatewayClosedLifecycleV2::starting();

    assert_eq!(
        lifecycle.snapshot(),
        RuntimeGatewayClosedSnapshotV2::Emergency {
            generation: RuntimeGatewayCoordinatorGenerationV2::FIRST,
            cause: RuntimeGatewayEmergencyCauseV2::Starting,
        }
    );
}

#[test]
fn exact_invalidations_advance_and_stale_or_reordered_calls_fail() {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let first = generation(lifecycle.snapshot());
    let disconnected = lifecycle
        .invalidate(
            first,
            RuntimeGatewayInvalidationCauseV2::TransportDisconnected,
        )
        .unwrap();
    let second = generation(disconnected);

    assert_eq!(second.get(), first.get() + 1);
    assert_eq!(
        lifecycle.invalidate(first, RuntimeGatewayInvalidationCauseV2::ControlOrphaned),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration)
    );

    let repeated = lifecycle
        .invalidate(
            second,
            RuntimeGatewayInvalidationCauseV2::TransportDisconnected,
        )
        .unwrap();
    let third = generation(repeated);
    assert_eq!(third.get(), second.get() + 1);
    assert_eq!(
        lifecycle.invalidate(
            second,
            RuntimeGatewayInvalidationCauseV2::OwnershipUncertain,
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration)
    );
}

#[test]
fn shutdown_is_terminal_idempotent_and_generation_checked() {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let first = generation(lifecycle.snapshot());

    assert_eq!(
        lifecycle.shutdown(RuntimeGatewayCoordinatorGenerationV2::new(
            std::num::NonZeroU64::new(2).unwrap(),
        )),
        Err(RuntimeGatewayClosedTransitionErrorV2::StaleGeneration)
    );

    let shutdown = lifecycle.shutdown(first).unwrap();
    let terminal = generation(shutdown);
    assert_eq!(terminal.get(), first.get() + 1);
    assert_eq!(lifecycle.shutdown(terminal), Ok(shutdown));
    assert_eq!(
        lifecycle.invalidate(
            terminal,
            RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
        ),
        Err(RuntimeGatewayClosedTransitionErrorV2::Shutdown)
    );
}

#[test]
fn emergency_causes_are_finite_and_exhaustive() {
    fn code(cause: RuntimeGatewayEmergencyCauseV2) -> &'static str {
        match cause {
            RuntimeGatewayEmergencyCauseV2::Starting => "starting",
            RuntimeGatewayEmergencyCauseV2::TransportDisconnected => "transport_disconnected",
            RuntimeGatewayEmergencyCauseV2::ControlOrphaned => "control_orphaned",
            RuntimeGatewayEmergencyCauseV2::OwnershipUncertain => "ownership_uncertain",
            RuntimeGatewayEmergencyCauseV2::CapabilityNotReady => "capability_not_ready",
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation => "protocol_violation",
        }
    }

    assert_eq!(
        [
            RuntimeGatewayEmergencyCauseV2::Starting,
            RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
            RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
            RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
            RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        ]
        .map(code),
        [
            "starting",
            "transport_disconnected",
            "control_orphaned",
            "ownership_uncertain",
            "capability_not_ready",
            "protocol_violation",
        ]
    );
}

#[test]
fn startup_origin_cannot_be_reentered_through_invalidation() {
    fn emergency(cause: RuntimeGatewayInvalidationCauseV2) -> RuntimeGatewayEmergencyCauseV2 {
        cause.into()
    }

    assert_eq!(
        [
            RuntimeGatewayInvalidationCauseV2::TransportDisconnected,
            RuntimeGatewayInvalidationCauseV2::ControlOrphaned,
            RuntimeGatewayInvalidationCauseV2::OwnershipUncertain,
            RuntimeGatewayInvalidationCauseV2::CapabilityNotReady,
            RuntimeGatewayInvalidationCauseV2::ProtocolViolation,
        ]
        .map(emergency),
        [
            RuntimeGatewayEmergencyCauseV2::TransportDisconnected,
            RuntimeGatewayEmergencyCauseV2::ControlOrphaned,
            RuntimeGatewayEmergencyCauseV2::OwnershipUncertain,
            RuntimeGatewayEmergencyCauseV2::CapabilityNotReady,
            RuntimeGatewayEmergencyCauseV2::ProtocolViolation,
        ]
    );
}
