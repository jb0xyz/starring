use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use automation_runtime_controller::{
    encode_runtime_live_attestation_record_v1, runtime_desired_target_digest_v1,
    runtime_live_attestation_digest_v1, GatewayShardIdV1, RuntimeBuildRevisionV1,
    RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeClaimNextExecutionV1, RuntimeConvergenceMutationV1, RuntimeConvergenceSessionStateV1,
    RuntimeConvergenceSessionV1, RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1,
    RuntimeGatewayOwnerLeaseDurationV1, RuntimeGatewayOwnerLeaseObservationV1,
    RuntimeLiveAttestationRecordV1, RuntimeMutationReceiptV1, RuntimeObserveGatewayOwnerLeaseV1,
    RuntimeObserveWriterFenceV1, RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
    RuntimeReleaseGatewayOwnerLeaseV1, RuntimeRenewGatewayOwnerLeaseOutcomeV1,
    RuntimeRenewGatewayOwnerLeaseV1, RuntimeWriterFenceObservationV1,
};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ControllerId, DrainAttestationV1,
    GatewayReadyAttestationV1, GatewayReadyKindV1, LiveAttestationV1, PanelCertificateId,
    PanelCertificateV1, PanelReportDigestV1, PreflightAttestationV1, ProcessInstanceId,
    RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1,
    RuntimeGeneration, RuntimePendingConditionV1, SupersedingDeploymentV1, TransitionOutcomeV1,
};
use automation_runtime_execution_postgres::{
    observe_runtime_execution_database_identity_v1, PostgresRuntimeExecutionV1,
    RuntimeExecutionDatabaseExpectationV1,
    RuntimeExecutionPersistenceErrorV1, MIGRATOR,
};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions, PgSslMode};
use sqlx::types::Json;
use sqlx::{Connection, Executor, PgPool};
use automation_runtime_worker::{
    classify_unknown_gateway_owner_acquire_v1, classify_unknown_gateway_owner_release_v1,
    classify_unknown_gateway_owner_renew_v1, RuntimeGatewayOwnerAcquireRecoveryV1,
    RuntimeGatewayOwnerLeasePortV1, RuntimeGatewayOwnerReleaseRecoveryV1,
    RuntimeGatewayOwnerRenewRecoveryV1, RuntimeWriterFenceObservationPortV1,
};

const READINESS_FUNCTION: &str = "public.starring_runtime_execution_database_readiness_v1()";
const EXACT_TARGET_FUNCTIONS: [&str; 3] = [
    "public.starring_runtime_exact_target_database_readiness_v1()",
    "public.starring_runtime_exact_target_reader_database_identity_v1()",
    "public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)",
];
const SERVING_FUNCTIONS: [&str; 4] = [
    "public.starring_runtime_serving_database_readiness_v1()",
    "public.starring_runtime_serving_database_identity_v1()",
    "public.starring_runtime_serving_heartbeat_v1(text,text,text,text,text,bigint,bigint,bigint,bigint)",
    "public.starring_runtime_serving_disconnect_v1(text,text,text,text,text,bigint,bigint,bigint)",
];
const EXECUTOR_FUNCTIONS: [&str; 15] = [
    "public.starring_runtime_execution_database_readiness_v1()",
    "public.starring_runtime_execution_database_identity_v1()",
    "public.starring_runtime_execution_claim_next_v1(text,bigint)",
    "public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)",
    "public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)",
    "public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)",
    "public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)",
    "public.starring_runtime_execution_recover_stale_live_v1()",
    "public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)",
    "public.starring_runtime_gateway_owner_observe_v1(text)",
    "public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)",
    "public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)",
    "public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)",
    "public.starring_runtime_writer_fence_observe_v1()",
    "public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)",
];
const EXPECTED_READINESS_DEFINITION_SHA256_V1: &str =
    "b5362bc1b081789a5b3ac4881fc2ea00c340a013630f7d5c809958ed1c045ec3";
