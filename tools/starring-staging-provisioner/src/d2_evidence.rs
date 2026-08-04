use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::str::FromStr;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgRow, Postgres};
use sqlx::{Connection, Decode, PgConnection, Row, Type};

use crate::d2::{
    connect_inspection_admin_database, connect_inspection_database, load_config,
    verify_d2_destruction_keychain_contract, D2ConfigV1, D2ProvisionerErrorV1,
};

const RULESET_KEY: &str = "studyroom";
const DATABASE_NAME: &str = "starring_runtime_staging";
const ROUTE_IDENTITY_KIND: &str = "starring.d2.route-identity.v1";
const SERVING_IDENTITY_KIND: &str = "starring.d2.serving-identity.v1";
const EFFECT_IDENTITY_KIND: &str = "starring.d2.effect-identity.v1";
const DESTROY_KIND: &str = "starring.d2.database-destruction.v1";
const PROVISIONER_ADVISORY_LOCK_SQL: &str = "SELECT pg_catalog.pg_try_advisory_lock(pg_catalog.hashtextextended('starring-d2-sealed-provisioner:' || $1, 0))";
const DESTROY_ADVISORY_LOCK_SQL: &str = "SELECT pg_catalog.pg_try_advisory_lock(pg_catalog.hashtextextended('starring-d2-sealed-destroy:' || $1, 0))";
const DROP_DATABASE_SQL: &str = "DROP DATABASE starring_runtime_staging";
const EXPECTED_MIGRATION_COUNT: i64 = 125;
const EXPECTED_MIGRATION_HEAD: i64 = 202608040004;
const EXPECTED_MIGRATION_HEAD_CHECKSUM: &str =
    "2ac0c69bfa9bd5f99c092bdf1d8ac06510bc0c467c8a17cd62a0412f3f409a1128d4afbe5ca2136b77c34eadd91c3056";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D2InspectionCheckpointV1 {
    Authoring,
    Live,
    Interaction,
    Duplicate,
    Restart,
    Reconciliation,
    Replacement,
    Precleanup,
    Absence,
}

impl D2InspectionCheckpointV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Authoring => "authoring",
            Self::Live => "live",
            Self::Interaction => "interaction",
            Self::Duplicate => "duplicate",
            Self::Restart => "restart",
            Self::Reconciliation => "reconciliation",
            Self::Replacement => "replacement",
            Self::Precleanup => "precleanup",
            Self::Absence => "absence",
        }
    }

    const fn kind(self) -> &'static str {
        match self {
            Self::Authoring => "starring.d2.db-authoring-evidence.v1",
            Self::Live => "starring.d2.db-live-evidence.v1",
            Self::Interaction => "starring.d2.db-interaction-evidence.v1",
            Self::Duplicate => "starring.d2.db-duplicate-evidence.v1",
            Self::Restart => "starring.d2.db-reconstruction-evidence.v1",
            Self::Reconciliation => "starring.d2.db-reconciliation-evidence.v1",
            Self::Replacement => "starring.d2.db-replacement-evidence.v1",
            Self::Precleanup => "starring.d2.db-precleanup-evidence.v1",
            Self::Absence => "starring.d2.db-absence-evidence.v1",
        }
    }
}

