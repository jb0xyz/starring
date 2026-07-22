#![allow(dead_code, unused_imports)]

use std::future::Future;
use std::num::NonZeroU32;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use automation_panel_installation::strict::{
    StrictDeclaredPanelV1, StrictPanelCleanupIntentV1, StrictPanelCleanupKindV1,
    StrictPanelInstallKindV1, StrictPanelInstallationStore, StrictPanelMessagePayloadV1,
    StrictPanelMessageRefV1, StrictPanelOperationJournal, StrictPanelOperationKeyV1,
    StrictPanelOperationStateV1, StrictPanelOperationV1, StrictPanelPostIntentV1,
};
use automation_panel_installation::{
    PanelInstallation, PanelInstallationKey, PanelInstallationStore,
};
use automation_ruleset::{RuleSetContentHash, RuleSetVersionId};
use automation_runtime_controller::{
    RuntimeClaimNextExecutionV1, RuntimeConvergenceMutationV1, RuntimeConvergencePort,
    RuntimeConvergenceSessionV1, RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1,
};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    ControllerId, ControllerLeaseV1, DeploymentId, DrainAttestationV1, FencingToken,
    GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, PanelCertificateId,
    PanelCertificateV1, PreflightAttestationV1, ProcessInstanceId, PromotionId,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentPhaseV1, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_convergence_postgres::{
    prepare_requested_deployment_v1, ClaimDeploymentV1, ClaimNextDeploymentV1,
    DeploymentAvailabilityV1, DeploymentMutationV1, EnqueueDeploymentOutcomeV1,
    EnqueueDeploymentV1, GatewayShardIdV1, HeartbeatServingLeaseV1, LiveMetadataV1,
    MarkServingDisconnectedV1, PanelReportDigestV1, PostgresRuntimeConvergence,
    PostgresRuntimeConvergenceConfigV1, PostgresRuntimeExactTargetReader,
    RecoverBlockedDeploymentV1, RecoverStaleLiveV1, RuntimeBuildRevisionV1,
    RuntimeConvergenceStoreError, RuntimeDeploymentScopeV1, RuntimeExactTargetV1,
    SubmitDeploymentMutationV1, SubmitLiveAttestationV1, MIGRATOR,
};
use automation_runtime_panel_postgres::{
    verify_runtime_panel_database_with_timeouts_v1, PostgresFencedStrictPanelStoreV1,
    RuntimePanelDatabaseExpectationV1, RuntimePanelDatabaseTimeoutsV1, RuntimePanelErrorClassV1,
    RuntimePanelLatchedErrorV1, RuntimePanelPersistenceErrorV1, RuntimePanelSessionIdV1,
};
use automation_state::PanelSpec;
use chrono::{DateTime, TimeDelta, Utc};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, MessageId};
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
    "a44fd4f629a1183147a25a8afb93b026de7e3f92efe737637da222617df0c655";
const ROTATED_BINDING_FINGERPRINT: &str =
    "7777777777777777777777777777777777777777777777777777777777777777";
const PANEL_KEY: &str = "entry";
const SPEC_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

const PANEL_CAPABILITIES: [&str; 8] = [
    "public.starring_runtime_panel_database_readiness_v1()",
    "public.starring_runtime_panel_reconciliation_claim_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text)",
    "public.starring_runtime_panel_reconciliation_check_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint)",
    "public.starring_runtime_panel_reconciliation_snapshot_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)",
    "public.starring_runtime_panel_reconciliation_installation_upsert_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text,bigint,text,text,text,bigint)",
    "public.starring_runtime_panel_reconciliation_installation_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)",
    "public.starring_runtime_panel_reconciliation_journal_put_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,smallint,text,text,jsonb)",
    "public.starring_runtime_panel_reconciliation_journal_remove_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint,bigint,text)",
];

const PRIVATE_PANEL_CAPABILITIES: [&str; 2] = [
    "public.starring_runtime_panel_execution_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint)",
    "public.starring_runtime_panel_reconciliation_lock_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)",
];

include!("../../automation-runtime-convergence-postgres/tests/postgres_convergence/support.rs");

struct RuntimeTestDatabase {
    name: String,
    administrator: PgConnection,
    connect_options: PgConnectOptions,
    pool: PgPool,
}

async fn isolated_runtime_database(label: &str) -> RuntimeTestDatabase {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let base = url
        .parse::<PgConnectOptions>()
        .expect("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL");
    let configured_database = base
        .get_database()
        .expect("STARRING_TEST_DATABASE_URL must name a database");
    assert!(
        configured_database.starts_with("starring_")
            && configured_database
                .split('_')
                .any(|segment| segment == "test")
            && configured_database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "refusing to create a database outside the strict Starring test namespace"
    );
    assert!(
        !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "runtime test database label must be a lowercase PostgreSQL identifier fragment"
    );
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string();
    let prefix = "starring_panel_test_";
    let label_length = 63usize
        .checked_sub(prefix.len() + suffix.len() + 1)
        .expect("runtime test database suffix must fit PostgreSQL identifier limit");
    let label = &label[..label.len().min(label_length)];
    let name = format!("{prefix}{label}_{suffix}");
    assert!(name.len() <= 63);
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let connect_options = base.database(&name);
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(connect_options.clone())
        .await
        .unwrap();
    RuntimeTestDatabase {
        name,
        administrator,
        connect_options,
        pool,
    }
}

