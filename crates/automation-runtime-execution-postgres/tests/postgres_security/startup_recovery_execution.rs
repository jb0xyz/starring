#[derive(Clone)]
struct StartupStaleLiveExecutionInput {
    recovery_id: String,
    originating_emergency_generation: i64,
    coordinator_generation: i64,
    action_authority_revision: i64,
    selection_authority_revision: i64,
    owner: StartupObservationOwnerTuple,
    minimum_database_now: DateTime<Utc>,
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_stale_live_execution_journals_no_candidate_and_freezes_replay() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let input = startup_stale_live_execution_input(
        &owner,
        "11000000000000000000000000000011",
        database_now(&database.owner_pool).await,
    );

    let read_committed = startup_stale_live_execution_query()
        .bind(&input.recovery_id)
        .bind(input.originating_emergency_generation)
        .bind(input.coordinator_generation)
        .bind(input.action_authority_revision)
        .bind(input.selection_authority_revision)
        .bind(&input.owner.0)
        .bind(&input.owner.1)
        .bind(input.owner.2)
        .bind(&input.owner.3)
        .bind(input.owner.4)
        .bind(input.owner.5)
        .bind(input.minimum_database_now)
        .fetch_one(&database.executor_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&read_committed, "RX004");

    let applied = execute_startup_stale_live(&database.executor_pool, &input, "5s")
        .await
        .unwrap();
    assert_eq!(applied["journal_outcome_name"], "applied");
    assert_eq!(applied["terminal_outcome_name"], "no_candidate");
    assert_eq!(applied["recovery_class"], "stale_live");
    let expected_projection = no_candidate_terminal_projection();
    assert_eq!(
        json_bytea(&applied["terminal_projection_bytes"]),
        expected_projection
    );
    let applied_digest = applied["terminal_digest"].clone();
    let applied_recorded_at = applied["recorded_at"].clone();

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
    let before = startup_stale_live_mutation_state(&database.owner_pool).await;

    let replayed = execute_startup_stale_live(&database.executor_pool, &input, "5s")
        .await
        .unwrap();
    assert_eq!(replayed["journal_outcome_name"], "replayed");
    assert_eq!(replayed["terminal_outcome_name"], "no_candidate");
    assert_eq!(
        json_bytea(&replayed["terminal_projection_bytes"]),
        expected_projection
    );
    assert_eq!(replayed["terminal_digest"], applied_digest);
    assert_eq!(replayed["recorded_at"], applied_recorded_at);
    assert_eq!(
        startup_stale_live_mutation_state(&database.owner_pool).await,
        before
    );
    assert_eq!(
        startup_stale_live_journal_count(&database.owner_pool, &input.recovery_id).await,
        1
    );