const TENANT: &str = "runtime-execution-tenant";
const INSTALLATION: &str = "runtime-execution-installation";
const PRINCIPAL: &str = "runtime-execution-principal";
const GUILD: u64 = 9_200_101;
const RULESET: &str = "runtime_execution_ruleset";
const PROMOTION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACTIVATION: &str = "runtime_execution_activation";
const DEPLOYMENT: &str = "runtime-execution-deployment";
const CONTENT_HASH: &str = "9f2bbed3d90d3439ebe5bb07a69f8ff179c29e8c71500b6890a7d24653a65ff6";
const BINDING_FINGERPRINT: &str =
    "a44fd4f629a1183147a25a8afb93b026de7e3f92efe737637da222617df0c655";
static UNIQUE_SUFFIX_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct IsolatedDatabase {
    name: String,
    role: String,
    administrator_role: String,
    administrator: PgConnection,
    owner_pool: PgPool,
    executor_pool: PgPool,
    foreign_database_options: PgConnectOptions,
    connect_options: PgConnectOptions,
    additional_roles: Vec<String>,
}

struct EphemeralPostgresCluster {
    root: PathBuf,
    data: PathBuf,
    socket: PathBuf,
    port: u16,
    administrator_role: String,
    pg_ctl: String,
    running: bool,
}

enum PostgresTestServer {
    External(Box<PgConnectOptions>),
    Ephemeral(EphemeralPostgresCluster),
}

impl PostgresTestServer {
    fn start() -> Self {
        if let Some(url) = std::env::var_os("STARRING_TEST_DATABASE_URL") {
            let url = url
                .into_string()
                .expect("STARRING_TEST_DATABASE_URL must be valid Unicode");
            let options = url
                .parse::<PgConnectOptions>()
                .expect("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL");
            let database = options
                .get_database()
                .expect("STARRING_TEST_DATABASE_URL must name a database");
            assert!(
                database.starts_with("starring_")
                    && database.split('_').any(|segment| segment == "test")
                    && canonical_identifier(database),
                "refusing to use a database outside the strict Starring test namespace"
            );
            Self::External(Box::new(options))
        } else {
            Self::Ephemeral(EphemeralPostgresCluster::start())
        }
    }

    fn connect_options(&self) -> PgConnectOptions {
        match self {
            Self::External(options) => options.as_ref().clone(),
            Self::Ephemeral(cluster) => cluster.connect_options(),
        }
    }
}

impl EphemeralPostgresCluster {
    fn start() -> Self {
        let suffix = unique_suffix();
        let root = PathBuf::from("/tmp").join(format!("sre-{}-{suffix:x}", std::process::id()));
        let data = root.join("data");
        let socket = root.join("socket");
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&socket).unwrap();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let administrator_role = "starring_test_administrator".to_string();
        let initdb = std::env::var("STARRING_TEST_INITDB").unwrap_or_else(|_| "initdb".into());
        let pg_ctl = std::env::var("STARRING_TEST_PG_CTL").unwrap_or_else(|_| "pg_ctl".into());
        let initialized = Command::new(&initdb)
            .args([
                "-D",
                data.to_str().unwrap(),
                "-A",
                "trust",
                "-U",
                administrator_role.as_str(),
                "--encoding=UTF8",
                "--no-locale",
                "--no-sync",
            ])
            .output()
            .unwrap();
        assert!(
            initialized.status.success(),
            "initdb failed: {}",
            String::from_utf8_lossy(&initialized.stderr)
        );
        let port = 40_000 + (suffix % 20_000) as u16;
        let cluster = Self {
            root,
            data,
            socket,
            port,
            administrator_role,
            pg_ctl,
            running: true,
        };
        let server_options = format!(
            "-F -k {} -h '' -p {} -c unix_socket_permissions=0700",
            cluster.socket.to_str().unwrap(),
            cluster.port
        );
        let log = cluster.root.join("postgres.log");
        let started = Command::new(&cluster.pg_ctl)
            .args([
                "-D",
                cluster.data.to_str().unwrap(),
                "-l",
                log.to_str().unwrap(),
                "-o",
                server_options.as_str(),
                "-w",
                "start",
            ])
            .output()
            .unwrap();
        if !started.status.success() {
            panic!(
                "pg_ctl start failed: {} {} {}",
                String::from_utf8_lossy(&started.stdout),
                String::from_utf8_lossy(&started.stderr),
                fs::read_to_string(log).unwrap_or_default()
            );
        }
        cluster
    }

    fn connect_options(&self) -> PgConnectOptions {
        PgConnectOptions::new()
            .host(self.socket.to_str().unwrap())
            .port(self.port)
            .username(&self.administrator_role)
            .database("postgres")
            .ssl_mode(PgSslMode::Disable)
    }
}

