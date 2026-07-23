use std::num::NonZeroU64;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use authoring_application_postgres::MIGRATOR;
use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, DeploymentId, InstallationId, PromotionId,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeGeneration,
    RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_convergence_postgres::{
    prepare_requested_deployment_v1, EnqueueDeploymentV1, PostgresRuntimeConvergence,
    PreparedRequestedDeploymentV1, RuntimeDeploymentScopeV1,
};
use chrono::{DateTime, TimeDelta, Utc};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, RoleId};
use resource_resolution::{
    approval_binding_fingerprint_v1, resource_binding_fingerprint_v2, ResolvedApprovalBinding,
    ResourceBindingFingerprint, ResourceBindingMap,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::types::Json;
use sqlx::{Connection, Postgres, Transaction};

include!("postgres_product_apply/database_fixture.rs");
include!("postgres_product_apply/apply_support.rs");
include!("postgres_product_apply/apply_semantics.rs");
include!("postgres_product_apply/authority_drift.rs");
include!("postgres_product_apply/security_concurrency.rs");
include!("postgres_product_apply/product_apply_serving_slot.rs");
include!("postgres_product_apply/migration_security.rs");