    let cross_class = recovery_action_journal_input(
        &owner,
        "22000000000000000000000000000022",
        1,
        "reserved_awaiting_certification",
        database_now(&database.owner_pool).await,
        b"cross-class",
    );
    assert_eq!(
        record_recovery_action(&database.owner_pool, &cross_class, true, "5s")
            .await
            .unwrap()
            .0,
        "applied"
    );
    let cross_class_input = StartupStaleLiveExecutionInput {
        recovery_id: cross_class.recovery_id,
        originating_emergency_generation: cross_class.originating_emergency_generation,
        coordinator_generation: cross_class.coordinator_generation,
        action_authority_revision: cross_class.action_authority_revision,
        selection_authority_revision: cross_class.selection_authority_revision,
        owner,
        minimum_database_now: cross_class.minimum_database_now,
    };
    let cross_class_error =
        execute_startup_stale_live(&database.executor_pool, &cross_class_input, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&cross_class_error, "RX003");

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_stale_live_execution_progresses_once_with_canonical_projection() {
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
    let input = startup_stale_live_execution_input(
        &owner,
        "33000000000000000000000000000033",
        database_now(&database.owner_pool).await,
    );
    let pre_deployment =
        startup_stale_live_row(&database.owner_pool, "runtime_deployments").await;
    let pre_slot =
        startup_stale_live_row(&database.owner_pool, "runtime_slot_writer_fences_v2").await;
    let serving =
        startup_stale_live_row(&database.owner_pool, "runtime_serving_leases").await;
    let pre_state = startup_stale_live_mutation_state(&database.owner_pool).await;

    let applied = execute_startup_stale_live(&database.executor_pool, &input, "5s")
        .await
        .unwrap();
    assert_eq!(applied["journal_outcome_name"], "applied");
    assert_eq!(applied["terminal_outcome_name"], "progressed");
    let post_deployment =
        startup_stale_live_row(&database.owner_pool, "runtime_deployments").await;
    let post_slot =
        startup_stale_live_row(&database.owner_pool, "runtime_slot_writer_fences_v2").await;
    let post_state = startup_stale_live_mutation_state(&database.owner_pool).await;
    assert_eq!(post_state.0, pre_state.0 + 1);
    assert_eq!(post_state.1, "runtime_pending");
    assert!(post_state.2);
    assert_eq!(post_state.3, pre_state.3 + 1);
    assert_eq!(
        post_deployment["snapshot"]["phase"]["phase"],
        "runtime_pending"
    );
    assert_eq!(
        post_deployment["snapshot"]["phase"]["condition"]["condition"],
        "ready"
    );
    assert!(post_deployment["snapshot"]["live"].is_null());
    assert!(post_deployment["snapshot"]["panel_certificate"].is_null());
    assert!(post_deployment["snapshot"]["gateway_ready"].is_null());
    assert!(post_deployment["snapshot"]["last_live_recovery"].is_object());

    let projection = json_bytea(&applied["terminal_projection_bytes"]);
    let projection_rows = decode_progressed_terminal_projection(&projection);
    assert_eq!(projection_rows.0, pre_deployment);
    assert_eq!(projection_rows.1, post_deployment);
    assert_eq!(projection_rows.2, pre_slot);
    assert_eq!(projection_rows.3, post_slot);
    assert_eq!(projection_rows.4, serving);
    assert!((1..=2).contains(&projection_rows.5));

    let replayed = execute_startup_stale_live(&database.executor_pool, &input, "5s")
        .await
        .unwrap();
    assert_eq!(replayed["journal_outcome_name"], "replayed");
    assert_eq!(replayed["terminal_outcome_name"], "progressed");
    assert_eq!(
        replayed["terminal_projection_bytes"],
        applied["terminal_projection_bytes"]
    );
    assert_eq!(replayed["terminal_digest"], applied["terminal_digest"]);
    assert_eq!(replayed["recorded_at"], applied["recorded_at"]);
    assert_eq!(
        startup_stale_live_mutation_state(&database.owner_pool).await,
        post_state
    );
    assert_eq!(
        startup_stale_live_journal_count(&database.owner_pool, &input.recovery_id).await,
        1
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_stale_live_execution_accepts_projection_above_the_old_bound() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&database, 1_000).await;
    let mut padding = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *padding)
        .await
        .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *padding)
        .await
        .unwrap();
    let padding_clock = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT public.starring_runtime_mutation_clock()",
    )
    .fetch_one(&mut *padding)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET snapshot = pg_catalog.jsonb_set(\
                pg_catalog.jsonb_set(\
                    snapshot, '{projection_padding}', \
                    pg_catalog.to_jsonb(pg_catalog.repeat('x', 240000)), TRUE\
                ), \
                '{revision}', pg_catalog.to_jsonb(revision + 1), FALSE\
             ), \
             revision = revision + 1, \
             updated_at = GREATEST(\
                $2, updated_at + INTERVAL '1 microsecond'\
             ) \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .bind(padding_clock)
    .execute(&mut *padding)
    .await
    .unwrap();
    padding.commit().await.unwrap();
    let snapshot_size = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.octet_length(snapshot::TEXT)::BIGINT \
         FROM public.runtime_deployments \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert!((230_000..=262_144).contains(&snapshot_size));
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
    let input = startup_stale_live_execution_input(
        &owner,
        "66000000000000000000000000000066",
        database_now(&database.owner_pool).await,
    );
    let pre_deployment =
        startup_stale_live_row(&database.owner_pool, "runtime_deployments").await;

    let applied = execute_startup_stale_live(&database.executor_pool, &input, "5s")
        .await
        .unwrap();
    assert_eq!(applied["journal_outcome_name"], "applied");
    assert_eq!(applied["terminal_outcome_name"], "progressed");
    let projection = json_bytea(&applied["terminal_projection_bytes"]);
    assert!(projection.len() > 131_072);
    assert!(projection.len() <= 1_048_576);
    let decoded = decode_progressed_terminal_projection(&projection);
    assert_eq!(decoded.0, pre_deployment);
    assert_eq!(
        decoded.0["snapshot"]["projection_padding"],
        decoded.1["snapshot"]["projection_padding"]
    );
    assert_eq!(
        decoded.1,
        startup_stale_live_row(&database.owner_pool, "runtime_deployments").await
    );

    let replayed = execute_startup_stale_live(&database.executor_pool, &input, "5s")
        .await
        .unwrap();
    assert_eq!(replayed["journal_outcome_name"], "replayed");
    assert_eq!(
        replayed["terminal_projection_bytes"],
        applied["terminal_projection_bytes"]
    );
    assert_eq!(replayed["terminal_digest"], applied["terminal_digest"]);

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_stale_live_execution_blocks_on_the_earliest_slot() {
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
    let input = startup_stale_live_execution_input(
        &owner,
        "44000000000000000000000000000044",
        database_now(&database.owner_pool).await,
    );
    let before = startup_stale_live_mutation_state(&database.owner_pool).await;
    let mut blocker = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(\
            pg_catalog.concat('starring-runtime-serving-slot-v1:', $1, ':', $2), 0\
         ))",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *blocker)
    .await
    .unwrap();

    let blocked = execute_startup_stale_live(&database.executor_pool, &input, "200ms")
        .await
        .unwrap_err();
    assert_sqlstate(&blocked, "57014");
    blocker.rollback().await.unwrap();
    assert_eq!(
        startup_stale_live_mutation_state(&database.owner_pool).await,
        before
    );
    assert_eq!(
        startup_stale_live_journal_count(&database.owner_pool, &input.recovery_id).await,
        0
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_stale_live_execution_rolls_back_mutation_when_journaling_fails() {
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
    let input = startup_stale_live_execution_input(
        &owner,
        "55000000000000000000000000000055",
        database_now(&database.owner_pool).await,
    );
    let before = startup_stale_live_mutation_state(&database.owner_pool).await;
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_startup_recovery_actions_v2 \
         ADD CONSTRAINT runtime_startup_recovery_actions_v2_injected_failure \
         CHECK (recovery_id <> '55000000000000000000000000000055')",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let error = startup_stale_live_execution_query()
        .bind(&input.recovery_id)
        .bind(input.originating_emergency_generation)
        .bind(input.coordinator_generation)
        .bind(input.action_authority_revision)
        .bind(input.selection_authority_revision)
        .bind(&input.owner.0)
        .bind(&input.owner.1)
        .bind(input.owner.2)
        .bind(&input.owner.3)
        .bind(input.owner.4)
        .bind(input.owner.5)
        .bind(input.minimum_database_now)
        .fetch_one(&mut *transaction)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "23514");
    transaction.rollback().await.unwrap();
    assert_eq!(
        startup_stale_live_mutation_state(&database.owner_pool).await,
        before
    );
    assert_eq!(
        startup_stale_live_journal_count(&database.owner_pool, &input.recovery_id).await,
        0
    );

    cleanup(database).await;
    drop(server);
}

