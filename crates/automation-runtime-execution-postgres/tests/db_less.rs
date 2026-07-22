use std::time::Duration;

use automation_runtime_controller::RuntimeConvergenceErrorClassV1;
use automation_runtime_execution_postgres::{
    PostgresRuntimeExecutionV1, RuntimeExecutionDatabaseExpectationV1,
    RuntimeExecutionDatabaseTimeoutsV1,
};
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
async fn verified_factory_fails_closed_when_database_is_unreachable() {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_millis(100))
        .connect_lazy("postgresql://starring_runtime_execution:unused@127.0.0.1:1/starring_runtime")
        .unwrap();
    let expectation = RuntimeExecutionDatabaseExpectationV1::new(
        "01234567-89ab-cdef-8123-456789abcdef",
        "starring_runtime",
        "starring_runtime_execution",
    )
    .unwrap();
    let timeouts = RuntimeExecutionDatabaseTimeoutsV1::new(
        Duration::from_millis(100),
        Duration::from_millis(50),
    )
    .unwrap();
    let error = PostgresRuntimeExecutionV1::connect_verified(pool, expectation, timeouts)
        .await
        .err()
        .expect("unreachable database must not produce an adapter");
    assert_eq!(error.class(), RuntimeConvergenceErrorClassV1::Retryable);
}
