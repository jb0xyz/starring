use std::time::Duration;

use automation_runtime_convergence_postgres::{
    PostgresRuntimeConvergence, PostgresRuntimeConvergenceConfigV1, MIGRATOR,
};
use sqlx::postgres::PgPoolOptions;

#[test]
fn adapter_constructs_without_connecting() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://localhost/starring_test")
        .unwrap();
    let _adapter = PostgresRuntimeConvergence::new(pool);
    assert!(!MIGRATOR.iter().collect::<Vec<_>>().is_empty());
}

#[test]
fn adapter_rejects_unbounded_database_waits() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://localhost/starring_test")
        .unwrap();
    let config = PostgresRuntimeConvergenceConfigV1 {
        lock_timeout: Duration::from_secs(3),
        statement_timeout: Duration::from_secs(2),
        ..PostgresRuntimeConvergenceConfigV1::default()
    };
    assert!(PostgresRuntimeConvergence::with_config(pool.clone(), config).is_err());

    let config = PostgresRuntimeConvergenceConfigV1 {
        statement_timeout: Duration::from_secs(31),
        ..PostgresRuntimeConvergenceConfigV1::default()
    };
    assert!(PostgresRuntimeConvergence::with_config(pool.clone(), config).is_err());

    let config = PostgresRuntimeConvergenceConfigV1 {
        lock_timeout: Duration::from_nanos(1),
        ..PostgresRuntimeConvergenceConfigV1::default()
    };
    assert!(PostgresRuntimeConvergence::with_config(pool, config).is_err());
}
