mod row;
mod store;

pub use store::PostgresPromotionStore;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
