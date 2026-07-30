use std::collections::BTreeSet;
use std::time::Duration;

use automation_instance::{InstanceRegistrarV1, InstanceRouteReaderV1, InstanceTeardownStoreV1};
use automation_ruleset_dispatch::PinnedInstanceResolverV1;
use automation_runtime_interaction_postgres::{
    PostgresRuntimeInteractionV1, RuntimeInteractionDatabaseExpectationV1,
    RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionErrorClassV1,
    RuntimeInteractionPersistenceErrorV1, RuntimeInteractionRouteTimeoutV1,
    DEFAULT_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT, MAX_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT,
    MIGRATOR, MIN_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT,
};

fn assert_interaction_capabilities<T>()
where
    T: Clone
        + Send
        + Sync
        + InstanceRouteReaderV1
        + InstanceRegistrarV1
        + InstanceTeardownStoreV1
        + PinnedInstanceResolverV1,
{
}

#[test]
fn postgres_adapter_exposes_the_four_narrow_capabilities() {
    assert_interaction_capabilities::<PostgresRuntimeInteractionV1>();
}

#[test]
fn persistence_error_codes_and_classes_are_stable_and_unique() {
    let cases = [
        (
            RuntimeInteractionPersistenceErrorV1::InvalidInput,
            RuntimeInteractionErrorClassV1::InvalidInput,
            "runtime_interaction_invalid_input",
        ),
        (
            RuntimeInteractionPersistenceErrorV1::InvalidAuthority,
            RuntimeInteractionErrorClassV1::InvalidAuthority,
            "runtime_interaction_invalid_authority",
        ),
        (
            RuntimeInteractionPersistenceErrorV1::Conflict,
            RuntimeInteractionErrorClassV1::Conflict,
            "runtime_interaction_conflict",
        ),
        (
            RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt,
            RuntimeInteractionErrorClassV1::PersistenceCorrupt,
            "runtime_interaction_persistence_corrupt",
        ),
        (
            RuntimeInteractionPersistenceErrorV1::Timeout,
            RuntimeInteractionErrorClassV1::Timeout,
            "runtime_interaction_timeout",
        ),
        (
            RuntimeInteractionPersistenceErrorV1::Unavailable,
            RuntimeInteractionErrorClassV1::Unavailable,
            "runtime_interaction_unavailable",
        ),
        (
            RuntimeInteractionPersistenceErrorV1::Indeterminate,
            RuntimeInteractionErrorClassV1::Indeterminate,
            "runtime_interaction_indeterminate",
        ),
    ];
    let mut codes = BTreeSet::new();
    for (error, class, code) in cases {
        assert_eq!(error.class(), class);
        assert_eq!(error.code(), code);
        assert_eq!(error.to_string(), error.to_string().trim());
        assert!(codes.insert(code));
    }
    assert_eq!(codes.len(), cases.len());
}

#[test]
fn timeout_contract_accepts_only_bounded_whole_milliseconds() {
    let configured = RuntimeInteractionDatabaseTimeoutsV1::new(
        Duration::from_millis(2_000),
        Duration::from_millis(750),
    )
    .unwrap();
    assert_eq!(configured.statement_timeout(), Duration::from_secs(2));
    assert_eq!(configured.lock_timeout(), Duration::from_millis(750));
    for (statement, lock) in [
        (Duration::ZERO, Duration::from_millis(1)),
        (Duration::from_millis(1), Duration::ZERO),
        (Duration::from_millis(1), Duration::from_millis(1)),
        (Duration::from_millis(1), Duration::from_millis(2)),
        (Duration::from_secs(31), Duration::from_secs(1)),
        (Duration::from_secs(2), Duration::from_secs(31)),
        (Duration::from_millis(2), Duration::from_nanos(1)),
        (Duration::from_nanos(1), Duration::from_millis(1)),
    ] {
        assert_eq!(
            RuntimeInteractionDatabaseTimeoutsV1::new(statement, lock),
            Err(RuntimeInteractionPersistenceErrorV1::InvalidInput)
        );
    }
}

#[test]
fn route_timeout_contract_is_bounded_and_observable() {
    let defaults = RuntimeInteractionRouteTimeoutV1::default();
    assert_eq!(
        defaults.duration(),
        DEFAULT_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT
    );
    assert_eq!(
        RuntimeInteractionRouteTimeoutV1::new(MIN_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT)
            .unwrap()
            .duration(),
        MIN_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT
    );
    assert_eq!(
        RuntimeInteractionRouteTimeoutV1::new(MAX_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT)
            .unwrap()
            .duration(),
        MAX_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT
    );
    for invalid in [
        Duration::ZERO,
        Duration::from_nanos(1),
        MIN_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT - Duration::from_millis(1),
        MAX_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT + Duration::from_millis(1),
    ] {
        assert_eq!(
            RuntimeInteractionRouteTimeoutV1::new(invalid),
            Err(RuntimeInteractionPersistenceErrorV1::InvalidInput)
        );
    }
}

#[test]
fn database_expectation_accepts_only_canonical_identity_and_identifiers() {
    let expectation = RuntimeInteractionDatabaseExpectationV1::new(
        "01234567-89ab-cdef-8123-456789abcdef",
        "starring_runtime",
        "starring_runtime_interaction",
    )
    .unwrap();
    assert_eq!(
        expectation.database_identity(),
        "01234567-89ab-cdef-8123-456789abcdef"
    );
    assert_eq!(expectation.database_name(), "starring_runtime");
    assert_eq!(expectation.executor_role(), "starring_runtime_interaction");
    for (identity, database, role) in [
        (
            "00000000-0000-0000-0000-000000000000",
            "starring_runtime",
            "starring_runtime_interaction",
        ),
        (
            "01234567-89AB-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring_runtime_interaction",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdeg",
            "starring_runtime",
            "starring_runtime_interaction",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "StarringRuntime",
            "starring_runtime_interaction",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring-runtime",
            "starring_runtime_interaction",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring-runtime-interaction",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring_runtime",
            "1starring_runtime_interaction",
        ),
    ] {
        assert_eq!(
            RuntimeInteractionDatabaseExpectationV1::new(identity, database, role),
            Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
        );
    }
    let too_long = "a".repeat(64);
    assert_eq!(
        RuntimeInteractionDatabaseExpectationV1::new(
            "01234567-89ab-cdef-8123-456789abcdef",
            &too_long,
            "role"
        ),
        Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority)
    );
}

#[test]
fn interaction_database_migration_is_registered_after_convergence_identity() {
    let versions = MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let convergence_identity = versions
        .iter()
        .position(|version| *version == 202_607_220_025)
        .unwrap();
    let persisted_controller = versions
        .iter()
        .position(|version| *version == 202_607_220_026)
        .unwrap();
    let interaction = versions
        .iter()
        .position(|version| *version == 202_607_220_027)
        .unwrap();
    let teardown = versions
        .iter()
        .position(|version| *version == 202_607_300_004)
        .unwrap();
    assert!(convergence_identity < persisted_controller);
    assert_eq!(interaction, persisted_controller + 1);
    assert!(interaction < teardown);
}