impl Drop for EphemeralPostgresCluster {
    fn drop(&mut self) {
        if self.running {
            let _ = Command::new(&self.pg_ctl)
                .args([
                    "-D",
                    self.data.to_str().unwrap(),
                    "-m",
                    "immediate",
                    "-w",
                    "stop",
                ])
                .output();
            self.running = false;
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

async fn isolated_database(base: PgConnectOptions) -> IsolatedDatabase {
    let suffix = unique_suffix();
    let name = format!("starring_re_test_{suffix}");
    let role = format!("starring_re_executor_{suffix}");
    let password = format!("re_test_password_{suffix}");
    for identifier in [&name, &role] {
        assert!(canonical_identifier(identifier));
    }
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    administrator
        .execute(format!("CREATE DATABASE {name}").as_str())
        .await
        .unwrap();
    let owner_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(base.clone().database(&name))
        .await
        .unwrap();
    MIGRATOR.run(&owner_pool).await.unwrap();
    let password_literal = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
        .bind(&password)
        .fetch_one(&owner_pool)
        .await
        .unwrap();
    administrator
        .execute(
            format!(
                "CREATE ROLE {role} LOGIN PASSWORD {password_literal} NOINHERIT NOSUPERUSER \
                 NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4"
            )
            .as_str(),
        )
        .await
        .unwrap();
    let foreign_databases = sqlx::query_as::<_, (String, String)>(
        "SELECT database_row.datname::TEXT, \
            pg_catalog.quote_ident(database_row.datname)::TEXT \
         FROM pg_catalog.pg_database AS database_row \
         WHERE database_row.datallowconn AND database_row.datname <> $1 \
         ORDER BY database_row.datname",
    )
    .bind(&name)
    .fetch_all(&owner_pool)
    .await
    .unwrap();
    assert!(!foreign_databases.is_empty());
    for (_, quoted_database) in &foreign_databases {
        administrator
            .execute(
                format!("REVOKE ALL PRIVILEGES ON DATABASE {quoted_database} FROM PUBLIC").as_str(),
            )
            .await
            .unwrap();
    }
    for statement in [
        format!("REVOKE ALL PRIVILEGES ON DATABASE {name} FROM PUBLIC"),
        "REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC".to_string(),
        format!("GRANT CONNECT ON DATABASE {name} TO {role}"),
        format!("GRANT USAGE ON SCHEMA public TO {role}"),
    ] {
        owner_pool.execute(statement.as_str()).await.unwrap();
    }
    for function in EXECUTOR_FUNCTIONS {
        owner_pool
            .execute(format!("GRANT EXECUTE ON FUNCTION {function} TO {role}").as_str())
            .await
            .unwrap();
    }
    let administrator_role = base.get_username().to_string();
    let connect_options = base.clone().database(&name);
    let options = base.database(&name).username(&role).password(&password);
    let executor_pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET TimeZone = 'Asia/Seoul'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options.clone())
        .await
        .unwrap();
    let foreign_database_options = options.database(&foreign_databases[0].0);
    IsolatedDatabase {
        name,
        role,
        administrator_role,
        administrator,
        owner_pool,
        executor_pool,
        foreign_database_options,
        connect_options,
        additional_roles: Vec::new(),
    }
}

async fn cleanup(mut database: IsolatedDatabase) {
    database.executor_pool.close().await;
    database.owner_pool.close().await;
    database
        .administrator
        .execute(format!("DROP DATABASE {} WITH (FORCE)", database.name).as_str())
        .await
        .unwrap();
    database
        .administrator
        .execute(format!("DROP ROLE {}", database.role).as_str())
        .await
        .unwrap();
    for role in database.additional_roles {
        database
            .administrator
            .execute(format!("DROP ROLE {role}").as_str())
            .await
            .unwrap();
    }
}

async fn assert_cross_runtime_readiness(database: &mut IsolatedDatabase) {
    assert_controller_lookup_index(&database.owner_pool).await;
    let manifests = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT public.starring_runtime_exact_target_schema_manifest_v1(), \
            public.starring_runtime_serving_schema_manifest_v1(), \
            public.starring_runtime_execution_schema_manifest_v1()",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(manifests, (true, true, true));

    let mut drift = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_execution_mutation_markers \
         DROP CONSTRAINT runtime_execution_mutation_markers_payload_check",
    )
    .execute(&mut *drift)
    .await
    .unwrap();
    let drifted = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT public.starring_runtime_exact_target_schema_manifest_v1(), \
            public.starring_runtime_serving_schema_manifest_v1(), \
            public.starring_runtime_execution_schema_manifest_v1()",
    )
    .fetch_one(&mut *drift)
    .await
    .unwrap();
    assert_eq!(drifted, (true, true, false));
    drift.rollback().await.unwrap();

