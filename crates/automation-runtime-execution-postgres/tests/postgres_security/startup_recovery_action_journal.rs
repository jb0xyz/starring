type RecoveryActionJournalOwner = (String, String, i64, String, i64, DateTime<Utc>);
type RecoveryActionJournalOutcome = (
    String,
    String,
    String,
    i64,
    String,
    i64,
    DateTime<Utc>,
    DateTime<Utc>,
    DateTime<Utc>,
    String,
);

#[derive(Clone)]
struct RecoveryActionJournalInput {
    recovery_id: String,
    originating_emergency_generation: i64,
    coordinator_generation: i64,
    action_authority_revision: i64,
    selection_authority_revision: i64,
    recovery_class: String,
    owner: RecoveryActionJournalOwner,
    minimum_database_now: DateTime<Utc>,
    terminal_projection_bytes: Vec<u8>,
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn startup_recovery_action_journal_is_exact_private_and_append_only() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let owner = acquire_recovery_action_journal_owner(
        &database.executor_pool,
        "recovery-action-journal-process",
    )
    .await;
    let minimum_database_now = database_now(&database.owner_pool).await;
    let input = recovery_action_journal_input(
        &owner,
        "00112233445566778899aabbccddeeff",
        1,
        "stale_live",
        minimum_database_now,
        br#"{"outcome":"progressed"}"#,
    );

    let isolation_error = record_recovery_action(&database.owner_pool, &input, false, "5s")
        .await
        .unwrap_err();
    assert_sqlstate(&isolation_error, "RX004");

    let applied = record_recovery_action(&database.owner_pool, &input, true, "5s")
        .await
        .unwrap();
    assert_eq!(applied.0, "applied");
    assert_eq!(applied.1, owner.0);
    assert_eq!(applied.2, owner.1);
    assert_eq!(applied.3, owner.2);
    assert_eq!(applied.4, owner.3);
    assert_eq!(applied.5, owner.4);
    assert!(applied.6 >= input.minimum_database_now);
    assert_eq!(applied.7, owner.5);
    assert_eq!(applied.8, applied.6);
    assert!(canonical_sha256(&applied.9));

    let replayed = record_recovery_action(&database.owner_pool, &input, true, "5s")
        .await
        .unwrap();
    assert_eq!(replayed.0, "replayed");
    assert_eq!(replayed.8, applied.8);
    assert_eq!(replayed.9, applied.9);
    assert_eq!(
        recovery_action_journal_count(&database.owner_pool, input.recovery_id.as_str()).await,
        1
    );

    let persisted_digest = recovery_action_journal_digest(
        &database.owner_pool,
        &input,
        applied.8,
        input.recovery_class.as_str(),
        input.action_authority_revision,
        input.selection_authority_revision,
        input.terminal_projection_bytes.as_slice(),
    )
    .await;
    assert_eq!(persisted_digest, applied.9);
    let mut class_digests = BTreeSet::new();
    for recovery_class in [
        "stale_live",
        "reserved_awaiting_certification",
        "suspended_local_effect",
        "pending_runtime_drain_intent",
    ] {
        class_digests.insert(
            recovery_action_journal_digest(
                &database.owner_pool,
                &input,
                applied.8,
                recovery_class,
                input.action_authority_revision,
                input.selection_authority_revision,
                input.terminal_projection_bytes.as_slice(),
            )
            .await,
        );
    }
    assert_eq!(class_digests.len(), 4);
    let different_action_digest = recovery_action_journal_digest(
        &database.owner_pool,
        &input,
        applied.8,
        input.recovery_class.as_str(),
        3,
        2,
        input.terminal_projection_bytes.as_slice(),
    )
    .await;
    assert_ne!(different_action_digest, applied.9);
    let different_projection_digest = recovery_action_journal_digest(
        &database.owner_pool,
        &input,
        applied.8,
        input.recovery_class.as_str(),
        input.action_authority_revision,
        input.selection_authority_revision,
        br#"{"outcome":"no_candidate"}"#,
    )
    .await;
    assert_ne!(different_projection_digest, applied.9);
    let mut different_owner = input.clone();
    different_owner.owner.4 += 1;
    let different_owner_digest = recovery_action_journal_digest(
        &database.owner_pool,
        &different_owner,
        applied.8,
        input.recovery_class.as_str(),
        input.action_authority_revision,
        input.selection_authority_revision,
        input.terminal_projection_bytes.as_slice(),
    )
    .await;
    assert_ne!(different_owner_digest, applied.9);
    let different_time_digest = recovery_action_journal_digest(
        &database.owner_pool,
        &input,
        applied.8 + TimeDelta::microseconds(1),
        input.recovery_class.as_str(),
        input.action_authority_revision,
        input.selection_authority_revision,
        input.terminal_projection_bytes.as_slice(),
    )
    .await;
    assert_ne!(different_time_digest, applied.9);

