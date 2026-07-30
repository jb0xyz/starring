#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_reserved_awaiting_execution_journals_no_candidate_once() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let minimum_database_now = database_now(&database.owner_pool).await;
    let first = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "66000000000000000000000000000066",
        minimum_database_now,
    )
    .await
    .unwrap();
    assert_eq!(first["journal_outcome_name"], "applied");
    assert_eq!(first["terminal_outcome_name"], "no_candidate");
    let replay = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "66000000000000000000000000000066",
        minimum_database_now,
    )
    .await
    .unwrap();
    assert_eq!(replay["journal_outcome_name"], "replayed");
    assert_eq!(replay["terminal_outcome_name"], "no_candidate");
    assert_eq!(
        replay["terminal_projection_bytes"],
        first["terminal_projection_bytes"]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_id = '66000000000000000000000000000066'",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        1
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_reserved_awaiting_execution_progresses_and_replays_exactly_once() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;

    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
         expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners WHERE gateway_shard_id = 'shard:0'",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let minimum_database_now = database_now(&database.owner_pool).await;
    let before = reserved_awaiting_execution_state(&database.owner_pool).await;
    let first = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "77000000000000000000000000000077",
        minimum_database_now,
    )
    .await
    .unwrap();
    assert_eq!(first["journal_outcome_name"], "applied");
    assert_eq!(first["terminal_outcome_name"], "progressed");
    assert_eq!(first["recovery_class"], "reserved_awaiting_certification");
    let after = reserved_awaiting_execution_state(&database.owner_pool).await;
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.1, "reconciling_panels");
    assert_eq!(after.2, before.2);
    assert_eq!(after.3, before.3 + 1);
    assert_eq!(after.4, 1);
    assert_eq!(after.5, 1);
    assert!(after.6);

    let replay = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "77000000000000000000000000000077",
        minimum_database_now,
    )
    .await
    .unwrap();
    assert_eq!(replay["journal_outcome_name"], "replayed");
    assert_eq!(replay["terminal_outcome_name"], "progressed");
    assert_eq!(
        replay["terminal_projection_bytes"],
        first["terminal_projection_bytes"]
    );
    assert_eq!(replay["terminal_digest"], first["terminal_digest"]);
    assert_eq!(
        reserved_awaiting_execution_state(&database.owner_pool).await,
        after
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn terminalized_reserved_awaiting_execution_is_claimable_without_losing_audit_records() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;

    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
         expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners WHERE gateway_shard_id = 'shard:0'",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let before = reserved_awaiting_execution_state(&database.owner_pool).await;
    let recovered = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "78000000000000000000000000000078",
        database_now(&database.owner_pool).await,
    )
    .await
    .unwrap();
    assert_eq!(recovered["journal_outcome_name"], "applied");
    assert_eq!(recovered["terminal_outcome_name"], "progressed");
    let reset = reserved_awaiting_execution_state(&database.owner_pool).await;
    assert_eq!(reset.0, before.0 + 1);
    assert_eq!(reset.1, "reconciling_panels");
    assert_eq!(reset.2, before.2);
    assert_eq!(reset.4, 1);

    let mut claim = database.executor_pool.begin().await.unwrap();
    assert_eq!(
        raw_selector_claim(
            &mut claim,
            "runtime-terminalized-certification-reclaim-controller",
        )
        .await
        .unwrap(),
        Some("applied".to_owned())
    );
    claim.commit().await.unwrap();

    let claimed = reserved_awaiting_execution_state(&database.owner_pool).await;
    assert_eq!(claimed.0, reset.0 + 1);
    assert_eq!(claimed.1, "reconciling_panels");
    assert_eq!(claimed.2, reset.2 + 1);
    assert_eq!(claimed.3, reset.3 + 1);
    assert_eq!(claimed.4, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_certification_operations_v2",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        1
    );
    let mut replay = database.executor_pool.begin().await.unwrap();
    assert_eq!(
        raw_selector_claim(
            &mut replay,
            "runtime-terminalized-certification-reclaim-controller",
        )
        .await
        .unwrap(),
        Some("replayed".to_owned())
    );
    replay.commit().await.unwrap();
    assert_eq!(
        reserved_awaiting_execution_state(&database.owner_pool).await,
        claimed
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_unreserved_awaiting_execution_resets_old_process_evidence_and_replays() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;
    remove_current_certification_reservation(&database.owner_pool).await;

    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
         expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners WHERE gateway_shard_id = 'shard:0'",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let minimum_database_now = database_now(&database.owner_pool).await;
    let before = reserved_awaiting_execution_state(&database.owner_pool).await;
    let first = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "7a00000000000000000000000000007a",
        minimum_database_now,
    )
    .await
    .unwrap();
    assert_eq!(first["journal_outcome_name"], "applied");
    assert_eq!(first["terminal_outcome_name"], "progressed");
    assert_eq!(first["recovery_class"], "reserved_awaiting_certification");

    let after = reserved_awaiting_execution_state(&database.owner_pool).await;
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.1, "reconciling_panels");
    assert_eq!(after.2, before.2);
    assert_eq!(after.3, before.3 + 1);
    assert_eq!(after.4, 0);
    assert_eq!(after.5, 1);
    assert!(after.6);
    let cleared = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT \
            snapshot -> 'panel_certificate' = 'null'::JSONB, \
            snapshot -> 'gateway_ready' = 'null'::JSONB, \
            snapshot -> 'live' = 'null'::JSONB \
         FROM public.runtime_deployments \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(cleared, (true, true, true));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_certification_operations_v2",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        0
    );
    let replay = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "7a00000000000000000000000000007a",
        minimum_database_now,
    )
    .await
    .unwrap();
    assert_eq!(replay["journal_outcome_name"], "replayed");
    assert_eq!(replay["terminal_outcome_name"], "progressed");
    assert_eq!(
        replay["terminal_projection_bytes"],
        first["terminal_projection_bytes"]
    );
    assert_eq!(replay["terminal_digest"], first["terminal_digest"]);
    assert_eq!(
        reserved_awaiting_execution_state(&database.owner_pool).await,
        after
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_unreserved_awaiting_execution_skips_earlier_blocked_candidate() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let (adapter, _) = blocked_and_clear_unreserved_awaiting_candidates(&database).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let blocked_before = startup_awaiting_candidate_image(&database.owner_pool, DEPLOYMENT).await;
    let clear_before =
        startup_awaiting_candidate_image(&database.owner_pool, SELECTOR_DEPLOYMENT).await;
    let result = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "7b00000000000000000000000000007b",
        database_now(&database.owner_pool).await,
    )
    .await
    .unwrap();
    assert_eq!(result["journal_outcome_name"], "applied");
    assert_eq!(result["terminal_outcome_name"], "progressed");
    assert_eq!(
        startup_awaiting_candidate_image(&database.owner_pool, DEPLOYMENT).await,
        blocked_before
    );
    let clear_after =
        startup_awaiting_candidate_image(&database.owner_pool, SELECTOR_DEPLOYMENT).await;
    assert_ne!(clear_after, clear_before);
    assert_eq!(
        clear_after.0["phase"],
        Value::String("reconciling_panels".to_owned())
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_id = '7b00000000000000000000000000007b'",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        1
    );

    drop(adapter);
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_reserved_awaiting_execution_skips_earlier_blocked_unreserved_candidate() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let (adapter, mut reserved_session) =
        blocked_and_clear_unreserved_awaiting_candidates(&database).await;
    reserve_startup_awaiting_candidate(
        &database,
        &adapter,
        &mut reserved_session,
        "7c00000000000000000000000000007c",
    )
    .await;
    expire_current_gateway_owner(&database.owner_pool).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let blocked_before = startup_awaiting_candidate_image(&database.owner_pool, DEPLOYMENT).await;
    let reserved_before =
        startup_awaiting_candidate_image(&database.owner_pool, SELECTOR_DEPLOYMENT).await;
    let result = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "7d00000000000000000000000000007d",
        database_now(&database.owner_pool).await,
    )
    .await
    .unwrap();
    assert_eq!(result["journal_outcome_name"], "applied");
    assert_eq!(result["terminal_outcome_name"], "progressed");
    assert_eq!(
        startup_awaiting_candidate_image(&database.owner_pool, DEPLOYMENT).await,
        blocked_before
    );
    let reserved_after =
        startup_awaiting_candidate_image(&database.owner_pool, SELECTOR_DEPLOYMENT).await;
    assert_ne!(reserved_after, reserved_before);
    assert_eq!(
        reserved_after.0["phase"],
        Value::String("reconciling_panels".to_owned())
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_certification_operation_terminals_v2 \
             WHERE operation_id = '7c00000000000000000000000000007c'",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        1
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_reserved_awaiting_execution_blocks_on_pending_drain_without_journal() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;
    seed_pending_product_drain_for_startup_observation(&database.owner_pool).await;
    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
         expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners WHERE gateway_shard_id = 'shard:0'",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let before = reserved_awaiting_execution_state(&database.owner_pool).await;
    let error = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "88000000000000000000000000000088",
        database_now(&database.owner_pool).await,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX007");
    assert_eq!(
        reserved_awaiting_execution_state(&database.owner_pool).await,
        before
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_id = '88000000000000000000000000000088'",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        0
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_unreserved_awaiting_execution_blocks_on_pending_drain_without_journal() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;
    remove_current_certification_reservation(&database.owner_pool).await;
    seed_pending_product_drain_for_startup_observation(&database.owner_pool).await;
    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
         expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners WHERE gateway_shard_id = 'shard:0'",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let before = reserved_awaiting_execution_state(&database.owner_pool).await;
    let error = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "8a00000000000000000000000000008a",
        database_now(&database.owner_pool).await,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX007");
    assert_eq!(
        reserved_awaiting_execution_state(&database.owner_pool).await,
        before
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_id = '8a00000000000000000000000000008a'",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        0
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_reserved_awaiting_execution_rejects_missing_slot_without_mutation() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;
    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
         expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners WHERE gateway_shard_id = 'shard:0'",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let mut corruption = database.owner_pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_slot_writer_fences_v2 DISABLE TRIGGER USER")
        .execute(&mut *corruption)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM public.runtime_slot_writer_fences_v2 \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *corruption)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_slot_writer_fences_v2 ENABLE TRIGGER USER")
        .execute(&mut *corruption)
        .await
        .unwrap();
    corruption.commit().await.unwrap();
    let error = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        "99000000000000000000000000000099",
        database_now(&database.owner_pool).await,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_id = '99000000000000000000000000000099'",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        0
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_reserved_awaiting_execution_rolls_back_when_journaling_fails() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;
    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
         expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners WHERE gateway_shard_id = 'shard:0'",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let before = reserved_awaiting_execution_state(&database.owner_pool).await;
    let minimum_database_now = database_now(&database.owner_pool).await;
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', '5s', TRUE)")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_startup_recovery_actions_v2 \
         ADD CONSTRAINT runtime_startup_reserved_awaiting_injected_failure \
         CHECK (recovery_id <> 'aa0000000000000000000000000000aa')",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let error = startup_reserved_awaiting_execution_query()
        .bind("aa0000000000000000000000000000aa")
        .bind(&owner.0)
        .bind(&owner.1)
        .bind(owner.2)
        .bind(&owner.3)
        .bind(owner.4)
        .bind(owner.5)
        .bind(minimum_database_now)
        .fetch_one(&mut *transaction)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "23514");
    transaction.rollback().await.unwrap();
    assert_eq!(
        reserved_awaiting_execution_state(&database.owner_pool).await,
        before
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_id = 'aa0000000000000000000000000000aa'",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        0
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_reserved_awaiting_execution_serializes_two_recovery_ids() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;
    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
         expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners WHERE gateway_shard_id = 'shard:0'",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let before = reserved_awaiting_execution_state(&database.owner_pool).await;
    let minimum_database_now = database_now(&database.owner_pool).await;
    let mut holder = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *holder)
        .await
        .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', '10s', TRUE)")
        .execute(&mut *holder)
        .await
        .unwrap();
    let holder_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let holder_result = startup_reserved_awaiting_execution_query()
        .bind("bb0000000000000000000000000000bb")
        .bind(&owner.0)
        .bind(&owner.1)
        .bind(owner.2)
        .bind(&owner.3)
        .bind(owner.4)
        .bind(owner.5)
        .bind(minimum_database_now)
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    assert_eq!(holder_result.0["journal_outcome_name"], "applied");
    assert_eq!(holder_result.0["terminal_outcome_name"], "progressed");

    let mut waiter = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *waiter)
        .await
        .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', '10s', TRUE)")
        .execute(&mut *waiter)
        .await
        .unwrap();
    let waiter_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *waiter)
        .await
        .unwrap();
    let (waiter_result, ()) = tokio::join!(
        startup_reserved_awaiting_execution_query()
            .bind("cc0000000000000000000000000000cc")
            .bind(&owner.0)
            .bind(&owner.1)
            .bind(owner.2)
            .bind(&owner.3)
            .bind(owner.4)
            .bind(owner.5)
            .bind(minimum_database_now)
            .fetch_one(&mut *waiter),
        async {
            wait_for_advisory_lock_blocked_by(&database.owner_pool, waiter_pid, holder_pid).await;
            holder.commit().await.unwrap();
        }
    );
    let waiter_error = waiter_result.unwrap_err();
    assert_sqlstate(&waiter_error, "40001");
    waiter.rollback().await.unwrap();
    let after = reserved_awaiting_execution_state(&database.owner_pool).await;
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.1, "reconciling_panels");
    assert_eq!(after.2, before.2);
    assert_eq!(after.3, before.3 + 1);
    assert_eq!(after.4, 1);
    assert_eq!(after.5, 1);
    assert!(after.6);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_id = 'cc0000000000000000000000000000cc'",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        0
    );
    cleanup(database).await;
    drop(server);
}

