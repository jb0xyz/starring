use std::fs;
use std::num::{NonZeroU32, NonZeroU64};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_controller::{
    RuntimeAttestationIdV1, RuntimeCertificationOperationIdV2, RuntimeConvergenceErrorClassV1,
    RuntimeDeploymentScopeV1, RuntimeDrainIntentIdV2, RuntimeHeartbeatServingV1,
    RuntimeLiveAttestationDigestV2, RuntimeServingIdentityV1, RuntimeServingIdentityV2,
    RuntimeServingLeasePort, RuntimeServingReceiptV1, RuntimeServingSessionV1,
};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    CommandGuardV1, ControllerId, DeploymentId, DrainAttestationV1, FencingToken,
    GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1,
    PanelCertificateId, PanelCertificateV1, PanelReportDigestV1, PreflightAttestationV1,
    ProcessInstanceId, PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_serving_postgres::{
    PostgresRuntimeServingLeaseV1, RuntimePendingDrainServingLookupV1,
    RuntimePendingDrainServingObservationV1, RuntimeServingDatabaseExpectationV1,
    RuntimeServingDatabaseTimeoutsV1, RuntimeServingPersistenceErrorV1, MIGRATOR,
};
use chrono::{DateTime, TimeDelta, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions, PgSslMode};
use sqlx::types::Json;
use sqlx::{Connection, Executor, PgPool};

const TENANT: &str = "serving-test-tenant";
const INSTALLATION: &str = "serving-test-installation";
const PRINCIPAL: &str = "serving-test-principal";
const DEPLOYMENT: &str = "serving-test-deployment";
const PROCESS: &str = "serving-test-process";
const RULESET: &str = "serving_test_ruleset";
const GUILD: GuildId = GuildId(9300101);
const PROMOTION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACTIVATION: &str = "serving_test_activation";
const ATTESTATION: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CONTENT_HASH: &str = "9f2bbed3d90d3439ebe5bb07a69f8ff179c29e8c71500b6890a7d24653a65ff6";
const BINDING_FINGERPRINT: &str =
    "a44fd4f629a1183147a25a8afb93b026de7e3f92efe737637da222617df0c655";
const PENDING_DRAIN_PRODUCT_OPERATION: &str = "1234567890abcdef1234567890abcdef";
const PENDING_DRAIN_INTENT: &str = "fedcba0987654321fedcba0987654321";
const PENDING_DRAIN_CERTIFICATION_OPERATION: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
const READINESS_FUNCTION: &str = "public.starring_runtime_serving_database_readiness_v1()";
const IDENTITY_FUNCTION: &str = "public.starring_runtime_serving_database_identity_v1()";
const HEARTBEAT_FUNCTION: &str = "public.starring_runtime_serving_heartbeat_v1(TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BIGINT)";
const DISCONNECT_FUNCTION: &str =
    "public.starring_runtime_serving_disconnect_v1(TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT)";
const OBSERVE_V2_FUNCTION: &str =
    "public.starring_runtime_serving_observe_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT)";
const HEARTBEAT_V2_FUNCTION: &str = "public.starring_runtime_serving_heartbeat_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT,BIGINT)";
const DISCONNECT_V2_FUNCTION: &str = "public.starring_runtime_serving_disconnect_if_current_v2(TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT)";
const OBSERVE_PENDING_DRAIN_SOURCE_FUNCTION: &str =
    "public.starring_runtime_serving_observe_pending_drain_source_v1(TEXT,BIGINT,TEXT)";
const DISCONNECT_PENDING_DRAIN_SOURCE_IF_EXPIRED_FUNCTION: &str =
    "public.starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1(TEXT,BIGINT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,TEXT,BIGINT,TEXT,BIGINT,TEXT,TEXT,TEXT,BIGINT,BIGINT,BIGINT)";

struct IsolatedDatabase {
    name: String,
    role: String,
    administrator: PgConnection,
    owner_pool: PgPool,
    executor_pool: PgPool,
    deadline_pool: PgPool,
    foreign_database_options: PgConnectOptions,
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
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = PathBuf::from("/tmp").join(format!("srs-{}-{suffix:x}", std::process::id()));
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
        let initdb = std::env::var("STARRING_TEST_INITDB").unwrap_or_else(|_| "initdb".to_string());
        let pg_ctl = std::env::var("STARRING_TEST_PG_CTL").unwrap_or_else(|_| "pg_ctl".to_string());
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
async fn serving_mutations_are_replay_safe_bounded_and_least_privilege() {
    let cluster = EphemeralPostgresCluster::start();
    let database = isolated_database(cluster.connect_options()).await;
    let owner_pool = database.owner_pool.clone();
    let executor_pool = database.executor_pool.clone();
    let deadline_pool = database.deadline_pool.clone();
    let foreign_database_options = database.foreign_database_options.clone();
    let name = database.name.clone();
    let role = database.role.clone();
    let outcome = tokio::spawn(async move {
        serving_scenario(
            owner_pool,
            executor_pool,
            deadline_pool,
            foreign_database_options,
            name,
            role,
        )
        .await;
    })
    .await;
    cleanup(database).await;
    outcome.expect("restricted serving proof must complete");
    drop(cluster);
}

async fn isolated_database(base: PgConnectOptions) -> IsolatedDatabase {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("starring_rs_test_{suffix}");
    let role = format!("starring_rs_executor_{suffix}");
    let password = format!("rs_test_password_{suffix}");
    for identifier in [&name, &role] {
        assert!(
            identifier.len() <= 63
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        );
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
    let readiness_definition_digest = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(\
            pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(\
                'public.starring_runtime_serving_database_readiness_v1()'\
            )), 'UTF8')), 'hex')",
    )
    .fetch_one(&owner_pool)
    .await
    .unwrap();
    assert_eq!(
        readiness_definition_digest,
        "e598fb40785ccd66ce44ec6c7f85e52fd9e004ab1e05de9c0c03963f06df45f1"
    );
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
    for statement in [
        format!("REVOKE ALL PRIVILEGES ON DATABASE {name} FROM PUBLIC"),
        "REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC".to_string(),
        format!("GRANT CONNECT ON DATABASE {name} TO {role}"),
        format!("GRANT USAGE ON SCHEMA public TO {role}"),
        format!("GRANT EXECUTE ON FUNCTION {READINESS_FUNCTION} TO {role}"),
        format!("GRANT EXECUTE ON FUNCTION {IDENTITY_FUNCTION} TO {role}"),
        format!("GRANT EXECUTE ON FUNCTION {HEARTBEAT_FUNCTION} TO {role}"),
        format!("GRANT EXECUTE ON FUNCTION {DISCONNECT_FUNCTION} TO {role}"),
        format!("GRANT EXECUTE ON FUNCTION {OBSERVE_V2_FUNCTION} TO {role}"),
        format!("GRANT EXECUTE ON FUNCTION {HEARTBEAT_V2_FUNCTION} TO {role}"),
        format!("GRANT EXECUTE ON FUNCTION {DISCONNECT_V2_FUNCTION} TO {role}"),
        format!("GRANT EXECUTE ON FUNCTION {OBSERVE_PENDING_DRAIN_SOURCE_FUNCTION} TO {role}"),
        format!(
            "GRANT EXECUTE ON FUNCTION {DISCONNECT_PENDING_DRAIN_SOURCE_IF_EXPIRED_FUNCTION} TO {role}"
        ),
    ] {
        owner_pool.execute(statement.as_str()).await.unwrap();
    }
    seed_live_fixture(&owner_pool).await;
    let options = base.database(&name).username(&role).password(&password);
    let foreign_databases = sqlx::query_as::<_, (String, String)>(
        "SELECT database_row.datname::TEXT, \
            pg_catalog.quote_ident(database_row.datname)::TEXT \
         FROM pg_catalog.pg_database AS database_row \
         WHERE database_row.datallowconn \
            AND database_row.datname <> $1 \
         ORDER BY database_row.datname",
    )
    .bind(&name)
    .fetch_all(&owner_pool)
    .await
    .unwrap();
    assert!(!foreign_databases.is_empty());
    for (_, quoted_database_name) in &foreign_databases {
        administrator
            .execute(
                format!(
                    "REVOKE ALL PRIVILEGES ON DATABASE {} FROM PUBLIC",
                    quoted_database_name
                )
                .as_str(),
            )
            .await
            .unwrap();
    }
    let foreign_database_options = options.clone().database(&foreign_databases[0].0);
    let executor_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.clone())
        .await
        .unwrap();
    let deadline_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    IsolatedDatabase {
        name,
        role,
        administrator,
        owner_pool,
        executor_pool,
        deadline_pool,
        foreign_database_options,
    }
}