    let mut replay_mismatch = input.clone();
    replay_mismatch.terminal_projection_bytes = br#"{"outcome":"no_candidate"}"#.to_vec();
    let replay_mismatch_error =
        record_recovery_action(&database.owner_pool, &replay_mismatch, true, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&replay_mismatch_error, "RX003");
    let mut class_reinterpretation = input.clone();
    class_reinterpretation.recovery_class =
        "reserved_awaiting_certification".to_string();
    let class_reinterpretation_error = record_recovery_action(
        &database.owner_pool,
        &class_reinterpretation,
        true,
        "5s",
    )
    .await
    .unwrap_err();
    assert_sqlstate(&class_reinterpretation_error, "RX003");

    assert_recovery_action_executor_denied_after_guc_spoof(
        &database,
        &input,
        &applied,
    )
    .await;
    let private_helper_error =
        record_recovery_action(&database.executor_pool, &input, true, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&private_helper_error, "42501");
    let private_digest_error = recovery_action_journal_digest_result(
        &database.executor_pool,
        &input,
        applied.8,
        input.recovery_class.as_str(),
        input.action_authority_revision,
        input.selection_authority_revision,
        input.terminal_projection_bytes.as_slice(),
    )
    .await
    .unwrap_err();
    assert_sqlstate(&private_digest_error, "42501");
    let table_read_error = sqlx::query(
        "SELECT recovery_id \
         FROM public.runtime_startup_recovery_actions_v2",
    )
    .fetch_optional(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&table_read_error, "42501");

    assert_recovery_action_direct_mutations_rejected(&database, &input).await;

    let second_minimum = database_now(&database.owner_pool).await;
    assert!(second_minimum >= applied.8);
    let second = recovery_action_journal_input(
        &owner,
        input.recovery_id.as_str(),
        2,
        "reserved_awaiting_certification",
        second_minimum,
        br#"{"outcome":"no_candidate"}"#,
    );
    let second_applied = record_recovery_action(&database.owner_pool, &second, true, "5s")
        .await
        .unwrap();
    assert_eq!(second_applied.0, "applied");
    let third = recovery_action_journal_input(
        &owner,
        input.recovery_id.as_str(),
        3,
        "suspended_local_effect",
        database_now(&database.owner_pool).await,
        br#"{"outcome":"suspended"}"#,
    );
    assert!(third.minimum_database_now >= second_applied.8);
    let third_applied = record_recovery_action(&database.owner_pool, &third, true, "5s")
        .await
        .unwrap();
    assert_eq!(third_applied.0, "applied");
    let fourth = recovery_action_journal_input(
        &owner,
        input.recovery_id.as_str(),
        4,
        "pending_runtime_drain_intent",
        database_now(&database.owner_pool).await,
        br#"{"outcome":"drained"}"#,
    );
    assert!(fourth.minimum_database_now >= third_applied.8);
    let fourth_applied = record_recovery_action(&database.owner_pool, &fourth, true, "5s")
        .await
        .unwrap();
    assert_eq!(fourth_applied.0, "applied");
    assert_eq!(
        recovery_action_journal_count(&database.owner_pool, input.recovery_id.as_str()).await,
        4
    );

    let mut stale_lower_bound = recovery_action_journal_input(
        &owner,
        input.recovery_id.as_str(),
        5,
        "stale_live",
        applied.8,
        br#"{"outcome":"progressed_again"}"#,
    );
    stale_lower_bound.minimum_database_now = applied.8;
    let stale_lower_bound_error =
        record_recovery_action(&database.owner_pool, &stale_lower_bound, true, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&stale_lower_bound_error, "RX004");