fn startup_reserved_awaiting_execution_query<'query>(
) -> sqlx::query::QueryScalar<'query, sqlx::Postgres, Json<Value>, sqlx::postgres::PgArguments> {
    sqlx::query_scalar(
        "SELECT pg_catalog.to_jsonb(result) \
         FROM public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(\
            $1,1,1,2,1,$2,$3,$4,$5,$6,$7,$8\
         ) AS result",
    )
}

async fn execute_startup_reserved_awaiting(
    pool: &PgPool,
    owner: &StartupObservationOwnerTuple,
    recovery_id: &str,
    minimum_database_now: DateTime<Utc>,
) -> Result<Value, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', '5s', TRUE)")
        .execute(&mut *transaction)
        .await?;
    let result = startup_reserved_awaiting_execution_query()
        .bind(recovery_id)
        .bind(&owner.0)
        .bind(&owner.1)
        .bind(owner.2)
        .bind(&owner.3)
        .bind(owner.4)
        .bind(owner.5)
        .bind(minimum_database_now)
        .fetch_one(&mut *transaction)
        .await;
    match result {
        Ok(Json(value)) => {
            transaction.commit().await?;
            Ok(value)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn reserved_awaiting_execution_state(
    pool: &PgPool,
) -> (i64, String, i64, i64, i64, i64, bool) {
    sqlx::query_as(
        "SELECT deployment.revision, deployment.phase, \
            deployment.convergence_attempt_no, fence.writer_epoch, \
            (SELECT pg_catalog.count(*) \
             FROM public.runtime_certification_operation_terminals_v2), \
            (SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_class = 'reserved_awaiting_certification'), \
            deployment.controller_id IS NULL \
         FROM public.runtime_deployments AS deployment \
         INNER JOIN public.runtime_slot_writer_fences_v2 AS fence \
            ON fence.slot_guild_id = deployment.guild_id \
            AND fence.slot_ruleset_key = deployment.ruleset_key \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn remove_current_certification_reservation(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_certification_operations_v2 \
         DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(
        sqlx::query(
            "DELETE FROM public.runtime_certification_operations_v2 \
             WHERE deployment_id = $1",
        )
        .bind(DEPLOYMENT)
        .execute(&mut *transaction)
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    sqlx::query(
        "ALTER TABLE public.runtime_certification_operations_v2 \
         ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn blocked_and_clear_unreserved_awaiting_candidates(
    database: &IsolatedDatabase,
) -> (PostgresRuntimeExecutionV1, RuntimeConvergenceSessionV1) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let blocked = selector_gateway_ready_session(
        database,
        &adapter,
        "startup-unreserved-starvation-blocked-controller",
        "startup-unreserved-starvation-blocked-panel",
        "startup-unreserved-starvation-blocked-process",
    )
    .await;
    let drain = canonical_product_drain(blocked.snapshot());
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &drain)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    seed_second_claimable_deployment(&database.owner_pool).await;
    let clear = selector_gateway_ready_session(
        database,
        &adapter,
        "startup-unreserved-starvation-clear-controller",
        "startup-unreserved-starvation-clear-panel",
        "startup-unreserved-starvation-clear-process",
    )
    .await;
    let requested_order = sqlx::query_scalar::<_, bool>(
        "SELECT blocked.requested_at < clear.requested_at \
         FROM public.runtime_deployments AS blocked \
         CROSS JOIN public.runtime_deployments AS clear \
         WHERE blocked.deployment_id = $1 AND clear.deployment_id = $2",
    )
    .bind(DEPLOYMENT)
    .bind(SELECTOR_DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert!(requested_order);
    (adapter, clear)
}

async fn startup_awaiting_candidate_image(
    pool: &PgPool,
    deployment_id: &str,
) -> (Json<Value>, Json<Value>) {
    sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(deployment), pg_catalog.to_jsonb(fence) \
         FROM public.runtime_deployments AS deployment \
         INNER JOIN public.runtime_slot_writer_fences_v2 AS fence \
            ON fence.slot_guild_id = deployment.guild_id \
            AND fence.slot_ruleset_key = deployment.ruleset_key \
         WHERE deployment.deployment_id = $1",
    )
    .bind(deployment_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn reserve_startup_awaiting_candidate(
    database: &IsolatedDatabase,
    adapter: &PostgresRuntimeExecutionV1,
    session: &mut RuntimeConvergenceSessionV1,
    operation_id: &str,
) {
    let execution = session.current_execution_receipt().unwrap();
    let panel = execution.snapshot.panel_certificate.as_ref().unwrap();
    let process_identity = automation_runtime_convergence::RuntimeProcessIdentityV1 {
        target: execution.snapshot.target.clone(),
        runtime_generation: execution.snapshot.runtime_generation,
        process_instance_id: panel.process_instance_id.clone(),
    };
    let (lease_epoch, owner_revision) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT lease_epoch, owner_revision \
         FROM public.starring_runtime_gateway_owner_acquire_v1($1,$2,$3,$4)",
    )
    .bind(CERTIFICATION_SHARD)
    .bind(process_identity.process_instance_id.as_str())
    .bind(CERTIFICATION_BUILD)
    .bind(300_000_i64)
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    let input = automation_runtime_controller::RuntimeCertificationReservationInputV2 {
        operation_id: automation_runtime_controller::RuntimeCertificationOperationIdV2::parse(
            operation_id,
        )
        .unwrap(),
        binding_pin: automation_runtime_controller::RuntimeBindingPinV1 {
            tenant_id: execution.snapshot.identity.tenant_id.clone(),
            installation_id: execution.snapshot.identity.installation_id.clone(),
            installation_authority_revision: std::num::NonZeroU64::MIN,
            binding_revision: execution.snapshot.target.binding_revision,
            binding_fingerprint: execution.snapshot.target.binding_fingerprint.clone(),
        },
        gateway_owner_lease_id: automation_runtime_controller::RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: automation_runtime_controller::GatewayShardIdV1::parse(
                CERTIFICATION_SHARD,
            )
            .unwrap(),
            process_instance_id: process_identity.process_instance_id.clone(),
            lease_epoch: std::num::NonZeroU64::new(lease_epoch as u64).unwrap(),
            expected_build_revision: automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                CERTIFICATION_BUILD,
            )
            .unwrap(),
        },
        observed_owner_revision: std::num::NonZeroU64::new(owner_revision as u64).unwrap(),
        runtime_build_revision: automation_runtime_controller::RuntimeBuildRevisionV1::parse(
            CERTIFICATION_BUILD,
        )
        .unwrap(),
        panel: automation_runtime_controller::RuntimePanelEvidenceV2 {
            certificate_id: panel.certificate_id.clone(),
            report_digest: panel.report_digest.clone(),
            process_identity,
            controller_fencing_token: execution.fencing_token,
        },
        serving_lease_for: Duration::from_millis(CERTIFICATION_LEASE_MILLISECONDS as u64),
    };
    let reservation = session.begin_certification_reservation_v2(input).unwrap();
    let outcome =
        automation_runtime_worker::RuntimeCertificationReservationPortV2::reserve_certification_intent(
            adapter,
            reservation,
        )
        .await
        .unwrap();
    session.apply_certification_reservation_v2(outcome).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_reserved_awaiting_projection_parser_rejects_corruption() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;

    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
         expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners WHERE gateway_shard_id = 'shard:0'",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let minimum_database_now = database_now(&database.owner_pool).await;
    let recovery_id = "99000000000000000000000000000099";
    let result = execute_startup_reserved_awaiting(
        &database.executor_pool,
        &owner,
        recovery_id,
        minimum_database_now,
    )
    .await
    .unwrap();
    assert_eq!(result["terminal_outcome_name"], "progressed");

    let projection = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT terminal_projection_bytes \
         FROM public.runtime_startup_recovery_actions_v2 \
         WHERE recovery_id = $1",
    )
    .bind(recovery_id)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let stable_state = reserved_awaiting_execution_state(&database.owner_pool).await;
    assert!(reserved_projection_is_exact(&database.owner_pool, recovery_id, &projection).await);

    let mut truncated = projection.clone();
    truncated.pop();
    assert!(!reserved_projection_is_exact(&database.owner_pool, recovery_id, &truncated).await);

    let mut appended = projection.clone();
    appended.push(0);
    assert!(!reserved_projection_is_exact(&database.owner_pool, recovery_id, &appended).await);

    let (region_offsets, first_frame_length_offset) =
        reserved_projection_corruption_offsets(&projection);
    for offset in region_offsets {
        let mut corrupted = projection.clone();
        corrupted[offset] ^= 1;
        assert!(
            !reserved_projection_is_exact(&database.owner_pool, recovery_id, &corrupted).await,
            "{offset}"
        );
    }

    let mut forged_length = projection.clone();
    forged_length[first_frame_length_offset] = 1;
    assert!(!reserved_projection_is_exact(&database.owner_pool, recovery_id, &forged_length).await);
    assert_eq!(
        reserved_awaiting_execution_state(&database.owner_pool).await,
        stable_state
    );

    cleanup(database).await;
    drop(server);
}

