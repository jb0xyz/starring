#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_progresses_and_replays_exactly_once() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_claimable_deployment(&database.owner_pool).await;
    let mut claim = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *claim)
        .await
        .unwrap();
    sqlx::query(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_claim_next_v1(\
            'suspended-local-controller', 300000\
         )",
    )
    .fetch_one(&mut *claim)
    .await
    .unwrap();
    claim.commit().await.unwrap();
    seed_exact_local_suspension(&database.owner_pool).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let minimum_database_now = database_now(&database.owner_pool).await;
    let before = suspended_local_state(&database.owner_pool).await;
    let first = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "ab0000000000000000000000000000ab",
        minimum_database_now,
        41,
    )
    .await
    .unwrap();
    assert_eq!(first["journal_outcome_name"], "applied");
    assert_eq!(first["terminal_outcome_name"], "progressed");
    assert_eq!(first["recovery_class"], "suspended_local_effect");
    let after = suspended_local_state(&database.owner_pool).await;
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, before.1);
    assert_eq!(after.2, before.2 + 1);
    assert_eq!(after.3, "route_absent");
    assert_eq!(after.4, "none");
    assert_eq!(after.5, 1);

    let replay = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "ab0000000000000000000000000000ab",
        minimum_database_now,
        41,
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
    assert_eq!(suspended_local_state(&database.owner_pool).await, after);

    let mismatch = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "ab0000000000000000000000000000ab",
        minimum_database_now,
        42,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&mismatch, "RX004");
    assert_eq!(suspended_local_state(&database.owner_pool).await, after);

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_binds_no_candidate_evidence() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let minimum_database_now = database_now(&database.owner_pool).await;
    let first = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "ac0000000000000000000000000000ac",
        minimum_database_now,
        51,
    )
    .await
    .unwrap();
    assert_eq!(first["journal_outcome_name"], "applied");
    assert_eq!(first["terminal_outcome_name"], "no_candidate");
    let replay = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "ac0000000000000000000000000000ac",
        minimum_database_now,
        51,
    )
    .await
    .unwrap();
    assert_eq!(replay["journal_outcome_name"], "replayed");
    assert_eq!(replay["terminal_outcome_name"], "no_candidate");
    let mismatch = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "ac0000000000000000000000000000ac",
        minimum_database_now,
        52,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&mismatch, "RX004");
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_accepts_canonical_quiescent_root() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    prepare_local_suspension(&database, false).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let result = execute_startup_suspended_local_with_resume(
        &database.executor_pool,
        &owner,
        "ae0000000000000000000000000000ae",
        database_now(&database.owner_pool).await,
        61,
        6,
    )
    .await
    .unwrap();
    assert_eq!(result["journal_outcome_name"], "applied");
    assert_eq!(result["terminal_outcome_name"], "no_candidate");
    assert_eq!(suspended_action_count(&database.owner_pool).await, 1);
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_retains_previous_serving_obligation() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    prepare_local_suspension(&database, true).await;
    promote_suspension_to_local_and_previous(&database.owner_pool).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let result = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "a1000000a1000000a1000000a1000000",
        database_now(&database.owner_pool).await,
        66,
    )
    .await
    .unwrap();
    assert_eq!(result["terminal_outcome_name"], "progressed");
    let state = suspended_local_state(&database.owner_pool).await;
    assert_eq!(state.2, 2);
    assert_eq!(state.3, "route_absent");
    assert_eq!(state.4, "previous_serving");
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_rejects_malformed_quiescent_root() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    prepare_local_suspension(&database, false).await;
    replace_suspension_root(
        &database.owner_pool,
        br#"{"format_version":2,"local_effect":{"kind":"none"},"drain_obligation":{"kind":"none"}}"#,
        true,
    )
    .await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let error = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "af0000000000000000000000000000af",
        database_now(&database.owner_pool).await,
        71,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(suspended_action_count(&database.owner_pool).await, 0);
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_rejects_corrupt_route_absent_root() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    prepare_local_suspension(&database, true).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let minimum_database_now = database_now(&database.owner_pool).await;
    let first = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "b0000000b0000000b0000000b0000000",
        minimum_database_now,
        81,
    )
    .await
    .unwrap();
    assert_eq!(first["terminal_outcome_name"], "progressed");
    replace_suspension_root(
        &database.owner_pool,
        br#"{"format_version":2,"suspension_id":"ad0000000000000000000000000000ad"}"#,
        false,
    )
    .await;
    let error = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "b1000000b1000000b1000000b1000001",
        minimum_database_now,
        82,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(suspended_action_count(&database.owner_pool).await, 1);
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_rejects_lost_owner_and_higher_priority() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    prepare_local_suspension(&database, true).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let mut wrong_owner = owner.clone();
    wrong_owner.4 += 1;
    let owner_error = execute_startup_suspended_local(
        &database.executor_pool,
        &wrong_owner,
        "b2000000b2000000b2000000b2000002",
        database_now(&database.owner_pool).await,
        91,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&owner_error, "RX001");
    assert_eq!(suspended_action_count(&database.owner_pool).await, 0);
    expire_gateway_owner(&database.owner_pool).await;
    let expired_error = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "b7000000b7000000b7000000b7000007",
        database_now(&database.owner_pool).await,
        93,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&expired_error, "RX001");
    assert_eq!(suspended_action_count(&database.owner_pool).await, 0);
    cleanup(database).await;
    drop(server);

    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&database, 300_000).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let priority_error = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "b3000000b3000000b3000000b3000003",
        database_now(&database.owner_pool).await,
        92,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&priority_error, "RX003");
    assert_eq!(suspended_action_count(&database.owner_pool).await, 0);
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_distinguishes_pending_drain_symmetry() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    prepare_local_suspension(&database, true).await;
    seed_pending_drain(&database.owner_pool, true).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let error = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "b4000000b4000000b4000000b4000004",
        database_now(&database.owner_pool).await,
        101,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX007");
    assert_eq!(suspended_action_count(&database.owner_pool).await, 0);
    cleanup(database).await;
    drop(server);

    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    prepare_local_suspension(&database, true).await;
    seed_pending_drain(&database.owner_pool, false).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let error = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "b5000000b5000000b5000000b5000005",
        database_now(&database.owner_pool).await,
        102,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(suspended_action_count(&database.owner_pool).await, 0);
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_rolls_back_when_journal_insert_fails() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    prepare_local_suspension(&database, true).await;
    sqlx::query(
        "CREATE FUNCTION public.fail_suspended_recovery_journal_insert() \
         RETURNS TRIGGER LANGUAGE plpgsql AS $function$ \
         BEGIN RAISE EXCEPTION USING ERRCODE = 'P0001', MESSAGE = 'injected'; END; \
         $function$",
    )
    .execute(&database.owner_pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER fail_suspended_recovery_journal_insert \
         BEFORE INSERT ON public.runtime_startup_recovery_actions_v2 \
         FOR EACH ROW EXECUTE FUNCTION public.fail_suspended_recovery_journal_insert()",
    )
    .execute(&database.owner_pool)
    .await
    .unwrap();
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let before = suspended_local_state(&database.owner_pool).await;
    let error = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "b6000000b6000000b6000000b6000006",
        database_now(&database.owner_pool).await,
        111,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "P0001");
    assert_eq!(suspended_local_state(&database.owner_pool).await, before);
    assert_eq!(suspended_action_count(&database.owner_pool).await, 0);
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_exposes_only_public_capability() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let private_error = sqlx::query(
        "SELECT starring_runtime_private_v2.starring_runtime_suspended_route_bytes_v2(\
            '{}'::JSONB\
         )",
    )
    .execute(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&private_error, "42501");
    let table_error =
        sqlx::query("SELECT * FROM public.runtime_suspended_attempts_v2")
            .execute(&database.executor_pool)
            .await
            .unwrap_err();
    assert_sqlstate(&table_error, "42501");
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_serializes_competing_recovery_ids() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    prepare_local_suspension(&database, true).await;
    let owner = acquire_startup_observation_owner(&database.executor_pool).await;
    let minimum_database_now = database_now(&database.owner_pool).await;
    let left_pool = database.executor_pool.clone();
    let right_pool = database.executor_pool.clone();
    let left_owner = owner.clone();
    let right_owner = owner.clone();
    let left = tokio::spawn(async move {
        execute_startup_suspended_local(
            &left_pool,
            &left_owner,
            "d1000000d1000000d1000000d1000000",
            minimum_database_now,
            121,
        )
        .await
    });
    let right = tokio::spawn(async move {
        execute_startup_suspended_local(
            &right_pool,
            &right_owner,
            "d2000000d2000000d2000000d2000000",
            minimum_database_now,
            121,
        )
        .await
    });
    let outcomes = [left.await.unwrap(), right.await.unwrap()];
    let progressed_count = outcomes
        .iter()
        .filter(|result| {
            result
                .as_ref()
                .is_ok_and(|value| value["terminal_outcome_name"] == "progressed")
        })
        .count();
    assert_eq!(progressed_count, 1);
    let state = suspended_local_state(&database.owner_pool).await;
    assert_eq!(state.2, 2);
    assert_eq!(state.3, "route_absent");
    assert_eq!(state.4, "none");
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_suspended_local_execution_defers_to_unresolved_reservation() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;
    let owner = sqlx::query_as(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
            expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners \
         WHERE process_instance_id IS NOT NULL",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let error = execute_startup_suspended_local(
        &database.executor_pool,
        &owner,
        "d3000000d3000000d3000000d3000000",
        database_now(&database.owner_pool).await,
        131,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX003");
    assert_eq!(suspended_action_count(&database.owner_pool).await, 0);
    cleanup(database).await;
    drop(server);
}