async fn cleanup(mut database: IsolatedDatabase) {
    database.deadline_pool.close().await;
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
}

async fn serving_scenario(
    owner_pool: PgPool,
    executor_pool: PgPool,
    deadline_pool: PgPool,
    foreign_database_options: PgConnectOptions,
    database_name: String,
    role: String,
) {
    let database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(&owner_pool)
    .await
    .unwrap();
    assert_restricted_boundary(&executor_pool).await;
    let owner_role = sqlx::query_scalar::<_, String>("SELECT session_user::TEXT")
        .fetch_one(&owner_pool)
        .await
        .unwrap();
    let owner_expectation = RuntimeServingDatabaseExpectationV1::new(
        database_identity.clone(),
        database_name.clone(),
        owner_role,
    )
    .unwrap();
    assert!(matches!(
        PostgresRuntimeServingLeaseV1::connect_verified_default(
            owner_pool.clone(),
            owner_expectation
        )
        .await,
        Err(RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch)
    ));
    let expectation = RuntimeServingDatabaseExpectationV1::new(
        database_identity.clone(),
        database_name.clone(),
        role.clone(),
    )
    .unwrap();
    let adapter = PostgresRuntimeServingLeaseV1::connect_verified_default(
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
    let pending_drain_lookup = RuntimePendingDrainServingLookupV1::new(
        RuntimeDrainIntentIdV2::parse("00112233445566778899aabbccddeeff").unwrap(),
        NonZeroU64::new(1).unwrap(),
        [0xab; 32],
    )
    .unwrap();
    assert!(matches!(
        adapter
            .observe_pending_drain_source_serving_v1(&pending_drain_lookup)
            .await
            .unwrap(),
        RuntimePendingDrainServingObservationV1::Diverged { .. }
    ));
    let unchanged_pending_drain_serving = serving_lease_image(&owner_pool).await;
    let pending_drain_identity = RuntimeServingIdentityV2 {
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: TenantId::parse(TENANT).unwrap(),
            installation_id: InstallationId::parse(INSTALLATION).unwrap(),
            deployment_id: DeploymentId::parse(DEPLOYMENT).unwrap(),
        },
        operation_id: RuntimeCertificationOperationIdV2::parse("00112233445566778899aabbccddeeff")
            .unwrap(),
        attestation_digest: RuntimeLiveAttestationDigestV2::parse(ATTESTATION).unwrap(),
        process_identity: RuntimeProcessIdentityV1 {
            target: runtime_target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: ProcessInstanceId::parse(PROCESS).unwrap(),
        },
        lease_epoch: NonZeroU64::new(1).unwrap(),
        revision: NonZeroU64::new(1).unwrap(),
    };
    assert_eq!(
        adapter
            .disconnect_pending_drain_source_serving_if_expired_v1(
                &pending_drain_lookup,
                &pending_drain_identity,
            )
            .await
            .unwrap_err(),
        RuntimeServingPersistenceErrorV1::OwnershipLost
    );
    assert_eq!(
        serving_lease_image(&owner_pool).await,
        unchanged_pending_drain_serving
    );
    let foreign_database_error = match PgConnection::connect_with(&foreign_database_options).await {
        Ok(connection) => {
            connection.close().await.unwrap();
            panic!("serving executor connected to another database");
        }
        Err(error) => error,
    };
    assert_eq!(
        foreign_database_error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some("42501")
    );
    let foreign_database_name = foreign_database_options.get_database().unwrap();
    let (quoted_foreign_database, quoted_executor_role) = sqlx::query_as::<_, (String, String)>(
        "SELECT pg_catalog.quote_ident($1), pg_catalog.quote_ident($2)",
    )
    .bind(foreign_database_name)
    .bind(&role)
    .fetch_one(&owner_pool)
    .await
    .unwrap();
    owner_pool
        .execute(
            format!(
                "GRANT CONNECT ON DATABASE {quoted_foreign_database} TO {quoted_executor_role}"
            )
            .as_str(),
        )
        .await
        .unwrap();
    assert_eq!(
        adapter.verify_database_v1().await.unwrap_err(),
        RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch
    );
    PgConnection::connect_with(&foreign_database_options)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    owner_pool
        .execute(
            format!(
                "REVOKE CONNECT ON DATABASE {quoted_foreign_database} FROM {quoted_executor_role}"
            )
            .as_str(),
        )
        .await
        .unwrap();
    assert!(adapter.verify_database_v1().await.is_ok());
    assert_readiness_trust_anchor(&owner_pool, &executor_pool, &expectation, &adapter).await;
    let wrong_expectation = RuntimeServingDatabaseExpectationV1::new(
        expectation.database_identity(),
        "starring_wrong_database",
        expectation.executor_role(),
    )
    .unwrap();
    assert!(matches!(
        PostgresRuntimeServingLeaseV1::connect_verified_default(
            executor_pool.clone(),
            wrong_expectation
        )
        .await,
        Err(RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch)
    ));

    let mut session = serving_session(&owner_pool).await;
    let first_request = session.begin_heartbeat(Duration::from_secs(60)).unwrap();
    assert_writer_fence_snapshot_and_fail_closed(&owner_pool, &adapter, &first_request).await;
    let first = adapter
        .heartbeat_serving(first_request.clone())
        .await
        .unwrap();
    let first_replay = adapter
        .heartbeat_serving(first_request.clone())
        .await
        .unwrap();
    assert_eq!(first, first_replay);
    session.apply_heartbeat(first).unwrap();

    let lost_request = session.begin_heartbeat(Duration::from_secs(60)).unwrap();
    let lost_receipt = adapter
        .heartbeat_serving(lost_request.clone())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.automation_installations SET lifecycle_state = 'suspended', \
         updated_at = updated_at + INTERVAL '1 microsecond' \
         WHERE installation_id = $1",
    )
    .bind(INSTALLATION)
    .execute(&owner_pool)
    .await
    .unwrap();
    let recovered_receipt = adapter
        .heartbeat_serving(lost_request.clone())
        .await
        .unwrap();
    assert_eq!(lost_receipt, recovered_receipt);
    sqlx::query(
        "UPDATE public.automation_installations SET lifecycle_state = 'active', \
         updated_at = updated_at + INTERVAL '1 microsecond' \
         WHERE installation_id = $1",
    )
    .bind(INSTALLATION)
    .execute(&owner_pool)
    .await
    .unwrap();
    session.apply_heartbeat(recovered_receipt).unwrap();

    let deadline_timeouts = RuntimeServingDatabaseTimeoutsV1::new(
        Duration::from_millis(250),
        Duration::from_millis(50),
    )
    .unwrap();
    let deadline_adapter = PostgresRuntimeServingLeaseV1::connect_verified(
        deadline_pool.clone(),
        expectation.clone(),
        deadline_timeouts,
    )
    .await
    .unwrap();
    let held = deadline_pool.acquire().await.unwrap();
    let exhausted = deadline_adapter.verify_database_v1().await.unwrap_err();
    assert_eq!(exhausted, RuntimeServingPersistenceErrorV1::Timeout);
    drop(held);
    assert!(deadline_adapter.verify_database_v1().await.is_ok());
    let cancellation_adapter = PostgresRuntimeServingLeaseV1::connect_verified(
        deadline_pool.clone(),
        expectation,
        RuntimeServingDatabaseTimeoutsV1::new(Duration::from_secs(2), Duration::from_secs(1))
            .unwrap(),
    )
    .await
    .unwrap();

    let cancelled_request = session.begin_heartbeat(Duration::from_secs(60)).unwrap();
    let mut blocker = owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT deployment_id FROM public.runtime_deployments WHERE deployment_id = $1 FOR UPDATE",
    )
    .bind(DEPLOYMENT)
    .execute(&mut *blocker)
    .await
    .unwrap();
    let cancelled = tokio::time::timeout(
        Duration::from_millis(100),
        cancellation_adapter.heartbeat_serving(cancelled_request.clone()),
    )
    .await;
    assert!(cancelled.is_err());
    let replacement = tokio::time::timeout(Duration::from_millis(500), deadline_pool.acquire())
        .await
        .expect("cancelled mutation must release the only pool permit")
        .unwrap();
    drop(replacement);
    blocker.commit().await.unwrap();
    let cancelled_recovery = deadline_adapter
        .heartbeat_serving(cancelled_request)
        .await
        .unwrap();
    session.apply_heartbeat(cancelled_recovery).unwrap();

    let disconnect_request = session.begin_disconnect().unwrap();
    sqlx::query(
        "UPDATE public.product_control_plane_identity \
         SET database_identity = '11111111-2222-3333-8444-555555555555'::UUID WHERE singleton",
    )
    .execute(&owner_pool)
    .await
    .unwrap();
    let rotated = adapter
        .mark_serving_disconnected(disconnect_request.clone())
        .await
        .unwrap_err();
    assert_eq!(
        rotated,
        RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch
    );
    assert_eq!(
        rotated.class(),
        RuntimeConvergenceErrorClassV1::InvalidState
    );
    sqlx::query("UPDATE public.product_control_plane_identity SET database_identity = $1::UUID WHERE singleton")
        .bind(adapter.initial_readiness().database_identity.as_str())
        .execute(&owner_pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.automation_installations SET lifecycle_state = 'suspended', \
         updated_at = updated_at + INTERVAL '1 microsecond' \
         WHERE installation_id = $1",
    )
    .bind(INSTALLATION)
    .execute(&owner_pool)
    .await
    .unwrap();
    let disconnected = adapter
        .mark_serving_disconnected(disconnect_request.clone())
        .await
        .unwrap();
    let disconnected_replay = adapter
        .mark_serving_disconnected(disconnect_request)
        .await
        .unwrap();
    assert_eq!(disconnected, disconnected_replay);
    session.apply_disconnect(disconnected).unwrap();
    assert_pending_drain_heartbeat_and_expired_disconnect(&owner_pool, &adapter, &first_request)
        .await;
}

