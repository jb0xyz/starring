use std::future::Future;
use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use automation_ruleset::{RuleSetContentHash, RuleSetVersionId};
use automation_runtime_controller::{
    runtime_desired_target_digest_v1, RuntimeConvergenceSessionV1, RuntimeExecutionReceiptV1,
    RuntimePreviousServingObservationPort, RuntimePreviousServingStateV1,
};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    ControllerId, ControllerLeaseV1, DeploymentId, DrainAttestationV1, FencingToken,
    GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, PanelCertificateId,
    PanelCertificateV1, PreflightAttestationV1, ProcessInstanceId, PromotionId,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeGeneration,
    RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_convergence_postgres::{
    prepare_requested_deployment_v1, ClaimDeploymentV1, ClaimNextDeploymentV1,
    DeploymentAvailabilityV1, DeploymentMutationV1, EnqueueDeploymentOutcomeV1,
    EnqueueDeploymentV1, GatewayShardIdV1, HeartbeatServingLeaseV1, LiveMetadataV1,
    MarkServingDisconnectedV1, PanelReportDigestV1, PostgresRuntimeConvergence,
    PostgresRuntimeConvergenceConfigV1, PostgresRuntimeExactTargetReader,
    RecoverBlockedDeploymentV1, RecoverStaleLiveV1, RenewDeploymentV1, RuntimeBuildRevisionV1,
    RuntimeConvergenceStoreError, RuntimeDeploymentScopeV1,
    RuntimeExactTargetDatabaseExpectationV1, RuntimeExactTargetDatabaseTimeoutsV1,
    SubmitDeploymentMutationV1, SubmitLiveAttestationV1, MIGRATOR,
};
use chrono::{DateTime, TimeDelta, Utc};
use discord_model::GuildId;
use resource_resolution::{
    installation_authority_payload_digest_v1, InstallationAuthorityPayloadIdentityV1,
    InstallationAuthorityPolicyV1, InstallationAuthorityScopeV1, ResourceBindingFingerprint,
};
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use sqlx::types::Json;
use sqlx::{Connection, PgPool};

const TENANT: &str = "runtime-pg-tenant";
const INSTALLATION: &str = "runtime-pg-installation";
const PRINCIPAL: &str = "runtime-pg-principal";
const GUILD: GuildId = GuildId(9200101);
const RULESET: &str = "runtime_pg_ruleset";
const PROMOTION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACTIVATION: &str = "runtime_pg_activation";
const DEPLOYMENT: &str = "runtime-pg-deployment";
const NEXT_PROMOTION: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const NEXT_ACTIVATION: &str = "runtime_pg_activation_next";
const NEXT_DEPLOYMENT: &str = "runtime-pg-deployment-next";
const CONTENT_HASH: &str = "9f2bbed3d90d3439ebe5bb07a69f8ff179c29e8c71500b6890a7d24653a65ff6";
const NEXT_CONTENT_HASH: &str = "91d936ba08910497f8f31e16e7f2b1ffce5ee9447a4636d47ddddc5c79fb0103";
const BINDING_FINGERPRINT: &str =
    "a44fd4f629a1183147a25a8afb93b026de7e3f92efe737637da222617df0c655";
const ROTATED_BINDING_FINGERPRINT: &str =
    "7777777777777777777777777777777777777777777777777777777777777777";

include!("postgres_convergence/migration_acl.rs");
include!("postgres_convergence/binding_authority.rs");
include!("postgres_convergence/convergence_attempt.rs");
include!("postgres_convergence/guard_exactness.rs");
include!("postgres_convergence/lifecycle.rs");
include!("postgres_convergence/hydration.rs");
include!("postgres_convergence/previous_serving.rs");
include!("postgres_convergence/accept_drain.rs");
include!("postgres_convergence/support.rs");