async fn prepare_local_suspension(database: &IsolatedDatabase, exact: bool) {
    seed_claimable_deployment(&database.owner_pool).await;
    let mut claim = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *claim)
        .await
        .unwrap();
    sqlx::query(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_claim_next_v1(\
            'suspended-local-controller', 300000\
         )",
    )
    .fetch_one(&mut *claim)
    .await
    .unwrap();
    claim.commit().await.unwrap();
    seed_local_suspension(&database.owner_pool, exact).await;
}

async fn promote_suspension_to_local_and_previous(pool: &PgPool) {
    let (fencing_token, root_v1, suspended_at) =
        sqlx::query_as::<_, (i64, String, DateTime<Utc>)>(
            "SELECT deployment.controller_fencing_token, \
                pg_catalog.convert_from(root.suspend_attempt_request_bytes, 'UTF8'), \
                suspended.suspended_at \
             FROM public.runtime_deployments AS deployment \
             INNER JOIN public.runtime_suspend_attempt_operations_v2 AS root \
                ON root.deployment_id = deployment.deployment_id \
             INNER JOIN public.runtime_suspended_attempts_v2 AS suspended \
                ON suspended.suspension_id = root.suspension_id \
             WHERE deployment.deployment_id = $1",
        )
        .bind(DEPLOYMENT)
        .fetch_one(pool)
        .await
        .unwrap();
    let route = format!(
        concat!(
            "{{\"identity\":{{\"target\":{{\"guild_id\":\"{}\",",
            "\"ruleset_key\":\"{}\",\"version\":1,\"content_hash\":\"{}\",",
            "\"binding_revision\":1,\"binding_fingerprint\":\"{}\"}},",
            "\"runtime_generation\":2,\"process_instance_id\":\"process:local\"}},",
            "\"controller_fencing_token\":{},\"route_incarnation\":1}}"
        ),
        GUILD, RULESET, CONTENT_HASH, BINDING_FINGERPRINT, fencing_token
    );
    let previous_process = format!(
        concat!(
            "{{\"target\":{{\"guild_id\":\"{}\",\"ruleset_key\":\"{}\",",
            "\"version\":1,\"content_hash\":\"{}\",\"binding_revision\":1,",
            "\"binding_fingerprint\":\"{}\"}},\"runtime_generation\":1,",
            "\"process_instance_id\":\"process:previous\"}}"
        ),
        GUILD, RULESET, CONTENT_HASH, BINDING_FINGERPRINT
    );
    let previous = format!(
        concat!(
            "{{\"scope\":{{\"tenant_id\":\"{}\",\"installation_id\":\"{}\",",
            "\"deployment_id\":\"previous-deployment\"}},",
            "\"attestation_id\":\"{}\",\"process\":{},",
            "\"lease_epoch\":1,\"revision\":1}}"
        ),
        TENANT,
        INSTALLATION,
        "d".repeat(64),
        previous_process
    );
    let local_effect = format!(
        "{{\"kind\":\"exact_route\",\"route\":{route},\"lifecycle\":\"staged\"}}"
    );
    let exact_drain =
        format!("{{\"kind\":\"exact_local_route\",\"route\":{route}}}");
    let local_and_previous = format!(
        "{{\"kind\":\"local_and_previous\",\"local\":{route},\"previous\":{previous}}}"
    );
    let mut root_v2 =
        root_v1.replace("\"runtime_generation\":1", "\"runtime_generation\":2");
    assert!(root_v2.contains(&exact_drain));
    root_v2 = root_v2.replacen(&exact_drain, &local_and_previous, 1);
    let digest = suspension_digest(root_v2.as_bytes());
    let mut transaction = pool.begin().await.unwrap();
    for statement in [
        "ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER",
        "ALTER TABLE public.runtime_suspend_attempt_operations_v2 DISABLE TRIGGER USER",
        "ALTER TABLE public.runtime_suspended_attempts_v2 DISABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    sqlx::query(
        "DELETE FROM public.runtime_suspended_attempts_v2 \
         WHERE suspension_id = 'ad0000000000000000000000000000ad'",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_suspend_attempt_operations_v2 \
         SET suspend_attempt_request_bytes = pg_catalog.convert_to($1, 'UTF8'), \
            suspend_attempt_digest = $2 \
         WHERE suspension_id = 'ad0000000000000000000000000000ad'",
    )
    .bind(&root_v2)
    .bind(&digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET runtime_generation = 2, \
            snapshot = pg_catalog.jsonb_set(\
                pg_catalog.jsonb_set(\
                    snapshot, '{runtime_generation}', '2'::JSONB, FALSE\
                ), \
                '{previous_runtime}', $1::JSONB, FALSE\
            ) \
         WHERE deployment_id = $2",
    )
    .bind(&previous_process)
    .bind(DEPLOYMENT)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_suspended_attempts_v2 (\
            suspension_id, suspend_attempt_digest, tenant_id, installation_id, \
            deployment_id, deployment_revision, convergence_attempt_no, \
            sidecar_revision, slot_guild_id, slot_ruleset_key, local_effect_kind, \
            local_effect_bytes, drain_obligation_kind, drain_obligation_bytes, \
            suspended_at\
         ) SELECT suspension_id, suspend_attempt_digest, tenant_id, installation_id, \
            deployment_id, deployment_revision, convergence_attempt_no, 1, $1, $2, \
            'exact_route', pg_catalog.convert_to($3, 'UTF8'), \
            'local_and_previous', pg_catalog.convert_to($4, 'UTF8'), $5 \
         FROM public.runtime_suspend_attempt_operations_v2 \
         WHERE suspension_id = 'ad0000000000000000000000000000ad'",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(&local_effect)
    .bind(&local_and_previous)
    .bind(suspended_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    for statement in [
        "ALTER TABLE public.runtime_suspended_attempts_v2 ENABLE TRIGGER USER",
        "ALTER TABLE public.runtime_suspend_attempt_operations_v2 ENABLE TRIGGER USER",
        "ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER",
    ] {
        sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn expire_gateway_owner(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_gateway_owners DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_gateway_owners \
         SET expires_at = pg_catalog.clock_timestamp() - INTERVAL '1 second' \
         WHERE gateway_shard_id = 'shard:0'",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_gateway_owners ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn seed_pending_drain(pool: &PgPool, symmetric: bool) {
    let snapshot = product_drain_snapshot(pool).await;
    let canonical = canonical_product_drain(&snapshot);
    seed_canonical_product_drain(pool, &canonical).await;
    if !symmetric {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query(
            "ALTER TABLE public.runtime_slot_writer_fences_v2 \
             DISABLE TRIGGER USER",
        )
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
        sqlx::query(
            "ALTER TABLE public.runtime_slot_writer_fences_v2 \
             ENABLE TRIGGER USER",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
    }
}

async fn replace_suspension_root(pool: &PgPool, payload: &[u8], quiescent: bool) {
    let digest = suspension_digest(payload);
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
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
        "ALTER TABLE public.runtime_suspended_attempts_v2 \
         DROP CONSTRAINT runtime_suspended_attempts_v2_root_fk",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_suspend_attempt_operations_v2 \
         SET suspend_attempt_request_bytes = $1, suspend_attempt_digest = $2 \
         WHERE suspension_id = 'ad0000000000000000000000000000ad'",
    )
    .bind(payload)
    .bind(&digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    if quiescent {
        sqlx::query(
            "UPDATE public.runtime_suspended_attempts_v2 \
             SET suspend_attempt_digest = $1, \
                local_effect_kind = 'none', \
                local_effect_bytes = pg_catalog.convert_to('{\"kind\":\"none\"}', 'UTF8'), \
                drain_obligation_kind = 'none', \
                drain_obligation_bytes = pg_catalog.convert_to('{\"kind\":\"none\"}', 'UTF8') \
             WHERE suspension_id = 'ad0000000000000000000000000000ad'",
        )
        .bind(&digest)
        .execute(&mut *transaction)
        .await
        .unwrap();
    } else {
        sqlx::query(
            "UPDATE public.runtime_suspended_attempts_v2 \
             SET suspend_attempt_digest = $1 \
             WHERE suspension_id = 'ad0000000000000000000000000000ad'",
        )
        .bind(&digest)
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
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

async fn suspended_action_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_startup_recovery_actions_v2 \
         WHERE recovery_class = 'suspended_local_effect'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_exact_local_suspension(pool: &PgPool) {
    seed_local_suspension(pool, true).await;
}

async fn seed_local_suspension(pool: &PgPool, exact: bool) {
    let deployment = sqlx::query_as::<_, (i64, i64, String, i64, DateTime<Utc>)>(
        "SELECT revision, convergence_attempt_no, controller_id, \
            controller_fencing_token, \
            (snapshot ->> 'requested_at')::TIMESTAMPTZ \
         FROM public.runtime_deployments \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap();
    let route = format!(
        concat!(
            "{{\"identity\":{{\"target\":{{\"guild_id\":\"{}\",",
            "\"ruleset_key\":\"{}\",\"version\":1,\"content_hash\":\"{}\",",
            "\"binding_revision\":1,\"binding_fingerprint\":\"{}\"}},",
            "\"runtime_generation\":1,\"process_instance_id\":\"process:local\"}},",
            "\"controller_fencing_token\":{},\"route_incarnation\":1}}"
        ),
        GUILD,
        RULESET,
        CONTENT_HASH,
        BINDING_FINGERPRINT,
        deployment.3
    );
    let (local_effect_kind, local_effect, drain_obligation_kind, drain_obligation) = if exact {
        (
            "exact_route",
            format!(
                "{{\"kind\":\"exact_route\",\"route\":{route},\"lifecycle\":\"staged\"}}"
            ),
            "exact_local_route",
            format!("{{\"kind\":\"exact_local_route\",\"route\":{route}}}"),
        )
    } else {
        (
            "none",
            "{\"kind\":\"none\"}".to_string(),
            "none",
            "{\"kind\":\"none\"}".to_string(),
        )
    };
    let root = format!(
        concat!(
            "{{\"format_version\":2,",
            "\"suspension_id\":\"ad0000000000000000000000000000ad\",",
            "\"action_id\":1,\"guard\":{{\"scope\":{{\"tenant_id\":\"{}\",",
            "\"installation_id\":\"{}\",\"deployment_id\":\"{}\"}},",
            "\"expected_revision\":{},\"controller_id\":\"{}\",",
            "\"fencing_token\":{},\"runtime_generation\":1,",
            "\"convergence_attempt\":{}}},\"source_phase\":\"requested\",",
            "\"failure\":{{\"failure_id\":\"failure:1\",",
            "\"kind\":\"environment_unavailable\",",
            "\"code\":\"dependency_unavailable\",",
            "\"message\":\"dependency unavailable\",",
            "\"recorded_at_unix_microseconds\":{}}},",
            "\"disposition\":{{\"kind\":\"retryable\",",
            "\"retry_not_before_unix_microseconds\":{}}},",
            "\"checkpoint\":\"verify_preflight\",",
            "\"local_effect\":{},\"drain_obligation\":{}}}"
        ),
        TENANT,
        INSTALLATION,
        DEPLOYMENT,
        deployment.0,
        deployment.2,
        deployment.3,
        deployment.1,
        deployment.4.timestamp_micros(),
        deployment.4.timestamp_micros() + 1_000_000,
        local_effect,
        drain_obligation
    );
    let digest = suspension_digest(root.as_bytes());
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
            deployment_revision, convergence_attempt_no, \
            suspend_attempt_request_bytes, suspend_attempt_digest\
         ) VALUES (\
            'ad0000000000000000000000000000ad', $1, $2, $3, $4, $5, \
            pg_catalog.convert_to($6, 'UTF8'), $7\
         )",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(deployment.0)
    .bind(deployment.1)
    .bind(&root)
    .bind(&digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_suspended_attempts_v2 (\
            suspension_id, suspend_attempt_digest, tenant_id, installation_id, \
            deployment_id, deployment_revision, convergence_attempt_no, \
            sidecar_revision, slot_guild_id, slot_ruleset_key, local_effect_kind, \
            local_effect_bytes, drain_obligation_kind, drain_obligation_bytes, \
            suspended_at\
         ) VALUES (\
            'ad0000000000000000000000000000ad', $1, $2, $3, $4, $5, $6, \
            1, $7, $8, $9, pg_catalog.convert_to($10, 'UTF8'), \
            $11, pg_catalog.convert_to($12, 'UTF8'), $13\
         )",
    )
    .bind(&digest)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(deployment.0)
    .bind(deployment.1)
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(local_effect_kind)
    .bind(&local_effect)
    .bind(drain_obligation_kind)
    .bind(&drain_obligation)
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

fn suspension_digest(payload: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let domain = b"starring.runtime.suspend_attempt.v2\0";
    let mut digest = Sha256::new();
    digest.update((domain.len() as u64).to_be_bytes());
    digest.update(domain);
    digest.update((payload.len() as u64).to_be_bytes());
    digest.update(payload);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn execute_startup_suspended_local(
    pool: &PgPool,
    owner: &StartupObservationOwnerTuple,
    recovery_id: &str,
    minimum_database_now: DateTime<Utc>,
    registry_sequence: i64,
) -> Result<Value, sqlx::Error> {
    execute_startup_suspended_local_with_resume(
        pool,
        owner,
        recovery_id,
        minimum_database_now,
        registry_sequence,
        0,
    )
    .await
}

async fn execute_startup_suspended_local_with_resume(
    pool: &PgPool,
    owner: &StartupObservationOwnerTuple,
    recovery_id: &str,
    minimum_database_now: DateTime<Utc>,
    registry_sequence: i64,
    last_resume_sequence: i64,
) -> Result<Value, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await?;
    let result = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.jsonb_build_object(\
            'journal_outcome_name', result.journal_outcome_name, \
            'terminal_outcome_name', result.terminal_outcome_name, \
            'recovery_class', result.recovery_class, \
            'terminal_projection_bytes', pg_catalog.encode(\
                result.terminal_projection_bytes, 'hex'\
            ), \
            'terminal_digest', result.terminal_digest\
         ) \
         FROM public.starring_runtime_startup_recovery_execute_suspended_local_v2(\
            $1,1,2,2,1,$2,$3,$4,$5,$6,$7,$8,$3,1,3,'ready',4,6,5,$10,$3,$9,0,0\
         ) AS result",
    )
    .bind(recovery_id)
    .bind(&owner.0)
    .bind(&owner.1)
    .bind(owner.2)
    .bind(&owner.3)
    .bind(owner.4)
    .bind(owner.5)
    .bind(minimum_database_now)
    .bind(registry_sequence)
    .bind(last_resume_sequence)
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

async fn suspended_local_state(pool: &PgPool) -> (i64, i64, i64, String, String, i64) {
    sqlx::query_as(
        "SELECT deployment.revision, fence.writer_epoch, \
            suspended.sidecar_revision, suspended.local_effect_kind, \
            suspended.drain_obligation_kind, \
            (SELECT pg_catalog.count(*) \
             FROM public.runtime_startup_recovery_actions_v2 \
             WHERE recovery_class = 'suspended_local_effect') \
         FROM public.runtime_deployments AS deployment \
         INNER JOIN public.runtime_slot_writer_fences_v2 AS fence \
            ON fence.slot_guild_id = deployment.guild_id \
            AND fence.slot_ruleset_key = deployment.ruleset_key \
         INNER JOIN public.runtime_suspended_attempts_v2 AS suspended \
            ON suspended.deployment_id = deployment.deployment_id \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}
