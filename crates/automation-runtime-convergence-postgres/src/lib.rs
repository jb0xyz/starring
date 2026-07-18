mod digest;
mod error;
mod model;
mod row;
mod store;

pub use error::RuntimeConvergenceStoreError;
pub use model::{
    AttestationIdV1, ClaimDeploymentV1, ClaimNextDeploymentV1, ClaimReceiptV1,
    DeploymentAvailabilityV1, DeploymentMutationV1, EnqueueDeploymentOutcomeV1,
    EnqueueDeploymentV1, GatewayShardIdV1, HeartbeatServingLeaseV1, LiveMetadataV1,
    MarkServingDisconnectedV1, MutationReceiptV1, PanelReportDigestV1,
    PostgresRuntimeConvergenceConfigV1, RecoverStaleLiveV1, RuntimeBuildRevisionV1,
    RuntimeDeploymentScopeV1, RuntimeDeploymentStatusV1, ServingLeaseIdentityV1,
    ServingLeaseReceiptV1, StrictLiveProjectionV1, SubmitDeploymentMutationV1,
    SubmitLiveAttestationV1,
};
pub use store::PostgresRuntimeConvergence;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
