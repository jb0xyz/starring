mod contract;
mod database;
mod error;
mod receipt;
mod receipt_row;
mod receipt_store;
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
pub use receipt::{
    RuntimeInteractionReceiptAuthorityV1, RuntimeInteractionReceiptClaimDuplicateV1,
    RuntimeInteractionReceiptClaimLeaseV1, RuntimeInteractionReceiptClaimOutcomeV1,
    RuntimeInteractionReceiptClaimRequestV1, RuntimeInteractionReceiptExclusiveClaimV1,
    RuntimeInteractionReceiptInitialResponseIntentDispositionV1,
    RuntimeInteractionReceiptInitialResponseIntentV1,
    RuntimeInteractionReceiptInitialResponseKindV1,
    RuntimeInteractionReceiptInitialResponseResultKindV1,
    RuntimeInteractionReceiptInitialResponseResultV1,
    RuntimeInteractionReceiptMutationDispositionV1, RuntimeInteractionReceiptOpaqueDigestV1,
    RuntimeInteractionReceiptRecoveredClaimV1, RuntimeInteractionReceiptRecoveryCandidateV1,
    RuntimeInteractionReceiptRecoveryDeferredReasonV1,
    RuntimeInteractionReceiptRecoveryObservationKindV1, RuntimeInteractionReceiptRecoveryOutcomeV1,
    RuntimeInteractionReceiptRecoveryRequestV1, RuntimeInteractionReceiptRecoveryRequiredReasonV1,
    RuntimeInteractionReceiptRecoveryScanCursorV1, RuntimeInteractionReceiptRecoveryScanKeyV1,
    RuntimeInteractionReceiptRecoveryScanPageV1, RuntimeInteractionReceiptRequestKindV1,
    RuntimeInteractionReceiptRouteV1, RuntimeInteractionReceiptTerminalOutcomeV1,
    RuntimeInteractionReceiptTerminalStateV1,
    RuntimeInteractionReceiptTerminalizeExpiredDispositionV1,
    RuntimeInteractionReceiptTerminalizeExpiredOutcomeV1,
    RuntimeInteractionReceiptTerminalizeExpiredRequestV1,
    RuntimeInteractionReceiptTokenExpiryDispositionV1,
    RuntimeInteractionReceiptTokenExpiryOutcomeV1, RuntimeInteractionReceiptTokenExpiryRequestV1,
    DEFAULT_RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE, MAX_RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE,
    MAX_RUNTIME_INTERACTION_RECEIPT_RECOVERY_SCAN_BATCH,
    MIN_RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE,
};
pub use route_timeout::{
    RuntimeInteractionRouteTimeoutV1, DEFAULT_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT,
    MAX_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT, MIN_RUNTIME_INTERACTION_ROUTE_READ_TIMEOUT,
};
pub use store::PostgresRuntimeInteractionV1;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