    let exact_pool =
        restricted_readiness_pool(database, "exact", EXACT_TARGET_FUNCTIONS.as_slice()).await;
    let exact_rows = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.starring_runtime_exact_target_database_readiness_v1()",
    )
    .fetch_one(&exact_pool)
    .await
    .unwrap();
    assert_eq!(exact_rows, 1);
    exact_pool.close().await;

    let serving_pool =
        restricted_readiness_pool(database, "serving", SERVING_FUNCTIONS.as_slice()).await;
    let serving_rows = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.starring_runtime_serving_database_readiness_v1()",
    )
    .fetch_one(&serving_pool)
    .await
    .unwrap();
    assert_eq!(serving_rows, 1);
    serving_pool.close().await;

    let execution_rows = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.starring_runtime_execution_database_readiness_v1()",
    )
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    assert_eq!(execution_rows, 1);
}

async fn assert_controller_lookup_index(pool: &PgPool) {
    let exact = sqlx::query_scalar::<_, bool>(
        "SELECT pg_catalog.count(*) = 1 \
         FROM pg_catalog.pg_index AS index_contract \
         JOIN pg_catalog.pg_class AS table_row \
             ON table_row.oid = index_contract.indrelid \
         JOIN pg_catalog.pg_namespace AS table_namespace \
             ON table_namespace.oid = table_row.relnamespace \
         JOIN pg_catalog.pg_class AS index_row \
             ON index_row.oid = index_contract.indexrelid \
         JOIN pg_catalog.pg_namespace AS index_namespace \
             ON index_namespace.oid = index_row.relnamespace \
         JOIN pg_catalog.pg_am AS index_method ON index_method.oid = index_row.relam \
         WHERE table_namespace.nspname = 'public' \
             AND table_row.relname = 'runtime_deployments' \
             AND index_namespace.nspname = 'public' \
             AND index_row.relname = 'runtime_deployments_active_controller_index' \
             AND index_row.relowner = table_row.relowner \
             AND index_row.relkind = 'i' AND index_row.relpersistence = 'p' \
             AND NOT index_row.relispartition AND index_method.amname = 'btree' \
             AND NOT index_contract.indisprimary \
             AND NOT index_contract.indisunique \
             AND index_contract.indisvalid AND index_contract.indisready \
             AND index_contract.indislive AND index_contract.indimmediate \
             AND NOT index_contract.indisclustered \
             AND NOT index_contract.indisreplident \
             AND NOT index_contract.indnullsnotdistinct \
             AND index_contract.indnkeyatts = 4 AND index_contract.indnatts = 4 \
             AND index_contract.indexprs IS NULL \
             AND pg_catalog.pg_get_expr( \
                 index_contract.indpred, index_contract.indrelid \
             ) = '(controller_id IS NOT NULL)' \
             AND pg_catalog.pg_get_indexdef(index_row.oid, 1, TRUE) = 'controller_id' \
             AND pg_catalog.pg_get_indexdef(index_row.oid, 2, TRUE) \
                 = 'controller_lease_expires_at' \
             AND pg_catalog.pg_get_indexdef(index_row.oid, 3, TRUE) \
                 = 'controller_acquired_at' \
             AND pg_catalog.pg_get_indexdef(index_row.oid, 4, TRUE) = 'deployment_id'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(exact);

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL enable_seqscan = off")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let plan = sqlx::query_scalar::<_, Json<Value>>(
        "EXPLAIN (FORMAT JSON, COSTS FALSE) \
         SELECT deployment.* FROM public.runtime_deployments AS deployment \
         WHERE deployment.controller_id = $1 \
             AND deployment.controller_lease_expires_at > $2 \
             AND deployment.phase NOT IN ('live', 'superseded', 'cancelled') \
         ORDER BY deployment.controller_acquired_at, deployment.deployment_id \
         LIMIT 1 FOR UPDATE",
    )
    .bind("runtime-index-plan-probe")
    .bind(database_now(pool).await)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    let plan = serde_json::to_string(&plan.0).unwrap();
    assert!(plan.contains("runtime_deployments_active_controller_index"));
    assert!(plan.contains("Index Cond"));
    assert!(plan.contains("controller_id"));
    assert!(plan.contains("controller_lease_expires_at"));
    transaction.rollback().await.unwrap();

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("DROP INDEX public.runtime_deployments_active_controller_index")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let manifests = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT public.starring_runtime_exact_target_schema_manifest_v1(), \
            public.starring_runtime_serving_schema_manifest_v1(), \
            public.starring_runtime_execution_schema_manifest_v1()",
    )
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(manifests, (true, true, false));
    transaction.rollback().await.unwrap();

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("DROP INDEX public.runtime_deployments_active_controller_index")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "CREATE INDEX runtime_deployments_active_controller_index \
         ON public.runtime_deployments (controller_id) \
         WHERE controller_id IS NOT NULL",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let manifests = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT public.starring_runtime_exact_target_schema_manifest_v1(), \
            public.starring_runtime_serving_schema_manifest_v1(), \
            public.starring_runtime_execution_schema_manifest_v1()",
    )
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(manifests, (false, false, false));
    transaction.rollback().await.unwrap();
}