async fn assert_pending_drain_heartbeat_and_expired_disconnect(
    owner_pool: &PgPool,
    adapter: &PostgresRuntimeServingLeaseV1,
    heartbeat_template: &RuntimeHeartbeatServingV1,
) {
    sqlx::query(
        "UPDATE public.automation_installations SET lifecycle_state = 'active', \
         updated_at = updated_at + INTERVAL '1 microsecond' \
         WHERE installation_id = $1",
    )
    .bind(INSTALLATION)
    .execute(owner_pool)
    .await
    .unwrap();
    let baseline_identity = seed_expired_v2_serving_fixture(owner_pool).await;
    let fresh_identity = reset_v2_serving_fixture(owner_pool, &baseline_identity, 2, true).await;
    let heartbeat_request = v2_heartbeat_request(heartbeat_template, &fresh_identity);
    assert_foreign_pending_drain_does_not_interrupt(owner_pool, &heartbeat_request).await;

    let lookup = create_pending_drain_fixture(owner_pool).await;
    let unchanged = serving_lease_image(owner_pool).await;
    let mut exact = owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *exact)
        .await
        .unwrap();
    let exact_error = raw_heartbeat(&mut exact, &heartbeat_request)
        .await
        .unwrap_err();
    assert_database_code(&exact_error, "RS003");
    exact.rollback().await.unwrap();
    assert_eq!(
        adapter
            .heartbeat_serving(heartbeat_request.clone())
            .await
            .unwrap_err(),
        RuntimeServingPersistenceErrorV1::AuthorityChanged
    );
    let v2_lease_for = Duration::from_secs(31);
    let mut exact_v2 = owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *exact_v2)
        .await
        .unwrap();
    let exact_v2_error = raw_heartbeat_v2(&mut exact_v2, &fresh_identity, v2_lease_for)
        .await
        .unwrap_err();
    assert_database_code(&exact_v2_error, "RS003");
    exact_v2.rollback().await.unwrap();
    assert_eq!(
        adapter
            .heartbeat_serving_v2(&fresh_identity, v2_lease_for)
            .await
            .unwrap_err(),
        RuntimeServingPersistenceErrorV1::AuthorityChanged
    );
    let mut replay_identity = fresh_identity.clone();
    replay_identity.revision = NonZeroU64::new(1).unwrap();
    assert_eq!(
        adapter
            .heartbeat_serving_v2(&replay_identity, v2_lease_for)
            .await
            .unwrap_err(),
        RuntimeServingPersistenceErrorV1::AuthorityChanged
    );
    assert_eq!(serving_lease_image(owner_pool).await, unchanged);

    set_attestation_fixture_triggers(owner_pool, false).await;
    sqlx::query(
        "UPDATE public.runtime_attestations SET v2_initial_lease_epoch = 2 \
         WHERE attestation_id = $1",
    )
    .bind(fresh_identity.attestation_digest.as_str())
    .execute(owner_pool)
    .await
    .unwrap();
    let mut diverged = owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *diverged)
        .await
        .unwrap();
    let diverged_error = raw_heartbeat(&mut diverged, &heartbeat_request)
        .await
        .unwrap_err();
    assert_database_code(&diverged_error, "RS004");
    diverged.rollback().await.unwrap();
    assert_eq!(
        adapter
            .heartbeat_serving(heartbeat_request.clone())
            .await
            .unwrap_err(),
        RuntimeServingPersistenceErrorV1::PersistenceCorrupt
    );
    assert_eq!(serving_lease_image(owner_pool).await, unchanged);
    sqlx::query(
        "UPDATE public.runtime_attestations SET v2_initial_lease_epoch = 1 \
         WHERE attestation_id = $1",
    )
    .bind(fresh_identity.attestation_digest.as_str())
    .execute(owner_pool)
    .await
    .unwrap();
    set_attestation_fixture_triggers(owner_pool, true).await;

    assert_cleared_pending_drain_does_not_interrupt(owner_pool, &heartbeat_request).await;

    let identity = reset_v2_serving_fixture(owner_pool, &baseline_identity, 3, false).await;
    let observation = adapter
        .observe_pending_drain_source_serving_v1(&lookup)
        .await
        .unwrap();
    let RuntimePendingDrainServingObservationV1::Expired {
        source, serving, ..
    } = observation
    else {
        panic!("pending drain source must observe the expired serving lease")
    };
    assert_eq!(source.intent_id(), lookup.intent_id());
    assert_eq!(
        source.source_intent_revision(),
        lookup.source_intent_revision()
    );
    assert_eq!(source.source_state_digest(), lookup.source_state_digest());
    assert_eq!(serving.identity, identity);

    let disconnected = adapter
        .disconnect_pending_drain_source_serving_if_expired_v1(&lookup, &identity)
        .await
        .unwrap();
    assert_eq!(disconnected.identity.revision.get(), 4);
    assert!(!disconnected.connected);
    assert!(!disconnected.serving);
    assert_eq!(disconnected.last_heartbeat_at, disconnected.expires_at);
    let disconnected_image = serving_lease_image(owner_pool).await;
    let replay = adapter
        .disconnect_pending_drain_source_serving_if_expired_v1(&lookup, &identity)
        .await
        .unwrap();
    assert_eq!(replay, disconnected);
    assert_eq!(serving_lease_image(owner_pool).await, disconnected_image);

    let fresh_identity = reset_v2_serving_fixture(owner_pool, &identity, 5, true).await;
    let fresh_image = serving_lease_image(owner_pool).await;
    assert_eq!(
        adapter
            .disconnect_pending_drain_source_serving_if_expired_v1(&lookup, &fresh_identity)
            .await
            .unwrap_err(),
        RuntimeServingPersistenceErrorV1::OwnershipLost
    );
    assert_eq!(serving_lease_image(owner_pool).await, fresh_image);

    let fenced_identity = reset_v2_serving_fixture(owner_pool, &identity, 6, false).await;
    mutate_writer_fence(
        owner_pool,
        "UPDATE public.runtime_writer_fence \
         SET fence_state = 'closed', fence_generation = 4, \
             cutover_lease_epoch_high_water = 1, \
             cutover_coordinator_id = '00112233445566778899aabbccddeeff', \
             cutover_expires_at = pg_catalog.clock_timestamp() + INTERVAL '1 hour' \
         WHERE singleton",
    )
    .await;
    let fenced_image = serving_lease_image(owner_pool).await;
    assert_eq!(
        adapter
            .disconnect_pending_drain_source_serving_if_expired_v1(&lookup, &fenced_identity)
            .await
            .unwrap_err(),
        RuntimeServingPersistenceErrorV1::RetryNotReady
    );
    assert_eq!(serving_lease_image(owner_pool).await, fenced_image);
    mutate_writer_fence(
        owner_pool,
        "UPDATE public.runtime_writer_fence \
         SET fence_state = 'open', fence_generation = 5, \
             cutover_lease_epoch_high_water = 1, \
             cutover_coordinator_id = NULL, cutover_expires_at = NULL \
         WHERE singleton",
    )
    .await;
}