async fn reserved_projection_is_exact(pool: &PgPool, recovery_id: &str, projection: &[u8]) -> bool {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL TimeZone = 'UTC'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let exact = sqlx::query_scalar::<_, bool>(
        "SELECT \
            starring_runtime_private_v2.starring_runtime_startup_reserved_projection_exact_v2(\
                $1, \
                action.recovery_id, \
                action.originating_emergency_generation, \
                action.coordinator_generation, \
                action.action_authority_revision, \
                action.selection_authority_revision, \
                action.recorded_at, \
                terminal\
            ) \
         FROM public.runtime_startup_recovery_actions_v2 AS action \
         INNER JOIN public.runtime_certification_operation_terminals_v2 AS terminal \
            ON terminal.operation_id = '00112233445566778899aabbccddeeff' \
         WHERE action.recovery_id = $2",
    )
    .bind(projection)
    .bind(recovery_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    exact
}

fn reserved_projection_corruption_offsets(projection: &[u8]) -> (Vec<usize>, usize) {
    let domain_length =
        usize::try_from(i64::from_be_bytes(projection[0..8].try_into().unwrap())).unwrap();
    assert!(domain_length > 0);
    let mut cursor = 8 + domain_length;
    assert_eq!(
        i16::from_be_bytes(projection[cursor..cursor + 2].try_into().unwrap()),
        2
    );
    cursor += 2;
    assert_eq!(
        i16::from_be_bytes(projection[cursor..cursor + 2].try_into().unwrap()),
        1
    );
    cursor += 2;
    let mut region_offsets = vec![8, cursor, cursor + 32];
    cursor += 96;
    let first_frame_length_offset = cursor;
    for _ in 0..5 {
        let frame_length = usize::try_from(i64::from_be_bytes(
            projection[cursor..cursor + 8].try_into().unwrap(),
        ))
        .unwrap();
        assert!(frame_length > 0);
        cursor += 8;
        region_offsets.push(cursor);
        cursor += frame_length;
    }
    region_offsets.push(cursor);
    assert!(cursor < projection.len());
    (region_offsets, first_frame_length_offset)
}