async fn run_migrated_runtime_database_test<F, Fut>(label: &str, test: F)
where
    F: FnOnce(PgPool, PgConnectOptions) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let database = isolated_runtime_database(label).await;
    let migration = MIGRATOR.run(&database.pool).await;
    if let Err(error) = migration {
        drop_runtime_database(database).await;
        panic!("runtime test database migration failed: {error}");
    }
    let outcome = tokio::spawn(test(
        database.pool.clone(),
        database.connect_options.clone(),
    ))
    .await;
    drop_runtime_database(database).await;
    outcome.expect("isolated runtime PostgreSQL test task must complete");
}

fn runtime_panel_role_suffix(database_name: &str) -> String {
    let hash = database_name
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    format!("{hash:016x}")
}

async fn drop_runtime_database(database: RuntimeTestDatabase) {
    let role_suffix = runtime_panel_role_suffix(&database.name);
    let cleanup_roles = [
        format!("runtime_panel_login_{role_suffix}"),
        format!("runtime_panel_grant_{role_suffix}"),
    ];
    sqlx::query(
        "SELECT pg_catalog.lo_unlink(large_object.oid) \
         FROM pg_catalog.pg_largeobject_metadata AS large_object",
    )
    .execute(&database.pool)
    .await
    .unwrap();
    for role in &cleanup_roles {
        let exists = sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regrole($1) IS NOT NULL")
            .bind(role)
            .fetch_one(&database.pool)
            .await
            .unwrap();
        if exists {
            sqlx::query(&format!("DROP OWNED BY {role}"))
                .execute(&database.pool)
                .await
                .unwrap();
        }
    }
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
    for role in cleanup_roles {
        sqlx::query(&format!("DROP ROLE IF EXISTS {role}"))
            .execute(&mut administrator)
            .await
            .unwrap();
    }
}

async fn controller_mutate(
    adapter: &PostgresRuntimeConvergence,
    session: &mut RuntimeConvergenceSessionV1,
    mutation: RuntimeConvergenceMutationV1,
) {
    let request = session.begin_mutation(mutation).unwrap();
    let receipt = <PostgresRuntimeConvergence as RuntimeConvergencePort>::mutate(adapter, request)
        .await
        .unwrap();
    session.apply_mutation(receipt).unwrap();
}

async fn advance_to_panel_reconciliation(
    pool: &PgPool,
) -> (
    PostgresRuntimeConvergence,
    RuntimeConvergenceSessionV1,
    RuntimeExactTargetV1,
) {
    seed_product_target(pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let execution = <PostgresRuntimeConvergence as RuntimeConvergencePort>::claim_next_execution(
        &adapter,
        RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse("runtime-panel-controller").unwrap(),
            lease_for: Duration::from_secs(90),
        },
    )
    .await
    .unwrap()
    .expect("seeded deployment must be claimable");
    let mut session = RuntimeConvergenceSessionV1::from_claim(execution).unwrap();
    let observed_at = session.acquired_at();
    controller_mutate(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            observed_runtime: None,
            checked_at: observed_at,
        }),
    )
    .await;
    controller_mutate(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::RequestDrain,
    )
    .await;
    controller_mutate(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: None,
            target_runtime_generation: RuntimeGeneration::FIRST,
            drained_at: observed_at,
        }),
    )
    .await;
    controller_mutate(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::BeginActivation,
    )
    .await;
    controller_mutate(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptActivation(ActivationAttestationV1 {
            activation_request_id: ActivationRequestId::parse(ACTIVATION).unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            kind: ActivationOutcomeKindV1::AlreadyActive,
            activated_at: observed_at,
        }),
    )
    .await;
    controller_mutate(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::BeginPanelReconciliation,
    )
    .await;
    assert!(matches!(
        session.snapshot().phase,
        RuntimeDeploymentPhaseV1::ReconcilingPanels
    ));
    let execution = session.current_execution_receipt().unwrap();
    let exact = PostgresRuntimeExactTargetReader::new(pool.clone())
        .load_for_execution(&execution)
        .await
        .unwrap();
    (adapter, session, exact)
}

fn panel_operation(state: StrictPanelOperationStateV1) -> StrictPanelOperationV1 {
    StrictPanelOperationV1 {
        key: StrictPanelOperationKeyV1 {
            guild_id: GUILD,
            ruleset_key: RULESET.parse().unwrap(),
            panel_key: PANEL_KEY.to_string(),
        },
        state,
    }
}

fn post_intent() -> StrictPanelPostIntentV1 {
    StrictPanelPostIntentV1 {
        panel: StrictDeclaredPanelV1 {
            spec: PanelSpec {
                key: PANEL_KEY.to_string(),
                channel: ResourceKey("hub".to_string()),
                content: "Welcome".to_string(),
                buttons: Vec::new(),
            },
            expected_payload: StrictPanelMessagePayloadV1 {
                content: "Welcome".to_string(),
                action_rows: Vec::new(),
            },
        },
        ruleset_version: RuleSetVersionId::FIRST,
        channel_id: ChannelId(9200601),
        spec_hash: SPEC_HASH.to_string(),
        install_kind: StrictPanelInstallKindV1::Fresh,
        previous_message: None,
    }
}

fn installation() -> PanelInstallation {
    PanelInstallation {
        guild_id: GUILD,
        ruleset_key: RULESET.parse().unwrap(),
        panel_key: PANEL_KEY.to_string(),
        installed_version: RuleSetVersionId::FIRST,
        channel_id: ChannelId(9200601),
        message_id: MessageId(9200701),
        spec_hash: SPEC_HASH.to_string(),
    }
}