    let mut origin_conflict = recovery_action_journal_input(
        &owner,
        input.recovery_id.as_str(),
        5,
        "stale_live",
        database_now(&database.owner_pool).await,
        br#"{"outcome":"progressed_again"}"#,
    );
    origin_conflict.originating_emergency_generation += 1;
    let origin_conflict_error =
        record_recovery_action(&database.owner_pool, &origin_conflict, true, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&origin_conflict_error, "RX003");

    let malformed = recovery_action_journal_input(
        &owner,
        "not-a-recovery-id",
        1,
        "stale_live",
        database_now(&database.owner_pool).await,
        b"x",
    );
    let malformed_error = record_recovery_action(&database.owner_pool, &malformed, true, "5s")
        .await
        .unwrap_err();
    assert_sqlstate(&malformed_error, "RX002");
    let mut overflow = input.clone();
    overflow.recovery_id = "99999999aaaaaaaabbbbbbbbcccccccc".to_string();
    overflow.selection_authority_revision = i64::MAX;
    overflow.action_authority_revision = i64::MAX;
    let overflow_error =
        record_recovery_action(&database.owner_pool, &overflow, true, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&overflow_error, "RX002");

    let future_minimum = database_now(&database.owner_pool).await + TimeDelta::seconds(30);
    let future_clock = recovery_action_journal_input(
        &owner,
        "ffeeddccbbaa99887766554433221100",
        1,
        "stale_live",
        future_minimum,
        b"x",
    );
    let future_clock_error =
        record_recovery_action(&database.owner_pool, &future_clock, true, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&future_clock_error, "RX004");

    force_recovery_action_recorded_at_into_future(&database.owner_pool, &input).await;
    let replay_clock_error = record_recovery_action(&database.owner_pool, &input, true, "5s")
        .await
        .unwrap_err();
    assert_sqlstate(&replay_clock_error, "RX004");

    let renewed = sqlx::query_as::<_, (String, i64, DateTime<Utc>)>(
        "SELECT outcome_name, owner_revision, expires_at \
         FROM public.starring_runtime_gateway_owner_renew_v1(\
            $1,$2,$3,$4,$5,5000\
         )",
    )
    .bind(&owner.0)
    .bind(&owner.1)
    .bind(owner.2)
    .bind(&owner.3)
    .bind(owner.4)
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    assert_eq!(renewed.0, "renewed");
    assert_eq!(renewed.1, owner.4 + 1);
    assert!(renewed.2 < owner.5);
    let old_owner_error =
        record_recovery_action(&database.owner_pool, &input, true, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&old_owner_error, "RX001");
    let renewed_owner = (
        owner.0.clone(),
        owner.1.clone(),
        owner.2,
        owner.3.clone(),
        renewed.1,
        renewed.2,
    );
    let release_outcome = sqlx::query_scalar::<_, String>(
        "SELECT outcome_name \
         FROM public.starring_runtime_gateway_owner_release_v1($1,$2,$3,$4)",
    )
    .bind(&renewed_owner.0)
    .bind(&renewed_owner.1)
    .bind(renewed_owner.2)
    .bind(&renewed_owner.3)
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    assert_eq!(release_outcome, "released");
    let released_input = recovery_action_journal_input(
        &renewed_owner,
        "aaaaaaaa11111111bbbbbbbb22222222",
        1,
        "stale_live",
        database_now(&database.owner_pool).await,
        b"released",
    );
    let released_error =
        record_recovery_action(&database.owner_pool, &released_input, true, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&released_error, "RX001");

