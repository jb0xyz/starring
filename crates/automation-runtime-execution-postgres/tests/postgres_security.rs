use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use automation_runtime_controller::{
    runtime_desired_target_digest_v1, RuntimeClaimNextExecutionV1, RuntimeConvergenceMutationV1,
    RuntimeConvergenceSessionStateV1, RuntimeConvergenceSessionV1, RuntimeExecutionGuardV1,
    RuntimeExecutionReceiptV1, RuntimeMutationReceiptV1,
};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ControllerId, DrainAttestationV1,
    PanelCertificateId, PanelCertificateV1, PanelReportDigestV1, PreflightAttestationV1,
    ProcessInstanceId, RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1,
    RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1,
    RuntimeGeneration, RuntimePendingConditionV1, TransitionOutcomeV1,
};
use automation_runtime_execution_postgres::{
    PostgresRuntimeExecutionV1, RuntimeExecutionDatabaseExpectationV1,
    RuntimeExecutionPersistenceErrorV1, MIGRATOR,
};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions, PgSslMode};
use sqlx::types::Json;
use sqlx::{Connection, Executor, PgPool};

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
const EXECUTOR_FUNCTIONS: [&str; 9] = [
    "public.starring_runtime_execution_database_readiness_v1()",
    "public.starring_runtime_execution_database_identity_v1()",
    "public.starring_runtime_execution_claim_next_v1(text,bigint)",
    "public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)",
    "public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)",
    "public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)",
    "public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)",
    "public.starring_runtime_execution_recover_stale_live_v1()",
    "public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)",
];
const EXPECTED_READINESS_DEFINITION_SHA256_V1: &str =
    "29acadef105024b086dc80c02420e1a60714341d011286d0a09a216136509927";
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

#[tokio::test]
#[ignore = "requires initdb and pg_ctl"]
async fn execution_database_is_function_only_and_least_privilege() {
    let cluster = EphemeralPostgresCluster::start();
    let mut database = isolated_database(cluster.connect_options()).await;
    assert_cross_runtime_readiness(&mut database).await;
    let owner_pool = database.owner_pool.clone();
    let executor_pool = database.executor_pool.clone();
    let foreign_database_options = database.foreign_database_options.clone();
    let database_name = database.name.clone();
    let role = database.role.clone();
    let administrator_role = database.administrator_role.clone();
    let outcome = tokio::spawn(async move {
        execution_security_scenario(
            owner_pool,
            executor_pool,
            foreign_database_options,
            database_name,
            role,
            administrator_role,
        )
        .await;
    })
    .await;
    cleanup(database).await;
    outcome.expect("restricted execution proof must complete");
    drop(cluster);
}