fn v2_heartbeat_request(
    template: &RuntimeHeartbeatServingV1,
    identity: &RuntimeServingIdentityV2,
) -> RuntimeHeartbeatServingV1 {
    let mut request = template.clone();
    request.identity.scope = identity.scope.clone();
    request.identity.attestation_id =
        RuntimeAttestationIdV1::parse(identity.attestation_digest.as_str()).unwrap();
    request.identity.process_instance_id = identity.process_identity.process_instance_id.clone();
    request.identity.runtime_generation = identity.process_identity.runtime_generation;
    request.identity.lease_epoch = identity.lease_epoch;
    request.identity.expected_revision = identity.revision;
    request
}

async fn assert_foreign_pending_drain_does_not_interrupt(
    owner_pool: &PgPool,
    heartbeat_request: &RuntimeHeartbeatServingV1,
) {
    let mut fixture = owner_pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *fixture)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations (installation_id, tenant_id, \
         discord_application_id, discord_guild_id, ruleset_key, lifecycle_state, \
         current_authority_revision) VALUES \
         ('serving-test-foreign-installation', $1, '9300302', '9300102', \
          'serving_test_foreign', 'active', 1)",
    )
    .bind(TENANT)
    .execute(&mut *fixture)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions (installation_id, \
         revision, tenant_id, binding_revision, resource_bindings, binding_fingerprint, \
         policy_revision, required_approvals, activation_ttl_seconds, authority_payload_digest, \
         created_by_principal_id, created_by_request_digest) \
         VALUES ('serving-test-foreign-installation', 1, $1, 1, '{}'::JSONB, $2, \
                 1, 1, 3600, $3, $4, $5)",
    )
    .bind(TENANT)
    .bind(BINDING_FINGERPRINT)
    .bind("8".repeat(64))
    .bind(PRINCIPAL)
    .bind("9".repeat(64))
    .execute(&mut *fixture)
    .await
    .unwrap();
    fixture.commit().await.unwrap();
    set_slot_fence_fixture_triggers(owner_pool, false).await;
    let unchanged = serving_lease_image(owner_pool).await;
    let mut transaction = owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '2s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_slot_writer_fences_v2 \
         SET pending_drain_intent_id = '11111111111111111111111111111111', \
             pending_product_operation_id = '22222222222222222222222222222222', \
             pending_tenant_id = $1, \
             pending_installation_id = 'serving-test-foreign-installation', \
             pending_deployment_id = 'serving-test-foreign-deployment', \
             pending_expected_revision = 1, \
             pending_marked_at = pg_catalog.clock_timestamp(), \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE slot_guild_id = '9300102' \
             AND slot_ruleset_key = 'serving_test_foreign'",
    )
    .bind(TENANT)
    .execute(&mut *transaction)
    .await
    .unwrap();
    raw_heartbeat(&mut transaction, heartbeat_request)
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    set_slot_fence_fixture_triggers(owner_pool, true).await;
    assert_eq!(serving_lease_image(owner_pool).await, unchanged);
}

async fn assert_cleared_pending_drain_does_not_interrupt(
    owner_pool: &PgPool,
    heartbeat_request: &RuntimeHeartbeatServingV1,
) {
    set_slot_fence_fixture_triggers(owner_pool, false).await;
    let unchanged = serving_lease_image(owner_pool).await;
    let mut transaction = owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '2s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_slot_writer_fences_v2 \
         SET pending_drain_intent_id = NULL, \
             pending_product_operation_id = NULL, \
             pending_tenant_id = NULL, \
             pending_installation_id = NULL, \
             pending_deployment_id = NULL, \
             pending_expected_revision = NULL, \
             pending_marked_at = NULL, \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    raw_heartbeat(&mut transaction, heartbeat_request)
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    set_slot_fence_fixture_triggers(owner_pool, true).await;
    assert_eq!(serving_lease_image(owner_pool).await, unchanged);
}

async fn set_attestation_fixture_triggers(pool: &PgPool, enabled: bool) {
    let action = if enabled { "ENABLE" } else { "DISABLE" };
    sqlx::query(&format!(
        "ALTER TABLE public.runtime_attestations {action} TRIGGER USER"
    ))
    .execute(pool)
    .await
    .unwrap();
}

async fn set_slot_fence_fixture_triggers(pool: &PgPool, enabled: bool) {
    let action = if enabled { "ENABLE" } else { "DISABLE" };
    sqlx::query(&format!(
        "ALTER TABLE public.runtime_slot_writer_fences_v2 {action} TRIGGER USER"
    ))
    .execute(pool)
    .await
    .unwrap();
}

fn assert_database_code(error: &sqlx::Error, expected: &str) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some(expected),
        "{error:?}"
    );
}

