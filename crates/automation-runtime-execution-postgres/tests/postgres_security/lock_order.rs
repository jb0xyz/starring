const TRY_RUNTIME_SERVING_SLOT_LOCK: &str = "SELECT pg_catalog.pg_try_advisory_xact_lock(\
        pg_catalog.hashtextextended(\
            pg_catalog.concat(\
                'starring-runtime-serving-slot-v1:', \
                $1::TEXT, \
                ':', \
                $2::TEXT\
            ), \
            0\
        )\
     )";
const LOCK_RUNTIME_SERVING_SLOT: &str = "SELECT pg_catalog.pg_advisory_xact_lock(\
        pg_catalog.hashtextextended(\
            pg_catalog.concat(\
                'starring-runtime-serving-slot-v1:', \
                $1::TEXT, \
                ':', \
                $2::TEXT\
            ), \
            0\
        )\
     )";

async fn wait_for_advisory_lock_blocked_by(pool: &PgPool, waiter: i32, blocker: i32) {
    for _ in 0..500 {
        let waiting = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(\
                SELECT 1 \
                FROM pg_catalog.pg_locks AS waiting \
                JOIN pg_catalog.pg_locks AS holding \
                    ON holding.locktype = waiting.locktype \
                    AND holding.database IS NOT DISTINCT FROM waiting.database \
                    AND holding.classid IS NOT DISTINCT FROM waiting.classid \
                    AND holding.objid IS NOT DISTINCT FROM waiting.objid \
                    AND holding.objsubid IS NOT DISTINCT FROM waiting.objsubid \
                WHERE waiting.pid = $1 \
                    AND holding.pid = $2 \
                    AND waiting.locktype = 'advisory' \
                    AND NOT waiting.granted \
                    AND holding.granted\
             )",
        )
        .bind(waiter)
        .bind(blocker)
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("expected PostgreSQL advisory lock wait");
}

