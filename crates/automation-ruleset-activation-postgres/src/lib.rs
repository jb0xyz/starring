mod row;
mod store;

pub use store::PostgresActivationRequestStore;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
