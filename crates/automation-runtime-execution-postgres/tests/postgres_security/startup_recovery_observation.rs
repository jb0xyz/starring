use std::num::NonZeroU64;
use std::time::Instant;

use automation_runtime_controller::{
    RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseReceiptV1,
    RuntimeGatewayReadyKindV2, RuntimeRecoveryIdV2,
};
use automation_runtime_worker::{
    accept_runtime_registry_recovery_empty_observation_v2, RuntimeAcceptedStartupRecoveryOutcomeV2,
    RuntimeCapabilityReadinessKindV2, RuntimeCapabilityReadinessReceiptV2,
    RuntimeCapabilityReadinessSetV2, RuntimeClosedDrainRecoveryPermitV2,
    RuntimeClosedRecoveryInputV2, RuntimeClosedRecoveryRegistryEvidenceV2,
    RuntimeGatewayClosedLifecycleV2, RuntimePausedGatewayObservationV2,
    RuntimePausedGatewaySequenceV2, RuntimeRegistryGlobalObservationSequenceV2,
    RuntimeRegistryRecoveryObservationInputV2, RuntimeStartupRecoveryObservationPortV2,
};

type StartupObservationOwnerTuple = (String, String, i64, String, i64, DateTime<Utc>);

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_is_exact_serializable_and_executor_only() {
    let server = PostgresTestServer::start();
    let mut database = isolated_database(server.connect_options()).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;

    let read_committed = startup_observation_query()
        .bind(&owner.0)
        .bind(&owner.1)
        .bind(owner.2)
        .bind(&owner.3)
        .bind(owner.4)
        .bind(owner.5)
        .fetch_one(&database.executor_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&read_committed, "RX004");

    let observed = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(observed["outcome_name"], "observed");
    assert_eq!(observed["observed_gateway_shard_id"], owner.0);
    assert_eq!(observed["observed_process_instance_id"], owner.1);
    assert_eq!(observed["observed_lease_epoch"], owner.2);
    assert_eq!(observed["observed_runtime_build_revision"], owner.3);
    assert_eq!(observed["observed_owner_revision"], owner.4);
    assert_eq!(observed["serving_state_name"], "empty");
    assert_eq!(observed["serving_count"], 0);
    assert!(observed["serving_earliest_expiry"].is_null());
    assert!(observed["serving_retry_after_milliseconds"].is_null());
    assert_eq!(observed["recoverable_awaiting_certification_count"], 0);
    assert_eq!(observed["suspended_local_effect_count"], 0);
    assert_eq!(observed["pending_runtime_drain_intent_count"], 0);
    assert_eq!(observed["acknowledged_product_handoff_count"], 0);

    let not_current = observe_startup_state(&database.executor_pool, &owner, owner.4 + 1).await;
    assert_eq!(not_current["outcome_name"], "not_current");
    assert!(not_current["serving_state_name"].is_null());
    assert!(not_current["recoverable_awaiting_certification_count"].is_null());
    assert!(not_current["acknowledged_product_handoff_count"].is_null());

    let denied = sqlx::query("SELECT * FROM public.runtime_gateway_owners")
        .fetch_all(&database.executor_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&denied, "42501");

    assert_cross_runtime_readiness(&mut database).await;
    cleanup(database).await;
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_counts_pending_and_rejects_historical_ambiguity() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_claimable_deployment(&database.owner_pool).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;

    seed_pending_product_drain_for_startup_observation(&database.owner_pool).await;
    let pending = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(pending["outcome_name"], "observed");
    assert_eq!(pending["serving_state_name"], "empty");
    assert_eq!(pending["pending_runtime_drain_intent_count"], 1);
    assert_eq!(pending["acknowledged_product_handoff_count"], 0);

    seed_nonawaiting_certification_root_for_startup_observation(&database.owner_pool).await;
    let ambiguous = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(ambiguous["outcome_name"], "ambiguous");
    assert_eq!(ambiguous["serving_state_name"], "ambiguous");
    assert!(ambiguous["recoverable_awaiting_certification_count"].is_null());
    assert!(ambiguous["pending_runtime_drain_intent_count"].is_null());
    assert!(ambiguous["acknowledged_product_handoff_count"].is_null());

    cleanup(database).await;
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_detects_drain_state_constraint_drift() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;

    let mut drift = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *drift)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_drain_intents_v2 \
         DROP CONSTRAINT runtime_drain_intents_v2_state_check",
    )
    .execute(&mut *drift)
    .await
    .unwrap();
    let Json(ambiguous) = startup_observation_query()
        .bind(&owner.0)
        .bind(&owner.1)
        .bind(owner.2)
        .bind(&owner.3)
        .bind(owner.4)
        .bind(owner.5)
        .fetch_one(&mut *drift)
        .await
        .unwrap();
    assert_eq!(ambiguous["outcome_name"], "ambiguous");
    assert!(ambiguous["acknowledged_product_handoff_count"].is_null());
    drift.rollback().await.unwrap();

    cleanup(database).await;
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_classifies_live_leases_and_unsupported_current_owner() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&database, 200_000).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;

    let foreign = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(foreign["outcome_name"], "observed");
    assert_eq!(foreign["serving_state_name"], "foreign_fresh");
    assert_eq!(foreign["serving_count"], 1);
    assert!(foreign["serving_earliest_expiry"].is_string());
    let retry = foreign["serving_retry_after_milliseconds"]
        .as_i64()
        .unwrap();
    assert!((1..=1000).contains(&retry));

    seed_current_owner_live_ambiguity(&database.owner_pool, &owner.1).await;
    let ambiguous = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(ambiguous["outcome_name"], "ambiguous");
    assert_eq!(ambiguous["serving_state_name"], "ambiguous");
    assert!(ambiguous["serving_count"].is_null());
    assert!(ambiguous["recoverable_awaiting_certification_count"].is_null());

    cleanup(database).await;
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_classifies_exact_stale_live() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&database, 1_000).await;
    let expiry = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT expires_at FROM public.runtime_serving_leases \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    wait_for_database_time(&database.owner_pool, expiry).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;

    let stale = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(stale["outcome_name"], "observed");
    assert_eq!(stale["serving_state_name"], "recoverable_stale");
    assert_eq!(stale["serving_count"], 1);
    assert!(stale["serving_earliest_expiry"].is_null());
    assert!(stale["serving_retry_after_milliseconds"].is_null());

    cleanup(database).await;
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_requires_exactly_one_suspension_terminal() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_claimable_deployment(&database.owner_pool).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    seed_exact_route_suspension_for_startup_observation(&database.owner_pool).await;

    let active = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(active["outcome_name"], "observed");
    assert_eq!(active["suspended_local_effect_count"], 1);

    seed_suspension_completion_for_startup_observation(&database.owner_pool).await;
    let both = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(both["outcome_name"], "ambiguous");
    assert_eq!(both["serving_state_name"], "ambiguous");
    assert!(both["suspended_local_effect_count"].is_null());

    remove_active_suspension_for_startup_observation(&database.owner_pool).await;
    let completed = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(completed["outcome_name"], "observed");
    assert_eq!(completed["suspended_local_effect_count"], 0);

    remove_suspension_completion_for_startup_observation(&database.owner_pool).await;
    let neither = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(neither["outcome_name"], "ambiguous");
    assert_eq!(neither["serving_state_name"], "ambiguous");
    assert!(neither["suspended_local_effect_count"].is_null());

    cleanup(database).await;
}

