mod artifact;
mod controller;
mod database_identity;
mod digest;
mod error;
mod evidence;
mod hydration;
mod model;
mod prepare;
mod projection;
mod row;
mod store;

pub use database_identity::PostgresRuntimeConvergenceDatabaseIdentityReader;
pub use error::RuntimeConvergenceStoreError;
pub use evidence::{
    project_runtime_deployment_status_v1, project_runtime_deployment_status_v2,
    RuntimeDeploymentStatusEvidenceV1, RuntimeDeploymentStatusEvidenceV2,
    RuntimeDeploymentStatusExpectationV1,
};
pub use hydration::{
    verify_runtime_exact_target_database_v1, verify_runtime_exact_target_database_with_timeouts_v1,
    PostgresRuntimeExactTargetReader, RuntimeExactTargetDatabaseExpectationV1,
    RuntimeExactTargetDatabaseReadinessV1, RuntimeExactTargetDatabaseTimeoutsV1,
    RuntimeExactTargetV1, DEFAULT_RUNTIME_EXACT_TARGET_LOCK_TIMEOUT,
    DEFAULT_RUNTIME_EXACT_TARGET_STATEMENT_TIMEOUT, MAX_RUNTIME_EXACT_TARGET_DATABASE_TIMEOUT,
};
pub use model::{
    AttestationIdV1, ClaimDeploymentV1, ClaimExecutionReceiptV1, ClaimNextDeploymentV1,
    ClaimReceiptV1, DeploymentAvailabilityV1, DeploymentMutationV1, EnqueueDeploymentOutcomeV1,
    EnqueueDeploymentV1, GatewayShardIdV1, HeartbeatServingLeaseV1, LiveMetadataV1,
    MarkServingDisconnectedV1, MutationReceiptV1, PanelReportDigestV1,
    PostgresRuntimeConvergenceConfigV1, RecoverBlockedDeploymentV1, RecoverStaleLiveV1,
    RenewDeploymentV1, RuntimeAttestationObservationV2, RuntimeBuildRevisionV1,
    RuntimeConvergenceAttemptV1, RuntimeDeploymentScopeV1, RuntimeDeploymentStatusV1,
    RuntimeDeploymentStatusV2, RuntimeServingFreshnessV2, RuntimeServingObservationV2,
    ServingLeaseIdentityV1, ServingLeaseReceiptV1, StrictLiveProjectionV1,
    SubmitDeploymentMutationV1, SubmitLiveAttestationV1,
};
pub use prepare::{prepare_requested_deployment_v1, PreparedRequestedDeploymentV1};
pub use store::PostgresRuntimeConvergence;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
