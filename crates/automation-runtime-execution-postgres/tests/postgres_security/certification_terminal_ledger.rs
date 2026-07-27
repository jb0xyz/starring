#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn certification_terminal_ledger_is_immutable_and_reclassifies_only_exact_history() {
    let server = PostgresTestServer::start();
    let mut database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;

    let root = sqlx::query_as::<_, (String, String, String, String, String, i64, i64)>(
        "SELECT operation_id, intent_fingerprint, tenant_id, installation_id, \
            deployment_id, deployment_revision, convergence_attempt_no \
         FROM public.runtime_certification_operations_v2",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let owner = sqlx::query_as::<_, StartupObservationOwnerTuple>(
        "SELECT gateway_shard_id, process_instance_id, lease_epoch, \
            expected_build_revision, owner_revision, expires_at \
         FROM public.runtime_gateway_owners \
         WHERE process_instance_id IS NOT NULL",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();

    let unresolved = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(unresolved["outcome_name"], "observed");
    assert_eq!(unresolved["recoverable_awaiting_certification_count"], 1);

    let denied = sqlx::query(
        "SELECT operation_id \
         FROM public.runtime_certification_operation_terminals_v2",
    )
    .fetch_optional(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&denied, "42501");
    for statement in [
        "INSERT INTO public.runtime_certification_operation_terminals_v2 \
         SELECT * FROM public.runtime_certification_operation_terminals_v2",
        "UPDATE public.runtime_certification_operation_terminals_v2 \
         SET terminal_outcome_name = terminal_outcome_name",
        "DELETE FROM public.runtime_certification_operation_terminals_v2",
        "TRUNCATE TABLE public.runtime_certification_operation_terminals_v2",
    ] {
        let error = sqlx::query(statement)
            .execute(&database.executor_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "42501");
    }

    let terminal_at = database_now(&database.owner_pool).await;
    let receipt = br#"{"outcome":"awaiting_reset","version":2}"#.to_vec();
    let resulting_revision = root.5 + 1;
    let digest = sqlx::query_scalar::<_, String>(
        "SELECT starring_runtime_private_v2.\
         starring_runtime_certification_terminal_digest_v2(\
            2::SMALLINT,$1,$2,$3,$4,$5,$6,$7,'awaiting_reset','reconciling_panels',\
            $8,$9,$10,$11\
         )",
    )
    .bind(&root.0)
    .bind(&root.1)
    .bind(&root.2)
    .bind(&root.3)
    .bind(&root.4)
    .bind(root.5)
    .bind(root.6)
    .bind(resulting_revision)
    .bind(root.6)
    .bind(terminal_at)
    .bind(&receipt)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let private_denied = sqlx::query_scalar::<_, String>(
        "SELECT starring_runtime_private_v2.\
         starring_runtime_certification_terminal_digest_v2(\
            2::SMALLINT,$1,$2,$3,$4,$5,$6,$7,'awaiting_reset','reconciling_panels',\
            $8,$9,$10,$11\
         )",
    )
    .bind(&root.0)
    .bind(&root.1)
    .bind(&root.2)
    .bind(&root.3)
    .bind(&root.4)
    .bind(root.5)
    .bind(root.6)
    .bind(resulting_revision)
    .bind(root.6)
    .bind(terminal_at)
    .bind(&receipt)
    .fetch_one(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&private_denied, "42501");

    let ungated = certification_terminal_insert_query()
        .bind(&root.0)
        .bind(&root.1)
        .bind(&root.2)
        .bind(&root.3)
        .bind(&root.4)
        .bind(root.5)
        .bind(root.6)
        .bind(resulting_revision)
        .bind(terminal_at)
        .bind(&receipt)
        .bind(&digest)
        .execute(&database.owner_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&ungated, "23514");

    insert_certification_terminal(
        &database.owner_pool,
        &root,
        resulting_revision,
        terminal_at,
        &receipt,
        &digest,
    )
    .await;
    let duplicate = try_insert_certification_terminal(
        &database.owner_pool,
        &root,
        resulting_revision,
        terminal_at,
        &receipt,
        &digest,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&duplicate, "23505");

    let premature = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(premature["outcome_name"], "ambiguous");
    assert!(premature["recoverable_awaiting_certification_count"].is_null());

    set_terminal_successor_deployment(
        &database.owner_pool,
        resulting_revision,
        "reconciling_panels",
    )
    .await;
    let completed = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(completed["outcome_name"], "observed");
    assert_eq!(completed["recoverable_awaiting_certification_count"], 0);

    set_terminal_successor_deployment(
        &database.owner_pool,
        resulting_revision + 1,
        "reconciling_panels",
    )
    .await;
    let advanced = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(advanced["outcome_name"], "observed");
    assert_eq!(advanced["recoverable_awaiting_certification_count"], 0);

    let unresolved_revision = resulting_revision + 1;
    set_terminal_successor_deployment(
        &database.owner_pool,
        unresolved_revision,
        "awaiting_gateway_ready",
    )
    .await;
    seed_additional_certification_root(&database.owner_pool, unresolved_revision, root.6).await;
    let mixed = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(mixed["outcome_name"], "observed");
    assert_eq!(mixed["recoverable_awaiting_certification_count"], 1);

    set_terminal_successor_deployment(
        &database.owner_pool,
        unresolved_revision,
        "reconciling_panels",
    )
    .await;
    let poisoned = observe_startup_state(&database.executor_pool, &owner, owner.4).await;
    assert_eq!(poisoned["outcome_name"], "ambiguous");
    assert!(poisoned["recoverable_awaiting_certification_count"].is_null());

    let stored = sqlx::query_as::<_, (String, Vec<u8>, String, i64)>(
        "SELECT terminal_outcome_name, terminal_receipt_bytes, \
            terminal_receipt_digest, resulting_deployment_revision \
         FROM public.runtime_certification_operation_terminals_v2 \
         WHERE operation_id = $1",
    )
    .bind(&root.0)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(stored.0, "awaiting_reset");
    assert_eq!(stored.1, receipt);
    assert_eq!(stored.2, digest);
    assert_eq!(stored.3, resulting_revision);

    for statement in [
        "UPDATE public.runtime_certification_operation_terminals_v2 \
         SET terminal_outcome_name = terminal_outcome_name",
        "DELETE FROM public.runtime_certification_operation_terminals_v2",
        "TRUNCATE TABLE public.runtime_certification_operation_terminals_v2",
    ] {
        let error = sqlx::query(statement)
            .execute(&database.owner_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "23514");
        assert!(error
            .to_string()
            .contains("runtime_certification_terminal_mutation_rejected"));
    }

    assert_cross_runtime_readiness(&mut database).await;
    cleanup(database).await;
}

fn certification_terminal_insert_query<'query>(
) -> sqlx::query::Query<'query, sqlx::Postgres, sqlx::postgres::PgArguments> {
    sqlx::query(
        "INSERT INTO public.runtime_certification_operation_terminals_v2 (\
            record_format_version, operation_id, intent_fingerprint, tenant_id, \
            installation_id, deployment_id, deployment_revision, \
            convergence_attempt_no, terminal_outcome_name, resulting_phase, \
            resulting_deployment_revision, resulting_convergence_attempt_no, \
            terminal_at, terminal_receipt_bytes, terminal_receipt_digest\
         ) VALUES (\
            2,$1,$2,$3,$4,$5,$6,$7,'awaiting_reset','reconciling_panels',\
            $8,$7,$9,$10,$11\
         )",
    )
}