fn startup_observation_query<'query>(
) -> sqlx::query::QueryScalar<'query, sqlx::Postgres, Json<Value>, sqlx::postgres::PgArguments> {
    sqlx::query_scalar(
        "SELECT pg_catalog.to_jsonb(observation) \
         FROM public.starring_runtime_startup_recovery_observe_v2(\
            $1,$2,$3,$4,$5,$6\
         ) AS observation",
    )
}

async fn acquire_startup_observation_owner(pool: &PgPool) -> StartupObservationOwnerTuple {
    sqlx::query_as(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
            expected_build_revision, owner_revision, expires_at \
         FROM public.starring_runtime_gateway_owner_acquire_v1(\
            'shard:0', 'startup-observation-process', \
            'startup-observation-build', 300000\
         ) \
         WHERE outcome_name = 'acquired'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn observe_startup_state(
    pool: &PgPool,
    owner: &StartupObservationOwnerTuple,
    expected_owner_revision: i64,
) -> Value {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let Json(observed) = startup_observation_query()
        .bind(&owner.0)
        .bind(&owner.1)
        .bind(owner.2)
        .bind(&owner.3)
        .bind(expected_owner_revision)
        .bind(owner.5)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    observed
}

async fn seed_live_for_startup_observation(database: &IsolatedDatabase, lease_milliseconds: i64) {
    let session = gateway_ready_session(database, "startup-observation-live-controller").await;
    let gateway_ready = gateway_ready_attestation(database, &session).await;
    let guard = session.execution_guard().unwrap();
    let mut transaction = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut transaction,
        &guard,
        serde_json::to_value(&gateway_ready).unwrap(),
        lease_milliseconds,
    )
    .await
    .unwrap();
    let input = certification_input(&guard, gateway_ready, &prepared);
    assert_eq!(
        raw_certify_commit(&mut transaction, &input, lease_milliseconds)
            .await
            .unwrap(),
        "applied"
    );
    transaction.commit().await.unwrap();
}

async fn seed_current_owner_live_ambiguity(pool: &PgPool, process_instance_id: &str) {
    let mut transaction = pool.begin().await.unwrap();
    for statement in [
        "ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER",
        "ALTER TABLE public.runtime_serving_leases DISABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    sqlx::query(
        "UPDATE public.runtime_serving_leases \
         SET process_instance_id = $1 \
         WHERE deployment_id = $2",
    )
    .bind(process_instance_id)
    .bind(DEPLOYMENT)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET snapshot = pg_catalog.jsonb_set(\
            snapshot, '{live,process_instance_id}', pg_catalog.to_jsonb($1::TEXT), FALSE\
         ) \
         WHERE deployment_id = $2",
    )
    .bind(process_instance_id)
    .bind(DEPLOYMENT)
    .execute(&mut *transaction)
    .await
    .unwrap();
    for statement in [
        "ALTER TABLE public.runtime_serving_leases ENABLE TRIGGER USER",
        "ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn seed_exact_route_suspension_for_startup_observation(pool: &PgPool) {
    let now = database_now(pool).await;
    let mut transaction = pool.begin().await.unwrap();
    for statement in [
        "ALTER TABLE public.runtime_suspend_attempt_operations_v2 DISABLE TRIGGER USER",
        "ALTER TABLE public.runtime_suspended_attempts_v2 DISABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO public.runtime_suspend_attempt_operations_v2 (\
            suspension_id, tenant_id, installation_id, deployment_id, \
            deployment_revision, convergence_attempt_no, suspend_attempt_request_bytes, \
            suspend_attempt_digest\
         ) VALUES (\
            '77777777777777777777777777777777', $1, $2, $3, 1, 1, \
            pg_catalog.convert_to('{}', 'UTF8'), $4\
         )",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind("8".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_suspended_attempts_v2 (\
            suspension_id, suspend_attempt_digest, tenant_id, installation_id, \
            deployment_id, deployment_revision, convergence_attempt_no, sidecar_revision, \
            slot_guild_id, slot_ruleset_key, local_effect_kind, local_effect_bytes, \
            drain_obligation_kind, drain_obligation_bytes, suspended_at\
         ) VALUES (\
            '77777777777777777777777777777777', $1, $2, $3, $4, 1, 1, 1, $5, $6, \
            'exact_route', pg_catalog.convert_to('{}', 'UTF8'), \
            'exact_local_route', pg_catalog.convert_to('{}', 'UTF8'), $7\
         )",
    )
    .bind("8".repeat(64))
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .unwrap();
    for statement in [
        "ALTER TABLE public.runtime_suspended_attempts_v2 ENABLE TRIGGER USER",
        "ALTER TABLE public.runtime_suspend_attempt_operations_v2 ENABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn seed_suspension_completion_for_startup_observation(pool: &PgPool) {
    let now = database_now(pool).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_suspend_attempt_completions_v2 \
         DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_suspend_attempt_completions_v2 (\
            suspension_id, suspend_attempt_digest, tenant_id, installation_id, \
            deployment_id, deployment_revision, convergence_attempt_no, \
            resulting_deployment_revision, resulting_convergence_attempt_no, \
            successor_controller_id, successor_controller_fencing_token, \
            successor_acquired_at, successor_expires_at, completed_at\
         ) VALUES (\
            '77777777777777777777777777777777', $1, $2, $3, $4, 1, 1, 2, 2, \
            'startup-observation-successor', 1, $5, $6, $5\
         )",
    )
    .bind("8".repeat(64))
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(now)
    .bind(now + TimeDelta::minutes(5))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_suspend_attempt_completions_v2 \
         ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn remove_active_suspension_for_startup_observation(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_suspended_attempts_v2 \
         DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM public.runtime_suspended_attempts_v2 \
         WHERE suspension_id = '77777777777777777777777777777777'",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_suspended_attempts_v2 \
         ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn remove_suspension_completion_for_startup_observation(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_suspend_attempt_completions_v2 \
         DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM public.runtime_suspend_attempt_completions_v2 \
         WHERE suspension_id = '77777777777777777777777777777777'",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_suspend_attempt_completions_v2 \
         ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn seed_pending_product_drain_for_startup_observation(pool: &PgPool) {
    let snapshot = product_drain_snapshot(pool).await;
    let canonical = canonical_product_drain(&snapshot);
    seed_canonical_product_drain(pool, &canonical).await;
}

async fn seed_nonawaiting_certification_root_for_startup_observation(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_certification_operations_v2 \
         DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_certification_operations_v2 (\
            operation_id, tenant_id, installation_id, deployment_id, \
            deployment_revision, convergence_attempt_no, certification_intent_bytes, \
            intent_fingerprint\
         ) VALUES (\
            '55555555555555555555555555555555', $1, $2, $3, 1, 1, \
            pg_catalog.convert_to('{}', 'UTF8'), $4\
         )",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind("6".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_certification_operations_v2 \
         ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_port_completes_empty_fixed_point_and_reuses_connection() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let before = startup_observation_executor_backend_pids(&database).await;
    assert!(!before.is_empty());
    let (mut lifecycle, mut permit, authorization) =
        authorize_startup_observation_port_call(&adapter, owner);

    let completed = RuntimeStartupRecoveryObservationPortV2::observe_startup_recovery(
        &adapter,
        authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();

    assert_eq!(
        startup_observation_executor_backend_pids(&database).await,
        before
    );
    let RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(fixed_point) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("empty startup observation must reach a fixed point")
    };
    assert_eq!(fixed_point.acknowledged_product_handoff_count(), 0);
    assert_eq!(
        lifecycle.validate_startup_recovery_fixed_point(&permit, &fixed_point),
        Ok(())
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_port_maps_lost_owner() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, lost_authorization) =
        authorize_startup_observation_port_call(&adapter, owner.clone());

    let released = RuntimeGatewayOwnerLeasePortV1::release_gateway_owner(
        &adapter,
        RuntimeReleaseGatewayOwnerLeaseV1 {
            lease_id: owner.lease_id,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        released,
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released { .. }
    ));
    assert!(matches!(
        RuntimeStartupRecoveryObservationPortV2::observe_startup_recovery(
            &adapter,
            lost_authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::OwnershipLost)
    ));

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_port_maps_ambiguous_state() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, ambiguous_authorization) = authorize_startup_observation_port_call(&adapter, owner);
    sqlx::query(
        "ALTER TABLE public.runtime_drain_intents_v2 \
         DROP CONSTRAINT runtime_drain_intents_v2_state_check",
    )
    .execute(&database.owner_pool)
    .await
    .unwrap();
    assert!(matches!(
        RuntimeStartupRecoveryObservationPortV2::observe_startup_recovery(
            &adapter,
            ambiguous_authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::ObservationAmbiguous)
    ));

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_port_elapsed_cutoff_wins_over_closed_pool() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, authorization) = authorize_startup_observation_port_call(&adapter, owner);
    database.executor_pool.close().await;

    assert!(matches!(
        RuntimeStartupRecoveryObservationPortV2::observe_startup_recovery(
            &adapter,
            authorization,
            Instant::now(),
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::Timeout)
    ));

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_observation_port_cancellation_detaches_blocked_connection() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let adapter = verified_execution_adapter_with_timeouts(
        &database,
        automation_runtime_execution_postgres::RuntimeExecutionDatabaseTimeoutsV1::new(
            Duration::from_secs(5),
            Duration::from_secs(4),
        )
        .unwrap(),
    )
    .await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, authorization) = authorize_startup_observation_port_call(&adapter, owner.clone());
    let mut blocker = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *blocker)
    .await
    .unwrap();

    let cancelled_adapter = adapter.clone();
    let observation = tokio::spawn(async move {
        RuntimeStartupRecoveryObservationPortV2::observe_startup_recovery(
            &cancelled_adapter,
            authorization,
            Instant::now() + Duration::from_secs(10),
        )
        .await
    });
    let blocked_backend = wait_for_blocked_startup_observation(&database.owner_pool).await;
    observation.abort();
    assert!(observation.await.unwrap_err().is_cancelled());
    blocker.rollback().await.unwrap();
    wait_for_startup_observation_backend_exit(&database.owner_pool, blocked_backend).await;

    let (mut lifecycle, mut permit, successor_authorization) =
        authorize_startup_observation_port_call(&adapter, owner);
    let successor = RuntimeStartupRecoveryObservationPortV2::observe_startup_recovery(
        &adapter,
        successor_authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert!(matches!(
        lifecycle
            .complete_startup_recovery_observation(&mut permit, successor)
            .unwrap(),
        RuntimeAcceptedStartupRecoveryOutcomeV2::FixedPoint(_)
    ));

    cleanup(database).await;
    drop(server);
}

async fn acquire_startup_observation_port_owner(
    adapter: &PostgresRuntimeExecutionV1,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    let outcome = RuntimeGatewayOwnerLeasePortV1::acquire_gateway_owner(
        adapter,
        gateway_owner_acquire_request(
            "startup-observation-port-process",
            "startup-observation-port-build",
        ),
    )
    .await
    .unwrap();
    let RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(receipt) = outcome else {
        panic!("startup observation port owner must be acquired")
    };
    receipt
}

fn authorize_startup_observation_port_call(
    adapter: &PostgresRuntimeExecutionV1,
    owner: RuntimeGatewayOwnerLeaseReceiptV1,
) -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    automation_runtime_worker::RuntimeAuthorizedStartupRecoveryObservationV2,
) {
    authorize_startup_observation_port_call_with_last_resume(adapter, owner, None)
}

