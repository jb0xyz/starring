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