fn installation_key() -> PanelInstallationKey {
    PanelInstallationKey {
        guild_id: GUILD,
        ruleset_key: RULESET.parse().unwrap(),
        panel_key: PANEL_KEY.to_string(),
    }
}

async fn claim_primed_store(
    pool: &PgPool,
    session: &RuntimeConvergenceSessionV1,
    exact: RuntimeExactTargetV1,
) -> PostgresFencedStrictPanelStoreV1 {
    let store = PostgresFencedStrictPanelStoreV1::claim(
        pool.clone(),
        session.execution_guard().unwrap(),
        exact,
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    store.prime().await.unwrap();
    store
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn claim_replay_busy_post_dispatch_conflict_and_stale_fence_are_enforced() {
    run_migrated_runtime_database_test("session_fence", |pool, _| async move {
        let (adapter, mut controller, exact) = advance_to_panel_reconciliation(&pool).await;
        let session_id = RuntimePanelSessionIdV1::generate().unwrap();
        let store = PostgresFencedStrictPanelStoreV1::claim_with_session_id(
            pool.clone(),
            controller.execution_guard().unwrap(),
            exact.clone(),
            Duration::from_secs(5),
            &session_id,
        )
        .await
        .unwrap();
        store.prime().await.unwrap();
        let replay = PostgresFencedStrictPanelStoreV1::claim_with_session_id(
            pool.clone(),
            controller.execution_guard().unwrap(),
            exact.clone(),
            Duration::from_secs(5),
            &session_id,
        )
        .await
        .unwrap();
        replay.prime().await.unwrap();
        assert_eq!(store.receipt().session_id, replay.receipt().session_id);
        assert_eq!(
            store.receipt().session_record_revision,
            replay.receipt().session_record_revision
        );
        assert_eq!(
            store.receipt().controller_lease_expires_at,
            replay.receipt().controller_lease_expires_at
        );
        assert!(replay.receipt().checked_at >= store.receipt().checked_at);
        let other_session = RuntimePanelSessionIdV1::generate().unwrap();
        let busy = match PostgresFencedStrictPanelStoreV1::claim_with_session_id(
            pool.clone(),
            controller.execution_guard().unwrap(),
            exact,
            Duration::from_secs(5),
            &other_session,
        )
        .await
        {
            Ok(_) => panic!("a different panel session must not steal the occupied slot"),
            Err(error) => error,
        };
        assert_eq!(busy, RuntimePanelPersistenceErrorV1::Conflict);
        let dispatch = panel_operation(StrictPanelOperationStateV1::PostDispatching {
            intent: post_intent(),
        });
        StrictPanelOperationJournal::put(&store, dispatch.clone())
            .await
            .unwrap();
        let replay_conflict = StrictPanelOperationJournal::put(&store, dispatch)
            .await
            .unwrap_err();
        assert_eq!(replay_conflict.0, "runtime_panel_conflict");
        assert_eq!(
            store.latched_error().await,
            Some(RuntimePanelLatchedErrorV1::Conflict)
        );
        let renewal = controller.begin_renewal(Duration::from_secs(120)).unwrap();
        let renewed = <PostgresRuntimeConvergence as RuntimeConvergencePort>::renew_execution(
            &adapter, renewal,
        )
        .await
        .unwrap();
        controller.apply_renewal(renewed).unwrap();
        assert_eq!(
            replay
                .check_session(Duration::from_millis(1))
                .await
                .unwrap_err(),
            RuntimePanelPersistenceErrorV1::OwnershipLost
        );
        assert_eq!(
            replay.latched_error().await,
            Some(RuntimePanelLatchedErrorV1::OwnershipLost)
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn journal_and_installation_compare_and_swap_flow_is_legal() {
    run_migrated_runtime_database_test("cas_flow", |pool, _| async move {
        let (_, controller, exact) = advance_to_panel_reconciliation(&pool).await;
        let store = claim_primed_store(&pool, &controller, exact).await;
        let intent = post_intent();
        StrictPanelOperationJournal::put(
            &store,
            panel_operation(StrictPanelOperationStateV1::PostDispatching {
                intent: intent.clone(),
            }),
        )
        .await
        .unwrap();
        StrictPanelOperationJournal::put(
            &store,
            panel_operation(StrictPanelOperationStateV1::PostApplied {
                intent,
                message_id: MessageId(9200701),
            }),
        )
        .await
        .unwrap();
        PanelInstallationStore::upsert(&store, installation())
            .await
            .unwrap();
        StrictPanelOperationJournal::remove(
            &store,
            &StrictPanelOperationKeyV1 {
                guild_id: GUILD,
                ruleset_key: RULESET.parse().unwrap(),
                panel_key: PANEL_KEY.to_string(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            StrictPanelInstallationStore::list_slot(&store, GUILD, &RULESET.parse().unwrap())
                .await
                .unwrap(),
            vec![installation()]
        );
        StrictPanelOperationJournal::put(
            &store,
            panel_operation(StrictPanelOperationStateV1::CleanupPending {
                intent: StrictPanelCleanupIntentV1 {
                    message: StrictPanelMessageRefV1 {
                        channel_id: ChannelId(9200601),
                        message_id: MessageId(9200701),
                    },
                    kind: StrictPanelCleanupKindV1::Removed,
                    remove_installation: true,
                },
            }),
        )
        .await
        .unwrap();
        StrictPanelInstallationStore::remove(&store, &installation_key())
            .await
            .unwrap();
        StrictPanelOperationJournal::remove(
            &store,
            &StrictPanelOperationKeyV1 {
                guild_id: GUILD,
                ruleset_key: RULESET.parse().unwrap(),
                panel_key: PANEL_KEY.to_string(),
            },
        )
        .await
        .unwrap();
        assert!(
            StrictPanelInstallationStore::list_slot(&store, GUILD, &RULESET.parse().unwrap())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            StrictPanelOperationJournal::list_slot(&store, GUILD, &RULESET.parse().unwrap())
                .await
                .unwrap()
                .is_empty()
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn current_authority_drift_is_distinct_from_execution_ownership_loss() {
    run_migrated_runtime_database_test("authority_drift", |pool, _| async move {
        let (_, controller, exact) = advance_to_panel_reconciliation(&pool).await;
        let store = claim_primed_store(&pool, &controller, exact).await;
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
             SELECT installation_id, 2, tenant_id, binding_revision, resource_bindings, \
             binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
             $2, created_by_principal_id, $3 \
             FROM public.automation_installation_authority_versions \
             WHERE installation_id = $1 AND revision = 1",
        )
        .bind(INSTALLATION)
        .bind("5".repeat(64))
        .bind("6".repeat(64))
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE public.automation_installations SET current_authority_revision = 2, \
             updated_at = GREATEST(pg_catalog.clock_timestamp(), \
              updated_at + INTERVAL '1 microsecond') WHERE installation_id = $1",
        )
        .bind(INSTALLATION)
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            store
                .check_session(Duration::from_millis(1))
                .await
                .unwrap_err(),
            RuntimePanelPersistenceErrorV1::AuthorityChanged
        );
        assert_eq!(
            store.last_error_class().await,
            Some(RuntimePanelErrorClassV1::AuthorityChanged)
        );
        assert_eq!(
            store.latched_error().await,
            Some(RuntimePanelLatchedErrorV1::AuthorityChanged)
        );
    })
    .await;
}

async fn restricted_claim(
    connection: &mut PgConnection,
    guard: &RuntimeExecutionGuardV1,
    exact: &RuntimeExactTargetV1,
    session_id: &RuntimePanelSessionIdV1,
) {
    let target = &exact.snapshot.target;
    let rows = sqlx::query(
        "SELECT * FROM public.starring_runtime_panel_reconciliation_claim_v1(\
         $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(guard.expected_revision.get() as i64)
    .bind(guard.controller_id.as_str())
    .bind(guard.fencing_token.get() as i64)
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(guard.runtime_generation.get() as i64)
    .bind(target.guild_id.to_string())
    .bind(target.ruleset_key.as_str())
    .bind(i64::from(target.version.get()))
    .bind(target.content_hash.to_hex())
    .bind(target.binding_revision.get() as i64)
    .bind(target.binding_fingerprint.as_str())
    .bind(exact.installation_authority_revision as i64)
    .bind(exact.current_authority_revision as i64)
    .bind(session_id.as_str())
    .fetch_all(connection)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
}

async fn restricted_private_lock_error(
    connection: &mut PgConnection,
    guard: &RuntimeExecutionGuardV1,
    exact: &RuntimeExactTargetV1,
) -> sqlx::Error {
    let target = &exact.snapshot.target;
    sqlx::query(
        "SELECT * FROM public.starring_runtime_panel_execution_lock_v1(\
         $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(guard.expected_revision.get() as i64)
    .bind(guard.controller_id.as_str())
    .bind(guard.fencing_token.get() as i64)
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(guard.runtime_generation.get() as i64)
    .bind(target.guild_id.to_string())
    .bind(target.ruleset_key.as_str())
    .bind(i64::from(target.version.get()))
    .bind(target.content_hash.to_hex())
    .bind(target.binding_revision.get() as i64)
    .bind(target.binding_fingerprint.as_str())
    .bind(exact.installation_authority_revision as i64)
    .bind(exact.current_authority_revision as i64)
    .execute(connection)
    .await
    .unwrap_err()
}

fn database_error_code(error: &sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned())
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn restricted_executor_has_only_granted_panel_capabilities() {
    run_migrated_runtime_database_test("restricted_acl", |pool, _| async move {
        let (_, controller, exact) = advance_to_panel_reconciliation(&pool).await;
        let role = format!(
            "runtime_panel_executor_{}",
            &RuntimePanelSessionIdV1::generate().unwrap().as_str()[..16]
        );
        sqlx::query(&format!(
            "CREATE ROLE {role} NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB \
             NOCREATEROLE NOREPLICATION NOBYPASSRLS"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {role}"))
            .execute(&pool)
            .await
            .unwrap();
        for capability in PANEL_CAPABILITIES {
            sqlx::query(&format!("GRANT EXECUTE ON FUNCTION {capability} TO {role}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        let mut restricted = pool.acquire().await.unwrap();
        sqlx::query(&format!("SET ROLE {role}"))
            .execute(&mut *restricted)
            .await
            .unwrap();
        for capability in PANEL_CAPABILITIES {
            assert!(sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_function_privilege(current_user, $1, 'EXECUTE')",
            )
            .bind(capability)
            .fetch_one(&mut *restricted)
            .await
            .unwrap());
        }
        for capability in PRIVATE_PANEL_CAPABILITIES {
            assert!(!sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_function_privilege(current_user, $1, 'EXECUTE')",
            )
            .bind(capability)
            .fetch_one(&mut *restricted)
            .await
            .unwrap());
        }
        for relation in [
            "public.runtime_panel_reconciliation_sessions",
            "public.ruleset_panel_installations",
            "public.strict_panel_operation_journal",
        ] {
            for privilege in ["SELECT", "INSERT", "UPDATE", "DELETE"] {
                assert!(!sqlx::query_scalar::<_, bool>(
                    "SELECT pg_catalog.has_table_privilege(current_user, $1, $2)",
                )
                .bind(relation)
                .bind(privilege)
                .fetch_one(&mut *restricted)
                .await
                .unwrap());
            }
        }
        let guard = controller.execution_guard().unwrap();
        restricted_claim(
            &mut restricted,
            &guard,
            &exact,
            &RuntimePanelSessionIdV1::generate().unwrap(),
        )
        .await;
        let dml_error = sqlx::query("SELECT * FROM public.runtime_panel_reconciliation_sessions")
            .execute(&mut *restricted)
            .await
            .unwrap_err();
        assert_eq!(database_error_code(&dml_error).as_deref(), Some("42501"));
        let private_error = restricted_private_lock_error(&mut restricted, &guard, &exact).await;
        assert_eq!(
            database_error_code(&private_error).as_deref(),
            Some("42501")
        );
        sqlx::query("RESET ROLE")
            .execute(&mut *restricted)
            .await
            .unwrap();
        drop(restricted);
        sqlx::query(&format!("DROP OWNED BY {role}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&pool)
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn panel_store_lock_wait_is_bounded_by_transaction_local_timeout() {
    run_migrated_runtime_database_test("bounded_lock", |pool, _| async move {
        let (_, controller, exact) = advance_to_panel_reconciliation(&pool).await;
        let guard = controller.execution_guard().unwrap();
        let mut blocker = pool.begin().await.unwrap();
        sqlx::query(
            "SELECT deployment_id FROM public.runtime_deployments \
             WHERE deployment_id = $1 FOR UPDATE",
        )
        .bind(guard.scope.deployment_id.as_str())
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
        let error = match PostgresFencedStrictPanelStoreV1::claim_with_timeouts(
            pool.clone(),
            guard.clone(),
            exact.clone(),
            Duration::from_secs(1),
            RuntimePanelDatabaseTimeoutsV1::new(
                Duration::from_millis(500),
                Duration::from_millis(50),
            )
            .unwrap(),
        )
        .await
        {
            Ok(_) => panic!("a blocked panel claim must time out"),
            Err(error) => error,
        };
        assert_eq!(error, RuntimePanelPersistenceErrorV1::Timeout);
        blocker.rollback().await.unwrap();
        let store = PostgresFencedStrictPanelStoreV1::claim_with_timeouts(
            pool,
            guard,
            exact,
            Duration::from_secs(1),
            RuntimePanelDatabaseTimeoutsV1::new(
                Duration::from_millis(500),
                Duration::from_millis(50),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        store.prime().await.unwrap();
    })
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn restricted_login_readiness_enforces_exact_panel_boundary() {
    run_migrated_runtime_database_test("readiness_acl", |pool, options| async move {
        let database_name = options.get_database().unwrap().to_string();
        let role_suffix = runtime_panel_role_suffix(&database_name);
        let role = format!("runtime_panel_login_{role_suffix}");
        let grant_target = format!("runtime_panel_grant_{role_suffix}");
        let sequence = format!("runtime_panel_sequence_{role_suffix}");
        let extra_schema = format!("runtime_panel_extra_{role_suffix}");
        let aggregate_secret = format!("runtime_panel_secret_{role_suffix}");
        let aggregate_transition = format!("runtime_panel_transition_{role_suffix}");
        let aggregate = format!("runtime_panel_aggregate_{role_suffix}");
        let system_function = format!("runtime_panel_function_{role_suffix}");
        let system_type = format!("runtime_panel_type_{role_suffix}");
        let password = RuntimePanelSessionIdV1::generate().unwrap().to_string();
        sqlx::query(&format!(
            "REVOKE CONNECT, TEMPORARY ON DATABASE {database_name} FROM PUBLIC"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "CREATE ROLE {role} LOGIN PASSWORD '{password}' NOINHERIT NOSUPERUSER NOCREATEDB \
             NOCREATEROLE NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "CREATE ROLE {grant_target} NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB \
             NOCREATEROLE NOREPLICATION NOBYPASSRLS"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!("CREATE SEQUENCE public.{sequence}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {role}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!(
            "GRANT CONNECT ON DATABASE {database_name} TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        for capability in PANEL_CAPABILITIES {
            sqlx::query(&format!("GRANT EXECUTE ON FUNCTION {capability} TO {role}"))
                .execute(&pool)
                .await
                .unwrap();
        }

        let database_identity = sqlx::query_scalar::<_, String>(
            "SELECT identity.database_identity::TEXT \
             FROM public.product_control_plane_identity AS identity \
             WHERE identity.singleton",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let expectation = RuntimePanelDatabaseExpectationV1::new(
            database_identity.clone(),
            database_name.clone(),
            role.clone(),
        )
        .unwrap();
        let restricted_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone().username(&role).password(&password))
            .await
            .unwrap();
        let settings_before = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT pg_catalog.current_setting('statement_timeout'), \
                    pg_catalog.current_setting('lock_timeout'), \
                    pg_catalog.current_setting('idle_in_transaction_session_timeout'), \
                    pg_catalog.current_setting('search_path')",
        )
        .fetch_one(&restricted_pool)
        .await
        .unwrap();
        let readiness = verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::new(
                Duration::from_millis(900),
                Duration::from_millis(100),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(readiness.database_identity, database_identity);
        assert_eq!(readiness.database_name, database_name);
        assert_eq!(readiness.executor_role, role);
        let settings_after = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT pg_catalog.current_setting('statement_timeout'), \
                    pg_catalog.current_setting('lock_timeout'), \
                    pg_catalog.current_setting('idle_in_transaction_session_timeout'), \
                    pg_catalog.current_setting('search_path')",
        )
        .fetch_one(&restricted_pool)
        .await
        .unwrap();
        assert_eq!(settings_after, settings_before);

        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_table_privilege($1, 'pg_catalog.pg_authid', 'SELECT')",
        )
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap());
        sqlx::query(&format!(
            "GRANT SELECT ON TABLE pg_catalog.pg_authid TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_table_privilege($1, 'pg_catalog.pg_authid', 'SELECT')",
        )
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap());
        let visible_role_count =
            sqlx::query_scalar::<_, i64>("SELECT pg_catalog.count(*) FROM pg_catalog.pg_authid")
                .fetch_one(&restricted_pool)
                .await
                .unwrap();
        assert!(visible_role_count > 0);
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE SELECT ON TABLE pg_catalog.pg_authid FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
        .unwrap();

        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_table_privilege( \
             $1, 'pg_catalog.pg_shadow', 'SELECT')",
        )
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap());
        sqlx::query("GRANT SELECT ON TABLE pg_catalog.pg_shadow TO PUBLIC")
            .execute(&pool)
            .await
            .unwrap();
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_table_privilege( \
             $1, 'pg_catalog.pg_shadow', 'SELECT')",
        )
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap());
        let public_shadow_count =
            sqlx::query_scalar::<_, i64>("SELECT pg_catalog.count(*) FROM pg_catalog.pg_shadow")
                .fetch_one(&restricted_pool)
                .await
                .unwrap();
        assert!(public_shadow_count > 0);
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query("REVOKE SELECT ON TABLE pg_catalog.pg_shadow FROM PUBLIC")
            .execute(&pool)
            .await
            .unwrap();
        verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
        .unwrap();

        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_table_privilege( \
             $1, 'information_schema._pg_user_mappings', 'SELECT')",
        )
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap());
        sqlx::query("GRANT SELECT ON TABLE information_schema._pg_user_mappings TO PUBLIC")
            .execute(&pool)
            .await
            .unwrap();
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_table_privilege( \
             $1, 'information_schema._pg_user_mappings', 'SELECT')",
        )
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap());
        let _ = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM information_schema._pg_user_mappings",
        )
        .fetch_one(&restricted_pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query("REVOKE SELECT ON TABLE information_schema._pg_user_mappings FROM PUBLIC")
            .execute(&pool)
            .await
            .unwrap();
        verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
        .unwrap();

        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege( \
             $1, 'pg_catalog.pg_current_logfile()', 'EXECUTE')",
        )
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap());
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION pg_catalog.pg_current_logfile() TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege( \
             $1, 'pg_catalog.pg_current_logfile()', 'EXECUTE')",
        )
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap());
        let _ = sqlx::query_scalar::<_, Option<String>>(
            "SELECT pg_catalog.pg_current_logfile()::TEXT",
        )
        .fetch_one(&restricted_pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION pg_catalog.pg_current_logfile() FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
        .unwrap();

        sqlx::query(&format!(
            "CREATE FUNCTION pg_catalog.{system_function}() RETURNS BIGINT \
             LANGUAGE sql AS 'SELECT 7::BIGINT'"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION pg_catalog.{system_function}() TO PUBLIC"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT function_row.oid >= 16384 \
             AND NOT EXISTS ( \
              SELECT 1 FROM pg_catalog.pg_init_privs AS initial \
              WHERE initial.classoid = 'pg_catalog.pg_proc'::REGCLASS \
               AND initial.objoid = function_row.oid \
               AND initial.objsubid = 0 \
             ) \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(format!("pg_catalog.{system_function}()"))
        .fetch_one(&pool)
        .await
        .unwrap());
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
        )
        .bind(&role)
        .bind(format!("pg_catalog.{system_function}()"))
        .fetch_one(&pool)
        .await
        .unwrap());
        let system_function_result = sqlx::query_scalar::<_, i64>(&format!(
            "SELECT pg_catalog.{system_function}()"
        ))
        .fetch_one(&restricted_pool)
        .await
        .unwrap();
        assert_eq!(system_function_result, 7);
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "DROP FUNCTION pg_catalog.{system_function}()"
        ))
        .execute(&pool)
        .await
        .unwrap();
        verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
        .unwrap();

        sqlx::query(&format!(
            "CREATE DOMAIN pg_catalog.{system_type} AS TEXT"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT type_row.oid >= 16384 \
             AND NOT EXISTS ( \
              SELECT 1 FROM pg_catalog.pg_init_privs AS initial \
              WHERE initial.classoid = 'pg_catalog.pg_type'::REGCLASS \
               AND initial.objoid = type_row.oid \
               AND initial.objsubid = 0 \
             ) \
             FROM pg_catalog.pg_type AS type_row \
             WHERE type_row.oid = pg_catalog.to_regtype($1)",
        )
        .bind(format!("pg_catalog.{system_type}"))
        .fetch_one(&pool)
        .await
        .unwrap());
        let system_type_result = sqlx::query_scalar::<_, String>(&format!(
            "SELECT 'probe'::pg_catalog.{system_type}::TEXT"
        ))
        .fetch_one(&restricted_pool)
        .await
        .unwrap();
        assert_eq!(system_type_result, "probe");
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!("DROP DOMAIN pg_catalog.{system_type}"))
            .execute(&pool)
            .await
            .unwrap();
        verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
        .unwrap();

        let function_contracts_valid = sqlx::query_scalar::<_, bool>(
            "SELECT NOT EXISTS ( \
               SELECT 1 \
               FROM pg_catalog.unnest($1::TEXT[]) AS expected(identity) \
               LEFT JOIN pg_catalog.pg_proc AS function_row \
                ON function_row.oid = pg_catalog.to_regprocedure(expected.identity) \
               LEFT JOIN pg_catalog.pg_class AS identity_relation \
                ON identity_relation.oid = pg_catalog.to_regclass( \
                 'public.product_control_plane_identity') \
               WHERE function_row.oid IS NULL \
                OR function_row.proowner <> identity_relation.relowner \
                OR NOT function_row.prosecdef \
                OR function_row.proconfig \
                 IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[] \
                OR EXISTS ( \
                 SELECT 1 \
                 FROM pg_catalog.aclexplode(COALESCE( \
                  function_row.proacl, \
                  pg_catalog.acldefault('f', function_row.proowner) \
                 )) AS privilege \
                 WHERE privilege.grantee = 0 \
                ) \
              )",
        )
        .bind(
            PANEL_CAPABILITIES
                .into_iter()
                .chain(PRIVATE_PANEL_CAPABILITIES)
                .collect::<Vec<_>>(),
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(function_contracts_valid);
        let readiness_signature = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_function_result(function_row.oid) \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure( \
              'public.starring_runtime_panel_database_readiness_v1()')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            readiness_signature,
            "TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)"
        );

        sqlx::query(&format!(
            "CREATE TABLE public.{aggregate_secret}(value BIGINT NOT NULL)"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "INSERT INTO public.{aggregate_secret}(value) VALUES (41)"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON TABLE public.{aggregate_secret} FROM PUBLIC, {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "CREATE FUNCTION public.{aggregate_transition}(state_value BIGINT, input_value BIGINT) \
             RETURNS BIGINT LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog \
             AS $body$ BEGIN RETURN state_value + input_value + ( \
              SELECT secret.value FROM public.{aggregate_secret} AS secret \
             ); END $body$"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION \
             public.{aggregate_transition}(BIGINT, BIGINT) FROM PUBLIC, {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "CREATE AGGREGATE public.{aggregate}(BIGINT) ( \
             SFUNC = public.{aggregate_transition}, STYPE = BIGINT, INITCOND = '0' \
             )"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION public.{aggregate}(BIGINT) TO PUBLIC"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_table_privilege($1, $2, 'SELECT')",
        )
        .bind(&role)
        .bind(format!("public.{aggregate_secret}"))
        .fetch_one(&pool)
        .await
        .unwrap());
        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
        )
        .bind(&role)
        .bind(format!(
            "public.{aggregate_transition}(bigint,bigint)"
        ))
        .fetch_one(&pool)
        .await
        .unwrap());
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
        )
        .bind(&role)
        .bind(format!("public.{aggregate}(bigint)"))
        .fetch_one(&pool)
        .await
        .unwrap());
        let leaked = sqlx::query_scalar::<_, i64>(&format!(
            "SELECT public.{aggregate}(input.value) \
             FROM (VALUES (1::BIGINT)) AS input(value)"
        ))
        .fetch_one(&restricted_pool)
        .await
        .unwrap();
        assert_eq!(leaked, 42);
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "DROP AGGREGATE public.{aggregate}(BIGINT)"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "DROP FUNCTION public.{aggregate_transition}(BIGINT, BIGINT)"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!("DROP TABLE public.{aggregate_secret}"))
            .execute(&pool)
            .await
            .unwrap();
        verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
        .unwrap();

        let large_object_oid = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.lo_from_bytea( \
              0, pg_catalog.convert_to('panel-secret', 'UTF8') \
             )::BIGINT",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT SELECT ON LARGE OBJECT {large_object_oid} TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let large_object_secret = sqlx::query_scalar::<_, String>(&format!(
            "SELECT pg_catalog.convert_from( \
              pg_catalog.lo_get({large_object_oid}::OID), 'UTF8' \
             )"
        ))
        .fetch_one(&restricted_pool)
        .await
        .unwrap();
        assert_eq!(large_object_secret, "panel-secret");
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE SELECT ON LARGE OBJECT {large_object_oid} FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("SELECT pg_catalog.lo_unlink($1::OID)")
            .bind(large_object_oid as i32)
            .execute(&pool)
            .await
            .unwrap();
        verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
        .unwrap();

        let mut restricted = restricted_pool.acquire().await.unwrap();
        for sql in [
            "SELECT * FROM public.product_control_plane_identity",
            "SELECT * FROM public.runtime_panel_reconciliation_sessions",
            "INSERT INTO public.runtime_panel_reconciliation_sessions DEFAULT VALUES",
            "UPDATE public.runtime_panel_reconciliation_sessions SET guild_id = guild_id WHERE FALSE",
            "DELETE FROM public.runtime_panel_reconciliation_sessions WHERE FALSE",
        ] {
            let error = sqlx::query(sql)
                .execute(&mut *restricted)
                .await
                .unwrap_err();
            assert_eq!(database_error_code(&error).as_deref(), Some("42501"));
        }
        for sql in [
            format!("SELECT pg_catalog.nextval('public.{sequence}'::pg_catalog.regclass)"),
            format!("SELECT last_value FROM public.{sequence}"),
            format!(
                "SELECT pg_catalog.setval('public.{sequence}'::pg_catalog.regclass, 1, FALSE)"
            ),
        ] {
            let error = sqlx::query(&sql)
                .execute(&mut *restricted)
                .await
                .unwrap_err();
            assert_eq!(database_error_code(&error).as_deref(), Some("42501"));
        }
        for sql in [
            "SELECT * FROM public.starring_runtime_panel_execution_lock_v1(NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
            "SELECT * FROM public.starring_runtime_exact_target_reader_database_identity_v1()",
        ] {
            let error = sqlx::query(sql)
                .execute(&mut *restricted)
                .await
                .unwrap_err();
            assert_eq!(database_error_code(&error).as_deref(), Some("42501"));
        }
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {} TO {grant_target}",
            PANEL_CAPABILITIES[0]
        ))
        .execute(&mut *restricted)
        .await
        .unwrap();
        drop(restricted);
        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
        )
        .bind(&grant_target)
        .bind(PANEL_CAPABILITIES[0])
        .fetch_one(&pool)
        .await
        .unwrap());
        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT COALESCE(pg_catalog.bool_or(privilege.is_grantable), FALSE) \
             FROM pg_catalog.pg_proc AS function_row \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
              function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
             )) AS privilege \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
              AND privilege.grantee = pg_catalog.to_regrole($2)",
        )
        .bind(PANEL_CAPABILITIES[0])
        .bind(&role)
        .fetch_one(&pool)
        .await
        .unwrap());

        let wrong_expectation = RuntimePanelDatabaseExpectationV1::new(
            "11234567-89ab-cdef-8123-456789abcdef",
            &database_name,
            &role,
        )
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &wrong_expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::InvalidAuthority)
        );

        sqlx::query(&format!("ALTER ROLE {role} INHERIT"))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!("ALTER ROLE {role} NOINHERIT"))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(&format!("ALTER ROLE {role} CONNECTION LIMIT -1"))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!("ALTER ROLE {role} CONNECTION LIMIT 4"))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(&format!("GRANT {grant_target} TO {role}"))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!("REVOKE {grant_target} FROM {role}"))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(&format!("GRANT {role} TO {grant_target}"))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!("REVOKE {role} FROM {grant_target}"))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(&format!(
            "ALTER ROLE {role} SET statement_timeout = '5s'"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!("ALTER ROLE {role} RESET statement_timeout"))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(&format!(
            "ALTER DATABASE {database_name} SET statement_timeout = '5s'"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "ALTER DATABASE {database_name} RESET statement_timeout"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "REVOKE CONNECT ON DATABASE {database_name} FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "GRANT CONNECT ON DATABASE {database_name} TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT CONNECT ON DATABASE {database_name} TO {role} WITH GRANT OPTION"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE GRANT OPTION FOR CONNECT ON DATABASE {database_name} FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT TEMPORARY ON DATABASE {database_name} TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE TEMPORARY ON DATABASE {database_name} FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!("GRANT CREATE ON SCHEMA public TO {role}"))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!("REVOKE CREATE ON SCHEMA public FROM {role}"))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(&format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             GRANT SELECT ON TABLES TO {grant_target}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             REVOKE SELECT ON TABLES FROM {grant_target}"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION \
             public.starring_runtime_exact_target_reader_database_identity_v1() TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION \
             public.starring_runtime_exact_target_reader_database_identity_v1() FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SELECT ON TABLE public.runtime_panel_reconciliation_sessions TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE SELECT ON TABLE public.runtime_panel_reconciliation_sessions FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SET ON PARAMETER log_statement TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE SET ON PARAMETER log_statement FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!("CREATE SCHEMA {extra_schema}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("GRANT USAGE ON SCHEMA {extra_schema} TO {role}"))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!("REVOKE USAGE ON SCHEMA {extra_schema} FROM {role}"))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(&format!("CREATE TABLE {extra_schema}.probe(value BIGINT)"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!(
            "GRANT SELECT ON TABLE {extra_schema}.probe TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE SELECT ON TABLE {extra_schema}.probe FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!("CREATE SEQUENCE {extra_schema}.probe_sequence"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!(
            "GRANT USAGE ON SEQUENCE {extra_schema}.probe_sequence TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE USAGE ON SEQUENCE {extra_schema}.probe_sequence FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "CREATE FUNCTION {extra_schema}.probe() RETURNS BIGINT \
             LANGUAGE sql AS 'SELECT 1::BIGINT'"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION {extra_schema}.probe() FROM PUBLIC"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {extra_schema}.probe() TO {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            verify_runtime_panel_database_with_timeouts_v1(
                &restricted_pool,
                &expectation,
                RuntimePanelDatabaseTimeoutsV1::default(),
            )
            .await,
            Err(RuntimePanelPersistenceErrorV1::PersistenceCorrupt)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {extra_schema}.probe() FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(&format!("DROP SCHEMA {extra_schema} CASCADE"))
            .execute(&pool)
            .await
            .unwrap();
        verify_runtime_panel_database_with_timeouts_v1(
            &restricted_pool,
            &expectation,
            RuntimePanelDatabaseTimeoutsV1::default(),
        )
        .await
        .unwrap();

        restricted_pool.close().await;
        sqlx::query(&format!("DROP OWNED BY {role}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("DROP ROLE {grant_target}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SEQUENCE public.{sequence}"))
            .execute(&pool)
            .await
            .unwrap();
    })
    .await;
}