fn startup_stale_live_execution_query<'query>(
) -> sqlx::query::QueryScalar<'query, sqlx::Postgres, Json<Value>, sqlx::postgres::PgArguments> {
    sqlx::query_scalar(
        "SELECT pg_catalog.to_jsonb(execution) \
         FROM public.starring_runtime_startup_recovery_execute_stale_live_v2(\
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12\
         ) AS execution",
    )
}

fn startup_stale_live_execution_input(
    owner: &StartupObservationOwnerTuple,
    recovery_id: &str,
    minimum_database_now: DateTime<Utc>,
) -> StartupStaleLiveExecutionInput {
    StartupStaleLiveExecutionInput {
        recovery_id: recovery_id.to_string(),
        originating_emergency_generation: 1,
        coordinator_generation: 1,
        action_authority_revision: 2,
        selection_authority_revision: 1,
        owner: owner.clone(),
        minimum_database_now,
    }
}

async fn execute_startup_stale_live(
    pool: &PgPool,
    input: &StartupStaleLiveExecutionInput,
    statement_timeout: &str,
) -> Result<Value, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, TRUE)")
        .bind(statement_timeout)
        .execute(&mut *transaction)
        .await?;
    let result = startup_stale_live_execution_query()
        .bind(&input.recovery_id)
        .bind(input.originating_emergency_generation)
        .bind(input.coordinator_generation)
        .bind(input.action_authority_revision)
        .bind(input.selection_authority_revision)
        .bind(&input.owner.0)
        .bind(&input.owner.1)
        .bind(input.owner.2)
        .bind(&input.owner.3)
        .bind(input.owner.4)
        .bind(input.owner.5)
        .bind(input.minimum_database_now)
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