fn authorize_startup_observation_port_call_with_last_resume(
    adapter: &PostgresRuntimeExecutionV1,
    owner: RuntimeGatewayOwnerLeaseReceiptV1,
    last_resume_sequence: Option<u64>,
) -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    automation_runtime_worker::RuntimeAuthorizedStartupRecoveryObservationV2,
) {
    let mut lifecycle = RuntimeGatewayClosedLifecycleV2::starting();
    let generation = lifecycle.snapshot().generation();
    let process = owner.lease_id.process_instance_id.clone();
    let paused = RuntimePausedGatewayObservationV2::new(
        generation,
        process.clone(),
        NonZeroU64::new(2).unwrap(),
        RuntimeGatewayReadyKindV2::Ready,
        NonZeroU64::new(3).unwrap(),
        RuntimePausedGatewaySequenceV2::new(
            RuntimeGatewayAdmissionSequenceV2::new(NonZeroU64::new(5).unwrap()),
            RuntimeGatewayAdmissionSequenceV2::new(NonZeroU64::new(4).unwrap()),
            last_resume_sequence.map(|sequence| {
                RuntimeGatewayAdmissionSequenceV2::new(NonZeroU64::new(sequence).unwrap())
            }),
        )
        .unwrap(),
    );
    let registry = accept_runtime_registry_recovery_empty_observation_v2(
        process,
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                NonZeroU64::new(6).unwrap(),
            ),
            retained_slot_count: 0,
            retained_empty_tombstone_count: 0,
            staged_route_count: 0,
            serving_route_count: 0,
            draining_route_count: 0,
            sealed_slot_count: 0,
            active_interaction_count: 0,
            failed_closed_slot_count: 0,
            registry_failed_closed: false,
        },
    )
    .unwrap();
    let (_, mut permit) = lifecycle
        .begin_recovery(
            generation,
            RuntimeClosedRecoveryInputV2::new(
                RuntimeRecoveryIdV2::parse("fedcba98765432100123456789abcdef").unwrap(),
                owner,
                startup_observation_port_readiness(adapter, 0),
                paused,
                RuntimeClosedRecoveryRegistryEvidenceV2::Empty(registry),
            ),
        )
        .unwrap();
    let iteration = lifecycle
        .refresh_recovery_readiness(&mut permit, startup_observation_port_readiness(adapter, 1))
        .unwrap();
    let authorization = lifecycle
        .begin_startup_recovery_observation(&mut permit, iteration)
        .unwrap();
    (lifecycle, permit, authorization)
}