async fn restricted_readiness_pool(
    database: &mut IsolatedDatabase,
    capability: &str,
    functions: &[&str],
) -> PgPool {
    let suffix = unique_suffix();
    let role = format!("starring_re_{capability}_{suffix}");
    let password = format!("re_{capability}_password_{suffix}");
    assert!(canonical_identifier(&role));
    let password_literal = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
        .bind(&password)
        .fetch_one(&database.owner_pool)
        .await
        .unwrap();
    database
        .administrator
        .execute(
            format!(
                "CREATE ROLE {role} LOGIN PASSWORD {password_literal} NOINHERIT NOSUPERUSER \
                 NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4"
            )
            .as_str(),
        )
        .await
        .unwrap();
    database.additional_roles.push(role.clone());
    for statement in [
        format!("GRANT CONNECT ON DATABASE {} TO {role}", database.name),
        format!("GRANT USAGE ON SCHEMA public TO {role}"),
    ] {
        database
            .owner_pool
            .execute(statement.as_str())
            .await
            .unwrap();
    }
    for function in functions {
        database
            .owner_pool
            .execute(format!("GRANT EXECUTE ON FUNCTION {function} TO {role}").as_str())
            .await
            .unwrap();
    }
    PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            database
                .connect_options
                .clone()
                .username(&role)
                .password(&password),
        )
        .await
        .unwrap()
}