#[tokio::test]
#[ignore = "requires initdb and pg_ctl"]
async fn execution_mutations_are_proven_and_closed() {
    let cluster = EphemeralPostgresCluster::start();

    let canonicality_database = isolated_database(cluster.connect_options()).await;
    mutation_canonicality_and_expiry_scenario(&canonicality_database).await;
    cleanup(canonicality_database).await;

    let future_evidence_database = isolated_database(cluster.connect_options()).await;
    future_activation_failure_scenario(&future_evidence_database).await;
    cleanup(future_evidence_database).await;

    let recovery_database = isolated_database(cluster.connect_options()).await;
    retry_recovery_and_blocked_failure_scenario(&recovery_database).await;
    cleanup(recovery_database).await;

    let authority_database = isolated_database(cluster.connect_options()).await;
    replay_rechecks_current_authority_scenario(&authority_database).await;
    cleanup(authority_database).await;

    drop(cluster);
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

async fn execution_security_scenario(
    owner_pool: PgPool,
    executor_pool: PgPool,
    foreign_database_options: PgConnectOptions,
    database_name: String,
    role: String,
    administrator_role: String,
) {
    assert_exact_executor_capabilities(&owner_pool, &executor_pool, &role).await;
    assert_readiness_identity(&owner_pool, &executor_pool, &database_name, &role).await;
    assert_verified_adapter(&owner_pool, &executor_pool, &database_name, &role).await;
    assert_wrong_role_rejected(&owner_pool).await;
    assert_cross_database_rejected(
        &owner_pool,
        &executor_pool,
        &foreign_database_options,
        &role,
    )
    .await;
    assert_raw_sql_rejected(&executor_pool, &administrator_role).await;
    assert_invalid_operations_are_non_mutating(&owner_pool, &executor_pool).await;
    assert_claim_and_renew_success(&owner_pool, &executor_pool, &database_name, &role).await;
    assert_readiness_definition_sha(&owner_pool).await;
}

async fn assert_verified_adapter(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    database_name: &str,
    role: &str,
) {
    let database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let expectation =
        RuntimeExecutionDatabaseExpectationV1::new(database_identity.clone(), database_name, role)
            .unwrap();
    let adapter = PostgresRuntimeExecutionV1::connect_verified_default(
        executor_pool.clone(),
        expectation.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        adapter.initial_readiness().database_identity,
        database_identity
    );
    assert_eq!(adapter.initial_readiness().database_name, database_name);
    assert_eq!(adapter.initial_readiness().executor_role, role);
    assert!(adapter.verify_database_v1().await.is_ok());
    let wrong_expectation = RuntimeExecutionDatabaseExpectationV1::new(
        expectation.database_identity(),
        "starring_wrong_execution_database",
        expectation.executor_role(),
    )
    .unwrap();
    assert!(matches!(
        PostgresRuntimeExecutionV1::connect_verified_default(
            executor_pool.clone(),
            wrong_expectation
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch)
    ));
}

async fn assert_exact_executor_capabilities(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    role: &str,
) {
    let mut actual = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.format( \
            '%I.%I(%s)', namespace.nspname, function_row.proname, \
            pg_catalog.replace( \
                pg_catalog.oidvectortypes(function_row.proargtypes), ', ', ',' \
            ) \
         )::TEXT \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = function_row.pronamespace \
         WHERE function_row.oid >= 16384 \
            AND namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND pg_catalog.has_function_privilege( \
                pg_catalog.to_regrole(session_user), function_row.oid, 'EXECUTE' \
            ) \
         ORDER BY 1",
    )
    .fetch_all(executor_pool)
    .await
    .unwrap();
    let mut expected = EXECUTOR_FUNCTIONS
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);

    let role_oid = sqlx::query_scalar::<_, i32>(
        "SELECT role.oid::INT4 FROM pg_catalog.pg_roles AS role WHERE role.rolname = $1",
    )
    .bind(role)
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let (direct_execute_count, grantable_execute_count, public_execute_count) =
        sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT \
                pg_catalog.count(*) FILTER ( \
                    WHERE privilege.grantee = $1 \
                        AND privilege.privilege_type = 'EXECUTE' \
                ), \
                pg_catalog.count(*) FILTER ( \
                    WHERE privilege.grantee = $1 AND privilege.is_grantable \
                ), \
                pg_catalog.count(*) FILTER (WHERE privilege.grantee = 0) \
             FROM pg_catalog.pg_proc AS function_row \
             INNER JOIN pg_catalog.pg_namespace AS namespace \
                ON namespace.oid = function_row.pronamespace \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, \
                pg_catalog.acldefault('f', function_row.proowner) \
             )) AS privilege \
             WHERE function_row.oid >= 16384 \
                AND namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
                AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_'",
        )
        .bind(role_oid)
        .fetch_one(owner_pool)
        .await
        .unwrap();
    assert_eq!(direct_execute_count, 9);
    assert_eq!(grantable_execute_count, 0);
    assert_eq!(public_execute_count, 0);

    let raw_relation_capabilities = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND relation.relkind IN ('r', 'p', 'v', 'm', 'f') \
            AND ( \
                pg_catalog.has_table_privilege(session_user, relation.oid, 'SELECT') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'INSERT') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'UPDATE') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'DELETE') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'TRUNCATE') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'REFERENCES') \
                OR pg_catalog.has_table_privilege(session_user, relation.oid, 'TRIGGER') \
            )",
    )
    .fetch_one(executor_pool)
    .await
    .unwrap();
    assert_eq!(raw_relation_capabilities, 0);
    let raw_column_capabilities = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_attribute AS attribute \
         INNER JOIN pg_catalog.pg_class AS relation \
            ON relation.oid = attribute.attrelid \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND attribute.attnum > 0 \
            AND NOT attribute.attisdropped \
            AND ( \
                pg_catalog.has_column_privilege( \
                    session_user, relation.oid, attribute.attname, 'SELECT' \
                ) \
                OR pg_catalog.has_column_privilege( \
                    session_user, relation.oid, attribute.attname, 'INSERT' \
                ) \
                OR pg_catalog.has_column_privilege( \
                    session_user, relation.oid, attribute.attname, 'UPDATE' \
                ) \
                OR pg_catalog.has_column_privilege( \
                    session_user, relation.oid, attribute.attname, 'REFERENCES' \
                ) \
            )",
    )
    .fetch_one(executor_pool)
    .await
    .unwrap();
    assert_eq!(raw_column_capabilities, 0);
    let raw_sequence_capabilities = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_class AS sequence \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = sequence.relnamespace \
         WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND sequence.relkind = 'S' \
            AND ( \
                pg_catalog.has_sequence_privilege(session_user, sequence.oid, 'USAGE') \
                OR pg_catalog.has_sequence_privilege(session_user, sequence.oid, 'SELECT') \
                OR pg_catalog.has_sequence_privilege(session_user, sequence.oid, 'UPDATE') \
            )",
    )
    .fetch_one(executor_pool)
    .await
    .unwrap();
    assert_eq!(raw_sequence_capabilities, 0);

    let (connect, create, temporary, schema_usage, schema_create) =
        sqlx::query_as::<_, (bool, bool, bool, bool, bool)>(
            "SELECT \
                pg_catalog.has_database_privilege(session_user, current_database(), 'CONNECT'), \
                pg_catalog.has_database_privilege(session_user, current_database(), 'CREATE'), \
                pg_catalog.has_database_privilege(session_user, current_database(), 'TEMPORARY'), \
                pg_catalog.has_schema_privilege(session_user, 'public', 'USAGE'), \
                pg_catalog.has_schema_privilege(session_user, 'public', 'CREATE')",
        )
        .fetch_one(executor_pool)
        .await
        .unwrap();
    assert!(connect);
    assert!(!create);
    assert!(!temporary);
    assert!(schema_usage);
    assert!(!schema_create);

    let (public_database_acl, public_schema_acl, membership_count, owned_object_count) =
        sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT \
                (SELECT pg_catalog.count(*) \
                 FROM pg_catalog.pg_database AS database_row \
                 CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
                    database_row.datacl, \
                    pg_catalog.acldefault('d', database_row.datdba) \
                 )) AS privilege \
                 WHERE database_row.datname = current_database() \
                    AND privilege.grantee = 0), \
                (SELECT pg_catalog.count(*) \
                 FROM pg_catalog.pg_namespace AS namespace \
                 CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
                    namespace.nspacl, \
                    pg_catalog.acldefault('n', namespace.nspowner) \
                 )) AS privilege \
                 WHERE namespace.nspname = 'public' \
                    AND privilege.grantee = 0), \
                (SELECT pg_catalog.count(*) \
                 FROM pg_catalog.pg_auth_members AS membership \
                 WHERE membership.member = pg_catalog.to_regrole(session_user) \
                    OR membership.roleid = pg_catalog.to_regrole(session_user)), \
                (SELECT pg_catalog.count(*) FROM ( \
                    SELECT relation.oid FROM pg_catalog.pg_class AS relation \
                        WHERE relation.relowner = pg_catalog.to_regrole(session_user) \
                    UNION ALL \
                    SELECT namespace.oid FROM pg_catalog.pg_namespace AS namespace \
                        WHERE namespace.nspowner = pg_catalog.to_regrole(session_user) \
                    UNION ALL \
                    SELECT function_row.oid FROM pg_catalog.pg_proc AS function_row \
                        WHERE function_row.proowner = pg_catalog.to_regrole(session_user) \
                    UNION ALL \
                    SELECT database_row.oid FROM pg_catalog.pg_database AS database_row \
                        WHERE database_row.datdba = pg_catalog.to_regrole(session_user) \
                 ) AS owned)",
        )
        .fetch_one(executor_pool)
        .await
        .unwrap();
    assert_eq!(public_database_acl, 0);
    assert_eq!(public_schema_acl, 0);
    assert_eq!(membership_count, 0);
    assert_eq!(owned_object_count, 0);

    let (superuser, inherit, create_role, create_db, can_login, replication, bypass_rls, config) =
        sqlx::query_as::<
            _,
            (
                bool,
                bool,
                bool,
                bool,
                bool,
                bool,
                bool,
                Option<Vec<String>>,
            ),
        >(
            "SELECT role.rolsuper, role.rolinherit, role.rolcreaterole, role.rolcreatedb, \
                role.rolcanlogin, role.rolreplication, role.rolbypassrls, role.rolconfig \
             FROM pg_catalog.pg_roles AS role WHERE role.rolname = session_user",
        )
        .fetch_one(executor_pool)
        .await
        .unwrap();
    assert!(!superuser);
    assert!(!inherit);
    assert!(!create_role);
    assert!(!create_db);
    assert!(can_login);
    assert!(!replication);
    assert!(!bypass_rls);
    assert!(config.is_none_or(|entries| entries.is_empty()));
}