fn startup_observation_port_readiness(
    adapter: &PostgresRuntimeExecutionV1,
    freshness_step: i64,
) -> RuntimeCapabilityReadinessSetV2 {
    let initial = adapter.initial_readiness();
    let checked_at = initial.checked_at + TimeDelta::microseconds(freshness_step);
    let receipt = |kind, role| {
        RuntimeCapabilityReadinessReceiptV2::new(
            kind,
            &initial.database_identity,
            &initial.database_name,
            role,
            checked_at,
        )
        .unwrap()
    };
    RuntimeCapabilityReadinessSetV2::new(
        receipt(
            RuntimeCapabilityReadinessKindV2::Convergence,
            "startup_observation_convergence",
        ),
        receipt(
            RuntimeCapabilityReadinessKindV2::ExactTarget,
            "startup_observation_exact_target",
        ),
        receipt(
            RuntimeCapabilityReadinessKindV2::Panel,
            "startup_observation_panel",
        ),
        receipt(
            RuntimeCapabilityReadinessKindV2::Serving,
            "startup_observation_serving",
        ),
        receipt(
            RuntimeCapabilityReadinessKindV2::Interaction,
            "startup_observation_interaction",
        ),
    )
    .unwrap()
}

async fn startup_observation_executor_backend_pids(database: &IsolatedDatabase) -> Vec<i32> {
    sqlx::query_scalar::<_, Vec<i32>>(
        "SELECT COALESCE(\
            pg_catalog.array_agg(activity.pid ORDER BY activity.pid), \
            ARRAY[]::INTEGER[]\
         ) \
         FROM pg_catalog.pg_stat_activity AS activity \
         WHERE activity.datname = pg_catalog.current_database() \
            AND activity.usename = $1 \
            AND activity.backend_type = 'client backend' \
            AND activity.state = 'idle' \
            AND activity.xact_start IS NULL",
    )
    .bind(&database.role)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap()
}