async fn assert_runtime_deployment_row_available(pool: &PgPool) {
    let mut probe = pool.begin().await.unwrap();
    let deployment_id = sqlx::query_scalar::<_, String>(
        "SELECT deployment_id \
         FROM public.runtime_deployments \
         WHERE deployment_id = $1 \
         FOR UPDATE NOWAIT",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&mut *probe)
    .await
    .unwrap();
    assert_eq!(deployment_id, DEPLOYMENT);
    probe.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn stale_serializable_writer_fence_snapshot_aborts_with_40001() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;

    let mut stale = database.owner_pool.begin().await.unwrap();
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

    let mut cutover = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *cutover)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence DISABLE TRIGGER USER")
        .execute(&mut *cutover)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_writer_fence \
         SET fence_state = 'closed', \
             fence_generation = 2, \
             cutover_lease_epoch_high_water = 1, \
             cutover_coordinator_id = '00112233445566778899aabbccddeeff', \
             cutover_expires_at = pg_catalog.clock_timestamp() + INTERVAL '1 hour' \
         WHERE singleton",
    )
    .execute(&mut *cutover)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence ENABLE TRIGGER USER")
        .execute(&mut *cutover)
        .await
        .unwrap();
    cutover.commit().await.unwrap();

    let error = sqlx::query("SELECT * FROM public.starring_runtime_writer_fence_observe_v1()")
        .fetch_all(&mut *stale)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "40001");
    stale.rollback().await.unwrap();

    let durable = sqlx::query_as::<_, (String, i64, i64, Option<String>, Option<DateTime<Utc>>)>(
        "SELECT fence_state, fence_generation, cutover_lease_epoch_high_water, \
            cutover_coordinator_id, cutover_expires_at \
         FROM public.runtime_writer_fence WHERE singleton",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(durable.0, "closed");
    assert_eq!(durable.1, 2);
    assert_eq!(durable.2, 1);
    assert_eq!(
        durable.3.as_deref(),
        Some("00112233445566778899aabbccddeeff")
    );
    assert!(durable.4.unwrap() > database_now(&database.owner_pool).await);

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn stale_recovery_releases_rejected_candidate_slot_before_outer_transaction_ends() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let session = gateway_ready_session(&database, "runtime-slot-release-controller").await;
    let gateway_ready = gateway_ready_attestation(&database, &session).await;
    let guard = session.execution_guard().unwrap();
    let mut certification = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut certification,
        &guard,
        serde_json::to_value(&gateway_ready).unwrap(),
        1_000,
    )
    .await
    .unwrap();
    let input = certification_input(&guard, gateway_ready, &prepared);
    assert_eq!(
        raw_certify_commit(&mut certification, &input, 1_000)
            .await
            .unwrap(),
        "applied"
    );
    certification.commit().await.unwrap();

    let expiry = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT expires_at FROM public.runtime_serving_leases \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    wait_for_database_time(&database.owner_pool, expiry).await;
    let eligible = sqlx::query_as::<_, (String, bool)>(
        "SELECT deployment.phase, \
            lease.expires_at <= pg_catalog.clock_timestamp() \
         FROM public.runtime_deployments AS deployment \
         JOIN public.runtime_serving_leases AS lease \
            ON lease.guild_id = deployment.guild_id \
            AND lease.ruleset_key = deployment.ruleset_key \
            AND lease.deployment_id = deployment.deployment_id \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(eligible, ("live".to_string(), true));

    let mut deployment_blocker = database.owner_pool.begin().await.unwrap();
    let locked_deployment = sqlx::query_scalar::<_, String>(
        "SELECT deployment_id FROM public.runtime_deployments \
         WHERE deployment_id = $1 \
         FOR UPDATE",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&mut *deployment_blocker)
    .await
    .unwrap();
    assert_eq!(locked_deployment, DEPLOYMENT);

    let mut initial_slot_probe = database.owner_pool.begin().await.unwrap();
    let slot_initially_available = sqlx::query_scalar::<_, bool>(TRY_RUNTIME_SERVING_SLOT_LOCK)
        .bind(GUILD.to_string())
        .bind(RULESET)
        .fetch_one(&mut *initial_slot_probe)
        .await
        .unwrap();
    assert!(slot_initially_available);
    initial_slot_probe.rollback().await.unwrap();

    let mut recovery = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *recovery)
        .await
        .unwrap();
    let recovered = sqlx::query_scalar::<_, String>(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_recover_stale_live_v1()",
    )
    .fetch_all(&mut *recovery)
    .await
    .unwrap();
    assert!(recovered.is_empty());

    let mut slot_probe = database.owner_pool.begin().await.unwrap();
    let slot_available = sqlx::query_scalar::<_, bool>(TRY_RUNTIME_SERVING_SLOT_LOCK)
        .bind(GUILD.to_string())
        .bind(RULESET)
        .fetch_one(&mut *slot_probe)
        .await
        .unwrap();
    assert!(slot_available);

    slot_probe.rollback().await.unwrap();
    recovery.rollback().await.unwrap();
    deployment_blocker.rollback().await.unwrap();
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn runtime_observer_waits_for_writer_then_slot_before_deployment_row() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(&database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-observer-lock-order-controller",
        Duration::from_secs(300),
    )
    .await;
    let preflight = PreflightAttestationV1 {
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        observed_runtime: None,
        checked_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptPreflight(preflight),
    )
    .await;
    mutate_applied(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::RequestDrain,
    )
    .await;
    let guard = session.execution_guard().unwrap();

    let mut writer_holder = database.owner_pool.begin().await.unwrap();
    let writer_holder_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *writer_holder)
        .await
        .unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *writer_holder)
    .await
    .unwrap();

    let mut writer_observer_transaction = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '10s'")
        .execute(&mut *writer_observer_transaction)
        .await
        .unwrap();
    let writer_observer_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *writer_observer_transaction)
        .await
        .unwrap();
    let writer_guard = guard.clone();
    let writer_observer = tokio::spawn(async move {
        let observation =
            raw_observe_previous_serving(&mut writer_observer_transaction, &writer_guard).await?;
        writer_observer_transaction.commit().await?;
        Ok::<_, sqlx::Error>(observation)
    });
    wait_for_advisory_lock_blocked_by(&database.owner_pool, writer_observer_pid, writer_holder_pid)
        .await;
    assert_runtime_deployment_row_available(&database.owner_pool).await;
    let mut slot_probe = database.owner_pool.begin().await.unwrap();
    let slot_available = sqlx::query_scalar::<_, bool>(TRY_RUNTIME_SERVING_SLOT_LOCK)
        .bind(GUILD.to_string())
        .bind(RULESET)
        .fetch_one(&mut *slot_probe)
        .await
        .unwrap();
    assert!(slot_available);
    slot_probe.rollback().await.unwrap();
    writer_holder.rollback().await.unwrap();
    assert_eq!(
        writer_observer.await.unwrap().unwrap(),
        Some("absent".to_string())
    );

    let mut slot_holder = database.owner_pool.begin().await.unwrap();
    let slot_holder_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *slot_holder)
        .await
        .unwrap();
    sqlx::query(LOCK_RUNTIME_SERVING_SLOT)
        .bind(GUILD.to_string())
        .bind(RULESET)
        .execute(&mut *slot_holder)
        .await
        .unwrap();

    let mut slot_observer_transaction = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '10s'")
        .execute(&mut *slot_observer_transaction)
        .await
        .unwrap();
    let slot_observer_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *slot_observer_transaction)
        .await
        .unwrap();
    let slot_guard = guard.clone();
    let slot_observer = tokio::spawn(async move {
        let observation =
            raw_observe_previous_serving(&mut slot_observer_transaction, &slot_guard).await?;
        slot_observer_transaction.commit().await?;
        Ok::<_, sqlx::Error>(observation)
    });
    wait_for_advisory_lock_blocked_by(&database.owner_pool, slot_observer_pid, slot_holder_pid)
        .await;
    assert_runtime_deployment_row_available(&database.owner_pool).await;
    slot_holder.rollback().await.unwrap();
    assert_eq!(
        slot_observer.await.unwrap().unwrap(),
        Some("absent".to_string())
    );

    cleanup(database).await;
    drop(server);
}