impl Display for D2InspectionCheckpointV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for D2InspectionCheckpointV1 {
    type Err = D2ProvisionerErrorV1;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "authoring" => Ok(Self::Authoring),
            "live" => Ok(Self::Live),
            "interaction" => Ok(Self::Interaction),
            "duplicate" => Ok(Self::Duplicate),
            "restart" => Ok(Self::Restart),
            "reconciliation" => Ok(Self::Reconciliation),
            "replacement" => Ok(Self::Replacement),
            "precleanup" => Ok(Self::Precleanup),
            "absence" => Ok(Self::Absence),
            _ => Err(D2ProvisionerErrorV1::Arguments),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct D2InspectionReportV1 {
    schema_version: u32,
    kind: &'static str,
    observed_at: String,
    #[serde(flatten)]
    evidence: D2InspectionEvidenceV1,
}

impl D2InspectionReportV1 {
    pub fn to_json(&self) -> Result<String, D2ProvisionerErrorV1> {
        serde_json::to_string(self).map_err(|_| D2ProvisionerErrorV1::Inspection)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum D2DestroyOutcomeV1 {
    Destroyed,
    ExactReplay,
}

impl D2DestroyOutcomeV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Destroyed => "destroyed",
            Self::ExactReplay => "exact_replay",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct D2DestroyReportV1 {
    schema_version: u32,
    kind: &'static str,
    outcome: D2DestroyOutcomeV1,
    installation_id: String,
    database_absent: bool,
}

impl D2DestroyReportV1 {
    pub const fn outcome(&self) -> D2DestroyOutcomeV1 {
        self.outcome
    }

    pub const fn database_absent(&self) -> bool {
        self.database_absent
    }

    pub fn to_json(&self) -> Result<String, D2ProvisionerErrorV1> {
        serde_json::to_string(self).map_err(|_| D2ProvisionerErrorV1::Destruction)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum D2DestructionPlanV1 {
    Drop,
    ExactReplay,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
enum D2InspectionEvidenceV1 {
    Authoring(Box<AuthoringEvidenceV1>),
    Live(Box<LiveEvidenceV1>),
    Interaction(Box<InteractionEvidenceV1>),
    Duplicate(Box<DuplicateEvidenceV1>),
    Restart(Box<RestartEvidenceV1>),
    Reconciliation(Box<ReconciliationEvidenceV1>),
    Replacement(Box<ReplacementEvidenceV1>),
    Precleanup(Box<PrecleanupEvidenceV1>),
    Absence(Box<AbsenceEvidenceV1>),
}

#[derive(Clone, Debug, Serialize)]
struct AuthoringEvidenceV1 {
    generation_encrypted: bool,
    projection_state: String,
    generation: i64,
    generation_count: i64,
    payload_digest: String,
    worker_request_id: String,
    worker_completion_sha256: String,
    installation_id: String,
    authoring_session_id: String,
    generation_created_at: String,
}

#[derive(Clone, Debug, Serialize)]
struct LiveEvidenceV1 {
    installation_id: String,
    promotion_id: String,
    deployment_id: String,
    attestation_id: String,
    deployment_revision: i64,
    convergence_attempt: i64,
    process_instance_id: String,
    last_heartbeat_at: String,
    lease_expires_at: String,
    route_identity: RouteIdentityV1,
    serving_identity: ServingIdentityV1,
}

#[derive(Clone, Debug, Serialize)]
struct InteractionEvidenceV1 {
    create_interaction_id: String,
    join_interaction_id: String,
    actor_user_id: String,
    joined_role_id: String,
    deployment_id: String,
    route_identity: RouteIdentityV1,
    instance_id: String,
    role_ids: Vec<String>,
    channel_ids: Vec<String>,
    panel_message_ids: Vec<String>,
    ephemeral_count: i64,
}

#[derive(Clone, Debug, Serialize)]
struct DuplicateEvidenceV1 {
    interaction_id: String,
    effect_identity: EffectIdentityV1,
    external_effect_count: i64,
    receipt_state: String,
}

#[derive(Clone, Debug, Serialize)]
struct RestartEvidenceV1 {
    route_reconstructed: bool,
    instance_reconstructed: bool,
    deployment_id: String,
    source_route_identity: RouteIdentityV1,
    reconstructed_route_identity: RouteIdentityV1,
    source_serving_identity: ServingIdentityV1,
    reconstructed_serving_identity: ServingIdentityV1,
    instance_id: String,
    pinned_ruleset_digest: String,
    probe_interaction_id: String,
    process_instance_id: String,
}

#[derive(Clone, Debug, Serialize)]
struct ReconciliationEvidenceV1 {
    effect_identity: EffectIdentityV1,
    interaction_id: String,
    route_identity: RouteIdentityV1,
    output_role_id: String,
    reconciliation_state: String,
    duplicate_external_effect_count: i64,
    unsafe_deletion_count: i64,
}

#[derive(Clone, Debug, Serialize)]
struct ReplacementEvidenceV1 {
    installation_id: String,
    source_promotion_id: String,
    replacement_promotion_id: String,
    source_deployment_id: String,
    source_route_identity: RouteIdentityV1,
    replacement_deployment_id: String,
    replacement_route_identity: RouteIdentityV1,
    previous_target_drained: bool,
    replacement_live: bool,
    prior_route_absent: bool,
}

#[derive(Clone, Debug, Serialize)]
struct PrecleanupEvidenceV1 {
    installation_id: String,
    scoped_installation_count: i64,
    scoped_deployment_count: i64,
    terminal_product_operation_count: i64,
    unresolved_product_operation_count: i64,
    unresolved_receipt_count: i64,
    unresolved_journal_entry_count: i64,
    unresolved_rollback_count: i64,
    ready_for_cleanup: bool,
}

#[derive(Clone, Debug, Serialize)]
struct AbsenceEvidenceV1 {
    run_id: String,
    installation_id: String,
    database_absent: bool,
}

#[derive(Clone, Debug)]
struct InspectionScopeV1 {
    run_id: String,
    tenant_id: String,
    installation_id: String,
    guild_id: String,
    application_id: String,
}

impl InspectionScopeV1 {
    fn from_config(config: &D2ConfigV1) -> Self {
        Self {
            run_id: config.run_id.clone(),
            tenant_id: format!("tenant:{}", config.resource_prefix),
            installation_id: format!("installation:{}", config.resource_prefix),
            guild_id: config.discord_guild_id.clone(),
            application_id: config.discord_application_id.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct RouteIdentityV1 {
    deployment_id: String,
    runtime_generation: i64,
    route_controller_fencing_token: i64,
    route_incarnation: i64,
    origin_process_instance_id: String,
    origin_serving_lease_epoch: i64,
    origin_serving_revision: i64,
    origin_gateway_shard_id: String,
    origin_gateway_owner_lease_epoch: i64,
    origin_gateway_owner_revision: i64,
}

#[derive(Clone, Debug, Serialize)]
struct ServingIdentityV1 {
    guild_id: String,
    ruleset_key: String,
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    attestation_id: String,
    process_instance_id: String,
    runtime_generation: i64,
    target_version: i64,
    target_content_hash: String,
    binding_revision: i64,
    binding_fingerprint: String,
    lease_epoch: i64,
    revision: i64,
}

#[derive(Clone, Debug, Serialize)]
struct EffectIdentityV1 {
    application_id: String,
    interaction_id: String,
    action_index: i16,
}

struct LiveObservationV1 {
    route: RouteIdentityV1,
    serving: ServingIdentityV1,
    promotion_id: String,
    deployment_revision: i64,
    convergence_attempt: i64,
    process_instance_id: String,
    last_heartbeat_at: String,
    lease_expires_at: String,
}

pub async fn destroy_d2_from_manifest(
    manifest_path: &Path,
) -> Result<D2DestroyReportV1, D2ProvisionerErrorV1> {
    let config = load_config(manifest_path)?;
    let scope = InspectionScopeV1::from_config(&config);
    verify_d2_destruction_keychain_contract(&config)?;
    let mut admin = connect_inspection_admin_database(&config)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    configure_destroy_admin(&mut admin).await?;
    for statement in [PROVISIONER_ADVISORY_LOCK_SQL, DESTROY_ADVISORY_LOCK_SQL] {
        let acquired: bool = sqlx::query_scalar(statement)
            .bind(&scope.run_id)
            .fetch_one(&mut admin)
            .await
            .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
        if !acquired {
            return Err(D2ProvisionerErrorV1::Destruction);
        }
    }
    match destruction_plan(target_database_state(&mut admin).await?)? {
        D2DestructionPlanV1::ExactReplay => {
            verify_d2_destruction_keychain_contract(&config)?;
            Ok(destroy_report(&scope, D2DestroyOutcomeV1::ExactReplay))
        }
        D2DestructionPlanV1::Drop => {
            validate_destroy_target(&config, &scope, &mut admin).await?;
            if destruction_plan(target_database_state(&mut admin).await?)?
                != D2DestructionPlanV1::Drop
            {
                return Err(D2ProvisionerErrorV1::Destruction);
            }
            sqlx::query(DROP_DATABASE_SQL)
                .execute(&mut admin)
                .await
                .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
            if destruction_plan(target_database_state(&mut admin).await?)?
                != D2DestructionPlanV1::ExactReplay
            {
                return Err(D2ProvisionerErrorV1::Destruction);
            }
            verify_d2_destruction_keychain_contract(&config)?;
            Ok(destroy_report(&scope, D2DestroyOutcomeV1::Destroyed))
        }
    }
}

fn destroy_report(scope: &InspectionScopeV1, outcome: D2DestroyOutcomeV1) -> D2DestroyReportV1 {
    D2DestroyReportV1 {
        schema_version: 1,
        kind: DESTROY_KIND,
        outcome,
        installation_id: scope.installation_id.clone(),
        database_absent: true,
    }
}

async fn configure_destroy_admin(
    connection: &mut PgConnection,
) -> Result<(), D2ProvisionerErrorV1> {
    for setting in [
        "SET statement_timeout = '3s'",
        "SET lock_timeout = '500ms'",
        "SET idle_in_transaction_session_timeout = '5s'",
        "SET search_path = pg_catalog",
    ] {
        sqlx::query(setting)
            .execute(&mut *connection)
            .await
            .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    }
    Ok(())
}

async fn target_database_state(
    connection: &mut PgConnection,
) -> Result<(i64, i64), D2ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT (SELECT pg_catalog.count(*) FROM pg_catalog.pg_database WHERE datname = $1) AS database_count, (SELECT pg_catalog.count(*) FROM pg_catalog.pg_stat_activity WHERE datname = $1) AS backend_count",
    )
    .bind(DATABASE_NAME)
    .fetch_one(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    Ok((
        row.try_get("database_count")
            .map_err(|_| D2ProvisionerErrorV1::Destruction)?,
        row.try_get("backend_count")
            .map_err(|_| D2ProvisionerErrorV1::Destruction)?,
    ))
}

fn destruction_plan(
    (database_count, backend_count): (i64, i64),
) -> Result<D2DestructionPlanV1, D2ProvisionerErrorV1> {
    match (database_count, backend_count) {
        (0, 0) => Ok(D2DestructionPlanV1::ExactReplay),
        (1, 0) => Ok(D2DestructionPlanV1::Drop),
        _ => Err(D2ProvisionerErrorV1::Destruction),
    }
}

async fn validate_destroy_target(
    config: &D2ConfigV1,
    scope: &InspectionScopeV1,
    admin: &mut PgConnection,
) -> Result<(), D2ProvisionerErrorV1> {
    let mut target = connect_inspection_database(config)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    begin_read_snapshot(&mut target)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    let validation = async {
        verify_current_d2_schema(&mut target).await?;
        verify_exact_destroy_scope(&mut target, scope).await?;
        inspect_precleanup(&mut target, scope)
            .await
            .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
        let target_pid: i32 = sqlx::query_scalar("SELECT pg_catalog.pg_backend_pid()")
            .fetch_one(&mut target)
            .await
            .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
        verify_exclusive_target_backend(admin, target_pid).await
    }
    .await;
    if let Err(error) = validation {
        let _ = sqlx::query("ROLLBACK").execute(&mut target).await;
        return Err(error);
    }
    sqlx::query("COMMIT")
        .execute(&mut target)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    target
        .close()
        .await
        .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    Ok(())
}

async fn verify_current_d2_schema(
    connection: &mut PgConnection,
) -> Result<(), D2ProvisionerErrorV1> {
    let exact: bool = sqlx::query_scalar(
        "SELECT (SELECT pg_catalog.count(*) = $1 AND pg_catalog.max(migration.version) = $2 AND pg_catalog.count(*) FILTER (WHERE NOT migration.success) = 0 FROM public._sqlx_migrations AS migration) AND COALESCE((SELECT pg_catalog.encode(migration.checksum, 'hex') = $3 FROM public._sqlx_migrations AS migration WHERE migration.version = $2 AND migration.success), FALSE) AND public.starring_runtime_exact_target_schema_manifest_v1() AND public.starring_runtime_exact_target_schema_manifest_v2() AND public.starring_runtime_execution_schema_manifest_v1() AND public.starring_runtime_interaction_effect_schema_manifest_v1() AND public.starring_runtime_interaction_receipt_schema_manifest_v1() AND public.starring_runtime_interaction_schema_manifest_v1() AND public.starring_runtime_serving_schema_manifest_v1()",
    )
    .bind(EXPECTED_MIGRATION_COUNT)
    .bind(EXPECTED_MIGRATION_HEAD)
    .bind(EXPECTED_MIGRATION_HEAD_CHECKSUM)
    .fetch_one(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    if exact {
        Ok(())
    } else {
        Err(D2ProvisionerErrorV1::Destruction)
    }
}

async fn verify_exact_destroy_scope(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<(), D2ProvisionerErrorV1> {
    let exact: bool = sqlx::query_scalar(
        "SELECT (SELECT pg_catalog.count(*) = 1 FROM public.product_tenants) AND (SELECT pg_catalog.count(*) = 1 FROM public.product_tenants AS tenant WHERE tenant.tenant_id = $1 AND tenant.lifecycle_state = 'active' AND tenant.display_metadata = '{\"environment\":\"staging\",\"onboarding\":\"operator_v1\"}'::JSONB) AND (SELECT pg_catalog.count(*) = 1 FROM public.automation_installations) AND (SELECT pg_catalog.count(*) = 1 FROM public.automation_installations AS installation WHERE installation.tenant_id = $1 AND installation.installation_id = $2 AND installation.discord_guild_id = $3 AND installation.discord_application_id = $4 AND installation.ruleset_key = $5 AND installation.lifecycle_state = 'active') AND EXISTS (SELECT 1 FROM public.automation_installation_authority_versions AS authority WHERE authority.tenant_id = $1 AND authority.installation_id = $2) AND NOT EXISTS (SELECT 1 FROM public.automation_installation_authority_versions AS authority WHERE authority.tenant_id IS DISTINCT FROM $1 OR authority.installation_id IS DISTINCT FROM $2) AND EXISTS (SELECT 1 FROM public.authoring_sessions AS session WHERE session.tenant_id = $1 AND session.installation_id = $2) AND NOT EXISTS (SELECT 1 FROM public.authoring_sessions AS session WHERE session.tenant_id IS DISTINCT FROM $1 OR session.installation_id IS DISTINCT FROM $2) AND EXISTS (SELECT 1 FROM public.runtime_deployments AS deployment WHERE deployment.tenant_id = $1 AND deployment.installation_id = $2 AND deployment.guild_id = $3 AND deployment.ruleset_key = $5) AND NOT EXISTS (SELECT 1 FROM public.runtime_deployments AS deployment WHERE deployment.tenant_id IS DISTINCT FROM $1 OR deployment.installation_id IS DISTINCT FROM $2 OR deployment.guild_id IS DISTINCT FROM $3 OR deployment.ruleset_key IS DISTINCT FROM $5) AND NOT EXISTS (SELECT 1 FROM public.runtime_interaction_receipt_roots_v1 AS root WHERE root.application_id IS DISTINCT FROM $4 OR root.tenant_id IS DISTINCT FROM $1 OR root.installation_id IS DISTINCT FROM $2 OR root.guild_id IS DISTINCT FROM $3 OR root.ruleset_key IS DISTINCT FROM $5) AND NOT EXISTS (SELECT 1 FROM public.runtime_product_operations_v2 AS operation WHERE operation.tenant_id IS DISTINCT FROM $1 OR operation.installation_id IS DISTINCT FROM $2 OR operation.expected_target_guild_id IS DISTINCT FROM $3 OR operation.expected_target_ruleset_key IS DISTINCT FROM $5) AND (SELECT pg_catalog.count(*) = 1 FROM public.runtime_slot_writer_fences_v2) AND (SELECT pg_catalog.count(*) = 1 FROM public.runtime_slot_writer_fences_v2 AS fence WHERE fence.slot_guild_id = $3 AND fence.slot_ruleset_key = $5)",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(&scope.application_id)
    .bind(RULESET_KEY)
    .fetch_one(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    if exact {
        Ok(())
    } else {
        Err(D2ProvisionerErrorV1::Destruction)
    }
}

async fn verify_exclusive_target_backend(
    connection: &mut PgConnection,
    target_pid: i32,
) -> Result<(), D2ProvisionerErrorV1> {
    let exact: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) = 1 AND pg_catalog.count(*) FILTER (WHERE activity.pid = $2 AND activity.usename = 'starring_cluster_admin' AND activity.application_name = 'starring-d2-sealed-inspector' AND activity.client_addr = '127.0.0.1'::INET AND activity.backend_type = 'client backend') = 1 FROM pg_catalog.pg_stat_activity AS activity WHERE activity.datname = $1",
    )
    .bind(DATABASE_NAME)
    .bind(target_pid)
    .fetch_one(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Destruction)?;
    if exact {
        Ok(())
    } else {
        Err(D2ProvisionerErrorV1::Destruction)
    }
}

pub async fn inspect_d2_from_manifest(
    manifest_path: &Path,
    checkpoint: D2InspectionCheckpointV1,
) -> Result<D2InspectionReportV1, D2ProvisionerErrorV1> {
    let config = load_config(manifest_path)?;
    let scope = InspectionScopeV1::from_config(&config);
    let mut connection = if checkpoint == D2InspectionCheckpointV1::Absence {
        connect_inspection_admin_database(&config).await?
    } else {
        connect_inspection_database(&config).await?
    };
    begin_read_snapshot(&mut connection).await?;
    let result = inspect_snapshot(&mut connection, &scope, checkpoint).await;
    match result {
        Ok((observed_at, evidence)) => {
            sqlx::query("COMMIT")
                .execute(&mut connection)
                .await
                .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
            Ok(D2InspectionReportV1 {
                schema_version: 1,
                kind: checkpoint.kind(),
                observed_at,
                evidence,
            })
        }
        Err(error) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut connection).await;
            Err(error)
        }
    }
}

async fn begin_read_snapshot(connection: &mut PgConnection) -> Result<(), D2ProvisionerErrorV1> {
    sqlx::query("BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *connection)
        .await
        .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    for setting in [
        "SET LOCAL statement_timeout = '3s'",
        "SET LOCAL lock_timeout = '500ms'",
        "SET LOCAL idle_in_transaction_session_timeout = '5s'",
        "SET LOCAL search_path = pg_catalog",
    ] {
        sqlx::query(setting)
            .execute(&mut *connection)
            .await
            .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    }
    Ok(())
}

async fn inspect_snapshot(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
    checkpoint: D2InspectionCheckpointV1,
) -> Result<(String, D2InspectionEvidenceV1), D2ProvisionerErrorV1> {
    let observed_at: String = sqlx::query_scalar(
        "SELECT pg_catalog.to_char(pg_catalog.transaction_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    let evidence = match checkpoint {
        D2InspectionCheckpointV1::Authoring => {
            D2InspectionEvidenceV1::Authoring(Box::new(inspect_authoring(connection, scope).await?))
        }
        D2InspectionCheckpointV1::Live => {
            D2InspectionEvidenceV1::Live(Box::new(inspect_live(connection, scope).await?))
        }
        D2InspectionCheckpointV1::Interaction => D2InspectionEvidenceV1::Interaction(Box::new(
            inspect_interaction(connection, scope).await?,
        )),
        D2InspectionCheckpointV1::Duplicate => {
            D2InspectionEvidenceV1::Duplicate(Box::new(inspect_duplicate(connection, scope).await?))
        }
        D2InspectionCheckpointV1::Restart => {
            D2InspectionEvidenceV1::Restart(Box::new(inspect_restart(connection, scope).await?))
        }
        D2InspectionCheckpointV1::Reconciliation => D2InspectionEvidenceV1::Reconciliation(
            Box::new(inspect_reconciliation(connection, scope).await?),
        ),
        D2InspectionCheckpointV1::Replacement => D2InspectionEvidenceV1::Replacement(Box::new(
            inspect_replacement(connection, scope).await?,
        )),
        D2InspectionCheckpointV1::Precleanup => D2InspectionEvidenceV1::Precleanup(Box::new(
            inspect_precleanup(connection, scope).await?,
        )),
        D2InspectionCheckpointV1::Absence => {
            D2InspectionEvidenceV1::Absence(Box::new(inspect_absence(connection, scope).await?))
        }
    };
    Ok((observed_at, evidence))
}

async fn inspect_authoring(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<AuthoringEvidenceV1, D2ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT pg_catalog.count(*) OVER () AS scoped_count, session.session_id, session.current_generation, generation.stage, generation.candidate_hash, generation.safe_turn_projection_digest, (SELECT pg_catalog.count(*) FROM public.authoring_session_generations AS history WHERE history.tenant_id = session.tenant_id AND history.installation_id = session.installation_id AND history.session_id = session.session_id) AS generation_count, pg_catalog.jsonb_array_length(pg_catalog.convert_from(generation.safe_turn_projection, 'UTF8')::pg_catalog.jsonb -> 'model_completions') AS model_completion_count, pg_catalog.convert_from(generation.safe_turn_projection, 'UTF8')::pg_catalog.jsonb #>> '{model_completions,0,request_id}' AS worker_request_id, pg_catalog.convert_from(generation.safe_turn_projection, 'UTF8')::pg_catalog.jsonb #>> '{model_completions,0,completion_sha256}' AS worker_completion_sha256, pg_catalog.to_char(generation.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS generation_created_at, pg_catalog.octet_length(generation.snapshot_ciphertext) >= 16 AND pg_catalog.octet_length(generation.snapshot_nonce) BETWEEN 12 AND 32 AS sealed_snapshot_present FROM public.authoring_sessions AS session INNER JOIN public.authoring_session_generations AS generation ON generation.tenant_id = session.tenant_id AND generation.installation_id = session.installation_id AND generation.session_id = session.session_id AND generation.generation = session.current_generation WHERE session.tenant_id = $1 AND session.installation_id = $2 AND session.lifecycle_state = 'active' ORDER BY (generation.stage = 'preview_ready') DESC, session.updated_at DESC, session.session_id COLLATE pg_catalog.\"C\" DESC LIMIT 1",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?
    .ok_or(D2ProvisionerErrorV1::Inspection)?;
    let projection_state: String = get(&row, "stage")?;
    let candidate_ruleset_hash: String = get(&row, "candidate_hash")?;
    let safe_projection_digest: String = get(&row, "safe_turn_projection_digest")?;
    let scoped_count: i64 = get(&row, "scoped_count")?;
    let generation_count: i64 = get(&row, "generation_count")?;
    let model_completion_count: i32 = get(&row, "model_completion_count")?;
    let worker_request_id: String = get(&row, "worker_request_id")?;
    let worker_completion_sha256: String = get(&row, "worker_completion_sha256")?;
    if !exact_authoring_scope(scoped_count)
        || generation_count != 1
        || model_completion_count != 1
        || projection_state != "preview_ready"
        || !valid_sha256(&candidate_ruleset_hash)
        || !valid_sha256(&safe_projection_digest)
        || !valid_identifier(&worker_request_id)
        || !valid_sha256(&worker_completion_sha256)
        || !get::<bool>(&row, "sealed_snapshot_present")?
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    let authoring_session_id: String = get(&row, "session_id")?;
    if !valid_identifier(&authoring_session_id) {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(AuthoringEvidenceV1 {
        generation_encrypted: true,
        projection_state,
        generation: get(&row, "current_generation")?,
        generation_count,
        payload_digest: candidate_ruleset_hash,
        worker_request_id,
        worker_completion_sha256,
        installation_id: scope.installation_id.clone(),
        authoring_session_id,
        generation_created_at: get(&row, "generation_created_at")?,
    })
}

async fn inspect_live(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<LiveEvidenceV1, D2ProvisionerErrorV1> {
    let count = scoped_phase_count(connection, scope, "live").await?;
    if count != 1 {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    let observation = load_live_observation(connection, scope).await?;
    let deployment_id = observation.route.deployment_id.clone();
    let attestation_id = observation.serving.attestation_id.clone();
    Ok(LiveEvidenceV1 {
        installation_id: scope.installation_id.clone(),
        promotion_id: observation.promotion_id,
        deployment_id,
        attestation_id,
        deployment_revision: observation.deployment_revision,
        convergence_attempt: observation.convergence_attempt,
        process_instance_id: observation.process_instance_id,
        last_heartbeat_at: observation.last_heartbeat_at,
        lease_expires_at: observation.lease_expires_at,
        route_identity: observation.route,
        serving_identity: observation.serving,
    })
}

async fn load_live_observation(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<LiveObservationV1, D2ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT deployment.promotion_id, deployment.deployment_id, deployment.runtime_generation, deployment.convergence_attempt_no AS deployment_convergence_attempt, attestation.deployment_revision, attestation.convergence_attempt_no AS attestation_convergence_attempt, attestation.process_instance_id AS attested_process_instance_id, attestation.controller_fencing_token AS route_controller_fencing_token, attestation.v2_route_incarnation AS route_incarnation, attestation.process_instance_id AS origin_process_instance_id, lease.lease_epoch AS origin_serving_lease_epoch, lease.revision AS origin_serving_revision, attestation.gateway_shard_id AS origin_gateway_shard_id, (attestation.v2_route_admission #>> '{gateway_owner_lease_id,lease_epoch}')::BIGINT AS origin_gateway_owner_lease_epoch, (attestation.v2_route_admission ->> 'attested_owner_revision')::BIGINT AS origin_gateway_owner_revision, lease.guild_id, lease.ruleset_key, lease.tenant_id, lease.installation_id, lease.attestation_id, lease.process_instance_id, lease.target_version, lease.target_content_hash, lease.binding_revision, lease.binding_fingerprint, lease.lease_epoch, lease.revision AS serving_revision, pg_catalog.to_char(lease.last_heartbeat_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS last_heartbeat_at, pg_catalog.to_char(lease.expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS lease_expires_at FROM public.runtime_deployments AS deployment INNER JOIN public.runtime_attestations AS attestation ON attestation.tenant_id = deployment.tenant_id AND attestation.installation_id = deployment.installation_id AND attestation.deployment_id = deployment.deployment_id AND attestation.attestation_id = deployment.live_attestation_id INNER JOIN public.runtime_serving_leases AS lease ON lease.tenant_id = deployment.tenant_id AND lease.installation_id = deployment.installation_id AND lease.deployment_id = deployment.deployment_id AND lease.attestation_id = attestation.attestation_id WHERE deployment.tenant_id = $1 AND deployment.installation_id = $2 AND deployment.guild_id = $3 AND deployment.ruleset_key = $4 AND deployment.phase = 'live' AND deployment.convergence_attempt_no = attestation.convergence_attempt_no AND attestation.record_format_version = 2 AND attestation.process_instance_id = lease.process_instance_id AND lease.connected AND lease.serving AND pg_catalog.isfinite(lease.last_heartbeat_at) AND pg_catalog.isfinite(lease.expires_at) AND lease.last_heartbeat_at <= pg_catalog.transaction_timestamp() AND lease.expires_at > pg_catalog.transaction_timestamp()",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(RULESET_KEY)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?
    .ok_or(D2ProvisionerErrorV1::Inspection)?;
    let deployment_id: String = get(&row, "deployment_id")?;
    let runtime_generation: i64 = get(&row, "runtime_generation")?;
    let deployment_revision: i64 = get(&row, "deployment_revision")?;
    let convergence_attempt: i64 = get(&row, "attestation_convergence_attempt")?;
    let deployment_convergence_attempt: i64 = get(&row, "deployment_convergence_attempt")?;
    let process_instance_id: String = get(&row, "attested_process_instance_id")?;
    let route = RouteIdentityV1 {
        deployment_id: deployment_id.clone(),
        runtime_generation,
        route_controller_fencing_token: get(&row, "route_controller_fencing_token")?,
        route_incarnation: get(&row, "route_incarnation")?,
        origin_process_instance_id: get(&row, "origin_process_instance_id")?,
        origin_serving_lease_epoch: get(&row, "origin_serving_lease_epoch")?,
        origin_serving_revision: get(&row, "origin_serving_revision")?,
        origin_gateway_shard_id: get(&row, "origin_gateway_shard_id")?,
        origin_gateway_owner_lease_epoch: get(&row, "origin_gateway_owner_lease_epoch")?,
        origin_gateway_owner_revision: get(&row, "origin_gateway_owner_revision")?,
    };
    let serving = ServingIdentityV1 {
        guild_id: get(&row, "guild_id")?,
        ruleset_key: get(&row, "ruleset_key")?,
        tenant_id: get(&row, "tenant_id")?,
        installation_id: get(&row, "installation_id")?,
        deployment_id,
        attestation_id: get(&row, "attestation_id")?,
        process_instance_id: get(&row, "process_instance_id")?,
        runtime_generation,
        target_version: get(&row, "target_version")?,
        target_content_hash: get(&row, "target_content_hash")?,
        binding_revision: get(&row, "binding_revision")?,
        binding_fingerprint: get(&row, "binding_fingerprint")?,
        lease_epoch: get(&row, "lease_epoch")?,
        revision: get(&row, "serving_revision")?,
    };
    validate_route_identity(&route)?;
    validate_serving_identity(&serving, scope)?;
    if deployment_revision < 1
        || convergence_attempt < 1
        || convergence_attempt != deployment_convergence_attempt
        || process_instance_id != route.origin_process_instance_id
        || process_instance_id != serving.process_instance_id
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(LiveObservationV1 {
        route,
        serving,
        promotion_id: get(&row, "promotion_id")?,
        deployment_revision,
        convergence_attempt,
        process_instance_id,
        last_heartbeat_at: get(&row, "last_heartbeat_at")?,
        lease_expires_at: get(&row, "lease_expires_at")?,
    })
}

async fn inspect_interaction(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<InteractionEvidenceV1, D2ProvisionerErrorV1> {
    let create = sqlx::query(
        "SELECT root.application_id, root.interaction_id, root.actor_user_id, root.deployment_id, root.runtime_generation, root.route_controller_fencing_token, root.route_incarnation, root.origin_process_instance_id, root.origin_serving_lease_epoch, root.origin_serving_revision, root.origin_gateway_shard_id, root.origin_gateway_owner_lease_epoch, root.origin_gateway_owner_revision FROM public.runtime_interaction_receipt_roots_v1 AS root INNER JOIN public.runtime_interaction_receipt_heads_v1 AS head ON head.application_id = root.application_id AND head.interaction_id = root.interaction_id WHERE root.application_id = $1 AND root.tenant_id = $2 AND root.installation_id = $3 AND root.guild_id = $4 AND root.ruleset_key = $5 AND head.state = 'completed' AND EXISTS (SELECT 1 FROM public.runtime_interaction_effect_heads_v1 AS effect WHERE effect.application_id = root.application_id AND effect.interaction_id = root.interaction_id AND effect.action_kind = 'register_instance' AND effect.state IN ('known_succeeded', 'reconciled_succeeded')) ORDER BY root.created_at DESC, root.interaction_id COLLATE pg_catalog.\"C\" DESC LIMIT 1",
    )
    .bind(&scope.application_id)
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(RULESET_KEY)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?
    .ok_or(D2ProvisionerErrorV1::Inspection)?;
    let route = route_from_receipt(&create)?;
    let application_id: String = get(&create, "application_id")?;
    let create_interaction_id: String = get(&create, "interaction_id")?;
    let actor_user_id: String = get(&create, "actor_user_id")?;
    let join = latest_completed_instance_join(connection, scope).await?;
    let join_interaction_id: String = get(&join, "interaction_id")?;
    let instance_id: String = get(&join, "instance_id")?;
    let join_actor_user_id: String = get(&join, "actor_user_id")?;
    if create_interaction_id == join_interaction_id
        || get::<String>(&join, "deployment_id")? != route.deployment_id
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    let role_ids = successful_output_ids(
        connection,
        &application_id,
        &create_interaction_id,
        "created_role",
    )
    .await?;
    let channel_ids = successful_output_ids(
        connection,
        &application_id,
        &create_interaction_id,
        "created_channel",
    )
    .await?;
    let panel_message_ids = successful_output_ids(
        connection,
        &application_id,
        &create_interaction_id,
        "posted_message",
    )
    .await?;
    let registered_instance_ids = successful_output_ids(
        connection,
        &application_id,
        &create_interaction_id,
        "instance_state",
    )
    .await?;
    if role_ids.len() != 1
        || channel_ids.len() != 1
        || panel_message_ids.len() != 1
        || registered_instance_ids.len() != 1
        || registered_instance_ids[0] != instance_id
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    let (create_membership_guild_id, create_membership_user_id, create_role_id) =
        successful_role_membership(connection, &application_id, &create_interaction_id).await?;
    let (join_membership_guild_id, join_membership_user_id, joined_role_id) =
        successful_role_membership(connection, &application_id, &join_interaction_id).await?;
    let acknowledgements = sqlx::query(
        "SELECT pg_catalog.count(*) FILTER (WHERE interaction_id = $2) AS create_ack_count, pg_catalog.count(*) FILTER (WHERE interaction_id = $3) AS join_ack_count FROM public.runtime_interaction_receipt_heads_v1 WHERE application_id = $1 AND interaction_id IN ($2, $3) AND acknowledgement_kind IN ('defer_ephemeral', 'respond_ephemeral') AND acknowledgement_result = 'succeeded'",
    )
    .bind(&application_id)
    .bind(&create_interaction_id)
    .bind(&join_interaction_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    let create_ack_count: i64 = get(&acknowledgements, "create_ack_count")?;
    let join_ack_count: i64 = get(&acknowledgements, "join_ack_count")?;
    let ephemeral_count = create_ack_count + join_ack_count;
    if !valid_snowflake(&create_interaction_id)
        || !valid_snowflake(&join_interaction_id)
        || !valid_snowflake(&actor_user_id)
        || actor_user_id != join_actor_user_id
        || create_membership_guild_id != scope.guild_id
        || join_membership_guild_id != scope.guild_id
        || create_membership_user_id != actor_user_id
        || join_membership_user_id != actor_user_id
        || create_role_id != joined_role_id
        || !role_ids.contains(&joined_role_id)
        || !valid_identifier(&instance_id)
        || !valid_distinct_snowflakes(&role_ids)
        || !valid_distinct_snowflakes(&channel_ids)
        || !valid_distinct_snowflakes(&panel_message_ids)
        || create_ack_count != 1
        || join_ack_count != 1
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(InteractionEvidenceV1 {
        create_interaction_id,
        join_interaction_id,
        actor_user_id,
        joined_role_id,
        deployment_id: route.deployment_id.clone(),
        route_identity: route,
        instance_id,
        role_ids,
        channel_ids,
        panel_message_ids,
        ephemeral_count,
    })
}

async fn latest_completed_instance_join(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<PgRow, D2ProvisionerErrorV1> {
    sqlx::query(
        "SELECT root.application_id, root.interaction_id, root.actor_user_id, root.deployment_id, root.instance_id FROM public.runtime_interaction_receipt_roots_v1 AS root INNER JOIN public.runtime_interaction_receipt_heads_v1 AS head ON head.application_id = root.application_id AND head.interaction_id = root.interaction_id WHERE root.application_id = $1 AND root.tenant_id = $2 AND root.installation_id = $3 AND root.guild_id = $4 AND root.ruleset_key = $5 AND root.route_kind = 'instance' AND head.state = 'completed' AND EXISTS (SELECT 1 FROM public.runtime_interaction_effect_heads_v1 AS effect WHERE effect.application_id = root.application_id AND effect.interaction_id = root.interaction_id AND effect.action_kind = 'grant_role' AND effect.output_kind = 'role_membership' AND effect.state IN ('known_succeeded', 'reconciled_succeeded')) ORDER BY root.created_at DESC, root.interaction_id COLLATE pg_catalog.\"C\" DESC LIMIT 1",
    )
    .bind(&scope.application_id)
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(RULESET_KEY)
    .fetch_optional(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?
    .ok_or(D2ProvisionerErrorV1::Inspection)
}

async fn successful_role_membership(
    connection: &mut PgConnection,
    application_id: &str,
    interaction_id: &str,
) -> Result<(String, String, String), D2ProvisionerErrorV1> {
    let inputs: Vec<Value> = sqlx::query_scalar(
        "SELECT resolved_input FROM public.runtime_interaction_effect_heads_v1 WHERE application_id = $1 AND interaction_id = $2 AND action_kind = 'grant_role' AND output_kind = 'role_membership' AND state IN ('known_succeeded', 'reconciled_succeeded') ORDER BY action_index",
    )
    .bind(application_id)
    .bind(interaction_id)
    .fetch_all(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    if inputs.len() != 1 {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    resolved_role_membership_ids(&inputs[0])
}

fn resolved_role_membership_ids(
    document: &Value,
) -> Result<(String, String, String), D2ProvisionerErrorV1> {
    let object = document
        .as_object()
        .filter(|value| value.len() == 1)
        .ok_or(D2ProvisionerErrorV1::Inspection)?;
    let references = object
        .get("references")
        .and_then(Value::as_array)
        .filter(|value| value.len() == 3)
        .ok_or(D2ProvisionerErrorV1::Inspection)?;
    let mut guild_id = None;
    let mut user_id = None;
    let mut role_id = None;
    for reference in references {
        let fields = reference
            .as_object()
            .filter(|value| value.len() == 2)
            .ok_or(D2ProvisionerErrorV1::Inspection)?;
        let slot = fields
            .get("slot")
            .and_then(Value::as_str)
            .ok_or(D2ProvisionerErrorV1::Inspection)?;
        let id = fields
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| valid_snowflake(value))
            .ok_or(D2ProvisionerErrorV1::Inspection)?
            .to_owned();
        let target = match slot {
            "guild_id" => &mut guild_id,
            "user_id" => &mut user_id,
            "role_id" => &mut role_id,
            _ => return Err(D2ProvisionerErrorV1::Inspection),
        };
        if target.replace(id).is_some() {
            return Err(D2ProvisionerErrorV1::Inspection);
        }
    }
    Ok((
        guild_id.ok_or(D2ProvisionerErrorV1::Inspection)?,
        user_id.ok_or(D2ProvisionerErrorV1::Inspection)?,
        role_id.ok_or(D2ProvisionerErrorV1::Inspection)?,
    ))
}

async fn successful_output_ids(
    connection: &mut PgConnection,
    application_id: &str,
    interaction_id: &str,
    output_kind: &str,
) -> Result<Vec<String>, D2ProvisionerErrorV1> {
    sqlx::query_scalar(
        "SELECT output_id FROM public.runtime_interaction_effect_heads_v1 WHERE application_id = $1 AND interaction_id = $2 AND output_kind = $3 AND state IN ('known_succeeded', 'reconciled_succeeded') AND output_id IS NOT NULL ORDER BY action_index",
    )
    .bind(application_id)
    .bind(interaction_id)
    .bind(output_kind)
    .fetch_all(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)
}

async fn inspect_duplicate(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<DuplicateEvidenceV1, D2ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT root.application_id, root.interaction_id, head.state AS receipt_state, effect.action_index, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_heads_v1 AS candidate WHERE candidate.application_id = root.application_id AND candidate.interaction_id = root.interaction_id AND candidate.action_kind <> 'edit_response') AS external_effect_count, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS event WHERE event.application_id = effect.application_id AND event.interaction_id = effect.interaction_id AND event.action_index = effect.action_index AND event.event_kind IN ('known_succeeded', 'reconciled_success')) AS success_event_count FROM public.runtime_interaction_receipt_roots_v1 AS root INNER JOIN public.runtime_interaction_receipt_heads_v1 AS head ON head.application_id = root.application_id AND head.interaction_id = root.interaction_id INNER JOIN public.runtime_interaction_effect_heads_v1 AS effect ON effect.application_id = root.application_id AND effect.interaction_id = root.interaction_id AND effect.action_kind <> 'edit_response' WHERE root.application_id = $1 AND root.tenant_id = $2 AND root.installation_id = $3 AND root.guild_id = $4 AND root.ruleset_key = $5 AND head.state = 'completed' AND effect.state IN ('known_succeeded', 'reconciled_succeeded') AND (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_heads_v1 AS candidate WHERE candidate.application_id = root.application_id AND candidate.interaction_id = root.interaction_id AND candidate.action_kind <> 'edit_response') = 1 ORDER BY root.created_at DESC, root.interaction_id COLLATE pg_catalog.\"C\" DESC LIMIT 1",
    )
    .bind(&scope.application_id)
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(RULESET_KEY)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?
    .ok_or(D2ProvisionerErrorV1::Inspection)?;
    let effect_identity = EffectIdentityV1 {
        application_id: get(&row, "application_id")?,
        interaction_id: get(&row, "interaction_id")?,
        action_index: get(&row, "action_index")?,
    };
    let external_effect_count: i64 = get(&row, "external_effect_count")?;
    let success_event_count: i64 = get(&row, "success_event_count")?;
    let receipt_state: String = get(&row, "receipt_state")?;
    validate_effect_identity(&effect_identity)?;
    identity_hash(EFFECT_IDENTITY_KIND, &effect_identity)?;
    if external_effect_count != 1 || success_event_count != 1 || receipt_state != "completed" {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(DuplicateEvidenceV1 {
        interaction_id: effect_identity.interaction_id.clone(),
        effect_identity,
        external_effect_count,
        receipt_state,
    })
}

async fn inspect_reconciliation(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<ReconciliationEvidenceV1, D2ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT pg_catalog.count(*) OVER () AS candidate_count, root.application_id, root.interaction_id, root.deployment_id, root.runtime_generation, root.route_controller_fencing_token, root.route_incarnation, root.origin_process_instance_id, root.origin_serving_lease_epoch, root.origin_serving_revision, root.origin_gateway_shard_id, root.origin_gateway_owner_lease_epoch, root.origin_gateway_owner_revision, effect.action_index, effect.state AS effect_state, effect.output_id, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS event WHERE event.application_id = effect.application_id AND event.interaction_id = effect.interaction_id AND event.action_index = effect.action_index AND event.event_kind = 'indeterminate') AS indeterminate_event_count, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS event WHERE event.application_id = effect.application_id AND event.interaction_id = effect.interaction_id AND event.action_index = effect.action_index AND event.event_kind IN ('known_succeeded', 'reconciled_success')) AS success_event_count, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS event WHERE event.application_id = effect.application_id AND event.interaction_id = effect.interaction_id AND event.action_index = effect.action_index AND event.event_kind = 'reconciled_failure') AS failure_event_count, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS event WHERE event.application_id = effect.application_id AND event.interaction_id = effect.interaction_id AND event.action_index = effect.action_index AND event.event_kind = 'compensated') AS compensated_event_count, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_effect_events_v1 AS deleted WHERE deleted.application_id = effect.application_id AND deleted.interaction_id = effect.interaction_id AND deleted.action_index = effect.action_index AND deleted.event_kind = 'compensated' AND NOT EXISTS (SELECT 1 FROM public.runtime_interaction_effect_events_v1 AS intended WHERE intended.application_id = deleted.application_id AND intended.interaction_id = deleted.interaction_id AND intended.action_index = deleted.action_index AND intended.event_kind = 'compensation_intended' AND intended.event_revision < deleted.event_revision)) AS unsafe_deletion_count FROM public.runtime_interaction_receipt_roots_v1 AS root INNER JOIN public.runtime_interaction_receipt_heads_v1 AS head ON head.application_id = root.application_id AND head.interaction_id = root.interaction_id INNER JOIN public.runtime_interaction_effect_heads_v1 AS effect ON effect.application_id = root.application_id AND effect.interaction_id = root.interaction_id WHERE root.application_id = $1 AND root.tenant_id = $2 AND root.installation_id = $3 AND root.guild_id = $4 AND root.ruleset_key = $5 AND head.state = 'completed' AND effect.action_kind = 'create_role' AND effect.state = 'reconciled_succeeded' AND effect.output_kind = 'role' AND effect.output_id IS NOT NULL AND EXISTS (SELECT 1 FROM public.runtime_interaction_effect_events_v1 AS event WHERE event.application_id = effect.application_id AND event.interaction_id = effect.interaction_id AND event.action_index = effect.action_index AND event.event_kind = 'indeterminate') AND NOT EXISTS (SELECT 1 FROM public.runtime_interaction_effect_heads_v1 AS blocked WHERE blocked.application_id = root.application_id AND blocked.interaction_id = root.interaction_id AND blocked.action_kind <> 'edit_response' AND blocked.state IN ('intended', 'indeterminate', 'observing', 'observation_pending', 'compensation_intended', 'compensation_indeterminate', 'compensation_observing', 'compensation_observation_pending', 'recovery_required')) ORDER BY effect.updated_at DESC, root.interaction_id COLLATE pg_catalog.\"C\" DESC, effect.action_index LIMIT 1",
    )
    .bind(&scope.application_id)
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(RULESET_KEY)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?
    .ok_or(D2ProvisionerErrorV1::Inspection)?;
    let effect_identity = EffectIdentityV1 {
        application_id: get(&row, "application_id")?,
        interaction_id: get(&row, "interaction_id")?,
        action_index: get(&row, "action_index")?,
    };
    validate_effect_identity(&effect_identity)?;
    identity_hash(EFFECT_IDENTITY_KIND, &effect_identity)?;
    let effect_state: String = get(&row, "effect_state")?;
    let candidate_count: i64 = get(&row, "candidate_count")?;
    let indeterminate_event_count: i64 = get(&row, "indeterminate_event_count")?;
    let success_event_count: i64 = get(&row, "success_event_count")?;
    let failure_event_count: i64 = get(&row, "failure_event_count")?;
    let compensated_event_count: i64 = get(&row, "compensated_event_count")?;
    let unsafe_deletion_count: i64 = get(&row, "unsafe_deletion_count")?;
    let output_id: Option<String> = get(&row, "output_id")?;
    let output_role_id = output_id
        .filter(|value| valid_snowflake(value))
        .ok_or(D2ProvisionerErrorV1::Inspection)?;
    let duplicate_external_effect_count = success_event_count.saturating_sub(1);
    if candidate_count != 1
        || indeterminate_event_count != 1
        || effect_state != "reconciled_succeeded"
        || success_event_count != 1
        || failure_event_count != 0
        || compensated_event_count != 0
        || duplicate_external_effect_count != 0
        || unsafe_deletion_count != 0
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    let route_identity = route_from_receipt(&row)?;
    Ok(ReconciliationEvidenceV1 {
        interaction_id: effect_identity.interaction_id.clone(),
        effect_identity,
        route_identity,
        output_role_id,
        reconciliation_state: "known_success".to_owned(),
        duplicate_external_effect_count,
        unsafe_deletion_count,
    })
}

async fn inspect_restart(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<RestartEvidenceV1, D2ProvisionerErrorV1> {
    let rows = sqlx::query(
        "SELECT root.application_id, root.interaction_id, root.tenant_id, root.installation_id, root.guild_id, root.ruleset_key, root.deployment_id, root.attestation_id, root.runtime_generation, root.target_version, root.target_content_hash, root.binding_revision, root.binding_fingerprint, root.route_controller_fencing_token, root.route_incarnation, root.origin_process_instance_id, root.origin_serving_lease_epoch, root.origin_serving_revision, root.origin_gateway_shard_id, root.origin_gateway_owner_lease_epoch, root.origin_gateway_owner_revision, root.instance_id, root.instance_manifest_digest FROM public.runtime_interaction_receipt_roots_v1 AS root INNER JOIN public.runtime_interaction_receipt_heads_v1 AS head ON head.application_id = root.application_id AND head.interaction_id = root.interaction_id WHERE root.application_id = $1 AND root.tenant_id = $2 AND root.installation_id = $3 AND root.guild_id = $4 AND root.ruleset_key = $5 AND root.route_kind = 'instance' AND head.state = 'completed' ORDER BY root.created_at DESC, root.interaction_id COLLATE pg_catalog.\"C\" DESC",
    )
    .bind(&scope.application_id)
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(RULESET_KEY)
    .fetch_all(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    let reconstructed_row = rows.first().ok_or(D2ProvisionerErrorV1::Inspection)?;
    let reconstructed_route = route_from_receipt(reconstructed_row)?;
    let instance_id: String = get(reconstructed_row, "instance_id")?;
    let source_row = rows
        .iter()
        .find(|row| {
            let process = get::<String>(row, "origin_process_instance_id");
            let candidate_instance = get::<String>(row, "instance_id");
            let deployment = get::<String>(row, "deployment_id");
            matches!(process, Ok(value) if value != reconstructed_route.origin_process_instance_id)
                && matches!(candidate_instance, Ok(value) if value == instance_id)
                && matches!(deployment, Ok(value) if value == reconstructed_route.deployment_id)
        })
        .ok_or(D2ProvisionerErrorV1::Inspection)?;
    let source_route = route_from_receipt(source_row)?;
    let source_serving = serving_from_receipt(source_row, scope)?;
    let reconstructed_serving = serving_from_receipt(reconstructed_row, scope)?;
    let live = load_live_observation(connection, scope).await?;
    let pinned_ruleset_digest: String = get(reconstructed_row, "instance_manifest_digest")?;
    let source_instance_id: String = get(source_row, "instance_id")?;
    let route_rotated = identity_hash(ROUTE_IDENTITY_KIND, &source_route)?
        != identity_hash(ROUTE_IDENTITY_KIND, &reconstructed_route)?;
    let serving_rotated = identity_hash(SERVING_IDENTITY_KIND, &source_serving)?
        != identity_hash(SERVING_IDENTITY_KIND, &reconstructed_serving)?;
    let reconstructed = reconstructed_route.deployment_id == live.serving.deployment_id
        && reconstructed_route.origin_process_instance_id == live.serving.process_instance_id
        && reconstructed_route.runtime_generation == live.serving.runtime_generation;
    let probe_interaction_id: String = get(reconstructed_row, "interaction_id")?;
    if !valid_sha256(&pinned_ruleset_digest)
        || !valid_identifier(&instance_id)
        || !valid_snowflake(&probe_interaction_id)
        || instance_id != source_instance_id
        || source_route.deployment_id != reconstructed_route.deployment_id
        || !route_rotated
        || !serving_rotated
        || !reconstructed
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(RestartEvidenceV1 {
        route_reconstructed: true,
        instance_reconstructed: true,
        deployment_id: reconstructed_route.deployment_id.clone(),
        source_route_identity: source_route,
        reconstructed_route_identity: reconstructed_route.clone(),
        source_serving_identity: source_serving,
        reconstructed_serving_identity: reconstructed_serving,
        instance_id,
        pinned_ruleset_digest,
        probe_interaction_id,
        process_instance_id: reconstructed_route.origin_process_instance_id,
    })
}

fn serving_from_receipt(
    row: &PgRow,
    scope: &InspectionScopeV1,
) -> Result<ServingIdentityV1, D2ProvisionerErrorV1> {
    let serving = ServingIdentityV1 {
        guild_id: get(row, "guild_id")?,
        ruleset_key: get(row, "ruleset_key")?,
        tenant_id: get(row, "tenant_id")?,
        installation_id: get(row, "installation_id")?,
        deployment_id: get(row, "deployment_id")?,
        attestation_id: get(row, "attestation_id")?,
        process_instance_id: get(row, "origin_process_instance_id")?,
        runtime_generation: get(row, "runtime_generation")?,
        target_version: get(row, "target_version")?,
        target_content_hash: get(row, "target_content_hash")?,
        binding_revision: get(row, "binding_revision")?,
        binding_fingerprint: get(row, "binding_fingerprint")?,
        lease_epoch: get(row, "origin_serving_lease_epoch")?,
        revision: get(row, "origin_serving_revision")?,
    };
    validate_serving_identity(&serving, scope)?;
    Ok(serving)
}

fn route_from_receipt(row: &PgRow) -> Result<RouteIdentityV1, D2ProvisionerErrorV1> {
    let route = RouteIdentityV1 {
        deployment_id: get(row, "deployment_id")?,
        runtime_generation: get(row, "runtime_generation")?,
        route_controller_fencing_token: get(row, "route_controller_fencing_token")?,
        route_incarnation: get(row, "route_incarnation")?,
        origin_process_instance_id: get(row, "origin_process_instance_id")?,
        origin_serving_lease_epoch: get(row, "origin_serving_lease_epoch")?,
        origin_serving_revision: get(row, "origin_serving_revision")?,
        origin_gateway_shard_id: get(row, "origin_gateway_shard_id")?,
        origin_gateway_owner_lease_epoch: get(row, "origin_gateway_owner_lease_epoch")?,
        origin_gateway_owner_revision: get(row, "origin_gateway_owner_revision")?,
    };
    validate_route_identity(&route)?;
    Ok(route)
}

async fn inspect_replacement(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<ReplacementEvidenceV1, D2ProvisionerErrorV1> {
    let source_count = scoped_phase_count(connection, scope, "superseded").await?;
    let live_count = scoped_phase_count(connection, scope, "live").await?;
    if source_count != 1 || live_count != 1 {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    let source_row = load_phase_attestation(connection, scope, "superseded").await?;
    let replacement_row = load_phase_attestation(connection, scope, "live").await?;
    let source_route = route_from_attestation(&source_row)?;
    let replacement_route = route_from_attestation(&replacement_row)?;
    let live = load_live_observation(connection, scope).await?;
    if replacement_route.deployment_id != live.serving.deployment_id {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    let source_lease_count: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM public.runtime_serving_leases WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&source_route.deployment_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    let previous_runtime_verified: bool = sqlx::query_scalar(
        "SELECT replacement.previous_runtime #>> '{target,guild_id}' = source.guild_id AND replacement.previous_runtime #>> '{target,ruleset_key}' = source.ruleset_key AND (replacement.previous_runtime #>> '{target,version}')::BIGINT = source.target_version AND replacement.previous_runtime #>> '{target,content_hash}' = source.target_content_hash AND (replacement.previous_runtime #>> '{target,binding_revision}')::BIGINT = source.binding_revision AND replacement.previous_runtime #>> '{target,binding_fingerprint}' = source.binding_fingerprint AND (replacement.previous_runtime ->> 'runtime_generation')::BIGINT = source.runtime_generation AND replacement.previous_runtime ->> 'process_instance_id' = $4 FROM public.runtime_deployments AS replacement INNER JOIN public.runtime_deployments AS source ON source.tenant_id = replacement.tenant_id AND source.installation_id = replacement.installation_id WHERE replacement.tenant_id = $1 AND replacement.installation_id = $2 AND replacement.deployment_id = $3 AND source.deployment_id = $5",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&replacement_route.deployment_id)
    .bind(&source_route.origin_process_instance_id)
    .bind(&source_route.deployment_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?
    .ok_or(D2ProvisionerErrorV1::Inspection)?;
    if source_lease_count != 0 || !previous_runtime_verified {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(ReplacementEvidenceV1 {
        installation_id: scope.installation_id.clone(),
        source_promotion_id: get(&source_row, "promotion_id")?,
        replacement_promotion_id: get(&replacement_row, "promotion_id")?,
        source_deployment_id: source_route.deployment_id.clone(),
        source_route_identity: source_route,
        replacement_deployment_id: replacement_route.deployment_id.clone(),
        replacement_route_identity: replacement_route,
        previous_target_drained: true,
        replacement_live: true,
        prior_route_absent: true,
    })
}

async fn load_phase_attestation(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
    phase: &str,
) -> Result<PgRow, D2ProvisionerErrorV1> {
    sqlx::query(
        "SELECT deployment.promotion_id, deployment.deployment_id, deployment.runtime_generation, attestation.controller_fencing_token AS route_controller_fencing_token, attestation.v2_route_incarnation AS route_incarnation, attestation.process_instance_id AS origin_process_instance_id, attestation.v2_initial_lease_epoch AS origin_serving_lease_epoch, attestation.v2_initial_serving_revision AS origin_serving_revision, attestation.gateway_shard_id AS origin_gateway_shard_id, (attestation.v2_route_admission #>> '{gateway_owner_lease_id,lease_epoch}')::BIGINT AS origin_gateway_owner_lease_epoch, (attestation.v2_route_admission ->> 'attested_owner_revision')::BIGINT AS origin_gateway_owner_revision FROM public.runtime_deployments AS deployment INNER JOIN LATERAL (SELECT candidate.* FROM public.runtime_attestations AS candidate WHERE candidate.tenant_id = deployment.tenant_id AND candidate.installation_id = deployment.installation_id AND candidate.deployment_id = deployment.deployment_id AND candidate.record_format_version = 2 ORDER BY candidate.certified_at DESC, candidate.attestation_id COLLATE pg_catalog.\"C\" DESC LIMIT 1) AS attestation ON TRUE WHERE deployment.tenant_id = $1 AND deployment.installation_id = $2 AND deployment.guild_id = $3 AND deployment.ruleset_key = $4 AND deployment.phase = $5 ORDER BY deployment.updated_at DESC, deployment.deployment_id COLLATE pg_catalog.\"C\" DESC LIMIT 1",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(RULESET_KEY)
    .bind(phase)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?
    .ok_or(D2ProvisionerErrorV1::Inspection)
}

fn route_from_attestation(row: &PgRow) -> Result<RouteIdentityV1, D2ProvisionerErrorV1> {
    let route = RouteIdentityV1 {
        deployment_id: get(row, "deployment_id")?,
        runtime_generation: get(row, "runtime_generation")?,
        route_controller_fencing_token: get(row, "route_controller_fencing_token")?,
        route_incarnation: get(row, "route_incarnation")?,
        origin_process_instance_id: get(row, "origin_process_instance_id")?,
        origin_serving_lease_epoch: get(row, "origin_serving_lease_epoch")?,
        origin_serving_revision: get(row, "origin_serving_revision")?,
        origin_gateway_shard_id: get(row, "origin_gateway_shard_id")?,
        origin_gateway_owner_lease_epoch: get(row, "origin_gateway_owner_lease_epoch")?,
        origin_gateway_owner_revision: get(row, "origin_gateway_owner_revision")?,
    };
    validate_route_identity(&route)?;
    Ok(route)
}

async fn inspect_precleanup(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<PrecleanupEvidenceV1, D2ProvisionerErrorV1> {
    let row = sqlx::query(
        "SELECT (SELECT pg_catalog.count(*) FROM public.automation_installations AS installation WHERE installation.tenant_id = $1 AND installation.installation_id = $2) AS installation_count, (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment WHERE deployment.tenant_id = $1 AND deployment.installation_id = $2 AND deployment.guild_id = $3 AND deployment.ruleset_key = $4) AS deployment_count, (SELECT pg_catalog.count(*) FROM public.runtime_product_operations_v2 AS operation INNER JOIN public.runtime_product_drain_terminal_actions_v2 AS terminal ON terminal.product_operation_id = operation.product_operation_id WHERE operation.tenant_id = $1 AND operation.installation_id = $2 AND operation.expected_target_guild_id = $3 AND operation.expected_target_ruleset_key = $4) AS terminal_operation_count, (SELECT pg_catalog.count(*) FROM public.runtime_product_operations_v2 AS operation WHERE operation.tenant_id = $1 AND operation.installation_id = $2 AND operation.expected_target_guild_id = $3 AND operation.expected_target_ruleset_key = $4 AND NOT EXISTS (SELECT 1 FROM public.runtime_product_drain_terminal_actions_v2 AS terminal WHERE terminal.product_operation_id = operation.product_operation_id)) AS unresolved_operation_count, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_receipt_roots_v1 AS root INNER JOIN public.runtime_interaction_receipt_heads_v1 AS head ON head.application_id = root.application_id AND head.interaction_id = root.interaction_id WHERE root.application_id = $5 AND root.tenant_id = $1 AND root.installation_id = $2 AND root.guild_id = $3 AND root.ruleset_key = $4 AND head.state NOT IN ('completed', 'failed')) AS unresolved_receipt_count, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_receipt_roots_v1 AS root INNER JOIN public.runtime_interaction_effect_heads_v1 AS effect ON effect.application_id = root.application_id AND effect.interaction_id = root.interaction_id WHERE root.application_id = $5 AND root.tenant_id = $1 AND root.installation_id = $2 AND root.guild_id = $3 AND root.ruleset_key = $4 AND effect.action_kind <> 'edit_response' AND effect.state IN ('intended', 'indeterminate', 'observing', 'observation_pending', 'compensation_intended', 'compensation_indeterminate', 'compensation_observing', 'compensation_observation_pending', 'recovery_required')) AS unresolved_journal_count, (SELECT pg_catalog.count(*) FROM public.runtime_interaction_receipt_roots_v1 AS root INNER JOIN public.runtime_interaction_effect_rollbacks_v1 AS rollback ON rollback.application_id = root.application_id AND rollback.interaction_id = root.interaction_id WHERE root.application_id = $5 AND root.tenant_id = $1 AND root.installation_id = $2 AND root.guild_id = $3 AND root.ruleset_key = $4 AND rollback.state = 'required') AS unresolved_rollback_count",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(RULESET_KEY)
    .bind(&scope.application_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    let scoped_installation_count = get(&row, "installation_count")?;
    let scoped_deployment_count = get(&row, "deployment_count")?;
    let terminal_product_operation_count = get(&row, "terminal_operation_count")?;
    let unresolved_product_operation_count = get(&row, "unresolved_operation_count")?;
    let unresolved_receipt_count = get(&row, "unresolved_receipt_count")?;
    let unresolved_journal_entry_count = get(&row, "unresolved_journal_count")?;
    let unresolved_rollback_count = get(&row, "unresolved_rollback_count")?;
    let ready_for_cleanup = scoped_installation_count == 1
        && scoped_deployment_count > 0
        && unresolved_product_operation_count == 0
        && unresolved_receipt_count == 0
        && unresolved_journal_entry_count == 0
        && unresolved_rollback_count == 0;
    if !ready_for_cleanup {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(PrecleanupEvidenceV1 {
        installation_id: scope.installation_id.clone(),
        scoped_installation_count,
        scoped_deployment_count,
        terminal_product_operation_count,
        unresolved_product_operation_count,
        unresolved_receipt_count,
        unresolved_journal_entry_count,
        unresolved_rollback_count,
        ready_for_cleanup,
    })
}

async fn inspect_absence(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
) -> Result<AbsenceEvidenceV1, D2ProvisionerErrorV1> {
    let database_count: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_database WHERE datname = $1",
    )
    .bind(DATABASE_NAME)
    .fetch_one(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    if database_count != 0 {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(AbsenceEvidenceV1 {
        run_id: scope.run_id.clone(),
        installation_id: scope.installation_id.clone(),
        database_absent: true,
    })
}

async fn scoped_phase_count(
    connection: &mut PgConnection,
    scope: &InspectionScopeV1,
    phase: &str,
) -> Result<i64, D2ProvisionerErrorV1> {
    sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM public.runtime_deployments WHERE tenant_id = $1 AND installation_id = $2 AND guild_id = $3 AND ruleset_key = $4 AND phase = $5",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.guild_id)
    .bind(RULESET_KEY)
    .bind(phase)
    .fetch_one(connection)
    .await
    .map_err(|_| D2ProvisionerErrorV1::Inspection)
}

fn validate_route_identity(route: &RouteIdentityV1) -> Result<(), D2ProvisionerErrorV1> {
    if !valid_identifier(&route.deployment_id)
        || route.runtime_generation < 1
        || route.route_controller_fencing_token < 1
        || route.route_incarnation < 1
        || !valid_process_instance(&route.origin_process_instance_id)
        || route.origin_serving_lease_epoch < 1
        || route.origin_serving_revision < 1
        || !valid_identifier(&route.origin_gateway_shard_id)
        || route.origin_gateway_owner_lease_epoch < 1
        || route.origin_gateway_owner_revision < 1
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(())
}

fn validate_serving_identity(
    serving: &ServingIdentityV1,
    scope: &InspectionScopeV1,
) -> Result<(), D2ProvisionerErrorV1> {
    if serving.guild_id != scope.guild_id
        || !valid_snowflake(&serving.guild_id)
        || serving.ruleset_key != RULESET_KEY
        || !valid_identifier(&serving.ruleset_key)
        || serving.tenant_id != scope.tenant_id
        || !valid_identifier(&serving.tenant_id)
        || serving.installation_id != scope.installation_id
        || !valid_identifier(&serving.installation_id)
        || !valid_identifier(&serving.deployment_id)
        || !valid_sha256(&serving.attestation_id)
        || !valid_process_instance(&serving.process_instance_id)
        || serving.runtime_generation < 1
        || serving.target_version < 1
        || !valid_sha256(&serving.target_content_hash)
        || serving.binding_revision < 1
        || !valid_sha256(&serving.binding_fingerprint)
        || serving.lease_epoch < 1
        || serving.revision < 1
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(())
}

fn validate_effect_identity(effect: &EffectIdentityV1) -> Result<(), D2ProvisionerErrorV1> {
    if !valid_snowflake(&effect.application_id)
        || !valid_snowflake(&effect.interaction_id)
        || !(0..=255).contains(&effect.action_index)
    {
        return Err(D2ProvisionerErrorV1::Inspection);
    }
    Ok(())
}

fn exact_authoring_scope(scoped_count: i64) -> bool {
    scoped_count == 1
}

#[cfg(test)]
fn cleanup_blocking_effect_state(state: &str) -> bool {
    matches!(
        state,
        "intended"
            | "indeterminate"
            | "observing"
            | "observation_pending"
            | "compensation_intended"
            | "compensation_indeterminate"
            | "compensation_observing"
            | "compensation_observation_pending"
            | "recovery_required"
    )
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_process_instance(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_snowflake(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('0')
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 192
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b':' | b'.' | b'_' | b'-'))
        })
}

fn valid_distinct_snowflakes(values: &[String]) -> bool {
    !values.is_empty()
        && values.len() <= 128
        && values.iter().all(|value| valid_snowflake(value))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn get<T>(row: &PgRow, column: &str) -> Result<T, D2ProvisionerErrorV1>
where
    for<'value> T: Decode<'value, Postgres> + Type<Postgres>,
{
    row.try_get(column)
        .map_err(|_| D2ProvisionerErrorV1::Inspection)
}

fn identity_hash<T: Serialize>(kind: &str, identity: &T) -> Result<String, D2ProvisionerErrorV1> {
    let identity = serde_json::to_value(identity).map_err(|_| D2ProvisionerErrorV1::Inspection)?;
    let envelope = serde_json::json!({
        "schema_version": 1,
        "kind": kind,
        "identity": identity,
    });
    let mut canonical = String::new();
    write_canonical_json(&envelope, &mut canonical)?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

fn write_canonical_json(value: &Value, output: &mut String) -> Result<(), D2ProvisionerErrorV1> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&value.to_string()),
        Value::String(value) => output
            .push_str(&serde_json::to_string(value).map_err(|_| D2ProvisionerErrorV1::Inspection)?),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical_json(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(
                    &serde_json::to_string(key).map_err(|_| D2ProvisionerErrorV1::Inspection)?,
                );
                output.push(':');
                write_canonical_json(
                    values.get(*key).ok_or(D2ProvisionerErrorV1::Inspection)?,
                    output,
                )?;
            }
            output.push('}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_parser_is_closed() {
        let accepted = [
            "authoring",
            "live",
            "interaction",
            "duplicate",
            "restart",
            "reconciliation",
            "replacement",
            "precleanup",
            "absence",
        ];
        for value in accepted {
            assert_eq!(
                value.parse::<D2InspectionCheckpointV1>().unwrap().as_str(),
                value
            );
        }
        for value in ["", "Authoring", "cleanup", "live ", "interaction/raw"] {
            assert_eq!(
                value.parse::<D2InspectionCheckpointV1>(),
                Err(D2ProvisionerErrorV1::Arguments)
            );
        }
    }

    #[test]
    fn resolved_role_membership_is_exact_and_complete() {
        let document = serde_json::json!({
            "references": [
                {"slot": "guild_id", "id": "1533137713476272288"},
                {"slot": "role_id", "id": "1533137713476272290"},
                {"slot": "user_id", "id": "1056857223529250906"}
            ]
        });
        assert_eq!(
            resolved_role_membership_ids(&document).unwrap(),
            (
                "1533137713476272288".to_owned(),
                "1056857223529250906".to_owned(),
                "1533137713476272290".to_owned(),
            )
        );
    }

    #[test]
    fn resolved_role_membership_rejects_ambiguous_or_incomplete_inputs() {
        let cases = [
            serde_json::json!({
                "references": [
                    {"slot": "guild_id", "id": "1533137713476272288"},
                    {"slot": "role_id", "id": "1533137713476272290"},
                    {"slot": "role_id", "id": "1533137713476272291"}
                ]
            }),
            serde_json::json!({
                "references": [
                    {"slot": "guild_id", "id": "1533137713476272288"},
                    {"slot": "role_id", "id": "1533137713476272290"}
                ]
            }),
            serde_json::json!({
                "references": [
                    {"slot": "guild_id", "id": "1533137713476272288"},
                    {"slot": "role_id", "id": "1533137713476272290"},
                    {"slot": "member_id", "id": "1056857223529250906"}
                ]
            }),
            serde_json::json!({
                "references": [
                    {"slot": "guild_id", "id": "1533137713476272288"},
                    {"slot": "role_id", "id": "not-a-snowflake"},
                    {"slot": "user_id", "id": "1056857223529250906"}
                ]
            }),
        ];
        for document in cases {
            assert_eq!(
                resolved_role_membership_ids(&document),
                Err(D2ProvisionerErrorV1::Inspection)
            );
        }
    }

    #[test]
    fn canonical_identity_hashes_match_contract_vectors() {
        let route = RouteIdentityV1 {
            deployment_id: "deployment-1".to_owned(),
            runtime_generation: 1,
            route_controller_fencing_token: 2,
            route_incarnation: 3,
            origin_process_instance_id: "0123456789abcdef0123456789abcdef".to_owned(),
            origin_serving_lease_epoch: 4,
            origin_serving_revision: 5,
            origin_gateway_shard_id: "shard:0".to_owned(),
            origin_gateway_owner_lease_epoch: 6,
            origin_gateway_owner_revision: 7,
        };
        assert_eq!(
            identity_hash(ROUTE_IDENTITY_KIND, &route).unwrap(),
            "a1e5612095ca59f697a0259efe14bc88b415c54f52a78b8b10730c99bcf75aac"
        );
        let serving = ServingIdentityV1 {
            guild_id: "1533137713476272288".to_owned(),
            ruleset_key: "studyroom".to_owned(),
            tenant_id: "tenant:starring-d2-test".to_owned(),
            installation_id: "installation:starring-d2-test".to_owned(),
            deployment_id: "deployment-1".to_owned(),
            attestation_id: "a".repeat(64),
            process_instance_id: "0123456789abcdef0123456789abcdef".to_owned(),
            runtime_generation: 1,
            target_version: 2,
            target_content_hash: "b".repeat(64),
            binding_revision: 1,
            binding_fingerprint: "c".repeat(64),
            lease_epoch: 4,
            revision: 5,
        };
        assert_eq!(
            identity_hash(SERVING_IDENTITY_KIND, &serving).unwrap(),
            "a50ecc86255e0956f811e1a23df0ec25c2d657e09ba8febfef01f071fbe41101"
        );
        let effect = EffectIdentityV1 {
            application_id: "1533144492293754900".to_owned(),
            interaction_id: "1533137713476272288".to_owned(),
            action_index: 0,
        };
        assert_eq!(
            identity_hash(EFFECT_IDENTITY_KIND, &effect).unwrap(),
            "de863a5d60c52f8c4f21d63acf78c4c782392f6ee05a5ec28f6e3458070b71eb"
        );
    }

    #[test]
    fn authoring_scope_and_cleanup_blockers_are_closed() {
        assert!(exact_authoring_scope(1));
        assert!(!exact_authoring_scope(0));
        assert!(!exact_authoring_scope(2));
        for state in [
            "intended",
            "indeterminate",
            "observing",
            "observation_pending",
            "compensation_intended",
            "compensation_indeterminate",
            "compensation_observing",
            "compensation_observation_pending",
            "recovery_required",
        ] {
            assert!(cleanup_blocking_effect_state(state));
        }
        for state in [
            "planned",
            "known_succeeded",
            "known_failed",
            "reconciled_succeeded",
            "compensated",
        ] {
            assert!(!cleanup_blocking_effect_state(state));
        }
        let source = include_str!("d2_evidence.rs");
        assert!(source.contains("head.state NOT IN ('completed', 'failed')"));
        assert!(source.contains("'compensation_observation_pending', 'recovery_required'"));
    }

    #[test]
    fn serialized_evidence_is_redacted_and_versioned() {
        let route = RouteIdentityV1 {
            deployment_id: "deployment-1".to_owned(),
            runtime_generation: 1,
            route_controller_fencing_token: 2,
            route_incarnation: 3,
            origin_process_instance_id: "0123456789abcdef0123456789abcdef".to_owned(),
            origin_serving_lease_epoch: 4,
            origin_serving_revision: 5,
            origin_gateway_shard_id: "shard:0".to_owned(),
            origin_gateway_owner_lease_epoch: 6,
            origin_gateway_owner_revision: 7,
        };
        let serving = ServingIdentityV1 {
            guild_id: "1533137713476272288".to_owned(),
            ruleset_key: "studyroom".to_owned(),
            tenant_id: "tenant:starring-d2-test".to_owned(),
            installation_id: "installation:starring-d2-test".to_owned(),
            deployment_id: "deployment-1".to_owned(),
            attestation_id: "a".repeat(64),
            process_instance_id: "0123456789abcdef0123456789abcdef".to_owned(),
            runtime_generation: 1,
            target_version: 2,
            target_content_hash: "b".repeat(64),
            binding_revision: 1,
            binding_fingerprint: "c".repeat(64),
            lease_epoch: 4,
            revision: 5,
        };
        let report = D2InspectionReportV1 {
            schema_version: 1,
            kind: D2InspectionCheckpointV1::Live.kind(),
            observed_at: "2026-08-01T12:00:00.000000Z".to_owned(),
            evidence: D2InspectionEvidenceV1::Live(Box::new(LiveEvidenceV1 {
                installation_id: "installation:starring-d2-test".to_owned(),
                promotion_id: "d".repeat(64),
                deployment_id: "deployment-1".to_owned(),
                attestation_id: "a".repeat(64),
                deployment_revision: 8,
                convergence_attempt: 1,
                process_instance_id: "0123456789abcdef0123456789abcdef".to_owned(),
                last_heartbeat_at: "2026-08-01T11:59:58.000000Z".to_owned(),
                lease_expires_at: "2026-08-01T12:00:43.000000Z".to_owned(),
                route_identity: route,
                serving_identity: serving,
            })),
        };
        let payload = report.to_json().unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["kind"], "starring.d2.db-live-evidence.v1");
        let mut keys = parsed
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "attestation_id",
                "convergence_attempt",
                "deployment_id",
                "deployment_revision",
                "installation_id",
                "kind",
                "last_heartbeat_at",
                "lease_expires_at",
                "observed_at",
                "process_instance_id",
                "promotion_id",
                "route_identity",
                "schema_version",
                "serving_identity",
            ]
        );
        for forbidden in [
            "ciphertext",
            "nonce",
            "key_material",
            "bearer_token",
            "database_url",
            "transcript",
            "full_ruleset",
            "recovery_json",
        ] {
            assert!(!payload.contains(forbidden));
        }
    }

    #[test]
    fn adapter_bound_checkpoint_envelopes_are_exactly_flat() {
        let route = RouteIdentityV1 {
            deployment_id: "deployment-1".to_owned(),
            runtime_generation: 1,
            route_controller_fencing_token: 2,
            route_incarnation: 3,
            origin_process_instance_id: "0123456789abcdef0123456789abcdef".to_owned(),
            origin_serving_lease_epoch: 4,
            origin_serving_revision: 5,
            origin_gateway_shard_id: "shard:0".to_owned(),
            origin_gateway_owner_lease_epoch: 6,
            origin_gateway_owner_revision: 7,
        };
        let serving = ServingIdentityV1 {
            guild_id: "1533137713476272288".to_owned(),
            ruleset_key: "studyroom".to_owned(),
            tenant_id: "tenant:starring-d2-test".to_owned(),
            installation_id: "installation:starring-d2-test".to_owned(),
            deployment_id: "deployment-1".to_owned(),
            attestation_id: "a".repeat(64),
            process_instance_id: "0123456789abcdef0123456789abcdef".to_owned(),
            runtime_generation: 1,
            target_version: 2,
            target_content_hash: "b".repeat(64),
            binding_revision: 1,
            binding_fingerprint: "c".repeat(64),
            lease_epoch: 4,
            revision: 5,
        };
        let cases = [
            (
                D2InspectionCheckpointV1::Authoring,
                D2InspectionEvidenceV1::Authoring(Box::new(AuthoringEvidenceV1 {
                    generation_encrypted: true,
                    projection_state: "preview_ready".to_owned(),
                    generation: 1,
                    generation_count: 1,
                    payload_digest: "a".repeat(64),
                    worker_request_id: "worker-request-1".to_owned(),
                    worker_completion_sha256: "b".repeat(64),
                    installation_id: "installation:starring-d2-test".to_owned(),
                    authoring_session_id: "session-1".to_owned(),
                    generation_created_at: "2026-08-04T01:02:03.000000Z".to_owned(),
                })),
                vec![
                    "authoring_session_id",
                    "generation",
                    "generation_count",
                    "generation_created_at",
                    "generation_encrypted",
                    "installation_id",
                    "kind",
                    "observed_at",
                    "payload_digest",
                    "projection_state",
                    "schema_version",
                    "worker_completion_sha256",
                    "worker_request_id",
                ],
            ),
            (
                D2InspectionCheckpointV1::Live,
                D2InspectionEvidenceV1::Live(Box::new(LiveEvidenceV1 {
                    installation_id: "installation:starring-d2-test".to_owned(),
                    promotion_id: "d".repeat(64),
                    deployment_id: "deployment-1".to_owned(),
                    attestation_id: "a".repeat(64),
                    deployment_revision: 8,
                    convergence_attempt: 1,
                    process_instance_id: "0123456789abcdef0123456789abcdef".to_owned(),
                    last_heartbeat_at: "2026-08-01T11:59:58.000000Z".to_owned(),
                    lease_expires_at: "2026-08-01T12:00:43.000000Z".to_owned(),
                    route_identity: route.clone(),
                    serving_identity: serving.clone(),
                })),
                vec![
                    "attestation_id",
                    "convergence_attempt",
                    "deployment_id",
                    "deployment_revision",
                    "installation_id",
                    "kind",
                    "last_heartbeat_at",
                    "lease_expires_at",
                    "observed_at",
                    "process_instance_id",
                    "promotion_id",
                    "route_identity",
                    "schema_version",
                    "serving_identity",
                ],
            ),
            (
                D2InspectionCheckpointV1::Interaction,
                D2InspectionEvidenceV1::Interaction(Box::new(InteractionEvidenceV1 {
                    create_interaction_id: "1533137713476272288".to_owned(),
                    join_interaction_id: "1533137713476272289".to_owned(),
                    actor_user_id: "1056857223529250906".to_owned(),
                    joined_role_id: "1533137713476272290".to_owned(),
                    deployment_id: "deployment-1".to_owned(),
                    route_identity: route.clone(),
                    instance_id: "instance-1".to_owned(),
                    role_ids: vec!["1533137713476272290".to_owned()],
                    channel_ids: vec!["1533137713476272291".to_owned()],
                    panel_message_ids: vec!["1533137713476272292".to_owned()],
                    ephemeral_count: 2,
                })),
                vec![
                    "actor_user_id",
                    "channel_ids",
                    "create_interaction_id",
                    "deployment_id",
                    "ephemeral_count",
                    "instance_id",
                    "join_interaction_id",
                    "joined_role_id",
                    "kind",
                    "observed_at",
                    "panel_message_ids",
                    "role_ids",
                    "route_identity",
                    "schema_version",
                ],
            ),
            (
                D2InspectionCheckpointV1::Duplicate,
                D2InspectionEvidenceV1::Duplicate(Box::new(DuplicateEvidenceV1 {
                    interaction_id: "1533137713476272288".to_owned(),
                    effect_identity: EffectIdentityV1 {
                        application_id: "1533144492293754900".to_owned(),
                        interaction_id: "1533137713476272288".to_owned(),
                        action_index: 0,
                    },
                    external_effect_count: 1,
                    receipt_state: "completed".to_owned(),
                })),
                vec![
                    "effect_identity",
                    "external_effect_count",
                    "interaction_id",
                    "kind",
                    "observed_at",
                    "receipt_state",
                    "schema_version",
                ],
            ),
            (
                D2InspectionCheckpointV1::Restart,
                D2InspectionEvidenceV1::Restart(Box::new(RestartEvidenceV1 {
                    route_reconstructed: true,
                    instance_reconstructed: true,
                    deployment_id: "deployment-1".to_owned(),
                    source_route_identity: route.clone(),
                    reconstructed_route_identity: route.clone(),
                    source_serving_identity: serving.clone(),
                    reconstructed_serving_identity: serving.clone(),
                    instance_id: "instance-1".to_owned(),
                    pinned_ruleset_digest: "d".repeat(64),
                    probe_interaction_id: "1533137713476272288".to_owned(),
                    process_instance_id: "0123456789abcdef0123456789abcdef".to_owned(),
                })),
                vec![
                    "deployment_id",
                    "instance_id",
                    "instance_reconstructed",
                    "kind",
                    "observed_at",
                    "pinned_ruleset_digest",
                    "probe_interaction_id",
                    "process_instance_id",
                    "reconstructed_route_identity",
                    "reconstructed_serving_identity",
                    "route_reconstructed",
                    "schema_version",
                    "source_route_identity",
                    "source_serving_identity",
                ],
            ),
            (
                D2InspectionCheckpointV1::Reconciliation,
                D2InspectionEvidenceV1::Reconciliation(Box::new(ReconciliationEvidenceV1 {
                    effect_identity: EffectIdentityV1 {
                        application_id: "1533144492293754900".to_owned(),
                        interaction_id: "1533137713476272288".to_owned(),
                        action_index: 0,
                    },
                    interaction_id: "1533137713476272288".to_owned(),
                    route_identity: route.clone(),
                    output_role_id: "1533137713476272289".to_owned(),
                    reconciliation_state: "known_success".to_owned(),
                    duplicate_external_effect_count: 0,
                    unsafe_deletion_count: 0,
                })),
                vec![
                    "duplicate_external_effect_count",
                    "effect_identity",
                    "interaction_id",
                    "kind",
                    "observed_at",
                    "output_role_id",
                    "reconciliation_state",
                    "route_identity",
                    "schema_version",
                    "unsafe_deletion_count",
                ],
            ),
            (
                D2InspectionCheckpointV1::Replacement,
                D2InspectionEvidenceV1::Replacement(Box::new(ReplacementEvidenceV1 {
                    installation_id: "installation:starring-d2-test".to_owned(),
                    source_promotion_id: "a".repeat(64),
                    replacement_promotion_id: "b".repeat(64),
                    source_deployment_id: "deployment-1".to_owned(),
                    source_route_identity: route.clone(),
                    replacement_deployment_id: "deployment-2".to_owned(),
                    replacement_route_identity: route.clone(),
                    previous_target_drained: true,
                    replacement_live: true,
                    prior_route_absent: true,
                })),
                vec![
                    "installation_id",
                    "kind",
                    "observed_at",
                    "previous_target_drained",
                    "prior_route_absent",
                    "replacement_deployment_id",
                    "replacement_live",
                    "replacement_promotion_id",
                    "replacement_route_identity",
                    "schema_version",
                    "source_deployment_id",
                    "source_promotion_id",
                    "source_route_identity",
                ],
            ),
        ];
        for (checkpoint, evidence, expected) in cases {
            let report = D2InspectionReportV1 {
                schema_version: 1,
                kind: checkpoint.kind(),
                observed_at: "2026-08-01T12:00:00.000000Z".to_owned(),
                evidence,
            };
            let value: Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
            let mut observed = value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>();
            observed.sort_unstable();
            assert_eq!(observed, expected);
            assert_eq!(value["kind"], checkpoint.kind());
            assert!(value.get("checkpoint").is_none());
            assert!(value.get("evidence").is_none());
            assert!(value.get("run_id").is_none());
        }
    }

    #[test]
    fn inspection_queries_pin_read_only_snapshot_and_exact_scope() {
        let source = include_str!("d2_evidence.rs");
        assert!(source.contains("REPEATABLE READ READ ONLY"));
        assert!(source.contains("statement_timeout = '3s'"));
        assert!(source.contains("lock_timeout = '500ms'"));
        assert!(source.contains("tenant_id = $1"));
        assert!(source.contains("installation_id = $2"));
        assert!(!source.contains(&["SELECT ", "*"].concat()));
    }

    #[test]
    fn live_evidence_binds_attestation_process_and_active_lease() {
        let source = include_str!("d2_evidence.rs");
        for predicate in [
            "deployment.convergence_attempt_no = attestation.convergence_attempt_no",
            "attestation.process_instance_id = lease.process_instance_id",
            "lease.last_heartbeat_at <= pg_catalog.transaction_timestamp()",
            "lease.expires_at > pg_catalog.transaction_timestamp()",
        ] {
            assert!(source.contains(predicate));
        }
        for field in [
            "attestation.deployment_revision",
            "attestation.convergence_attempt_no AS attestation_convergence_attempt",
            "attestation.process_instance_id AS attested_process_instance_id",
            "AS last_heartbeat_at",
            "AS lease_expires_at",
        ] {
            assert!(source.contains(field));
        }
    }

    #[test]
    fn queried_columns_exist_in_migrations() {
        let authoring =
            include_str!("../../../migrations/202607190001_create_product_control_plane.sql");
        let trusted = include_str!(
            "../../../migrations/202607300001_add_trusted_authoring_generation_writer.sql"
        );
        let runtime =
            include_str!("../../../migrations/202607190002_create_runtime_convergence.sql");
        let certification =
            include_str!("../../../migrations/202607300003_finalize_runtime_certification_v2.sql");
        let receipts = include_str!(
            "../../../migrations/202607310022_add_runtime_interaction_receipts_v1.sql"
        );
        let effects = include_str!(
            "../../../migrations/202608010001_add_runtime_interaction_effect_journal_v1.sql"
        );
        let drain = include_str!(
            "../../../migrations/202607280003_add_runtime_product_drain_terminal_substrate_v2.sql"
        );
        for (migration, names) in [
            (
                authoring,
                &["authoring_sessions", "authoring_session_generations"][..],
            ),
            (trusted, &["safe_turn_projection_digest"][..]),
            (
                runtime,
                &[
                    "runtime_deployments",
                    "runtime_attestations",
                    "runtime_serving_leases",
                ][..],
            ),
            (
                certification,
                &["v2_route_admission", "v2_route_incarnation"][..],
            ),
            (
                receipts,
                &[
                    "runtime_interaction_receipt_roots_v1",
                    "runtime_interaction_receipt_heads_v1",
                ][..],
            ),
            (
                effects,
                &[
                    "runtime_interaction_effect_heads_v1",
                    "runtime_interaction_effect_events_v1",
                    "runtime_interaction_effect_rollbacks_v1",
                ][..],
            ),
            (drain, &["runtime_product_drain_terminal_actions_v2"][..]),
        ] {
            for name in names {
                assert!(migration.contains(name));
            }
        }
    }

    #[test]
    fn destruction_report_is_strict_redacted_and_replayable() {
        let scope = InspectionScopeV1 {
            run_id: "20260804-0123456789ab".to_owned(),
            tenant_id: "tenant:starring-d2-20260804-0123456789ab".to_owned(),
            installation_id: "installation:starring-d2-20260804-0123456789ab".to_owned(),
            guild_id: "1533137713476272288".to_owned(),
            application_id: "1533144492293754900".to_owned(),
        };
        for outcome in [
            D2DestroyOutcomeV1::Destroyed,
            D2DestroyOutcomeV1::ExactReplay,
        ] {
            let report = destroy_report(&scope, outcome);
            assert!(report.database_absent());
            assert_eq!(report.outcome(), outcome);
            let payload = report.to_json().unwrap();
            let value: Value = serde_json::from_str(&payload).unwrap();
            let mut keys = value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>();
            keys.sort_unstable();
            assert_eq!(
                keys,
                [
                    "database_absent",
                    "installation_id",
                    "kind",
                    "outcome",
                    "schema_version",
                ]
            );
            assert_eq!(value["schema_version"], 1);
            assert_eq!(value["kind"], DESTROY_KIND);
            assert_eq!(value["outcome"], outcome.as_str());
            assert_eq!(value["database_absent"], true);
            for forbidden in [
                "password",
                "token",
                "database_url",
                "ciphertext",
                "key_material",
                "transcript",
            ] {
                assert!(!payload.contains(forbidden));
            }
        }
    }

    #[test]
    fn destruction_plan_has_one_mutating_state_and_exact_replay() {
        assert_eq!(destruction_plan((1, 0)).unwrap(), D2DestructionPlanV1::Drop);
        assert_eq!(
            destruction_plan((0, 0)).unwrap(),
            D2DestructionPlanV1::ExactReplay
        );
        for state in [(0, 1), (1, 1), (2, 0), (-1, 0)] {
            assert_eq!(
                destruction_plan(state),
                Err(D2ProvisionerErrorV1::Destruction)
            );
        }
    }

    #[test]
    fn destruction_sql_is_fixed_nonforcing_and_current() {
        assert_eq!(
            DROP_DATABASE_SQL,
            ["DROP DATABASE ", DATABASE_NAME].concat()
        );
        assert!(!DROP_DATABASE_SQL.contains('$'));
        assert!(!DROP_DATABASE_SQL
            .to_ascii_uppercase()
            .contains(&["FOR", "CE"].concat()));
        assert!(!DROP_DATABASE_SQL.contains(';'));
        let migration =
            include_bytes!("../../../migrations/202608040004_refresh_serving_pending_product_drain_readiness_v1.sql");
        let digest = <sha2::Sha384 as sha2::Digest>::digest(migration);
        assert_eq!(format!("{digest:x}"), EXPECTED_MIGRATION_HEAD_CHECKSUM);
    }

    #[test]
    fn destruction_preflight_is_exclusive_exact_and_secret_free() {
        let source = include_str!("d2_evidence.rs");
        assert!(source.contains("pg_try_advisory_lock"));
        assert!(source.contains("starring-d2-sealed-provisioner:"));
        assert!(source.contains("starring-d2-sealed-destroy:"));
        assert!(source.contains("pg_stat_activity"));
        assert!(source.contains("activity.pid = $2"));
        assert!(source.contains("starring_runtime_exact_target_schema_manifest_v2()"));
        assert!(source.contains("starring_runtime_interaction_effect_schema_manifest_v1()"));
        assert!(source.contains("discord_application_id = $4"));
        assert!(source.contains("discord_guild_id = $3"));
        assert!(source.contains("ruleset_key = $5"));
        assert!(source.contains("target.close()"));
        assert!(!source.contains(&["pg_terminate", "_backend"].concat()));
        assert_eq!(
            D2ProvisionerErrorV1::Destruction.code(),
            "d2_destruction_failed"
        );
        assert_eq!(
            D2ProvisionerErrorV1::Destruction.to_string(),
            "d2_destruction_failed"
        );
    }
}