async fn create_pending_drain_fixture(pool: &PgPool) -> RuntimePendingDrainServingLookupV1 {
    let revision = sqlx::query_scalar::<_, i64>(
        "SELECT revision FROM public.runtime_deployments WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap();
    let target = runtime_target();
    let canonical = automation_runtime_controller::RuntimeCanonicalProductDrainV2::new(
        automation_runtime_controller::RuntimeProductMutationPreimageV2 {
            operation_id: automation_runtime_controller::RuntimeProductOperationIdV2::parse(
                PENDING_DRAIN_PRODUCT_OPERATION,
            )
            .unwrap(),
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse(TENANT).unwrap(),
                installation_id: InstallationId::parse(INSTALLATION).unwrap(),
                deployment_id: DeploymentId::parse(DEPLOYMENT).unwrap(),
            },
            expected_revision: automation_runtime_convergence::DeploymentRevision::new(
                u64::try_from(revision).unwrap(),
            )
            .unwrap(),
            slot: automation_runtime_controller::RuntimeServingSlotV2::from_target(&target),
            expected_target: target,
            mutation_kind: automation_runtime_controller::RuntimeProductMutationKindV2::Teardown,
            product_semantic_request_digest:
                automation_runtime_controller::RuntimeProductSemanticRequestDigestV2::parse(
                    "7".repeat(64),
                )
                .unwrap(),
        },
        RuntimeDrainIntentIdV2::parse(PENDING_DRAIN_INTENT).unwrap(),
    )
    .unwrap();
    let product = canonical.product_preimage();
    let drain = canonical.drain_preimage();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let created = sqlx::query_as::<_, (String, Option<i64>, Option<String>)>(
        "SELECT outcome_name, intent_revision, intent_state \
         FROM starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(\
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20\
         )",
    )
    .bind(product.operation_id.as_str())
    .bind(drain.key.intent_id.as_str())
    .bind(product.scope.tenant_id.as_str())
    .bind(product.scope.installation_id.as_str())
    .bind(product.scope.deployment_id.as_str())
    .bind(i64::try_from(product.expected_revision.get()).unwrap())
    .bind(product.slot.guild_id.to_string())
    .bind(product.slot.ruleset_key.as_str())
    .bind(product.expected_target.guild_id.to_string())
    .bind(product.expected_target.ruleset_key.as_str())
    .bind(i64::from(product.expected_target.version.get()))
    .bind(product.expected_target.content_hash.to_hex())
    .bind(i64::try_from(product.expected_target.binding_revision.get()).unwrap())
    .bind(product.expected_target.binding_fingerprint.as_str())
    .bind("teardown")
    .bind(product.product_semantic_request_digest.as_str())
    .bind(canonical.product_mutation_request_bytes())
    .bind(canonical.product_mutation_digest().as_str())
    .bind(canonical.drain_intent_request_bytes())
    .bind(canonical.drain_intent_digest().as_str())
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(
        created,
        ("inserted".to_string(), Some(1), Some("pending".to_string()))
    );
    transaction.commit().await.unwrap();
    let source = sqlx::query_as::<_, (i64, String)>(
        "SELECT intent_revision, canonical_state_digest \
         FROM public.runtime_drain_intents_v2 WHERE drain_intent_id = $1",
    )
    .bind(PENDING_DRAIN_INTENT)
    .fetch_one(pool)
    .await
    .unwrap();
    RuntimePendingDrainServingLookupV1::new(
        RuntimeDrainIntentIdV2::parse(PENDING_DRAIN_INTENT).unwrap(),
        NonZeroU64::new(u64::try_from(source.0).unwrap()).unwrap(),
        decode_hex_32(&source.1),
    )
    .unwrap()
}

async fn seed_expired_v2_serving_fixture(pool: &PgPool) -> RuntimeServingIdentityV2 {
    let request_bytes = b"{}".to_vec();
    let request_digest = framed_digest_v2(
        pool,
        "starring.runtime.certification_request.v2",
        &request_bytes,
    )
    .await;
    let live_bytes = format!(
        "{{\"format_version\":2,\"request_digest\":\"{request_digest}\",\"request\":{{}}}}"
    )
    .into_bytes();
    let attestation_digest =
        framed_digest_v2(pool, "starring.runtime.live_attestation.v2", &live_bytes).await;
    let record: Value = serde_json::from_slice(&live_bytes).unwrap();
    let route_admission = json!({
        "gateway_owner_lease_id": {
            "gateway_shard_id": "shard:0",
            "lease_epoch": 1,
            "expected_build_revision": "test-build-1"
        },
        "attested_owner_revision": 1,
        "gateway": {
            "connection_epoch": 1,
            "admission_revision": 1,
            "connected_event_sequence": 1,
            "resume_sequence": 2
        }
    });
    let prepared_snapshot = json!({
        "fixture": "pending-drain-serving-v2",
        "revision": 9,
        "phase": { "phase": "awaiting_gateway_ready" }
    });
    let certified_snapshot = json!({
        "fixture": "pending-drain-serving-v2",
        "revision": 10,
        "phase": { "phase": "live" }
    });
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(pool)
        .await
        .unwrap();
    set_v2_fixture_triggers(pool, false).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_gateway_owners (gateway_shard_id, process_instance_id, \
         lease_epoch, expected_build_revision, owner_revision, expires_at) \
         VALUES ('shard:0', $1, 1, 'test-build-1', 1, $2)",
    )
    .bind(PROCESS)
    .bind(now + TimeDelta::hours(1))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_gateway_owners SET owner_revision = 2 \
         WHERE gateway_shard_id = 'shard:0'",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let acknowledgement_bytes = vec![b'x'; 197];
    sqlx::query(
        "INSERT INTO public.runtime_ingress_open_acknowledgements_v2 (gateway_shard_id, \
         source_acknowledgement_revision, request_digest, canonical_request_bytes, \
         fence_generation, maintenance_gate_generation, process_instance_id, \
         owner_lease_epoch, expected_build_revision, observed_owner_revision, \
         requested_owner_observed_at, requested_owner_expires_at, connection_epoch, \
         admission_revision, connected_event_sequence, resume_sequence, \
         acknowledgement_revision, acknowledged_at, expires_at) \
         VALUES ('shard:0', NULL, pg_catalog.sha256($1), $1, 1, 1, $2, 1, \
                 'test-build-1', 2, $3, $4, 1, 1, 1, 2, 1, $3, $5)",
    )
    .bind(&acknowledgement_bytes)
    .bind(PROCESS)
    .bind(now)
    .bind(now + TimeDelta::hours(1))
    .bind(now + TimeDelta::seconds(9))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM public.runtime_serving_leases \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments SET live_attestation_id = $1 \
         WHERE deployment_id = $2",
    )
    .bind(&attestation_digest)
    .bind(DEPLOYMENT)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_attestations \
         SET attestation_id = $1, attestation_digest = $1, \
             record_format_version = 2, record = $2, v2_operation_id = $3, \
             v2_intent_fingerprint = $4, v2_request_digest = $5, \
             v2_request_bytes = $6, v2_live_attestation_bytes = $7, \
             v2_must_commit_before = certified_at + INTERVAL '1 hour', \
             v2_route_admission = $8, v2_route_incarnation = 1, \
             v2_route_activation_sequence = 1, v2_initial_lease_epoch = 1, \
             v2_initial_serving_revision = 1, v2_prepared_snapshot = $9, \
             v2_certified_snapshot = $10 \
         WHERE attestation_id = $11",
    )
    .bind(&attestation_digest)
    .bind(Json(record))
    .bind(PENDING_DRAIN_CERTIFICATION_OPERATION)
    .bind("6".repeat(64))
    .bind(&request_digest)
    .bind(&request_bytes)
    .bind(&live_bytes)
    .bind(Json(route_admission))
    .bind(Json(prepared_snapshot))
    .bind(Json(certified_snapshot))
    .bind(ATTESTATION)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_serving_leases \
         (guild_id, ruleset_key, tenant_id, installation_id, deployment_id, \
          attestation_id, process_instance_id, runtime_generation, target_version, \
          target_content_hash, binding_revision, binding_fingerprint, lease_epoch, \
          revision, connected, serving, acquired_at, last_heartbeat_at, expires_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,1,1,$8,1,$9,1,1,TRUE,TRUE,$10,$11,$12)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(&attestation_digest)
    .bind(PROCESS)
    .bind(CONTENT_HASH)
    .bind(BINDING_FINGERPRINT)
    .bind(now - TimeDelta::seconds(90))
    .bind(now - TimeDelta::seconds(60))
    .bind(now - TimeDelta::seconds(30))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    set_v2_fixture_triggers(pool, true).await;
    RuntimeServingIdentityV2 {
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: TenantId::parse(TENANT).unwrap(),
            installation_id: InstallationId::parse(INSTALLATION).unwrap(),
            deployment_id: DeploymentId::parse(DEPLOYMENT).unwrap(),
        },
        operation_id: RuntimeCertificationOperationIdV2::parse(
            PENDING_DRAIN_CERTIFICATION_OPERATION,
        )
        .unwrap(),
        attestation_digest: RuntimeLiveAttestationDigestV2::parse(attestation_digest).unwrap(),
        process_identity: RuntimeProcessIdentityV1 {
            target: runtime_target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: ProcessInstanceId::parse(PROCESS).unwrap(),
        },
        lease_epoch: NonZeroU64::new(1).unwrap(),
        revision: NonZeroU64::new(1).unwrap(),
    }
}

