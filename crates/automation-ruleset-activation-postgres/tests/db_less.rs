use automation_ruleset_activation_postgres::{PostgresActivationRequestStore, MIGRATOR};
use sqlx::postgres::PgPoolOptions;

#[test]
fn store_constructs_without_connecting() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://localhost/starring_test")
        .unwrap();
    let _store = PostgresActivationRequestStore::new(pool);
    assert!(!MIGRATOR.iter().collect::<Vec<_>>().is_empty());
}