async fn seed_claimable_deployment(pool: &PgPool) {
    let now = database_now(pool).await;
    let expires_at = now + TimeDelta::hours(1);
    let linked_at = now + TimeDelta::seconds(1);
    let request_digest = "e".repeat(64);
    let approval_payload_digest = "f".repeat(64);
    let approval_context_digest = "1".repeat(64);
    let approval_context = json!({
        "promotion_id": PROMOTION,
        "promotion_request_digest": request_digest,
        "approval_payload_digest": approval_payload_digest,
        "approval_context_digest": approval_context_digest,
        "binding": {
            "revision": 1,
            "required_bindings": [],
            "fingerprint": BINDING_FINGERPRINT
        },
        "baseline": { "state": "absent" },
        "policy": {
            "revision": 1,
            "required_approvals": 1,
            "ttl_seconds": 3600,
            "digest": "2".repeat(64)
        }
    });
    let promotion = promotion_record(now, expires_at, &request_digest, &approval_context);
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals (principal_id, discord_user_id) \
         VALUES ($1, $2)",
    )
    .bind(PRINCIPAL)
    .bind("9200201")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_tenants (tenant_id, lifecycle_state, display_name) \
         VALUES ($1, 'active', 'Runtime Execution PostgreSQL Test')",
    )
    .bind(TENANT)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations (installation_id, tenant_id, \
         discord_application_id, discord_guild_id, ruleset_key, lifecycle_state, \
         current_authority_revision) VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(INSTALLATION)
    .bind(TENANT)
    .bind("9200301")
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
         binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
         authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, 1, '{}'::JSONB, $3, 1, 1, 3600, $4, $5, $6)",
    )
    .bind(INSTALLATION)
    .bind(TENANT)
    .bind(BINDING_FINGERPRINT)
    .bind("3".repeat(64))
    .bind(PRINCIPAL)
    .bind("4".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_heads (guild_id, ruleset_key, next_version) \
         VALUES ($1, $2, 2)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions (guild_id, ruleset_key, version, \
         schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 1, 1, \
          pg_catalog.jsonb_build_object('version', 1, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $3, $4)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind("9200201")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) VALUES ($1, $2, 1)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    insert_activation_pending_promotion(&mut transaction, &request_digest, &promotion).await;
    sqlx::query(
        "INSERT INTO public.activation_requests (id, guild_id, ruleset_key, target_version, \
         target_content_hash, requester_id, required_approvals, state, created_at, expires_at, \
         authority_kind, link_state_name, approval_context, link_state, promotion_id, \
         promotion_request_digest, approval_payload_digest, approval_context_digest) \
         VALUES ($1, $2, $3, 1, $4, $5, 1, 'pending', $6, $7, 'product_authoring', \
                 'unlinked', $8, '{\"state\":\"unlinked\"}'::JSONB, $9, $10, $11, $12)",
    )
    .bind(ACTIVATION)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind("9200401")
    .bind(now)
    .bind(expires_at)
    .bind(Json(json!({
        "authority": "product_authoring",
        "context": approval_context
    })))
    .bind(PROMOTION)
    .bind(&request_digest)
    .bind(&approval_payload_digest)
    .bind(&approval_context_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET link_state_name = 'linked', \
         link_state = $2, linked_at = $3 WHERE id = $1",
    )
    .bind(ACTIVATION)
    .bind(Json(json!({ "state": "linked", "linked_at": linked_at })))
    .bind(linked_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET state = 'applied', applied_at = $2, \
         applied_by = $3, completion_kind = 'already_active', \
         activation_notices = '[]'::JSONB WHERE id = $1",
    )
    .bind(ACTIVATION)
    .bind(linked_at)
    .bind("9200501")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let requested_at =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    let snapshot = requested_snapshot(requested_at);
    let desired_target_digest = runtime_desired_target_digest_v1(
        &snapshot.identity,
        &snapshot.target,
        snapshot.runtime_generation.get(),
        1,
        snapshot.previous_runtime.as_ref(),
    );
    let snapshot_json = serde_json::to_value(&snapshot).unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', $1, TRUE)")
        .bind(requested_at.to_rfc3339())
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_deployments (deployment_id, tenant_id, installation_id, \
         promotion_id, activation_request_id, installation_authority_revision, guild_id, \
         ruleset_key, target_version, target_content_hash, binding_revision, \
         binding_fingerprint, desired_target_digest, runtime_generation, requested_at, \
         snapshot_format_version, snapshot, revision, phase, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, 1, $6, $7, 1, $8, 1, $9, $10, 1, $11, \
                 1, $12, 1, 'requested', $11, $11)",
    )
    .bind(DEPLOYMENT)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(PROMOTION)
    .bind(ACTIVATION)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind(BINDING_FINGERPRINT)
    .bind(desired_target_digest.as_str())
    .bind(requested_at)
    .bind(Json(snapshot_json))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', '', TRUE)")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

