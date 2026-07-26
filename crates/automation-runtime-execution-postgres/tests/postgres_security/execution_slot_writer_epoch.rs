const EXECUTION_SLOT_WRITER_EPOCH_MIGRATION: &str = include_str!(
    "../../../../migrations/202607240013_fence_runtime_execution_slot_writer_epoch.sql"
);
const EXECUTION_SLOT_WRITER_EPOCH_READINESS_DIGEST: &str =
    "b5362bc1b081789a5b3ac4881fc2ea00c340a013630f7d5c809958ed1c045ec3";
const EXECUTION_SLOT_WRITER_CAPABILITIES: [&str; 6] = [
    "public.starring_runtime_execution_database_identity_v1()",
    "public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)",
    "public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)",
    "public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)",
    "public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)",
    "public.starring_runtime_execution_database_readiness_v1()",
];
const EXECUTION_SLOT_WRITER_CATALOG: [&str; 9] = [
    "public.starring_runtime_execution_database_identity_v1()",
    "public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)",
    "public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)",
    "public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)",
    "public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)",
    "public.starring_runtime_execution_schema_manifest_v1()",
    "public.starring_runtime_execution_database_readiness_v1()",
    "starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)",
    "starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)",
];

type ExecutionSlotWriterImage = (SlotWriterFenceRow, Json<Value>, (i64, i64, i64, i64));
type ExecutionSlotWriterCatalogRow = (String, i64, i64, String, String);

async fn apply_execution_slot_writer_epoch_prerequisites(pool: &PgPool) {
    let mut applied_versions = Vec::new();
    for migration in MIGRATOR
        .iter()
        .filter(|migration| (202_607_240_010..=202_607_240_012).contains(&migration.version))
    {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        applied_versions.push(migration.version);
    }
    assert_eq!(
        applied_versions,
        vec![202_607_240_010, 202_607_240_011, 202_607_240_012]
    );
}

async fn create_execution_slot_writer_role(pool: &PgPool, role: &str, can_login: bool) {
    assert!(canonical_identifier(role));
    let login = if can_login { "LOGIN" } else { "NOLOGIN" };
    pool.execute(
        format!(
            "CREATE ROLE {role} {login} NOINHERIT NOSUPERUSER NOCREATEDB \
             NOCREATEROLE NOREPLICATION NOBYPASSRLS"
        )
        .as_str(),
    )
    .await
    .unwrap();
    for capability in EXECUTION_SLOT_WRITER_CAPABILITIES {
        pool.execute(format!("GRANT EXECUTE ON FUNCTION {capability} TO {role}").as_str())
            .await
            .unwrap();
    }
}

async fn execution_slot_writer_capability_image(pool: &PgPool) -> Vec<(String, i64, i64, String)> {
    let mut image = Vec::new();
    for identity in EXECUTION_SLOT_WRITER_CAPABILITIES {
        image.push(
            sqlx::query_as(
                "SELECT $1::TEXT, function_row.oid::BIGINT, \
                        function_row.proowner::BIGINT, \
                        COALESCE(function_row.proacl::TEXT, '') \
                 FROM pg_catalog.pg_proc AS function_row \
                 WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
            )
            .bind(identity)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    image
}

async fn assert_execution_slot_writer_capabilities(
    pool: &PgPool,
    role: &str,
) -> Vec<(String, i64, i64, String)> {
    let image = execution_slot_writer_capability_image(pool).await;
    assert_eq!(image.len(), EXECUTION_SLOT_WRITER_CAPABILITIES.len());
    assert_eq!(
        image
            .iter()
            .map(|row| row.3.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        1
    );
    for identity in EXECUTION_SLOT_WRITER_CAPABILITIES {
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
        )
        .bind(role)
        .bind(identity)
        .fetch_one(pool)
        .await
        .unwrap());
        let acl = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT \
                pg_catalog.count(*) FILTER ( \
                    WHERE privilege.grantee <> function_row.proowner \
                ), \
                pg_catalog.count(*) FILTER ( \
                    WHERE privilege.grantee = pg_catalog.to_regrole($1) \
                ), \
                pg_catalog.count(*) FILTER ( \
                    WHERE privilege.grantee <> function_row.proowner \
                        AND ( \
                            privilege.grantee = 0 \
                            OR privilege.grantee <> pg_catalog.to_regrole($1) \
                            OR privilege.grantor <> function_row.proowner \
                            OR privilege.privilege_type <> 'EXECUTE' \
                            OR privilege.is_grantable \
                        ) \
                ) \
             FROM pg_catalog.pg_proc AS function_row \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, \
                pg_catalog.acldefault('f', function_row.proowner) \
             )) AS privilege \
             WHERE function_row.oid = pg_catalog.to_regprocedure($2) \
             GROUP BY function_row.proowner",
        )
        .bind(role)
        .bind(identity)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(acl, (1, 1, 0));
    }
    image
}

