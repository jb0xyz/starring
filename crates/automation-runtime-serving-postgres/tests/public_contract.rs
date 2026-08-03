use std::collections::BTreeSet;
use std::num::NonZeroU64;
use std::time::Duration;

use automation_runtime_controller::{
    RuntimeConvergenceErrorClassV1, RuntimeDrainIntentIdV2, RuntimeServingLeasePort,
};
use automation_runtime_serving_postgres::{
    PostgresRuntimeServingLeaseV1, RuntimePendingDrainServingLookupV1,
    RuntimePendingDrainServingObservationV1, RuntimePendingDrainServingSourceEvidenceV1,
    RuntimeServingDatabaseExpectationV1, RuntimeServingDatabaseTimeoutsV1,
    RuntimeServingPersistenceErrorV1, MAX_RUNTIME_SERVING_LEASE_DURATION,
    MIN_RUNTIME_SERVING_LEASE_DURATION,
};

fn assert_serving_port<T>()
where
    T: Clone + Send + Sync + RuntimeServingLeasePort,
{
}

#[test]
fn postgres_adapter_exposes_only_the_narrow_serving_port() {
    assert_serving_port::<PostgresRuntimeServingLeaseV1>();
}

#[test]
fn pending_drain_lookup_is_exact_and_redacted() {
    let lookup = RuntimePendingDrainServingLookupV1::new(
        RuntimeDrainIntentIdV2::parse("00112233445566778899aabbccddeeff").unwrap(),
        NonZeroU64::new(7).unwrap(),
        [0xab; 32],
    )
    .unwrap();
    assert_eq!(
        lookup.intent_id().as_str(),
        "00112233445566778899aabbccddeeff"
    );
    assert_eq!(lookup.source_intent_revision().get(), 7);
    assert_eq!(lookup.source_state_digest(), &[0xab; 32]);
    assert_eq!(
        format!("{lookup:?}"),
        "RuntimePendingDrainServingLookupV1(<redacted>)"
    );
    let source = RuntimePendingDrainServingSourceEvidenceV1::from(&lookup);
    assert_eq!(source.intent_id(), lookup.intent_id());
    assert_eq!(
        source.source_intent_revision(),
        lookup.source_intent_revision()
    );
    assert_eq!(source.source_state_digest(), lookup.source_state_digest());
    assert_eq!(
        format!("{source:?}"),
        "RuntimePendingDrainServingSourceEvidenceV1(<redacted>)"
    );
    let observation: Option<RuntimePendingDrainServingObservationV1> = None;
    assert!(observation.is_none());
}