fn requested_snapshot(requested_at: DateTime<Utc>) -> RuntimeDeploymentSnapshotV1 {
    let identity = serde_json::from_value::<RuntimeDeploymentIdentityV1>(json!({
        "deployment_id": DEPLOYMENT,
        "tenant_id": TENANT,
        "installation_id": INSTALLATION,
        "promotion_id": PROMOTION,
        "activation_request_id": ACTIVATION
    }))
    .unwrap();
    let target = serde_json::from_value::<RuntimeDeploymentTargetV1>(json!({
        "guild_id": GUILD.to_string(),
        "ruleset_key": RULESET,
        "version": 1,
        "content_hash": CONTENT_HASH,
        "binding_revision": 1,
        "binding_fingerprint": BINDING_FINGERPRINT
    }))
    .unwrap();
    RuntimeDeployment::request(
        identity,
        target,
        RuntimeGeneration::FIRST,
        None,
        requested_at,
    )
    .unwrap()
    .snapshot()
}

fn promotion_record(
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    request_digest: &str,
    approval_context: &Value,
) -> Value {
    json!({
        "id": PROMOTION,
        "request_digest": request_digest,
        "revision": 3,
        "intent": {
            "authority": {
                "tenant_id": TENANT,
                "principal_id": PRINCIPAL,
                "installation_id": INSTALLATION,
                "guild_id": GUILD.to_string(),
                "ruleset_key": RULESET,
                "binding_revision": 1
            },
            "evidence": {
                "context_fingerprint": BINDING_FINGERPRINT
            }
        },
        "stage": {
            "state": "activation_pending",
            "publication": {
                "version": 1,
                "schema_version": 1,
                "content_hash": CONTENT_HASH,
                "disposition": "created",
                "registry_created_by": "9200401"
            },
            "activation": {
                "request_id": ACTIVATION,
                "target": {
                    "guild_id": GUILD.to_string(),
                    "ruleset_key": RULESET,
                    "version": 1,
                    "content_hash": CONTENT_HASH
                },
                "requester": "9200401",
                "required_approvals": 1,
                "observed_active": null,
                "created_at": created_at,
                "expires_at": expires_at,
                "disposition": "created",
                "request_state_at_journal": "pending",
                "approval_context": approval_context
            }
        },
        "created_at": created_at,
        "updated_at": created_at
    })
}

async fn insert_activation_pending_promotion(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request_digest: &str,
    record: &Value,
) {
    let mut prepared = record.clone();
    prepared["revision"] = json!(1);
    prepared["stage"] = json!({"state": "prepared"});
    let mut published = record.clone();
    published["revision"] = json!(2);
    published["stage"] = json!({
        "state": "published",
        "publication": record["stage"]["publication"].clone()
    });
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, installation_id, \
          principal_id, record) VALUES ($1, 1, 1, 'prepared', $2, $3, $4, $5, $6)",
    )
    .bind(PROMOTION)
    .bind(request_digest)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(PRINCIPAL)
    .bind(Json(&prepared))
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 2, stage = 'published', record = $2 WHERE id = $1",
    )
    .bind(PROMOTION)
    .bind(Json(&published))
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 3, stage = 'activation_pending', record = $2 WHERE id = $1",
    )
    .bind(PROMOTION)
    .bind(Json(record))
    .execute(&mut **transaction)
    .await
    .unwrap();
}

async fn database_now(pool: &PgPool) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn assert_sqlstate(error: &sqlx::Error, expected: &str) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some(expected)
    );
}

fn canonical_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    value.len() <= 63
        && (first.is_ascii_lowercase() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn canonical_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn unique_suffix() -> u128 {
    let clock = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    (clock << 64) | u128::from(UNIQUE_SUFFIX_SEQUENCE.fetch_add(1, Ordering::Relaxed))
}
