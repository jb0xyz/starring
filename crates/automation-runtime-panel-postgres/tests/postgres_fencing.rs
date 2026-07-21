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
    PostgresFencedStrictPanelStoreV1, RuntimePanelErrorClassV1, RuntimePanelLatchedErrorV1,
    RuntimePanelPersistenceErrorV1, RuntimePanelSessionIdV1,
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

const PANEL_CAPABILITIES: [&str; 7] = [
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

async fn drop_runtime_database(database: RuntimeTestDatabase) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
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