async fn assert_readiness_identity(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    database_name: &str,
    role: &str,
) {
    let database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let rows = sqlx::query_as::<_, (String, String, String, DateTime<Utc>)>(
        "SELECT * FROM public.starring_runtime_execution_database_readiness_v1()",
    )
    .fetch_all(executor_pool)
    .await
    .unwrap();
    let [(observed_identity, observed_database, observed_role, checked_at)] = rows.as_slice()
    else {
        panic!("readiness must return exactly one row")
    };
    assert_eq!(observed_identity, &database_identity);
    assert_eq!(observed_database, database_name);
    assert_eq!(observed_role, role);
    assert!(*checked_at <= Utc::now());
    let identity_rows = sqlx::query_scalar::<_, String>(
        "SELECT * FROM public.starring_runtime_execution_database_identity_v1()",
    )
    .fetch_all(executor_pool)
    .await
    .unwrap();
    assert_eq!(identity_rows, [database_identity]);
}

async fn assert_wrong_role_rejected(owner_pool: &PgPool) {
    let error = sqlx::query(format!("SELECT * FROM {READINESS_FUNCTION}").as_str())
        .fetch_all(owner_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RE001");
}

async fn assert_cross_database_rejected(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    foreign_database_options: &PgConnectOptions,
    role: &str,
) {
    let foreign_database = foreign_database_options.get_database().unwrap();
    let foreign_capabilities = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_database AS database_row \
         WHERE database_row.datallowconn \
            AND database_row.datname <> current_database() \
            AND ( \
                pg_catalog.has_database_privilege($1, database_row.oid, 'CONNECT') \
                OR pg_catalog.has_database_privilege($1, database_row.oid, 'CREATE') \
                OR pg_catalog.has_database_privilege($1, database_row.oid, 'TEMPORARY') \
            )",
    )
    .bind(role)
    .fetch_one(owner_pool)
    .await
    .unwrap();
    assert_eq!(foreign_capabilities, 0);
    let error = match PgConnection::connect_with(foreign_database_options).await {
        Ok(connection) => {
            connection.close().await.unwrap();
            panic!("execution role connected to foreign database {foreign_database}")
        }
        Err(error) => error,
    };
    assert_sqlstate(&error, "42501");
    let (quoted_foreign_database, quoted_role) = sqlx::query_as::<_, (String, String)>(
        "SELECT pg_catalog.quote_ident($1), pg_catalog.quote_ident($2)",
    )
    .bind(foreign_database)
    .bind(role)
    .fetch_one(owner_pool)
    .await
    .unwrap();
    owner_pool
        .execute(
            format!("GRANT CONNECT ON DATABASE {quoted_foreign_database} TO {quoted_role}")
                .as_str(),
        )
        .await
        .unwrap();
    let readiness_error = sqlx::query(format!("SELECT * FROM {READINESS_FUNCTION}").as_str())
        .fetch_all(executor_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&readiness_error, "RE001");
    PgConnection::connect_with(foreign_database_options)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    owner_pool
        .execute(
            format!("REVOKE CONNECT ON DATABASE {quoted_foreign_database} FROM {quoted_role}")
                .as_str(),
        )
        .await
        .unwrap();
    let readiness_rows = sqlx::query(format!("SELECT * FROM {READINESS_FUNCTION}").as_str())
        .fetch_all(executor_pool)
        .await
        .unwrap();
    assert_eq!(readiness_rows.len(), 1);
}