async fn framed_digest_v2(pool: &PgPool, domain: &str, payload: &[u8]) -> String {
    sqlx::query_scalar(
        "SELECT starring_runtime_private_v2.starring_runtime_framed_digest_v2(\
            pg_catalog.convert_to($1, 'UTF8') || pg_catalog.decode('00', 'hex'), $2\
         )",
    )
    .bind(domain)
    .bind(payload)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn set_v2_fixture_triggers(pool: &PgPool, enabled: bool) {
    let action = if enabled { "ENABLE" } else { "DISABLE" };
    for table in [
        "runtime_attestations",
        "runtime_deployments",
        "runtime_serving_leases",
    ] {
        sqlx::query(&format!("ALTER TABLE public.{table} {action} TRIGGER USER"))
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn reset_v2_serving_fixture(
    pool: &PgPool,
    baseline: &RuntimeServingIdentityV2,
    revision: u64,
    fresh: bool,
) -> RuntimeServingIdentityV2 {
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.runtime_serving_leases DISABLE TRIGGER USER")
        .execute(pool)
        .await
        .unwrap();
    let (last_heartbeat_at, expires_at) = if fresh {
        (now - TimeDelta::seconds(1), now + TimeDelta::seconds(30))
    } else {
        (now - TimeDelta::seconds(60), now - TimeDelta::seconds(30))
    };
    sqlx::query(
        "UPDATE public.runtime_serving_leases \
         SET revision = $1, connected = TRUE, serving = TRUE, \
             acquired_at = $2, last_heartbeat_at = $3, expires_at = $4 \
         WHERE guild_id = $5 AND ruleset_key = $6",
    )
    .bind(i64::try_from(revision).unwrap())
    .bind(now - TimeDelta::seconds(90))
    .bind(last_heartbeat_at)
    .bind(expires_at)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_serving_leases ENABLE TRIGGER USER")
        .execute(pool)
        .await
        .unwrap();
    let mut identity = baseline.clone();
    identity.revision = NonZeroU64::new(revision).unwrap();
    identity
}

fn decode_hex_32(value: &str) -> [u8; 32] {
    assert_eq!(value.len(), 64);
    let mut decoded = [0_u8; 32];
    for (index, byte) in decoded.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    decoded
}

async fn assert_writer_fence_snapshot_and_fail_closed(
    owner_pool: &PgPool,
    adapter: &PostgresRuntimeServingLeaseV1,
    request: &RuntimeHeartbeatServingV1,
) {
    let unchanged = serving_lease_image(owner_pool).await;
    let mut stale = owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *stale)
        .await
        .unwrap();
    let initial = sqlx::query_scalar::<_, String>(
        "SELECT fence_state FROM public.runtime_writer_fence WHERE singleton",
    )
    .fetch_one(&mut *stale)
    .await
    .unwrap();
    assert_eq!(initial, "open");

    mutate_writer_fence(
        owner_pool,
        "UPDATE public.runtime_writer_fence \
         SET fence_state = 'closed', fence_generation = 2, \
             cutover_lease_epoch_high_water = 1, \
             cutover_coordinator_id = '00112233445566778899aabbccddeeff', \
             cutover_expires_at = pg_catalog.clock_timestamp() + INTERVAL '1 hour' \
         WHERE singleton",
    )
    .await;

    let error = raw_heartbeat(&mut stale, request).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some("40001")
    );
    stale.rollback().await.unwrap();
    assert_eq!(serving_lease_image(owner_pool).await, unchanged);
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT fence_state FROM public.runtime_writer_fence WHERE singleton",
        )
        .fetch_one(owner_pool)
        .await
        .unwrap(),
        "closed"
    );

    let mut fresh_closed = owner_pool.begin().await.unwrap();
    let error = raw_heartbeat(&mut fresh_closed, request).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some("RS005")
    );
    fresh_closed.rollback().await.unwrap();
    let error = adapter
        .heartbeat_serving(request.clone())
        .await
        .unwrap_err();
    assert_eq!(error, RuntimeServingPersistenceErrorV1::RetryNotReady);
    assert_eq!(serving_lease_image(owner_pool).await, unchanged);

    mutate_writer_fence(owner_pool, "DELETE FROM public.runtime_writer_fence").await;

    let error = adapter
        .heartbeat_serving(request.clone())
        .await
        .unwrap_err();
    assert_eq!(error, RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
    assert_eq!(serving_lease_image(owner_pool).await, unchanged);

    mutate_writer_fence(
        owner_pool,
        "INSERT INTO public.runtime_writer_fence (\
            singleton, fence_state, fence_generation, cutover_lease_epoch_high_water, \
            cutover_coordinator_id, cutover_expires_at\
         ) VALUES (TRUE, 'open', 3, 1, NULL, NULL)",
    )
    .await;
    assert_eq!(serving_lease_image(owner_pool).await, unchanged);
}

async fn mutate_writer_fence(owner_pool: &PgPool, mutation: &str) {
    let mut transaction = owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(mutation)
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn raw_heartbeat(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: &RuntimeHeartbeatServingV1,
) -> Result<(), sqlx::Error> {
    let identity = &request.identity;
    sqlx::query(
        "SELECT * FROM public.starring_runtime_serving_heartbeat_v1(\
            $1, $2, $3, $4, $5, $6, $7, $8, $9\
         )",
    )
    .bind(identity.scope.tenant_id.as_str())
    .bind(identity.scope.installation_id.as_str())
    .bind(identity.scope.deployment_id.as_str())
    .bind(identity.attestation_id.as_str())
    .bind(identity.process_instance_id.as_str())
    .bind(i64::try_from(identity.runtime_generation.get()).unwrap())
    .bind(i64::try_from(identity.lease_epoch.get()).unwrap())
    .bind(i64::try_from(identity.expected_revision.get()).unwrap())
    .bind(i64::try_from(request.lease_for.as_millis()).unwrap())
    .fetch_all(&mut **transaction)
    .await
    .map(|_| ())
}

async fn raw_heartbeat_v2(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &RuntimeServingIdentityV2,
    lease_for: Duration,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT * FROM public.starring_runtime_serving_heartbeat_v2(\
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10\
         )",
    )
    .bind(identity.operation_id.as_str())
    .bind(identity.scope.tenant_id.as_str())
    .bind(identity.scope.installation_id.as_str())
    .bind(identity.scope.deployment_id.as_str())
    .bind(identity.attestation_digest.as_str())
    .bind(identity.process_identity.process_instance_id.as_str())
    .bind(i64::try_from(identity.process_identity.runtime_generation.get()).unwrap())
    .bind(i64::try_from(identity.lease_epoch.get()).unwrap())
    .bind(i64::try_from(identity.revision.get()).unwrap())
    .bind(i64::try_from(lease_for.as_millis()).unwrap())
    .fetch_all(&mut **transaction)
    .await
    .map(|_| ())
}