    let expiring_owner = acquire_recovery_action_journal_owner_for(
        &database.executor_pool,
        "recovery-action-expiring-process",
        1000,
    )
    .await;
    let expiring_input = recovery_action_journal_input(
        &expiring_owner,
        "cccccccc33333333dddddddd44444444",
        1,
        "stale_live",
        database_now(&database.owner_pool).await,
        b"expired",
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let expired_error =
        record_recovery_action(&database.owner_pool, &expiring_input, true, "5s")
            .await
            .unwrap_err();
    assert_sqlstate(&expired_error, "RX001");

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn startup_recovery_action_journal_shared_lock_and_serializable_retry_are_exact() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let lock_order_valid = sqlx::query_scalar::<_, bool>(
        "WITH definition AS (\
            SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(\
                'starring_runtime_private_v2.starring_runtime_startup_recovery_action_record_v2(text,bigint,bigint,bigint,bigint,text,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bytea)'\
            )) AS body\
         ) \
         SELECT pg_catalog.strpos(body, 'pg_catalog.pg_advisory_xact_lock_shared(') > 0 \
            AND pg_catalog.strpos(body, 'pg_catalog.pg_advisory_xact_lock_shared(') \
                < pg_catalog.strpos(body, 'starring-runtime-gateway-owner-v1:') \
            AND pg_catalog.strpos(body, 'starring-runtime-gateway-owner-v1:') \
                < pg_catalog.strpos(body, 'starring-runtime-startup-recovery-action-v2:') \
            AND pg_catalog.strpos(body, 'starring-runtime-startup-recovery-action-v2:') \
                < pg_catalog.strpos(body, 'FROM public.runtime_gateway_owners') \
         FROM definition",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert!(lock_order_valid);

    let owner = acquire_recovery_action_journal_owner(
        &database.executor_pool,
        "recovery-action-concurrency-process",
    )
    .await;
    let mut shared_holder = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock_shared(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *shared_holder)
    .await
    .unwrap();
    let compatible = recovery_action_journal_input(
        &owner,
        "0123456789abcdeffedcba9876543210",
        1,
        "stale_live",
        database_now(&database.owner_pool).await,
        b"shared-compatible",
    );
    let compatible_outcome =
        record_recovery_action(&database.owner_pool, &compatible, true, "500ms")
            .await
            .unwrap();
    assert_eq!(compatible_outcome.0, "applied");
    shared_holder.rollback().await.unwrap();

    let concurrent = recovery_action_journal_input(
        &owner,
        "abcdef0123456789abcdef0123456789",
        1,
        "pending_runtime_drain_intent",
        database_now(&database.owner_pool).await,
        b"concurrent",
    );
    let absence_barrier = tokio::sync::Barrier::new(2);
    let (left, right) = tokio::join!(
        record_recovery_action_after_absence_snapshot(
            &database.owner_pool,
            &concurrent,
            "5s",
            &absence_barrier
        ),
        record_recovery_action_after_absence_snapshot(
            &database.owner_pool,
            &concurrent,
            "5s",
            &absence_barrier
        )
    );
    let mut outcomes = Vec::new();
    let mut serialization_retry_observed = false;
    for result in [left, right] {
        match result {
            Ok(outcome) => outcomes.push(outcome.0),
            Err(error) => {
                assert_sqlstate(&error, "40001");
                serialization_retry_observed = true;
                let replay =
                    record_recovery_action(&database.owner_pool, &concurrent, true, "5s")
                        .await
                        .unwrap();
                outcomes.push(replay.0);
            }
        }
    }
    assert!(serialization_retry_observed);
    outcomes.sort_unstable();
    assert_eq!(outcomes, ["applied", "replayed"]);
    assert_eq!(
        recovery_action_journal_count(
            &database.owner_pool,
            concurrent.recovery_id.as_str()
        )
        .await,
        1
    );

    cleanup(database).await;
    drop(server);
}

fn recovery_action_journal_input(
    owner: &RecoveryActionJournalOwner,
    recovery_id: &str,
    selection_authority_revision: i64,
    recovery_class: &str,
    minimum_database_now: DateTime<Utc>,
    terminal_projection_bytes: &[u8],
) -> RecoveryActionJournalInput {
    RecoveryActionJournalInput {
        recovery_id: recovery_id.to_string(),
        originating_emergency_generation: 7,
        coordinator_generation: 11,
        action_authority_revision: selection_authority_revision + 1,
        selection_authority_revision,
        recovery_class: recovery_class.to_string(),
        owner: owner.clone(),
        minimum_database_now,
        terminal_projection_bytes: terminal_projection_bytes.to_vec(),
    }
}

async fn acquire_recovery_action_journal_owner(
    pool: &PgPool,
    process_instance_id: &str,
) -> RecoveryActionJournalOwner {
    acquire_recovery_action_journal_owner_for(pool, process_instance_id, 300_000).await
}

async fn acquire_recovery_action_journal_owner_for(
    pool: &PgPool,
    process_instance_id: &str,
    lease_milliseconds: i64,
) -> RecoveryActionJournalOwner {
    sqlx::query_as(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
            expected_build_revision, owner_revision, expires_at \
         FROM public.starring_runtime_gateway_owner_acquire_v1(\
            'shard:0', $1, 'recovery-action-build', $2\
         ) \
         WHERE outcome_name = 'acquired'",
    )
    .bind(process_instance_id)
    .bind(lease_milliseconds)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn record_recovery_action(
    pool: &PgPool,
    input: &RecoveryActionJournalInput,
    serializable: bool,
    statement_timeout: &str,
) -> Result<RecoveryActionJournalOutcome, sqlx::Error> {
    record_recovery_action_with_snapshot(
        pool,
        input,
        serializable,
        statement_timeout,
        None,
    )
    .await
}

async fn record_recovery_action_after_absence_snapshot(
    pool: &PgPool,
    input: &RecoveryActionJournalInput,
    statement_timeout: &str,
    absence_barrier: &tokio::sync::Barrier,
) -> Result<RecoveryActionJournalOutcome, sqlx::Error> {
    record_recovery_action_with_snapshot(
        pool,
        input,
        true,
        statement_timeout,
        Some(absence_barrier),
    )
    .await
}

async fn record_recovery_action_with_snapshot(
    pool: &PgPool,
    input: &RecoveryActionJournalInput,
    serializable: bool,
    statement_timeout: &str,
    absence_barrier: Option<&tokio::sync::Barrier>,
) -> Result<RecoveryActionJournalOutcome, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    if serializable {
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
            .execute(&mut *transaction)
            .await?;
    }
    sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.set_config('statement_timeout', $1, TRUE)",
    )
    .bind(statement_timeout)
    .fetch_one(&mut *transaction)
    .await?;
    if let Some(absence_barrier) = absence_barrier {
        let observed_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_id = $1",
        )
        .bind(&input.recovery_id)
        .fetch_one(&mut *transaction)
        .await?;
        assert_eq!(observed_count, 0);
        absence_barrier.wait().await;
    }
    let result = sqlx::query_as::<_, RecoveryActionJournalOutcome>(
        "SELECT outcome_name, observed_gateway_shard_id, \
            observed_process_instance_id, observed_lease_epoch, \
            observed_runtime_build_revision, observed_owner_revision, \
            database_now, observed_owner_expires_at, recorded_at, terminal_digest \
         FROM starring_runtime_private_v2.\
            starring_runtime_startup_recovery_action_record_v2(\
                $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14\
            )",
    )
    .bind(&input.recovery_id)
    .bind(input.originating_emergency_generation)
    .bind(input.coordinator_generation)
    .bind(input.action_authority_revision)
    .bind(input.selection_authority_revision)
    .bind(&input.recovery_class)
    .bind(&input.owner.0)
    .bind(&input.owner.1)
    .bind(input.owner.2)
    .bind(&input.owner.3)
    .bind(input.owner.4)
    .bind(input.owner.5)
    .bind(input.minimum_database_now)
    .bind(&input.terminal_projection_bytes)
    .fetch_one(&mut *transaction)
    .await;
    match result {
        Ok(outcome) => {
            let gates_clear = sqlx::query_scalar::<_, bool>(
                "SELECT \
                    COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_gate_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_format_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_recovery_id_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_origin_generation_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_coordinator_generation_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_authority_revision_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_selection_revision_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_class_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_gateway_shard_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_owner_process_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_owner_lease_epoch_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_owner_build_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_owner_revision_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_owner_expires_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_minimum_database_now_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_terminal_digest_v2', TRUE), '') = '' \
                    AND COALESCE(pg_catalog.current_setting('starring.runtime_startup_recovery_action_recorded_at_v2', TRUE), '') = ''",
            )
            .fetch_one(&mut *transaction)
            .await?;
            assert!(gates_clear);
            transaction.commit().await?;
            Ok(outcome)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn recovery_action_journal_digest(
    pool: &PgPool,
    input: &RecoveryActionJournalInput,
    recorded_at: DateTime<Utc>,
    recovery_class: &str,
    action_authority_revision: i64,
    selection_authority_revision: i64,
    terminal_projection_bytes: &[u8],
) -> String {
    recovery_action_journal_digest_result(
        pool,
        input,
        recorded_at,
        recovery_class,
        action_authority_revision,
        selection_authority_revision,
        terminal_projection_bytes,
    )
    .await
    .unwrap()
}

async fn recovery_action_journal_digest_result(
    pool: &PgPool,
    input: &RecoveryActionJournalInput,
    recorded_at: DateTime<Utc>,
    recovery_class: &str,
    action_authority_revision: i64,
    selection_authority_revision: i64,
    terminal_projection_bytes: &[u8],
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT starring_runtime_private_v2.\
            starring_runtime_startup_recovery_terminal_digest_v2(\
                2::SMALLINT,$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15\
            )",
    )
    .bind(&input.recovery_id)
    .bind(input.originating_emergency_generation)
    .bind(input.coordinator_generation)
    .bind(action_authority_revision)
    .bind(selection_authority_revision)
    .bind(recovery_class)
    .bind(&input.owner.0)
    .bind(&input.owner.1)
    .bind(input.owner.2)
    .bind(&input.owner.3)
    .bind(input.owner.4)
    .bind(input.owner.5)
    .bind(input.minimum_database_now)
    .bind(recorded_at)
    .bind(terminal_projection_bytes)
    .fetch_one(pool)
    .await
}

async fn recovery_action_journal_count(pool: &PgPool, recovery_id: &str) -> i64 {
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

async fn assert_recovery_action_executor_denied_after_guc_spoof(
    database: &IsolatedDatabase,
    input: &RecoveryActionJournalInput,
    applied: &RecoveryActionJournalOutcome,
) {
    let mut transaction = database.executor_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_gate_v2', 'insert', TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_format_v2', '2', TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_recovery_id_v2', $1, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_origin_generation_v2', $2::TEXT, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_coordinator_generation_v2', $3::TEXT, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_authority_revision_v2', $4::TEXT, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_selection_revision_v2', $5::TEXT, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_class_v2', $6, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_gateway_shard_v2', $7, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_owner_process_v2', $8, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_owner_lease_epoch_v2', $9::TEXT, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_owner_build_v2', $10, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_owner_revision_v2', $11::TEXT, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_owner_expires_v2', pg_catalog.encode(pg_catalog.timestamptz_send($12), 'hex'), TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_minimum_database_now_v2', pg_catalog.encode(pg_catalog.timestamptz_send($13), 'hex'), TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_terminal_digest_v2', $14, TRUE), \
            pg_catalog.set_config('starring.runtime_startup_recovery_action_recorded_at_v2', pg_catalog.encode(pg_catalog.timestamptz_send($15), 'hex'), TRUE)",
    )
    .bind(&input.recovery_id)
    .bind(input.originating_emergency_generation)
    .bind(input.coordinator_generation)
    .bind(input.action_authority_revision)
    .bind(input.selection_authority_revision)
    .bind(&input.recovery_class)
    .bind(&input.owner.0)
    .bind(&input.owner.1)
    .bind(input.owner.2)
    .bind(&input.owner.3)
    .bind(input.owner.4)
    .bind(input.owner.5)
    .bind(input.minimum_database_now)
    .bind(&applied.9)
    .bind(applied.8)
    .execute(&mut *transaction)
    .await
    .unwrap();
    let denied = sqlx::query(
        "INSERT INTO public.runtime_startup_recovery_actions_v2 (\
            record_format_version, recovery_id, \
            originating_emergency_generation, coordinator_generation, \
            action_authority_revision, selection_authority_revision, \
            recovery_class, gateway_shard_id, owner_process_instance_id, \
            owner_lease_epoch, owner_runtime_build_revision, owner_revision, \
            owner_expires_at, minimum_database_now, \
            terminal_projection_bytes, terminal_digest, recorded_at\
         ) VALUES (\
            2,$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16\
         )",
    )
    .bind(&input.recovery_id)
    .bind(input.originating_emergency_generation)
    .bind(input.coordinator_generation)
    .bind(input.action_authority_revision)
    .bind(input.selection_authority_revision)
    .bind(&input.recovery_class)
    .bind(&input.owner.0)
    .bind(&input.owner.1)
    .bind(input.owner.2)
    .bind(&input.owner.3)
    .bind(input.owner.4)
    .bind(input.owner.5)
    .bind(input.minimum_database_now)
    .bind(&input.terminal_projection_bytes)
    .bind(&applied.9)
    .bind(applied.8)
    .execute(&mut *transaction)
    .await
    .unwrap_err();
    assert_sqlstate(&denied, "42501");
    transaction.rollback().await.unwrap();
}

async fn assert_recovery_action_direct_mutations_rejected(
    database: &IsolatedDatabase,
    input: &RecoveryActionJournalInput,
) {
    let insert = sqlx::query(
        "INSERT INTO public.runtime_startup_recovery_actions_v2 (\
            record_format_version, recovery_id, \
            originating_emergency_generation, coordinator_generation, \
            action_authority_revision, selection_authority_revision, \
            recovery_class, gateway_shard_id, owner_process_instance_id, \
            owner_lease_epoch, owner_runtime_build_revision, owner_revision, \
            owner_expires_at, minimum_database_now, \
            terminal_projection_bytes, terminal_digest, recorded_at\
         ) VALUES (\
            2,'11112222333344445555666677778888',7,11,2,1,\
            'stale_live',$1,$2,$3,$4,$5,$6,$7,'x',\
            pg_catalog.repeat('a',64),$7\
         )",
    )
    .bind(&input.owner.0)
    .bind(&input.owner.1)
    .bind(input.owner.2)
    .bind(&input.owner.3)
    .bind(input.owner.4)
    .bind(input.owner.5)
    .bind(input.minimum_database_now);
    let mut transaction = database.owner_pool.begin().await.unwrap();
    let insert_error = insert.execute(&mut *transaction).await.unwrap_err();
    assert_sqlstate(&insert_error, "23514");
    transaction.rollback().await.unwrap();

    for statement in [
        "UPDATE public.runtime_startup_recovery_actions_v2 \
         SET terminal_digest = terminal_digest",
        "DELETE FROM public.runtime_startup_recovery_actions_v2",
        "TRUNCATE TABLE public.runtime_startup_recovery_actions_v2",
    ] {
        let mut transaction = database.owner_pool.begin().await.unwrap();
        let error = sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "23514");
        transaction.rollback().await.unwrap();
    }
}