async fn assert_raw_sql_rejected(executor_pool: &PgPool, administrator_role: &str) {
    let statements = [
        "SELECT deployment_id FROM public.runtime_deployments LIMIT 1".to_string(),
        "INSERT INTO public.runtime_deployments DEFAULT VALUES".to_string(),
        "UPDATE public.runtime_deployments SET revision = revision".to_string(),
        "DELETE FROM public.runtime_deployments".to_string(),
        "SELECT deployment_id FROM public.runtime_execution_mutation_markers LIMIT 1".to_string(),
        "UPDATE public.runtime_execution_mutation_markers \
         SET mutation_revision = mutation_revision"
            .to_string(),
        "CREATE TABLE public.runtime_execution_escape(value BIGINT)".to_string(),
        "CREATE TEMP TABLE runtime_execution_escape(value BIGINT)".to_string(),
        "CREATE ROLE runtime_execution_escape".to_string(),
        format!("SET ROLE {administrator_role}"),
    ];
    for statement in statements {
        let error = sqlx::query(&statement)
            .execute(executor_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "42501");
    }
}

async fn assert_invalid_operations_are_non_mutating(owner_pool: &PgPool, executor_pool: &PgPool) {
    let before = protected_counts(owner_pool).await;
    for statement in [
        "SELECT * FROM public.starring_runtime_execution_claim_next_v1('', 1000)",
        "SELECT * FROM public.starring_runtime_execution_mutate_v1( \
            '', '', '', 1, '', 1, 1, 1, 'preflight', '{}'::JSONB \
         )",
    ] {
        let error = sqlx::query(statement)
            .fetch_all(executor_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "RX002");
    }
    assert_eq!(protected_counts(owner_pool).await, before);
}

async fn protected_counts(pool: &PgPool) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT \
            (SELECT pg_catalog.count(*) FROM public.runtime_deployments), \
            (SELECT pg_catalog.count(*) FROM public.runtime_attestations), \
            (SELECT pg_catalog.count(*) FROM public.runtime_serving_leases), \
            (SELECT pg_catalog.count(*) \
             FROM public.runtime_execution_mutation_markers)",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_claim_and_renew_success(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    database_name: &str,
    role: &str,
) {
    seed_claimable_deployment(owner_pool).await;
    let database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    let expectation =
        RuntimeExecutionDatabaseExpectationV1::new(database_identity, database_name, role).unwrap();
    let adapter =
        PostgresRuntimeExecutionV1::connect_verified_default(executor_pool.clone(), expectation)
            .await
            .unwrap();
    let controller_id = ControllerId::parse("runtime-execution-controller").unwrap();
    let claim_lease = Duration::from_secs(90);
    let claim_request = RuntimeClaimNextExecutionV1 {
        controller_id: controller_id.clone(),
        lease_for: claim_lease,
    };
    let claimed = adapter
        .claim_next_execution(claim_request.clone())
        .await
        .unwrap()
        .expect("seeded deployment must be claimable");
    assert_execution_receipt(&claimed, &controller_id, 2, 1, 1, claim_lease);
    let replayed_claim = adapter
        .claim_next_execution(claim_request)
        .await
        .unwrap()
        .expect("owned deployment must replay its claim");
    assert_eq!(replayed_claim, claimed);
    assert_persisted_execution(
        owner_pool,
        2,
        "requested",
        &controller_id,
        1,
        1,
        claim_lease,
    )
    .await;

    let mut session = RuntimeConvergenceSessionV1::from_claim(claimed).unwrap();
    let renewal_lease = Duration::from_secs(120);
    let renewal_request = session.begin_renewal(renewal_lease).unwrap();
    let renewed = adapter
        .renew_execution(renewal_request.clone())
        .await
        .unwrap();
    assert_eq!(renewed.action_id, renewal_request.action_id);
    assert_execution_receipt(&renewed.execution, &controller_id, 3, 2, 1, renewal_lease);
    let replayed_renewal = adapter.renew_execution(renewal_request).await.unwrap();
    assert_eq!(replayed_renewal, renewed);
    session.apply_renewal(renewed.clone()).unwrap();
    assert_eq!(session.snapshot(), &renewed.execution.snapshot);
    assert_eq!(session.fencing_token(), renewed.execution.fencing_token);
    assert_eq!(
        session.convergence_attempt(),
        renewed.execution.convergence_attempt
    );
    assert_eq!(session.acquired_at(), renewed.execution.acquired_at);
    assert_eq!(session.expires_at(), renewed.execution.expires_at);
    assert_persisted_execution(
        owner_pool,
        3,
        "requested",
        &controller_id,
        2,
        1,
        renewal_lease,
    )
    .await;
    let competing_renewal = session.begin_renewal(Duration::from_secs(150)).unwrap();
    apply_cancel_successor(executor_pool, &competing_renewal.guard).await;
    let cancelled = persisted_cancelled_execution(owner_pool).await;
    assert_cancelled_execution(&cancelled, &controller_id);
    let error = adapter
        .renew_execution(competing_renewal)
        .await
        .unwrap_err();
    assert_eq!(error, RuntimeExecutionPersistenceErrorV1::OwnershipLost);
    assert_eq!(persisted_cancelled_execution(owner_pool).await, cancelled);
    assert_eq!(protected_counts(owner_pool).await, (1, 0, 0, 1));
}

