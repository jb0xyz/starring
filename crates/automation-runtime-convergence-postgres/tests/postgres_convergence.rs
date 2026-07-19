use std::future::Future;
use std::num::NonZeroU32;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use automation_ruleset::{RuleSetContentHash, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    ControllerId, DeploymentId, DrainAttestationV1, GatewayReadyAttestationV1, GatewayReadyKindV1,
    InstallationId, PanelCertificateId, PanelCertificateV1, PreflightAttestationV1,
    ProcessInstanceId, PromotionId, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_convergence_postgres::{
    prepare_requested_deployment_v1, ClaimDeploymentV1, ClaimNextDeploymentV1,
    DeploymentAvailabilityV1, DeploymentMutationV1, EnqueueDeploymentOutcomeV1,
    EnqueueDeploymentV1, GatewayShardIdV1, HeartbeatServingLeaseV1, LiveMetadataV1,
    MarkServingDisconnectedV1, PanelReportDigestV1, PostgresRuntimeConvergence,
    PostgresRuntimeConvergenceConfigV1, RecoverStaleLiveV1, RuntimeBuildRevisionV1,
    RuntimeConvergenceStoreError, RuntimeDeploymentScopeV1, SubmitDeploymentMutationV1,
    SubmitLiveAttestationV1, MIGRATOR,
};
use chrono::{DateTime, TimeDelta, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
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
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROTATED_BINDING_FINGERPRINT: &str =
    "7777777777777777777777777777777777777777777777777777777777777777";

include!("postgres_convergence/migration_acl.rs");
include!("postgres_convergence/binding_authority.rs");
include!("postgres_convergence/lifecycle.rs");
include!("postgres_convergence/support.rs");
