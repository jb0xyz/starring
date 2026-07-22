mod contract;
mod database;
mod error;
mod row;
mod store;

pub use database::{
    verify_runtime_interaction_database_v1, verify_runtime_interaction_database_with_timeouts_v1,
    RuntimeInteractionDatabaseExpectationV1, RuntimeInteractionDatabaseReadinessV1,
    RuntimeInteractionDatabaseTimeoutsV1, DEFAULT_RUNTIME_INTERACTION_LOCK_TIMEOUT,
    DEFAULT_RUNTIME_INTERACTION_STATEMENT_TIMEOUT, MAX_RUNTIME_INTERACTION_DATABASE_TIMEOUT,
};
pub use error::{RuntimeInteractionErrorClassV1, RuntimeInteractionPersistenceErrorV1};
pub use store::PostgresRuntimeInteractionV1;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
