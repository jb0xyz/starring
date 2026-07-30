mod connection;
mod contract;
mod database;
mod error;
mod row;
mod store;
mod v2;

pub use database::{
    verify_runtime_serving_database_v1, verify_runtime_serving_database_with_timeouts_v1,
    RuntimeServingDatabaseExpectationV1, RuntimeServingDatabaseReadinessV1,
    RuntimeServingDatabaseTimeoutsV1, DEFAULT_RUNTIME_SERVING_LOCK_TIMEOUT,
    DEFAULT_RUNTIME_SERVING_STATEMENT_TIMEOUT, MAX_RUNTIME_SERVING_DATABASE_TIMEOUT,
};
pub use error::RuntimeServingPersistenceErrorV1;
pub use store::{
    PostgresRuntimeServingLeaseV1, MAX_RUNTIME_SERVING_LEASE_DURATION,
    MIN_RUNTIME_SERVING_LEASE_DURATION,
};
pub use v2::RuntimeServingObservationV2;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
