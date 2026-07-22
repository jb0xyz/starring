use std::collections::BTreeSet;
use std::time::Duration;

use automation_runtime_controller::{
    RuntimeAcquireGatewayOwnerLeaseV1, RuntimeCertificationRequestV1,
    RuntimeConvergenceErrorClassV1, RuntimeExecutionConvergencePort, RuntimeMutationRequestV1,
    RuntimeObserveGatewayOwnerLeaseV1, RuntimeObservePreviousServingV1,
    RuntimePreviousServingObservationPort, RuntimeReleaseGatewayOwnerLeaseV1,
    RuntimeRenewGatewayOwnerLeaseV1,
};
use automation_runtime_execution_postgres::{
    observe_runtime_execution_database_identity_v1,
    observe_runtime_execution_database_identity_with_timeouts_v1, PostgresRuntimeExecutionV1,
    RuntimeExecutionDatabaseExpectationV1, RuntimeExecutionDatabaseIdentityObservationV1,
    RuntimeExecutionDatabaseTimeoutsV1, RuntimeExecutionPersistenceErrorV1,
    MAX_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION, MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION,
    MIN_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION, MIN_RUNTIME_GATEWAY_OWNER_LEASE_DURATION,
};
use automation_runtime_worker::RuntimeGatewayOwnerLeasePortV1;

fn assert_mutate_signature(
    adapter: &PostgresRuntimeExecutionV1,
    request: RuntimeMutationRequestV1,
) {
    std::mem::drop(adapter.mutate(request));
}

fn assert_certification_signature(
    adapter: &PostgresRuntimeExecutionV1,
    request: RuntimeCertificationRequestV1,
) {
    std::mem::drop(adapter.certify_live(request));
}

fn assert_observation_signature(
    adapter: &PostgresRuntimeExecutionV1,
    request: RuntimeObservePreviousServingV1,
) {
    std::mem::drop(adapter.observe_previous_serving(request));
}

fn assert_execution_port<T>()
where
    T: RuntimeExecutionConvergencePort<Error = RuntimeExecutionPersistenceErrorV1>
        + RuntimePreviousServingObservationPort,
{
}

fn assert_gateway_owner_port<T>()
where
    T: RuntimeGatewayOwnerLeasePortV1<Error = RuntimeExecutionPersistenceErrorV1>,
{
}

fn assert_gateway_owner_signatures(
    adapter: &PostgresRuntimeExecutionV1,
    observe: RuntimeObserveGatewayOwnerLeaseV1,
    acquire: RuntimeAcquireGatewayOwnerLeaseV1,
    renew: RuntimeRenewGatewayOwnerLeaseV1,
    release: RuntimeReleaseGatewayOwnerLeaseV1,
) {
    std::mem::drop(adapter.observe_gateway_owner(observe));
    std::mem::drop(adapter.acquire_gateway_owner(acquire));
    std::mem::drop(adapter.renew_gateway_owner(renew));
    std::mem::drop(adapter.release_gateway_owner(release));
}

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

#[test]
fn verified_adapter_exposes_the_scoped_execution_contract() {
    let _ = assert_mutate_signature;
    let _ = assert_certification_signature;
    let _ = assert_observation_signature;
    let _ = PostgresRuntimeExecutionV1::recover_next_stale_live;
    assert_execution_port::<PostgresRuntimeExecutionV1>();
    assert_gateway_owner_port::<PostgresRuntimeExecutionV1>();
    let _ = assert_gateway_owner_signatures;
    assert_eq!(
        MIN_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION,
        Duration::from_secs(1)
    );
    assert_eq!(
        MAX_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION,
        Duration::from_secs(300)
    );
    assert_eq!(
        MIN_RUNTIME_GATEWAY_OWNER_LEASE_DURATION,
        Duration::from_secs(1)
    );
    assert_eq!(
        MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION,
        Duration::from_secs(300)
    );
}

#[test]
fn execution_database_identity_observation_is_public_and_read_only() {
    let _ = observe_runtime_execution_database_identity_v1;
    let _ = observe_runtime_execution_database_identity_with_timeouts_v1;
    let _ = std::mem::size_of::<RuntimeExecutionDatabaseIdentityObservationV1>();
}