async fn insert_certification_terminal(
    pool: &PgPool,
    root: &(String, String, String, String, String, i64, i64),
    resulting_revision: i64,
    terminal_at: DateTime<Utc>,
    receipt: &[u8],
    digest: &str,
) {
    try_insert_certification_terminal(
        pool,
        root,
        resulting_revision,
        terminal_at,
        receipt,
        digest,
    )
    .await
    .unwrap();
}

async fn try_insert_certification_terminal(
    pool: &PgPool,
    root: &(String, String, String, String, String, i64, i64),
    resulting_revision: i64,
    terminal_at: DateTime<Utc>,
    receipt: &[u8],
    digest: &str,
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    for (name, value) in [
        (
            "starring.runtime_certification_terminal_action_v2",
            "insert".to_string(),
        ),
        (
            "starring.runtime_certification_terminal_operation_id_v2",
            root.0.clone(),
        ),
        (
            "starring.runtime_certification_terminal_outcome_v2",
            "awaiting_reset".to_string(),
        ),
        (
            "starring.runtime_certification_terminal_result_phase_v2",
            "reconciling_panels".to_string(),
        ),
        (
            "starring.runtime_certification_terminal_result_revision_v2",
            resulting_revision.to_string(),
        ),
        (
            "starring.runtime_certification_terminal_result_attempt_v2",
            root.6.to_string(),
        ),
        (
            "starring.runtime_certification_terminal_digest_v2",
            digest.to_string(),
        ),
    ] {
        sqlx::query("SELECT pg_catalog.set_config($1,$2,TRUE)")
            .bind(name)
            .bind(value)
            .execute(&mut *transaction)
            .await?;
    }
    let result = certification_terminal_insert_query()
        .bind(&root.0)
        .bind(&root.1)
        .bind(&root.2)
        .bind(&root.3)
        .bind(&root.4)
        .bind(root.5)
        .bind(root.6)
        .bind(resulting_revision)
        .bind(terminal_at)
        .bind(receipt)
        .bind(digest)
        .execute(&mut *transaction)
        .await;
    match result {
        Ok(_) => {
            transaction.commit().await?;
            Ok(())
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn set_terminal_successor_deployment(pool: &PgPool, revision: i64, phase: &str) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET revision = $2, phase = $3, \
            snapshot = pg_catalog.jsonb_set(\
                pg_catalog.jsonb_set(\
                    snapshot, '{revision}', pg_catalog.to_jsonb($2::BIGINT), FALSE\
                ), \
                '{phase,phase}', pg_catalog.to_jsonb($3::TEXT), FALSE\
            ) \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .bind(revision)
    .bind(phase)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn seed_additional_certification_root(pool: &PgPool, revision: i64, attempt: i64) {
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
            '99999999999999999999999999999999',$1,$2,$3,$4,$5,\
            pg_catalog.convert_to('{\"kind\":\"second\"}','UTF8'),$6\
         )",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(revision)
    .bind(attempt)
    .bind("9".repeat(64))
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