async fn execution_slot_writer_catalog_image(pool: &PgPool) -> Vec<ExecutionSlotWriterCatalogRow> {
    let mut image = Vec::new();
    for identity in EXECUTION_SLOT_WRITER_CATALOG {
        image.push(
            sqlx::query_as(
                "SELECT $1::TEXT, function_row.oid::BIGINT, \
                        function_row.proowner::BIGINT, \
                        COALESCE(function_row.proacl::TEXT, ''), \
                        pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to( \
                            pg_catalog.pg_get_functiondef(function_row.oid), \
                            'UTF8' \
                        )), 'hex') \
                 FROM pg_catalog.pg_proc AS function_row \
                 WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
            )
            .bind(identity)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    image
}

async fn execution_slot_writer_readiness_sha(pool: &PgPool) -> String {
    sqlx::query_scalar(
        "SELECT pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to( \
            pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure($1)), \
            'UTF8' \
         )), 'hex')",
    )
    .bind(READINESS_FUNCTION)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn execution_slot_writer_private_exposure(pool: &PgPool, role: &str) -> (bool, bool, bool) {
    sqlx::query_as(
        "SELECT \
            pg_catalog.has_schema_privilege( \
                $1, 'starring_runtime_private_v2', 'USAGE' \
            ), \
            pg_catalog.has_function_privilege( \
                $1, \
                'starring_runtime_private_v2.\
                    starring_runtime_slot_writer_fence_lock_v2(text,text)', \
                'EXECUTE' \
            ), \
            pg_catalog.has_function_privilege( \
                $1, \
                'starring_runtime_private_v2.\
                    starring_runtime_slot_writer_fence_begin_unsafe_v2(\
                        text,text,bigint\
                    )', \
                'EXECUTE' \
            )",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn drop_execution_slot_writer_role(pool: &PgPool, role: &str) {
    pool.execute(format!("DROP OWNED BY {role}").as_str())
        .await
        .unwrap();
    pool.execute(format!("DROP ROLE {role}").as_str())
        .await
        .unwrap();
}

async fn reopen_execution_slot_writer_migration_pool(
    server: &PostgresTestServer,
    database_name: &str,
    pool: PgPool,
) -> PgPool {
    pool.close().await;
    PgPoolOptions::new()
        .max_connections(1)
        .connect_with(server.connect_options().database(database_name))
        .await
        .unwrap()
}

fn execution_slot_writer_catalog_contract(
    image: &[ExecutionSlotWriterCatalogRow],
) -> Vec<(String, i64, i64, String)> {
    image
        .iter()
        .map(|row| (row.0.clone(), row.1, row.2, row.3.clone()))
        .collect()
}

async fn execution_slot_writer_epoch(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
) -> i64 {
    sqlx::query_scalar(
        "SELECT writer_epoch \
         FROM public.runtime_slot_writer_fences_v2 \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_one(executor)
    .await
    .unwrap()
}

async fn execution_slot_writer_image(database: &IsolatedDatabase) -> ExecutionSlotWriterImage {
    (
        slot_writer_fence_row(&database.owner_pool, &GUILD.to_string(), RULESET).await,
        persisted_deployment_image(&database.owner_pool).await,
        protected_counts(&database.owner_pool).await,
    )
}

async fn raw_epoch_renew(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: &automation_runtime_controller::RuntimeRenewExecutionV1,
) -> Result<String, sqlx::Error> {
    let guard = &request.guard;
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut **transaction)
        .await?;
    sqlx::query_scalar(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_renew_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, $9 \
         )",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(i64::try_from(guard.expected_revision.get()).unwrap())
    .bind(guard.controller_id.as_str())
    .bind(i64::try_from(guard.fencing_token.get()).unwrap())
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(i64::try_from(guard.runtime_generation.get()).unwrap())
    .bind(i64::try_from(request.lease_for.as_millis()).unwrap())
    .fetch_one(&mut **transaction)
    .await
}

async fn raw_epoch_mutate(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    guard: &RuntimeExecutionGuardV1,
    mutation_kind: &str,
    mutation_payload: &Value,
) -> Result<String, sqlx::Error> {
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut **transaction)
        .await?;
    sqlx::query_scalar(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_mutate_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10 \
         )",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(i64::try_from(guard.expected_revision.get()).unwrap())
    .bind(guard.controller_id.as_str())
    .bind(i64::try_from(guard.fencing_token.get()).unwrap())
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(i64::try_from(guard.runtime_generation.get()).unwrap())
    .bind(mutation_kind)
    .bind(Json(mutation_payload))
    .fetch_one(&mut **transaction)
    .await
}

async fn renew_slot_writer_epoch_scenario(database: &IsolatedDatabase) {
    let mut session = gateway_ready_session(database, "runtime-renew-slot-epoch-controller").await;
    let adapter = verified_execution_adapter(database).await;
    let request = session.begin_renewal(Duration::from_secs(400)).unwrap();
    let baseline = execution_slot_writer_image(database).await;

    let mut rollback = database.owner_pool.begin().await.unwrap();
    assert_eq!(
        raw_epoch_renew(&mut rollback, &request).await.unwrap(),
        "applied"
    );
    assert_eq!(
        execution_slot_writer_epoch(&mut *rollback).await,
        baseline.0 .2 + 1
    );
    assert_slot_writer_fence_gates_clear(&mut *rollback).await;
    rollback.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let applied = adapter.renew_execution(request.clone()).await.unwrap();
    let applied_image = execution_slot_writer_image(database).await;
    assert_eq!(applied_image.0 .2, baseline.0 .2 + 1);
    assert_ne!(applied_image.1, baseline.1);

    let replayed = adapter.renew_execution(request.clone()).await.unwrap();
    assert_eq!(replayed, applied);
    assert_eq!(execution_slot_writer_image(database).await, applied_image);
    session.apply_renewal(applied.clone()).unwrap();

    let canonical = canonical_product_drain(session.snapshot());
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let pending = execution_slot_writer_image(database).await;
    let pending_drain_counts = product_drain_row_counts(&database.owner_pool).await;
    assert_eq!(pending.0 .2, applied_image.0 .2 + 1);
    assert!(pending.0 .3.is_some());

    let pending_replay = adapter.renew_execution(request).await.unwrap();
    assert_eq!(pending_replay, applied);
    assert_eq!(execution_slot_writer_image(database).await, pending);
    assert_eq!(
        product_drain_row_counts(&database.owner_pool).await,
        pending_drain_counts
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn mutation_slot_writer_epoch_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-mutation-slot-epoch-controller",
        Duration::from_secs(300),
    )
    .await;
    let preflight = PreflightAttestationV1 {
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        observed_runtime: session.snapshot().previous_runtime.clone(),
        checked_at: database_now(&database.owner_pool).await,
    };
    let payload = serde_json::to_value(&preflight).unwrap();
    let request = session
        .begin_mutation(RuntimeConvergenceMutationV1::AcceptPreflight(preflight))
        .unwrap();
    let baseline = execution_slot_writer_image(database).await;

    let mut rollback = database.owner_pool.begin().await.unwrap();
    assert_eq!(
        raw_epoch_mutate(&mut rollback, &request.guard, "accept_preflight", &payload,)
            .await
            .unwrap(),
        "applied"
    );
    assert_eq!(
        execution_slot_writer_epoch(&mut *rollback).await,
        baseline.0 .2 + 1
    );
    assert_slot_writer_fence_gates_clear(&mut *rollback).await;
    rollback.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let applied = adapter.mutate(request.clone()).await.unwrap();
    assert!(matches!(
        applied.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    let applied_image = execution_slot_writer_image(database).await;
    assert_eq!(applied_image.0 .2, baseline.0 .2 + 1);
    assert_ne!(applied_image.1, baseline.1);

    let replayed = adapter.mutate(request).await.unwrap();
    assert!(matches!(
        replayed.outcome,
        TransitionOutcomeV1::Replayed { .. }
    ));
    assert_eq!(replayed.action_id, applied.action_id);
    assert_eq!(replayed.snapshot, applied.snapshot);
    assert_eq!(replayed.convergence_attempt, applied.convergence_attempt);
    assert_eq!(execution_slot_writer_image(database).await, applied_image);
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn certification_slot_writer_epoch_scenario(database: &IsolatedDatabase) {
    let mut session =
        gateway_ready_session(database, "runtime-certification-slot-epoch-controller").await;
    let adapter = verified_execution_adapter(database).await;
    let gateway_ready = gateway_ready_attestation(database, &session).await;
    let request = session
        .begin_certification(
            gateway_ready,
            adapter_certification_metadata(),
            Duration::from_millis(u64::try_from(CERTIFICATION_LEASE_MILLISECONDS).unwrap()),
        )
        .unwrap();
    let baseline = execution_slot_writer_image(database).await;

    let mut rollback = database.owner_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut rollback,
        &request.guard,
        serde_json::to_value(&request.gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap();
    assert_eq!(prepared.0, "apply");
    assert_eq!(
        execution_slot_writer_epoch(&mut *rollback).await,
        baseline.0 .2
    );
    let input = certification_input(&request.guard, request.gateway_ready.clone(), &prepared);
    assert_eq!(
        raw_certify_commit(&mut rollback, &input, CERTIFICATION_LEASE_MILLISECONDS,)
            .await
            .unwrap(),
        "applied"
    );
    assert_eq!(
        execution_slot_writer_epoch(&mut *rollback).await,
        baseline.0 .2 + 1
    );
    assert_slot_writer_fence_gates_clear(&mut *rollback).await;
    rollback.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let applied = adapter.certify_live(request.clone()).await.unwrap();
    assert!(matches!(
        applied.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    let applied_image = execution_slot_writer_image(database).await;
    assert_eq!(applied_image.0 .2, baseline.0 .2 + 1);
    assert_ne!(applied_image.1, baseline.1);
    let live_image = persisted_live_execution_image(&database.owner_pool).await;

    let replayed = adapter.certify_live(request).await.unwrap();
    assert!(matches!(
        replayed.outcome,
        TransitionOutcomeV1::Replayed { .. }
    ));
    assert_eq!(replayed.snapshot, applied.snapshot);
    assert_eq!(replayed.serving, applied.serving);
    assert_eq!(execution_slot_writer_image(database).await, applied_image);
    assert_eq!(
        persisted_live_execution_image(&database.owner_pool).await,
        live_image
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn pending_drain_closes_known_slot_execution_writers(database: &IsolatedDatabase) {
    let session = gateway_ready_session(database, "runtime-pending-slot-epoch-controller").await;
    let canonical = canonical_product_drain(session.snapshot());
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let pending = execution_slot_writer_image(database).await;
    let pending_drain_counts = product_drain_row_counts(&database.owner_pool).await;
    assert!(pending.0 .3.is_some());

    let adapter = verified_execution_adapter(database).await;

    let mut renewal_session = session.clone();
    let renewal = renewal_session
        .begin_renewal(Duration::from_secs(400))
        .unwrap();
    assert_eq!(
        adapter.renew_execution(renewal).await.unwrap_err(),
        RuntimeExecutionPersistenceErrorV1::RetryNotReady
    );
    assert_eq!(execution_slot_writer_image(database).await, pending);
    assert_eq!(
        product_drain_row_counts(&database.owner_pool).await,
        pending_drain_counts
    );

    let mut mutation_session = session.clone();
    let mutation = mutation_session
        .begin_mutation(RuntimeConvergenceMutationV1::RecordBlockedFailure {
            failure_id: RuntimeFailureId::parse("runtime-pending-slot-epoch-failure").unwrap(),
            kind: RuntimeFailureKindV1::InvariantViolation,
            code: "runtime_product_drain_pending".to_string(),
        })
        .unwrap();
    assert_eq!(
        adapter.mutate(mutation).await.unwrap_err(),
        RuntimeExecutionPersistenceErrorV1::RetryNotReady
    );
    assert_eq!(execution_slot_writer_image(database).await, pending);
    assert_eq!(
        product_drain_row_counts(&database.owner_pool).await,
        pending_drain_counts
    );

    let mut certification_session = session;
    let gateway_ready = gateway_ready_attestation(database, &certification_session).await;
    let certification = certification_session
        .begin_certification(
            gateway_ready,
            adapter_certification_metadata(),
            Duration::from_millis(u64::try_from(CERTIFICATION_LEASE_MILLISECONDS).unwrap()),
        )
        .unwrap();
    assert_eq!(
        adapter.certify_live(certification).await.unwrap_err(),
        RuntimeExecutionPersistenceErrorV1::RetryNotReady
    );
    assert_eq!(execution_slot_writer_image(database).await, pending);
    assert_eq!(
        product_drain_row_counts(&database.owner_pool).await,
        pending_drain_counts
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn execution_writer_wins_product_drain_epoch_race(database: &IsolatedDatabase) {
    let mut session =
        gateway_ready_session(database, "runtime-writer-first-epoch-controller").await;
    let request = session.begin_renewal(Duration::from_secs(400)).unwrap();
    let stale_canonical = canonical_product_drain(session.snapshot());
    let baseline = execution_slot_writer_image(database).await;

    let mut writer = database.owner_pool.begin().await.unwrap();
    let writer_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *writer)
        .await
        .unwrap();
    assert_eq!(
        raw_epoch_renew(&mut writer, &request).await.unwrap(),
        "applied"
    );
    assert_eq!(
        execution_slot_writer_epoch(&mut *writer).await,
        baseline.0 .2 + 1
    );

    let mut drain = begin_product_drain_first_apply(&database.owner_pool).await;
    let drain_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *drain)
        .await
        .unwrap();
    let (stale_result, ()) = tokio::join!(
        call_product_drain_first_apply(&mut *drain, &stale_canonical),
        async {
            wait_for_product_drain_first_apply_lock(&database.owner_pool, drain_pid, writer_pid)
                .await;
            writer.commit().await.unwrap();
        }
    );
    drain.rollback().await.unwrap();
    assert_sqlstate(&stale_result.unwrap_err(), "40001");

    let writer_committed = execution_slot_writer_image(database).await;
    assert_eq!(writer_committed.0 .2, baseline.0 .2 + 1);
    assert_ne!(writer_committed.1, baseline.1);
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));

    let stale_retry = committed_product_drain_first_apply(&database.owner_pool, &stale_canonical)
        .await
        .unwrap_err();
    assert_database_error(
        &stale_retry,
        "RX001",
        "runtime_product_drain_first_apply_deployment_mismatch",
    );
    assert_eq!(
        execution_slot_writer_image(database).await,
        writer_committed
    );
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));

    let current_snapshot = product_drain_snapshot(&database.owner_pool).await;
    let current_canonical = canonical_product_drain(&current_snapshot);
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &current_canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    assert_complete_product_drain_row(&inserted, &current_canonical);
    let pending = execution_slot_writer_image(database).await;
    assert_eq!(pending.0 .2, baseline.0 .2 + 2);
    assert!(pending.0 .3.is_some());
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (1, 1));
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn product_drain_wins_execution_writer_epoch_race(database: &IsolatedDatabase) {
    let mut session = gateway_ready_session(database, "runtime-drain-first-epoch-controller").await;
    let adapter = verified_execution_adapter(database).await;
    let request = session.begin_renewal(Duration::from_secs(400)).unwrap();
    let canonical = canonical_product_drain(session.snapshot());
    let baseline = execution_slot_writer_image(database).await;

    let mut drain = begin_product_drain_first_apply(&database.owner_pool).await;
    let drain_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *drain)
        .await
        .unwrap();
    let inserted = call_product_drain_first_apply(&mut *drain, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    assert_eq!(
        execution_slot_writer_epoch(&mut *drain).await,
        baseline.0 .2 + 1
    );

    let mut writer = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *writer)
        .await
        .unwrap();
    let _: String = sqlx::query_scalar(
        "SELECT public.starring_runtime_execution_database_identity_v1()",
    )
    .fetch_one(&mut *writer)
    .await
    .unwrap();
    let writer_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *writer)
        .await
        .unwrap();
    let (writer_result, ()) = tokio::join!(raw_epoch_renew(&mut writer, &request), async {
        wait_for_product_drain_first_apply_lock(&database.owner_pool, writer_pid, drain_pid).await;
        assert_eq!(
            execution_slot_writer_epoch(&database.owner_pool).await,
            baseline.0 .2
        );
        drain.commit().await.unwrap();
    });
    writer.rollback().await.unwrap();
    assert_sqlstate(&writer_result.unwrap_err(), "40001");

    let pending = execution_slot_writer_image(database).await;
    let pending_drain_counts = product_drain_row_counts(&database.owner_pool).await;
    assert_eq!(pending.0 .2, baseline.0 .2 + 1);
    assert!(pending.0 .3.is_some());
    assert_eq!(pending_drain_counts, (1, 1));

    assert_eq!(
        adapter.renew_execution(request.clone()).await.unwrap_err(),
        RuntimeExecutionPersistenceErrorV1::RetryNotReady
    );
    assert_eq!(execution_slot_writer_image(database).await, pending);
    assert_eq!(
        product_drain_row_counts(&database.owner_pool).await,
        pending_drain_counts
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn known_slot_execution_writer_epochs_are_atomic_replay_stable_and_pending_closed() {
    let server = PostgresTestServer::start();

    let renew_database = isolated_database(server.connect_options()).await;
    renew_slot_writer_epoch_scenario(&renew_database).await;
    cleanup(renew_database).await;

    let mutation_database = isolated_database(server.connect_options()).await;
    mutation_slot_writer_epoch_scenario(&mutation_database).await;
    cleanup(mutation_database).await;

    let certification_database = isolated_database(server.connect_options()).await;
    certification_slot_writer_epoch_scenario(&certification_database).await;
    cleanup(certification_database).await;

    let pending_database = isolated_database(server.connect_options()).await;
    pending_drain_closes_known_slot_execution_writers(&pending_database).await;
    cleanup(pending_database).await;

    let writer_first_database = isolated_database(server.connect_options()).await;
    execution_writer_wins_product_drain_epoch_race(&writer_first_database).await;
    cleanup(writer_first_database).await;

    let drain_first_database = isolated_database(server.connect_options()).await;
    product_drain_wins_execution_writer_epoch_race(&drain_first_database).await;
    cleanup(drain_first_database).await;

    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_slot_writer_epoch_upgrade_preserves_capabilities_and_rerun_is_atomic() {
    let server = PostgresTestServer::start();
    let (database_name, administrator, pool) =
        pre_slot_writer_fence_database(&server, "st_re_ee_up").await;
    apply_execution_slot_writer_epoch_prerequisites(&pool).await;
    let role = format!("st_re_epoch_maint_{:x}", unique_suffix());
    create_execution_slot_writer_role(&pool, &role, false).await;

    let capabilities_before = assert_execution_slot_writer_capabilities(&pool, &role).await;
    let catalog_before = execution_slot_writer_catalog_image(&pool).await;
    assert_eq!(
        execution_slot_writer_private_exposure(&pool, &role).await,
        (false, false, false)
    );
    let pool = reopen_execution_slot_writer_migration_pool(&server, &database_name, pool).await;

    let mut upgrade = pool.begin().await.unwrap();
    sqlx::raw_sql(EXECUTION_SLOT_WRITER_EPOCH_MIGRATION)
        .execute(&mut *upgrade)
        .await
        .unwrap();
    upgrade.commit().await.unwrap();

    assert_eq!(
        assert_execution_slot_writer_capabilities(&pool, &role).await,
        capabilities_before
    );
    assert_eq!(
        slot_writer_fence_manifests(&pool).await,
        (true, true, true, true)
    );
    assert_readiness_definition_sha(&pool, EXECUTION_SLOT_WRITER_EPOCH_READINESS_DIGEST).await;
    assert_eq!(
        execution_slot_writer_private_exposure(&pool, &role).await,
        (false, false, false)
    );

    let catalog_after = execution_slot_writer_catalog_image(&pool).await;
    assert_eq!(
        execution_slot_writer_catalog_contract(&catalog_after),
        execution_slot_writer_catalog_contract(&catalog_before)
    );
    let manifests_after = slot_writer_fence_manifests(&pool).await;
    let readiness_after = execution_slot_writer_readiness_sha(&pool).await;
    let private_after = execution_slot_writer_private_exposure(&pool, &role).await;

    let mut rerun = pool.begin().await.unwrap();
    let rerun_error = sqlx::raw_sql(EXECUTION_SLOT_WRITER_EPOCH_MIGRATION)
        .execute(&mut *rerun)
        .await
        .unwrap_err();
    rerun.rollback().await.unwrap();
    assert_database_error(
        &rerun_error,
        "RE001",
        "runtime_execution_slot_writer_epoch_preflight_drift",
    );
    assert_eq!(
        execution_slot_writer_catalog_image(&pool).await,
        catalog_after
    );
    assert_eq!(slot_writer_fence_manifests(&pool).await, manifests_after);
    assert_eq!(
        execution_slot_writer_readiness_sha(&pool).await,
        readiness_after
    );
    assert_eq!(
        execution_slot_writer_private_exposure(&pool, &role).await,
        private_after
    );
    assert_eq!(
        assert_execution_slot_writer_capabilities(&pool, &role).await,
        capabilities_before
    );

    drop_execution_slot_writer_role(&pool, &role).await;
    drop_slot_writer_fence_test_database(&database_name, administrator, pool).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_slot_writer_epoch_upgrade_rejects_login_executor_atomically() {
    let server = PostgresTestServer::start();
    let (database_name, administrator, pool) =
        pre_slot_writer_fence_database(&server, "st_re_ee_login").await;
    apply_execution_slot_writer_epoch_prerequisites(&pool).await;
    let role = format!("st_re_epoch_login_{:x}", unique_suffix());
    create_execution_slot_writer_role(&pool, &role, true).await;

    let capabilities_before = assert_execution_slot_writer_capabilities(&pool, &role).await;
    let catalog_before = execution_slot_writer_catalog_image(&pool).await;
    let manifests_before = slot_writer_fence_manifests(&pool).await;
    let readiness_before = execution_slot_writer_readiness_sha(&pool).await;
    let private_before = execution_slot_writer_private_exposure(&pool, &role).await;
    assert_eq!(manifests_before, (true, true, true, true));
    assert!(canonical_sha256(&readiness_before));
    assert_ne!(readiness_before, EXPECTED_READINESS_DEFINITION_SHA256_V1);
    assert_eq!(private_before, (false, false, false));
    let pool = reopen_execution_slot_writer_migration_pool(&server, &database_name, pool).await;

    let mut rejected = pool.begin().await.unwrap();
    let quiescence = sqlx::query_as::<_, (bool, i64, i64, i64)>(
        "SELECT role.rolcanlogin, \
            (SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_auth_members AS membership \
             WHERE membership.roleid = role.oid), \
            (SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_stat_activity AS activity \
             WHERE activity.datid = ( \
                    SELECT database_row.oid \
                    FROM pg_catalog.pg_database AS database_row \
                    WHERE database_row.datname = pg_catalog.current_database() \
                ) \
                AND activity.pid <> pg_catalog.pg_backend_pid() \
                AND activity.backend_type = 'client backend'), \
            (SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_prepared_xacts AS prepared \
             WHERE prepared.database = pg_catalog.current_database()) \
         FROM pg_catalog.pg_roles AS role \
         WHERE role.oid = pg_catalog.to_regrole($1)",
    )
    .bind(&role)
    .fetch_one(&mut *rejected)
    .await
    .unwrap();
    assert_eq!(quiescence, (true, 0, 0, 0));
    let error = sqlx::raw_sql(EXECUTION_SLOT_WRITER_EPOCH_MIGRATION)
        .execute(&mut *rejected)
        .await
        .unwrap_err();
    rejected.rollback().await.unwrap();
    assert_database_error(
        &error,
        "RE001",
        "runtime_execution_slot_writer_epoch_executor_not_quiesced",
    );

    assert_eq!(
        assert_execution_slot_writer_capabilities(&pool, &role).await,
        capabilities_before
    );
    assert_eq!(
        execution_slot_writer_catalog_image(&pool).await,
        catalog_before
    );
    assert_eq!(slot_writer_fence_manifests(&pool).await, manifests_before);
    assert_eq!(
        execution_slot_writer_readiness_sha(&pool).await,
        readiness_before
    );
    assert_eq!(
        execution_slot_writer_private_exposure(&pool, &role).await,
        private_before
    );

    drop_execution_slot_writer_role(&pool, &role).await;
    drop_slot_writer_fence_test_database(&database_name, administrator, pool).await;
    drop(server);
}
