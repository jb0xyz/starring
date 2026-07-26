use std::time::Duration;

use automation_runtime_interaction_postgres::{
    PostgresRuntimeInteractionV1, RuntimeInteractionDatabaseExpectationV1,
    RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionErrorClassV1,
};
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
async fn verified_factory_fails_closed_when_database_is_unreachable() {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_millis(100))
        .connect_lazy(
            "postgresql://starring_runtime_interaction:unused@127.0.0.1:1/starring_runtime",
        )
        .unwrap();
    let expectation = RuntimeInteractionDatabaseExpectationV1::new(
        "01234567-89ab-cdef-8123-456789abcdef",
        "starring_runtime",
        "starring_runtime_interaction",
    )
    .unwrap();
    let timeouts = RuntimeInteractionDatabaseTimeoutsV1::new(
        Duration::from_millis(100),
        Duration::from_millis(50),
    )
    .unwrap();
    let error = PostgresRuntimeInteractionV1::connect_verified(pool, expectation, timeouts)
        .await
        .err()
        .expect("unreachable database must not produce an adapter");
    assert!(matches!(
        error.class(),
        RuntimeInteractionErrorClassV1::Timeout | RuntimeInteractionErrorClassV1::Unavailable
    ));
}