async fn serving_lease_image(pool: &PgPool) -> Json<Value> {
    sqlx::query_scalar(
        "SELECT pg_catalog.to_jsonb(lease) \
         FROM public.runtime_serving_leases AS lease \
         WHERE lease.guild_id = $1 AND lease.ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_readiness_trust_anchor(
    owner_pool: &PgPool,
    executor_pool: &PgPool,
    expectation: &RuntimeServingDatabaseExpectationV1,
    adapter: &PostgresRuntimeServingLeaseV1,
) {
    let original_definition = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(\
         'public.starring_runtime_serving_database_readiness_v1()'))",
    )
    .fetch_one(owner_pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE OR REPLACE FUNCTION public.starring_runtime_serving_database_readiness_v1() \
         RETURNS TABLE(database_identity TEXT, database_name TEXT, executor_role TEXT, \
         checked_at TIMESTAMPTZ) LANGUAGE plpgsql VOLATILE STRICT PARALLEL UNSAFE \
         SECURITY DEFINER SET search_path = pg_catalog ROWS 1 AS $function$ \
         BEGIN RETURN QUERY SELECT identity.database_identity::TEXT, \
         pg_catalog.current_database()::TEXT, session_user::TEXT, \
         pg_catalog.clock_timestamp() FROM public.product_control_plane_identity AS identity \
         WHERE identity.singleton; END; $function$",
    )
    .execute(owner_pool)
    .await
    .unwrap();
    assert_eq!(
        adapter.verify_database_v1().await.unwrap_err(),
        RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch
    );
    assert!(matches!(
        PostgresRuntimeServingLeaseV1::connect_verified_default(
            executor_pool.clone(),
            expectation.clone()
        )
        .await,
        Err(RuntimeServingPersistenceErrorV1::DatabaseAuthorityMismatch)
    ));
    sqlx::query(&original_definition)
        .execute(owner_pool)
        .await
        .unwrap();
    assert!(adapter.verify_database_v1().await.is_ok());
}

async fn assert_restricted_boundary(pool: &PgPool) {
    for statement in [
        "SELECT deployment_id FROM public.runtime_deployments LIMIT 1",
        "UPDATE public.runtime_serving_leases SET connected = FALSE",
        "SELECT public.starring_runtime_mutation_clock()",
    ] {
        let error = sqlx::query(statement).execute(pool).await.unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|database| database.code())
                .as_deref(),
            Some("42501")
        );
    }
}

async fn seed_live_fixture(pool: &PgPool) {
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(pool)
        .await
        .unwrap();
    let requested_at = now - TimeDelta::seconds(10);
    let acquired_at = now - TimeDelta::seconds(5);
    let activation_expires_at = now + TimeDelta::hours(1);
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
    let prepared_snapshot = json!({
        "identity": {
            "deployment_id": DEPLOYMENT,
            "tenant_id": TENANT,
            "installation_id": INSTALLATION,
            "promotion_id": PROMOTION,
            "activation_request_id": ACTIVATION
        },
        "target": {
            "guild_id": GUILD.to_string(),
            "ruleset_key": RULESET,
            "version": 1,
            "content_hash": CONTENT_HASH,
            "binding_revision": 1,
            "binding_fingerprint": BINDING_FINGERPRINT
        },
        "runtime_generation": 1,
        "previous_runtime": null,
        "requested_at": requested_at,
        "revision": 1,
        "phase": { "phase": "requested" },
        "controller_lease": null,
        "last_fencing_token": null,
        "preflight": null,
        "drain": null,
        "activation": null,
        "panel_certificate": null,
        "gateway_ready": null,
        "live": null,
        "last_live_recovery": null,
        "last_runtime_failure": null
    });
    let record = promotion_record(
        requested_at,
        activation_expires_at,
        &request_digest,
        &approval_context,
    );
    for table in [
        "runtime_deployments",
        "runtime_attestations",
        "runtime_serving_leases",
    ] {
        sqlx::query(&format!("ALTER TABLE public.{table} DISABLE TRIGGER USER"))
            .execute(pool)
            .await
            .unwrap();
    }
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals (principal_id, discord_user_id) VALUES ($1, $2)",
    )
    .bind(PRINCIPAL)
    .bind("9300201")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_tenants (tenant_id, lifecycle_state, display_name) \
         VALUES ($1, 'active', 'Runtime Serving Test')",
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
    .bind("9300301")
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions (installation_id, \
         revision, tenant_id, binding_revision, resource_bindings, binding_fingerprint, \
         policy_revision, required_approvals, activation_ttl_seconds, authority_payload_digest, \
         created_by_principal_id, created_by_request_digest) \
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
    .bind("9300201")
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
    insert_activation_pending_promotion(&mut transaction, &request_digest, &record).await;
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
    .bind("9300401")
    .bind(requested_at)
    .bind(activation_expires_at)
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
    .bind(Json(
        json!({ "state": "linked", "linked_at": requested_at }),
    ))
    .bind(requested_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET state = 'applied', applied_at = $2, \
         applied_by = $3, completion_kind = 'already_active', \
         activation_notices = '[]'::JSONB WHERE id = $1",
    )
    .bind(ACTIVATION)
    .bind(requested_at)
    .bind("9300501")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_deployments (deployment_id, tenant_id, installation_id, \
         promotion_id, activation_request_id, installation_authority_revision, guild_id, \
         ruleset_key, target_version, target_content_hash, binding_revision, \
         binding_fingerprint, desired_target_digest, runtime_generation, requested_at, \
         snapshot_format_version, snapshot, revision, phase, live_attestation_id, live_at, \
         created_at, updated_at, policy_revision, desired_target_digest_version, \
         convergence_attempt_no) \
         VALUES ($1, $2, $3, $4, $5, 1, $6, $7, 1, $8, 1, $9, \
                 public.starring_runtime_desired_target_digest_v1($11::JSONB, 1), 1, $10, \
                 1, $11, 10, 'live', $12, $13, $10, $13, 1, 1, 1)",
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
    .bind(requested_at)
    .bind(Json(prepared_snapshot))
    .bind(ATTESTATION)
    .bind(acquired_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_attestations (attestation_id, attestation_digest, \
         deployment_id, deployment_revision, tenant_id, installation_id, promotion_id, \
         activation_request_id, guild_id, ruleset_key, target_version, target_content_hash, \
         binding_revision, binding_fingerprint, runtime_generation, controller_fencing_token, \
         process_instance_id, runtime_build_revision, panel_certificate_id, \
         panel_report_digest, gateway_shard_id, gateway_ready_kind, gateway_ready_at, \
         certified_at, record_format_version, record, created_at, convergence_attempt_no, \
         serving_lease_duration_nanos) \
         VALUES ($1, $1, $2, 10, $3, $4, $5, $6, $7, $8, 1, $9, 1, $10, 1, 1, \
                 $11, 'test-build-1', 'serving-test-panel', $12, 'shard:0', \
                 'discord_ready', $13, $13, 1, $14, $13, 1, 45000000000)",
    )
    .bind(ATTESTATION)
    .bind(DEPLOYMENT)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(PROMOTION)
    .bind(ACTIVATION)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(CONTENT_HASH)
    .bind(BINDING_FINGERPRINT)
    .bind(PROCESS)
    .bind("d".repeat(64))
    .bind(acquired_at)
    .bind(Json(
        json!({ "fixture": "starring runtime serving attestation record" }),
    ))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_serving_leases (guild_id, ruleset_key, tenant_id, \
         installation_id, deployment_id, attestation_id, process_instance_id, \
         runtime_generation, target_version, target_content_hash, binding_revision, \
         binding_fingerprint, lease_epoch, revision, connected, serving, acquired_at, \
         last_heartbeat_at, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 1, 1, $8, 1, $9, 1, 1, TRUE, TRUE, \
                 $10, $10, $11)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(ATTESTATION)
    .bind(PROCESS)
    .bind(CONTENT_HASH)
    .bind(BINDING_FINGERPRINT)
    .bind(acquired_at)
    .bind(acquired_at + TimeDelta::seconds(45))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    for table in [
        "runtime_deployments",
        "runtime_attestations",
        "runtime_serving_leases",
    ] {
        sqlx::query(&format!("ALTER TABLE public.{table} ENABLE TRIGGER USER"))
            .execute(pool)
            .await
            .unwrap();
    }
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
            "evidence": { "context_fingerprint": BINDING_FINGERPRINT }
        },
        "stage": {
            "state": "activation_pending",
            "publication": {
                "version": 1,
                "schema_version": 1,
                "content_hash": CONTENT_HASH,
                "disposition": "created",
                "registry_created_by": "9300401"
            },
            "activation": {
                "request_id": ACTIVATION,
                "target": {
                    "guild_id": GUILD.to_string(),
                    "ruleset_key": RULESET,
                    "version": 1,
                    "content_hash": CONTENT_HASH
                },
                "requester": "9300401",
                "required_approvals": NonZeroU32::new(1).unwrap(),
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
        "INSERT INTO public.authoring_promotions (id, record_format_version, revision, stage, \
         request_digest, tenant_id, installation_id, principal_id, record) \
         VALUES ($1, 1, 1, 'prepared', $2, $3, $4, $5, $6)",
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
        "UPDATE public.authoring_promotions SET revision = 2, stage = 'published', \
         record = $2 WHERE id = $1",
    )
    .bind(PROMOTION)
    .bind(Json(&published))
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions SET revision = 3, stage = 'activation_pending', \
         record = $2 WHERE id = $1",
    )
    .bind(PROMOTION)
    .bind(Json(record))
    .execute(&mut **transaction)
    .await
    .unwrap();
}