#[test]
fn persistence_error_codes_and_classes_are_stable_and_unique() {
    let cases = [
        (
            RuntimeServingPersistenceErrorV1::InvalidInput,
            RuntimeConvergenceErrorClassV1::InvalidState,
            "runtime_serving_invalid_input",
        ),
        (
            RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch,
            RuntimeConvergenceErrorClassV1::InvalidState,
            "runtime_serving_database_authority_mismatch",
        ),
        (
            RuntimeServingPersistenceErrorV1::OwnershipLost,
            RuntimeConvergenceErrorClassV1::OwnershipLost,
            "runtime_serving_ownership_lost",
        ),
        (
            RuntimeServingPersistenceErrorV1::AuthorityChanged,
            RuntimeConvergenceErrorClassV1::AuthorityBlocked,
            "runtime_serving_authority_changed",
        ),
        (
            RuntimeServingPersistenceErrorV1::PersistenceCorrupt,
            RuntimeConvergenceErrorClassV1::InvalidState,
            "runtime_serving_persistence_corrupt",
        ),
        (
            RuntimeServingPersistenceErrorV1::RetryNotReady,
            RuntimeConvergenceErrorClassV1::RetryNotReady,
            "runtime_serving_retry_not_ready",
        ),
        (
            RuntimeServingPersistenceErrorV1::Timeout,
            RuntimeConvergenceErrorClassV1::Retryable,
            "runtime_serving_timeout",
        ),
        (
            RuntimeServingPersistenceErrorV1::Concurrency,
            RuntimeConvergenceErrorClassV1::Retryable,
            "runtime_serving_concurrency",
        ),
        (
            RuntimeServingPersistenceErrorV1::Unavailable,
            RuntimeConvergenceErrorClassV1::Retryable,
            "runtime_serving_unavailable",
        ),
        (
            RuntimeServingPersistenceErrorV1::DatabaseFailure,
            RuntimeConvergenceErrorClassV1::InvalidState,
            "runtime_serving_database_failure",
        ),
        (
            RuntimeServingPersistenceErrorV1::Indeterminate,
            RuntimeConvergenceErrorClassV1::Retryable,
            "runtime_serving_indeterminate",
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
fn replay_safe_failures_remain_retryable() {
    for error in [
        RuntimeServingPersistenceErrorV1::Timeout,
        RuntimeServingPersistenceErrorV1::Concurrency,
        RuntimeServingPersistenceErrorV1::Unavailable,
        RuntimeServingPersistenceErrorV1::Indeterminate,
    ] {
        assert_eq!(
            <PostgresRuntimeServingLeaseV1 as RuntimeServingLeasePort>::classify_error(&error),
            RuntimeConvergenceErrorClassV1::Retryable
        );
    }
}

#[test]
fn timeout_and_lease_contracts_are_bounded() {
    let configured = RuntimeServingDatabaseTimeoutsV1::new(
        Duration::from_millis(2_000),
        Duration::from_millis(750),
    )
    .unwrap();
    assert_eq!(configured.statement_timeout(), Duration::from_secs(2));
    assert_eq!(configured.lock_timeout(), Duration::from_millis(750));
    assert_eq!(MAX_RUNTIME_SERVING_LEASE_DURATION, Duration::from_secs(300));
    assert_eq!(MIN_RUNTIME_SERVING_LEASE_DURATION, Duration::from_secs(1));
    for (statement, lock) in [
        (Duration::ZERO, Duration::from_millis(1)),
        (Duration::from_millis(1), Duration::ZERO),
        (Duration::from_millis(1), Duration::from_millis(1)),
        (Duration::from_millis(1), Duration::from_millis(2)),
        (Duration::from_secs(31), Duration::from_secs(1)),
        (Duration::from_millis(2), Duration::from_nanos(1)),
    ] {
        assert_eq!(
            RuntimeServingDatabaseTimeoutsV1::new(statement, lock),
            Err(RuntimeServingPersistenceErrorV1::InvalidInput)
        );
    }
}

#[test]
fn database_expectation_accepts_only_canonical_authority() {
    let expectation = RuntimeServingDatabaseExpectationV1::new(
        "01234567-89ab-cdef-8123-456789abcdef",
        "starring_runtime",
        "starring_runtime_serving",
    )
    .unwrap();
    assert_eq!(
        expectation.database_identity(),
        "01234567-89ab-cdef-8123-456789abcdef"
    );
    assert_eq!(expectation.database_name(), "starring_runtime");
    assert_eq!(expectation.executor_role(), "starring_runtime_serving");
    for (identity, database, role) in [
        (
            "00000000-0000-0000-0000-000000000000",
            "starring_runtime",
            "starring_runtime_serving",
        ),
        (
            "01234567-89AB-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring_runtime_serving",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "StarringRuntime",
            "starring_runtime_serving",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring-runtime",
            "starring_runtime_serving",
        ),
        (
            "01234567-89ab-cdef-8123-456789abcdef",
            "starring_runtime",
            "starring-runtime-serving",
        ),
    ] {
        assert_eq!(
            RuntimeServingDatabaseExpectationV1::new(identity, database, role),
            Err(RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch)
        );
    }
}