async fn wait_for_blocked_startup_observation(pool: &PgPool) -> i32 {
    for _ in 0..200 {
        let backend = sqlx::query_scalar::<_, i32>(
            "SELECT activity.pid \
             FROM pg_catalog.pg_stat_activity AS activity \
             WHERE activity.datname = pg_catalog.current_database() \
                AND activity.pid <> pg_catalog.pg_backend_pid() \
                AND activity.state = 'active' \
                AND activity.wait_event_type = 'Lock' \
                AND activity.query LIKE \
                    '%starring_runtime_startup_recovery_observe_v2%' \
             ORDER BY activity.pid \
             LIMIT 1",
        )
        .fetch_optional(pool)
        .await
        .unwrap();
        if let Some(backend) = backend {
            return backend;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("startup recovery observation did not reach the writer fence lock")
}

async fn wait_for_startup_observation_backend_exit(pool: &PgPool, backend: i32) {
    for _ in 0..200 {
        let absent = sqlx::query_scalar::<_, bool>(
            "SELECT NOT EXISTS (\
                SELECT 1 \
                FROM pg_catalog.pg_stat_activity AS activity \
                WHERE activity.pid = $1\
             )",
        )
        .bind(backend)
        .fetch_one(pool)
        .await
        .unwrap();
        if absent {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("cancelled startup recovery observation backend did not exit")
}