async fn serving_session(pool: &PgPool) -> RuntimeServingSessionV1 {
    let acquired_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT acquired_at FROM public.runtime_serving_leases \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_one(pool)
    .await
    .unwrap();
    let target = runtime_target();
    let identity = runtime_identity();
    let controller = ControllerId::parse("serving-test-controller").unwrap();
    let fencing_token = FencingToken::FIRST;
    let process = ProcessInstanceId::parse(PROCESS).unwrap();
    let mut deployment = RuntimeDeployment::request(
        identity.clone(),
        target.clone(),
        RuntimeGeneration::FIRST,
        None,
        acquired_at - TimeDelta::seconds(5),
    )
    .unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: controller.clone(),
            fencing_token,
            now: acquired_at - TimeDelta::seconds(4),
            expires_at: acquired_at + TimeDelta::seconds(120),
        })
        .unwrap();
    let guard = |deployment: &RuntimeDeployment| CommandGuardV1 {
        expected_revision: deployment.revision(),
        controller_id: controller.clone(),
        fencing_token,
        runtime_generation: RuntimeGeneration::FIRST,
        now: acquired_at,
    };
    deployment
        .accept_preflight(
            &guard(&deployment),
            PreflightAttestationV1 {
                target: target.clone(),
                runtime_generation: RuntimeGeneration::FIRST,
                observed_runtime: None,
                checked_at: acquired_at,
            },
        )
        .unwrap();
    deployment.request_drain(&guard(&deployment)).unwrap();
    deployment
        .accept_drain(
            &guard(&deployment),
            DrainAttestationV1 {
                previous_runtime: None,
                target_runtime_generation: RuntimeGeneration::FIRST,
                drained_at: acquired_at,
            },
        )
        .unwrap();
    deployment.begin_activation(&guard(&deployment)).unwrap();
    let activation = ActivationAttestationV1 {
        activation_request_id: ActivationRequestId::parse(ACTIVATION).unwrap(),
        target: target.clone(),
        runtime_generation: RuntimeGeneration::FIRST,
        kind: ActivationOutcomeKindV1::AlreadyActive,
        activated_at: acquired_at,
    };
    deployment
        .accept_activation(&guard(&deployment), activation)
        .unwrap();
    deployment
        .begin_panel_reconciliation(&guard(&deployment))
        .unwrap();
    deployment
        .accept_panel_certificate(
            &guard(&deployment),
            PanelCertificateV1 {
                certificate_id: PanelCertificateId::parse("serving-test-panel").unwrap(),
                report_digest: PanelReportDigestV1::parse("d".repeat(64)).unwrap(),
                target: target.clone(),
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id: process.clone(),
                declared_count: 0,
                installed_count: 0,
                unchanged_count: 0,
                skipped_transient_count: 0,
                skipped_unresolved_channel_count: 0,
                failed_count: 0,
                ambiguous_outcome_count: 0,
                stale_message_cleanup_pending_count: 0,
                orphan_message_cleanup_pending_count: 0,
                reposted_old_message_cleanup_pending_count: 0,
                reconciled_at: acquired_at,
            },
        )
        .unwrap();
    deployment
        .certify_live(
            &guard(&deployment),
            GatewayReadyAttestationV1 {
                target,
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id: process.clone(),
                kind: GatewayReadyKindV1::DiscordReady,
                ready_at: acquired_at,
            },
            acquired_at,
        )
        .unwrap();
    let snapshot = deployment.snapshot();
    let ownership = RuntimeServingReceiptV1 {
        identity: RuntimeServingIdentityV1 {
            scope: RuntimeDeploymentScopeV1::from_identity(&identity),
            attestation_id: RuntimeAttestationIdV1::parse(ATTESTATION).unwrap(),
            process_instance_id: process,
            runtime_generation: RuntimeGeneration::FIRST,
            lease_epoch: NonZeroU64::new(1).unwrap(),
            expected_revision: NonZeroU64::new(1).unwrap(),
        },
        runtime_generation: RuntimeGeneration::FIRST,
        acquired_at,
        last_heartbeat_at: acquired_at,
        expires_at: acquired_at + TimeDelta::seconds(45),
        connected: true,
        serving: true,
    };
    RuntimeServingSessionV1::restore(snapshot, ownership).unwrap()
}

fn runtime_identity() -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse(DEPLOYMENT).unwrap(),
        tenant_id: TenantId::parse(TENANT).unwrap(),
        installation_id: InstallationId::parse(INSTALLATION).unwrap(),
        promotion_id: PromotionId::parse(PROMOTION).unwrap(),
        activation_request_id: ActivationRequestId::parse(ACTIVATION).unwrap(),
    }
}

fn runtime_target() -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GUILD,
        ruleset_key: RuleSetKey::parse(RULESET).unwrap(),
        version: RuleSetVersionId::FIRST,
        content_hash: RuleSetContentHash::parse_hex(CONTENT_HASH).unwrap(),
        binding_revision: BindingRevision::FIRST,
        binding_fingerprint: ResourceBindingFingerprint::parse(BINDING_FINGERPRINT).unwrap(),
    }
}