async fn force_recovery_action_recorded_at_into_future(
    pool: &PgPool,
    input: &RecoveryActionJournalInput,
) {
    let future_recorded_at = database_now(pool).await + TimeDelta::seconds(30);
    assert!(future_recorded_at < input.owner.5);
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_startup_recovery_actions_v2 \
         DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_startup_recovery_actions_v2 AS action \
         SET recorded_at = $3, \
             terminal_digest = starring_runtime_private_v2.\
                starring_runtime_startup_recovery_terminal_digest_v2(\
                    action.record_format_version, action.recovery_id, \
                    action.originating_emergency_generation, \
                    action.coordinator_generation, \
                    action.action_authority_revision, \
                    action.selection_authority_revision, \
                    action.recovery_class, action.gateway_shard_id, \
                    action.owner_process_instance_id, action.owner_lease_epoch, \
                    action.owner_runtime_build_revision, action.owner_revision, \
                    action.owner_expires_at, action.minimum_database_now, \
                    $3, action.terminal_projection_bytes\
                ) \
         WHERE action.recovery_id = $1 \
            AND action.selection_authority_revision = $2",
    )
    .bind(&input.recovery_id)
    .bind(input.selection_authority_revision)
    .bind(future_recorded_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_startup_recovery_actions_v2 \
         ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}