async fn startup_stale_live_mutation_state(
    pool: &PgPool,
) -> (i64, String, bool, i64, DateTime<Utc>, DateTime<Utc>) {
    sqlx::query_as(
        "SELECT deployment.revision, deployment.phase, \
            deployment.live_attestation_id IS NULL, fence.writer_epoch, \
            deployment.updated_at, fence.updated_at \
         FROM public.runtime_deployments AS deployment \
         JOIN public.runtime_slot_writer_fences_v2 AS fence \
           ON fence.slot_guild_id = deployment.guild_id \
          AND fence.slot_ruleset_key = deployment.ruleset_key \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn startup_stale_live_row(pool: &PgPool, relation: &str) -> Value {
    let query = match relation {
        "runtime_deployments" => {
            "SELECT pg_catalog.to_jsonb(row_value) \
             FROM public.runtime_deployments AS row_value \
             WHERE row_value.deployment_id = $1"
        }
        "runtime_slot_writer_fences_v2" => {
            "SELECT pg_catalog.to_jsonb(row_value) \
             FROM public.runtime_slot_writer_fences_v2 AS row_value \
             WHERE row_value.slot_guild_id = $1"
        }
        "runtime_serving_leases" => {
            "SELECT pg_catalog.to_jsonb(row_value) \
             FROM public.runtime_serving_leases AS row_value \
             WHERE row_value.deployment_id = $1"
        }
        _ => unreachable!(),
    };
    let binding = if relation == "runtime_slot_writer_fences_v2" {
        GUILD.to_string()
    } else {
        DEPLOYMENT.to_string()
    };
    sqlx::query_scalar::<_, Json<Value>>(query)
        .bind(binding)
        .fetch_one(pool)
        .await
        .unwrap()
        .0
}

async fn startup_stale_live_journal_count(pool: &PgPool, recovery_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_startup_recovery_actions_v2 \
         WHERE recovery_id = $1",
    )
    .bind(recovery_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn no_candidate_terminal_projection() -> Vec<u8> {
    let domain = b"starring.runtime.startup_recovery.stale_live.terminal.v2";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(domain.len() as i64).to_be_bytes());
    bytes.extend_from_slice(domain);
    bytes.extend_from_slice(&2_i16.to_be_bytes());
    bytes.extend_from_slice(&0_i16.to_be_bytes());
    bytes
}

fn json_bytea(value: &Value) -> Vec<u8> {
    let encoded = value.as_str().unwrap();
    let encoded = encoded.strip_prefix("\\x").unwrap();
    assert_eq!(encoded.len() % 2, 0);
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()
        })
        .collect()
}

fn decode_progressed_terminal_projection(bytes: &[u8]) -> (Value, Value, Value, Value, Value, i16) {
    let domain = b"starring.runtime.startup_recovery.stale_live.terminal.v2";
    let mut offset = 0;
    let domain_length = read_projection_i64(bytes, &mut offset);
    assert_eq!(domain_length, domain.len() as i64);
    assert_eq!(&bytes[offset..offset + domain.len()], domain);
    offset += domain.len();
    assert_eq!(read_projection_i16(bytes, &mut offset), 2);
    assert_eq!(read_projection_i16(bytes, &mut offset), 1);
    let mut rows = Vec::new();
    for _ in 0..5 {
        let length = read_projection_i64(bytes, &mut offset);
        assert!(length > 1);
        let length = usize::try_from(length).unwrap();
        let field = &bytes[offset..offset + length];
        assert_eq!(field[0], 1);
        rows.push(serde_json::from_slice(&field[1..]).unwrap());
        offset += length;
    }
    let recovery_kind = read_projection_i16(bytes, &mut offset);
    assert_eq!(bytes.len() - offset, 16);
    (
        rows.remove(0),
        rows.remove(0),
        rows.remove(0),
        rows.remove(0),
        rows.remove(0),
        recovery_kind,
    )
}

fn read_projection_i64(bytes: &[u8], offset: &mut usize) -> i64 {
    let end = *offset + 8;
    let value = i64::from_be_bytes(bytes[*offset..end].try_into().unwrap());
    *offset = end;
    value
}

fn read_projection_i16(bytes: &[u8], offset: &mut usize) -> i16 {
    let end = *offset + 2;
    let value = i16::from_be_bytes(bytes[*offset..end].try_into().unwrap());
    *offset = end;
    value
}
