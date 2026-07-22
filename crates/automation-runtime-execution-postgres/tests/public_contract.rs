use std::collections::BTreeSet;
use std::time::Duration;

use automation_runtime_controller::RuntimeConvergenceErrorClassV1;
use automation_runtime_execution_postgres::{
    RuntimeExecutionDatabaseExpectationV1, RuntimeExecutionDatabaseTimeoutsV1,
    RuntimeExecutionPersistenceErrorV1,
};

#[test]
fn persistence_error_codes_and_classes_are_stable_and_unique() {
    let cases = [
        (
            RuntimeExecutionPersistenceErrorV1::InvalidInput,
            RuntimeConvergenceErrorClassV1::InvalidState,
            "runtime_execution_invalid_input",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch,
            RuntimeConvergenceErrorClassV1::InvalidState,
            "runtime_execution_database_authority_mismatch",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::OwnershipLost,
            RuntimeConvergenceErrorClassV1::OwnershipLost,
            "runtime_execution_ownership_lost",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::AuthorityChanged,
            RuntimeConvergenceErrorClassV1::AuthorityBlocked,
            "runtime_execution_authority_changed",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
            RuntimeConvergenceErrorClassV1::InvalidState,
            "runtime_execution_persistence_corrupt",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::RetryNotReady,
            RuntimeConvergenceErrorClassV1::RetryNotReady,
            "runtime_execution_retry_not_ready",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::Superseded,
            RuntimeConvergenceErrorClassV1::Superseded,
            "runtime_execution_superseded",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::Timeout,
            RuntimeConvergenceErrorClassV1::Retryable,
            "runtime_execution_timeout",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::Concurrency,
            RuntimeConvergenceErrorClassV1::Retryable,
            "runtime_execution_concurrency",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::Unavailable,
            RuntimeConvergenceErrorClassV1::Retryable,
            "runtime_execution_unavailable",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::DatabaseFailure,
            RuntimeConvergenceErrorClassV1::InvalidState,
            "runtime_execution_database_failure",
        ),
        (
            RuntimeExecutionPersistenceErrorV1::Indeterminate,
            RuntimeConvergenceErrorClassV1::Retryable,
            "runtime_execution_indeterminate",
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
fn timeout_contract_is_bounded() {
    let configured = RuntimeExecutionDatabaseTimeoutsV1::new(
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
        (Duration::from_millis(2), Duration::from_nanos(1)),
    ] {
        assert_eq!(
            RuntimeExecutionDatabaseTimeoutsV1::new(statement, lock),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
    }
}

#[test]
fn database_expectation_accepts_only_canonical_authority() {
    let expectation = RuntimeExecutionDatabaseExpectationV1::new(
        "01234567-89ab-cdef-8123-456789abcdef",
        "starring_runtime",
        "starring_runtime_execution",
    )
    .unwrap();
    assert_eq!(
        expectation.database_identity(),
        "01234567-89ab-cdef-8123-456789abcdef"
    );
    assert_eq!(expectation.database_name(), "starring_runtime");
    assert_eq!(expectation.executor_role(), "starring_runtime_execution");
    for (identity, database, role) in [
        (
            "00000000-0000-0000-0000-000000000000",
            "starring_runtime",
            "starring_runtime_execution",
        ),
        (
            "01234567-89AB-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring_runtime_execution",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "StarringRuntime",
            "starring_runtime_execution",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring-runtime",
            "starring_runtime_execution",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring-runtime-execution",
        ),
    ] {
        assert_eq!(
            RuntimeExecutionDatabaseExpectationV1::new(identity, database, role),
            Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch)
        );
    }
}