async fn mutation_canonicality_and_expiry_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-execution-canonicality-controller",
        Duration::from_secs(3),
    )
    .await;
    let checked_at = database_now(&database.owner_pool).await;
    let attestation = PreflightAttestationV1 {
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        observed_runtime: session.snapshot().previous_runtime.clone(),
        checked_at,
    };
    let guard = session.execution_guard().unwrap();
    let unchanged = persisted_deployment_image(&database.owner_pool).await;

    let mut noncanonical_version = serde_json::to_value(&attestation).unwrap();
    noncanonical_version["target"]["version"] = json!(1.0);
    let error = raw_mutate(
        &database.executor_pool,
        &guard,
        "accept_preflight",
        noncanonical_version,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );

    let mut noncanonical_time = serde_json::to_value(&attestation).unwrap();
    noncanonical_time["checked_at"] = json!("2026-07-22T24:00:00Z");
    let error = raw_mutate(
        &database.executor_pool,
        &guard,
        "accept_preflight",
        noncanonical_time,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );

    let mut noncanonical_fraction = serde_json::to_value(&attestation).unwrap();
    noncanonical_fraction["checked_at"] = json!((checked_at + TimeDelta::seconds(1))
        .format("%Y-%m-%dT%H:%M:%S.000Z")
        .to_string());
    let error = raw_mutate(
        &database.executor_pool,
        &guard,
        "accept_preflight",
        noncanonical_fraction,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );

    let request = session
        .begin_mutation(RuntimeConvergenceMutationV1::AcceptPreflight(
            attestation.clone(),
        ))
        .unwrap();
    let applied = adapter.mutate(request.clone()).await.unwrap();
    assert!(matches!(
        applied.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    let replayed = adapter.mutate(request.clone()).await.unwrap();
    assert!(matches!(
        replayed.outcome,
        TransitionOutcomeV1::Replayed { .. }
    ));
    assert_eq!(replayed.action_id, applied.action_id);
    assert_eq!(replayed.snapshot, applied.snapshot);
    assert_eq!(replayed.convergence_attempt, applied.convergence_attempt);
    session.apply_mutation(applied).unwrap();

    let database_time = database_now(&database.owner_pool).await;
    let remaining = (session.expires_at() - database_time)
        .to_std()
        .unwrap_or_default();
    tokio::time::sleep(remaining + Duration::from_millis(50)).await;
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    let error = adapter.mutate(request.clone()).await.unwrap_err();
    assert_eq!(error, RuntimeExecutionPersistenceErrorV1::OwnershipLost);
    let error = raw_mutate(
        &database.executor_pool,
        &request.guard,
        "accept_preflight",
        serde_json::to_value(attestation).unwrap(),
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX001");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
}

async fn future_activation_failure_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-execution-future-evidence-controller",
        Duration::from_secs(60),
    )
    .await;
    advance_to_activation_applying(&database.owner_pool, &adapter, &mut session).await;
    let guard = session.execution_guard().unwrap();
    let activated_at = database_now(&database.owner_pool).await + TimeDelta::seconds(20);
    let activation = ActivationAttestationV1 {
        activation_request_id: session.snapshot().identity.activation_request_id.clone(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        kind: ActivationOutcomeKindV1::Activated,
        activated_at,
    };
    let outcome = raw_mutate(
        &database.executor_pool,
        &guard,
        "accept_activation",
        serde_json::to_value(activation).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(outcome, ["applied"]);
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    assert_eq!(
        unchanged.0["snapshot"]["phase"]["condition"]["condition"],
        "ready"
    );
    let mut failure_guard = guard;
    failure_guard.expected_revision = failure_guard.expected_revision.next().unwrap();
    let failure = json!({
        "failure_id": "future-activation-failure",
        "kind": "gateway_start",
        "code": "gateway_start_failed",
        "attempt": failure_guard.convergence_attempt.get(),
        "retry_after_milliseconds": 1000
    });
    let error = raw_mutate(
        &database.executor_pool,
        &failure_guard,
        "record_retryable_failure",
        failure,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX005");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
}

async fn retry_recovery_and_blocked_failure_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-execution-retry-controller",
        Duration::from_secs(60),
    )
    .await;
    advance_to_activation_applying(&database.owner_pool, &adapter, &mut session).await;
    let activation = ActivationAttestationV1 {
        activation_request_id: session.snapshot().identity.activation_request_id.clone(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        kind: ActivationOutcomeKindV1::Activated,
        activated_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptActivation(activation),
    )
    .await;
    let retry_attempt = session.convergence_attempt();
    let retry_request = session
        .begin_mutation(RuntimeConvergenceMutationV1::RecordRetryableFailure {
            failure_id: RuntimeFailureId::parse("runtime-retryable-failure").unwrap(),
            kind: RuntimeFailureKindV1::GatewayStart,
            code: "gateway_start_failed".to_string(),
            attempt: retry_attempt,
            retry_after: Duration::from_millis(1),
        })
        .unwrap();
    let retry_receipt = adapter.mutate(retry_request.clone()).await.unwrap();
    assert!(matches!(
        retry_receipt.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    session.apply_mutation(retry_receipt).unwrap();
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    let error = raw_mutate(
        &database.executor_pool,
        &retry_request.guard,
        "record_retryable_failure",
        json!({
            "failure_id": "runtime-retryable-failure",
            "kind": "gateway_start",
            "code": "gateway_start_failed",
            "attempt": retry_attempt.get().to_string(),
            "retry_after_milliseconds": "1"
        }),
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
    assert_eq!(session.state(), RuntimeConvergenceSessionStateV1::Released);
    let retry_not_before = match &session.snapshot().phase {
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition:
                RuntimePendingConditionV1::Retryable {
                    retry_not_before, ..
                },
        } => *retry_not_before,
        _ => panic!("retryable failure must persist a retry boundary"),
    };
    wait_for_database_time(&database.owner_pool, retry_not_before).await;

    let mut resumed = claimed_session(
        &adapter,
        "runtime-execution-resume-controller",
        Duration::from_secs(60),
    )
    .await;
    assert_eq!(resumed.convergence_attempt().get(), retry_attempt.get() + 1);
    let activation_payload = serde_json::to_value(
        resumed
            .snapshot()
            .activation
            .as_ref()
            .expect("retry recovery must retain activation evidence"),
    )
    .unwrap();
    let resume_request = resumed
        .begin_mutation(RuntimeConvergenceMutationV1::ResumeRuntimePending)
        .unwrap();
    let resume_receipt = adapter.mutate(resume_request.clone()).await.unwrap();
    assert!(matches!(
        resume_receipt.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    resumed.apply_mutation(resume_receipt).unwrap();
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    let error = raw_mutate(
        &database.executor_pool,
        &resume_request.guard,
        "accept_activation",
        activation_payload,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
    mutate_applied(
        &adapter,
        &mut resumed,
        RuntimeConvergenceMutationV1::BeginPanelReconciliation,
    )
    .await;
    let certificate = PanelCertificateV1 {
        certificate_id: PanelCertificateId::parse("runtime-panel-certificate").unwrap(),
        report_digest: PanelReportDigestV1::parse("5".repeat(64)).unwrap(),
        target: resumed.snapshot().target.clone(),
        runtime_generation: resumed.snapshot().runtime_generation,
        process_instance_id: ProcessInstanceId::parse("runtime-process-instance").unwrap(),
        declared_count: 1,
        installed_count: 1,
        unchanged_count: 0,
        skipped_transient_count: 0,
        skipped_unresolved_channel_count: 0,
        failed_count: 0,
        ambiguous_outcome_count: 0,
        stale_message_cleanup_pending_count: 0,
        orphan_message_cleanup_pending_count: 0,
        reposted_old_message_cleanup_pending_count: 0,
        reconciled_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        &adapter,
        &mut resumed,
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate),
    )
    .await;
    mutate_applied(
        &adapter,
        &mut resumed,
        RuntimeConvergenceMutationV1::RecordBlockedFailure {
            failure_id: RuntimeFailureId::parse("runtime-blocked-failure").unwrap(),
            kind: RuntimeFailureKindV1::InvariantViolation,
            code: "invalid_runtime_state".to_string(),
        },
    )
    .await;
    assert_eq!(resumed.state(), RuntimeConvergenceSessionStateV1::Released);
    assert!(resumed.snapshot().controller_lease.is_none());
    assert!(matches!(
        resumed.snapshot().phase,
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { .. }
        }
    ));
    let persisted = persisted_deployment_image(&database.owner_pool).await;
    assert_eq!(
        persisted.0["snapshot"]["phase"]["condition"]["condition"],
        "blocked"
    );
    assert!(persisted.0["controller_id"].is_null());
}

async fn replay_rechecks_current_authority_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let controller_id = ControllerId::parse("runtime-execution-authority-controller").unwrap();
    let claim_request = RuntimeClaimNextExecutionV1 {
        controller_id: controller_id.clone(),
        lease_for: Duration::from_secs(60),
    };
    let claimed = adapter
        .claim_next_execution(claim_request)
        .await
        .unwrap()
        .expect("seeded execution must be claimable");
    let mut session = RuntimeConvergenceSessionV1::from_claim(claimed).unwrap();
    let renewal_request = session.begin_renewal(Duration::from_secs(90)).unwrap();
    let renewed = adapter
        .renew_execution(renewal_request.clone())
        .await
        .unwrap();
    session.apply_renewal(renewed.clone()).unwrap();
    let replayed_claim = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id,
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap()
        .expect("active execution must replay before authority drift");
    assert_eq!(replayed_claim, renewed.execution);
    assert_eq!(
        adapter
            .renew_execution(renewal_request.clone())
            .await
            .unwrap(),
        renewed
    );
    rotate_current_authority(&database.owner_pool).await;
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    let claim_error = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id: session.controller_id().clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap_err();
    assert_eq!(
        claim_error,
        RuntimeExecutionPersistenceErrorV1::AuthorityChanged
    );
    let renew_error = adapter.renew_execution(renewal_request).await.unwrap_err();
    assert_eq!(
        renew_error,
        RuntimeExecutionPersistenceErrorV1::AuthorityChanged
    );
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
}

async fn rotate_current_authority(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions (installation_id, \
         revision, tenant_id, binding_revision, resource_bindings, binding_fingerprint, \
         policy_revision, required_approvals, activation_ttl_seconds, \
         authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         SELECT installation_id, 2, tenant_id, 2, resource_bindings, \
         $2, policy_revision, required_approvals, activation_ttl_seconds, \
         $3, created_by_principal_id, $4 \
         FROM public.automation_installation_authority_versions \
         WHERE installation_id = $1 AND revision = 1",
    )
    .bind(INSTALLATION)
    .bind("b".repeat(64))
    .bind("6".repeat(64))
    .bind("7".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_installations \
         SET current_authority_revision = 2, \
             updated_at = GREATEST(pg_catalog.clock_timestamp(), \
                 updated_at + INTERVAL '1 microsecond') \
         WHERE tenant_id = $1 AND installation_id = $2 \
             AND current_authority_revision = 1",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    transaction.commit().await.unwrap();
}

async fn verified_execution_adapter(database: &IsolatedDatabase) -> PostgresRuntimeExecutionV1 {
    let database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let expectation = RuntimeExecutionDatabaseExpectationV1::new(
        database_identity,
        &database.name,
        &database.role,
    )
    .unwrap();
    PostgresRuntimeExecutionV1::connect_verified_default(
        database.executor_pool.clone(),
        expectation,
    )
    .await
    .unwrap()
}

async fn claimed_session(
    adapter: &PostgresRuntimeExecutionV1,
    controller_id: &str,
    lease_for: Duration,
) -> RuntimeConvergenceSessionV1 {
    let receipt = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse(controller_id).unwrap(),
            lease_for,
        })
        .await
        .unwrap()
        .expect("seeded execution must be claimable");
    RuntimeConvergenceSessionV1::from_claim(receipt).unwrap()
}

async fn advance_to_activation_applying(
    owner_pool: &PgPool,
    adapter: &PostgresRuntimeExecutionV1,
    session: &mut RuntimeConvergenceSessionV1,
) {
    let preflight = PreflightAttestationV1 {
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        observed_runtime: session.snapshot().previous_runtime.clone(),
        checked_at: database_now(owner_pool).await,
    };
    mutate_applied(
        adapter,
        session,
        RuntimeConvergenceMutationV1::AcceptPreflight(preflight),
    )
    .await;
    mutate_applied(adapter, session, RuntimeConvergenceMutationV1::RequestDrain).await;
    let drain = DrainAttestationV1 {
        previous_runtime: session.snapshot().previous_runtime.clone(),
        target_runtime_generation: session.snapshot().runtime_generation,
        drained_at: database_now(owner_pool).await,
    };
    mutate_applied(
        adapter,
        session,
        RuntimeConvergenceMutationV1::AcceptDrain(drain),
    )
    .await;
    mutate_applied(
        adapter,
        session,
        RuntimeConvergenceMutationV1::BeginActivation,
    )
    .await;
}

async fn mutate_applied(
    adapter: &PostgresRuntimeExecutionV1,
    session: &mut RuntimeConvergenceSessionV1,
    mutation: RuntimeConvergenceMutationV1,
) -> RuntimeMutationReceiptV1 {
    let request = session.begin_mutation(mutation).unwrap();
    let receipt = adapter.mutate(request).await.unwrap();
    assert!(matches!(
        receipt.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    session.apply_mutation(receipt.clone()).unwrap();
    receipt
}

async fn raw_mutate(
    executor_pool: &PgPool,
    guard: &RuntimeExecutionGuardV1,
    kind: &str,
    payload: Value,
) -> Result<Vec<String>, sqlx::Error> {
    let mut transaction = executor_pool.begin().await?;
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await?;
    let result = sqlx::query_scalar(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_mutate_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10 \
         )",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(i64::try_from(guard.expected_revision.get()).unwrap())
    .bind(guard.controller_id.as_str())
    .bind(i64::try_from(guard.fencing_token.get()).unwrap())
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(i64::try_from(guard.runtime_generation.get()).unwrap())
    .bind(kind)
    .bind(Json(payload))
    .fetch_all(&mut *transaction)
    .await;
    match result {
        Ok(outcomes) => {
            transaction.commit().await?;
            Ok(outcomes)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn persisted_deployment_image(pool: &PgPool) -> Json<Value> {
    sqlx::query_scalar(
        "SELECT pg_catalog.to_jsonb(deployment) \
         FROM public.runtime_deployments AS deployment \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn wait_for_database_time(pool: &PgPool, boundary: DateTime<Utc>) {
    let now = database_now(pool).await;
    let remaining = (boundary - now).to_std().unwrap_or_default();
    tokio::time::sleep(remaining + Duration::from_millis(10)).await;
}

fn assert_execution_receipt(
    receipt: &RuntimeExecutionReceiptV1,
    controller_id: &ControllerId,
    expected_revision: u64,
    expected_fencing_token: u64,
    expected_attempt: u32,
    expected_duration: Duration,
) {
    assert_eq!(receipt.snapshot.identity.deployment_id.as_str(), DEPLOYMENT);
    assert_eq!(receipt.snapshot.identity.tenant_id.as_str(), TENANT);
    assert_eq!(
        receipt.snapshot.identity.installation_id.as_str(),
        INSTALLATION
    );
    assert_eq!(receipt.snapshot.revision.get(), expected_revision);
    assert_eq!(&receipt.controller_id, controller_id);
    assert_eq!(receipt.fencing_token.get(), expected_fencing_token);
    assert_eq!(receipt.convergence_attempt.get(), expected_attempt);
    assert_eq!(
        receipt.expires_at - receipt.acquired_at,
        TimeDelta::from_std(expected_duration).unwrap()
    );
    let lease = receipt
        .snapshot
        .controller_lease
        .as_ref()
        .expect("claimed snapshot must embed its controller lease");
    assert_eq!(&lease.controller_id, controller_id);
    assert_eq!(lease.fencing_token, receipt.fencing_token);
    assert_eq!(lease.acquired_at, receipt.acquired_at);
    assert_eq!(lease.expires_at, receipt.expires_at);
    assert_eq!(
        receipt.snapshot.last_fencing_token,
        Some(receipt.fencing_token)
    );
}

async fn assert_persisted_execution(
    owner_pool: &PgPool,
    expected_revision: i64,
    expected_phase: &str,
    expected_controller: &ControllerId,
    expected_fencing_token: i64,
    expected_attempt: i64,
    expected_duration: Duration,
) {
    let count =
        sqlx::query_scalar::<_, i64>("SELECT pg_catalog.count(*) FROM public.runtime_deployments")
            .fetch_one(owner_pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let state = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            i64,
            i64,
            String,
            i64,
            i64,
            i64,
            i64,
            String,
        ),
    >(
        "SELECT revision, phase, controller_id, controller_fencing_token, \
            last_fencing_token, last_controller_id, convergence_attempt_no, \
            (EXTRACT(EPOCH FROM (controller_lease_expires_at \
                - controller_acquired_at)) * 1000)::BIGINT, \
            (snapshot ->> 'revision')::BIGINT, \
            (snapshot #>> '{controller_lease,fencing_token}')::BIGINT, \
            snapshot #>> '{controller_lease,controller_id}' \
         FROM public.runtime_deployments WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(owner_pool)
    .await
    .unwrap();
    assert_eq!(state.0, expected_revision);
    assert_eq!(state.1, expected_phase);
    assert_eq!(state.2, expected_controller.as_str());
    assert_eq!(state.3, expected_fencing_token);
    assert_eq!(state.4, expected_fencing_token);
    assert_eq!(state.5, expected_controller.as_str());
    assert_eq!(state.6, expected_attempt);
    assert_eq!(
        state.7,
        i64::try_from(expected_duration.as_millis()).unwrap()
    );
    assert_eq!(state.8, expected_revision);
    assert_eq!(state.9, expected_fencing_token);
    assert_eq!(state.10, expected_controller.as_str());
}

async fn apply_cancel_successor(
    executor_pool: &PgPool,
    guard: &automation_runtime_controller::RuntimeExecutionGuardV1,
) {
    let mut transaction = executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let rows = sqlx::query(
        "SELECT * FROM public.starring_runtime_execution_mutate_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, 'cancel', $9 \
         )",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(i64::try_from(guard.expected_revision.get()).unwrap())
    .bind(guard.controller_id.as_str())
    .bind(i64::try_from(guard.fencing_token.get()).unwrap())
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(i64::try_from(guard.runtime_generation.get()).unwrap())
    .bind(Json(json!({"reason": "renewal-race-successor"})))
    .fetch_all(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    transaction.commit().await.unwrap();
}

type CancelledExecutionState = (
    i64,
    String,
    Option<String>,
    Option<i64>,
    i64,
    String,
    i64,
    Json<Value>,
);

async fn persisted_cancelled_execution(owner_pool: &PgPool) -> CancelledExecutionState {
    sqlx::query_as(
        "SELECT revision, phase, controller_id, controller_fencing_token, \
            last_fencing_token, last_controller_id, convergence_attempt_no, snapshot \
         FROM public.runtime_deployments WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(owner_pool)
    .await
    .unwrap()
}

fn assert_cancelled_execution(state: &CancelledExecutionState, controller_id: &ControllerId) {
    assert_eq!(state.0, 4);
    assert_eq!(state.1, "cancelled");
    assert_eq!(state.2, None);
    assert_eq!(state.3, None);
    assert_eq!(state.4, 2);
    assert_eq!(state.5, controller_id.as_str());
    assert_eq!(state.6, 1);
    assert_eq!(state.7["revision"], json!(4));
    assert_eq!(state.7["phase"]["phase"], "cancelled");
    assert_eq!(state.7["phase"]["reason"], "renewal-race-successor");
    assert_eq!(state.7["controller_lease"], Value::Null);
    assert_eq!(state.7["last_fencing_token"], json!(2));
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

async fn assert_readiness_definition_sha(owner_pool: &PgPool) {
    let digest = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to( \
            pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure( \
                'public.starring_runtime_execution_database_readiness_v1()' \
            )), 'UTF8' \
         )), 'hex')",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    assert!(canonical_sha256(&digest));
    eprintln!("runtime execution readiness definition sha256: {digest}");
    assert_ne!(EXPECTED_READINESS_DEFINITION_SHA256_V1, "PENDING");
    assert_eq!(digest, EXPECTED_READINESS_DEFINITION_SHA256_V1);
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
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
