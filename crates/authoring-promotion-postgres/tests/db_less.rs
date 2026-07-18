use authoring_promotion_postgres::{PostgresPromotionStore, MIGRATOR};
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
    let _store = PostgresPromotionStore::new(pool);
    assert!(!MIGRATOR.iter().collect::<Vec<_>>().is_empty());
}
