mod contract;
mod database;
mod error;
mod route_connection;
mod route_timeout;
mod row;
mod store;

pub use database::{
    verify_runtime_interaction_database_v1, verify_runtime_interaction_database_with_timeouts_v1,
    RuntimeInteractionDatabaseExpectationV1, RuntimeInteractionDatabaseReadinessV1,
    RuntimeInteractionDatabaseTimeoutsV1, DEFAULT_RUNTIME_INTERACTION_LOCK_TIMEOUT,
    DEFAULT_RUNTIME_INTERACTION_STATEMENT_TIMEOUT, MAX_RUNTIME_INTERACTION_DATABASE_TIMEOUT,
};
pub use error::{RuntimeInteractionErrorClassV1, RuntimeInteractionPersistenceErrorV1};
pub use route_timeout::{
    RuntimeInteractionRouteTimeoutV1, DEFAULT_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT,
    MAX_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT, MIN_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT,
};
pub use store::PostgresRuntimeInteractionV1;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
