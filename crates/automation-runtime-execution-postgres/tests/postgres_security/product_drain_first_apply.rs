const PRODUCT_DRAIN_FIRST_APPLY_MIGRATION: &str = include_str!(
    "../../../../migrations/202607240009_add_product_drain_first_apply_core.sql"
);
const PRODUCT_DRAIN_FIRST_APPLY: &str = "SELECT * FROM \
    starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20\
    )";
const PRODUCT_DRAIN_FIRST_APPLY_IDENTITY: &str = "starring_runtime_private_v2.\
    starring_runtime_product_drain_first_apply_core_v2(\
        text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,\
        bytea,text,bytea,text\
    )";
const PRODUCT_DRAIN_FIRST_APPLY_GUCS: [&str; 3] = [
    "starring.runtime_product_drain_first_apply_stage_v2",
    "starring.runtime_product_drain_first_apply_product_operation_id_v2",
    "starring.runtime_product_drain_first_apply_drain_intent_id_v2",
];

#[derive(Debug, sqlx::FromRow)]
struct ProductDrainFirstApplyRow {
    outcome_name: String,
    locked_snapshot: Option<Json<Value>>,
    observed_at: Option<DateTime<Utc>>,
    product_tenant_id: Option<String>,
    product_installation_id: Option<String>,
    product_deployment_id: Option<String>,
    product_expected_revision: Option<i64>,
    product_operation_id: Option<String>,
    product_expected_target: Option<Json<Value>>,
    product_mutation_request_bytes: Option<Vec<u8>>,
    product_mutation_digest: Option<String>,
    drain_tenant_id: Option<String>,
    drain_installation_id: Option<String>,
    drain_deployment_id: Option<String>,
    drain_slot_guild_id: Option<String>,
    drain_slot_ruleset_key: Option<String>,
    drain_expected_revision: Option<i64>,
    drain_intent_id: Option<String>,
    drain_intent_request_bytes: Option<Vec<u8>>,
    drain_intent_digest: Option<String>,
    intent_revision: Option<i64>,
    intent_state: Option<String>,
}

async fn begin_product_drain_first_apply(
    pool: &PgPool,
) -> sqlx::Transaction<'_, sqlx::Postgres> {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '10s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction
}

async fn call_product_drain_first_apply(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) -> Result<ProductDrainFirstApplyRow, sqlx::Error> {
    call_product_drain_first_apply_with_roots(
        executor,
        canonical,
        canonical.product_mutation_request_bytes(),
        canonical.product_mutation_digest().as_str(),
        canonical.drain_intent_request_bytes(),
        canonical.drain_intent_digest().as_str(),
    )
    .await
}

async fn call_product_drain_first_apply_with_roots(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
    product_mutation_request_bytes: &[u8],
    product_mutation_digest: &str,
    drain_intent_request_bytes: &[u8],
    drain_intent_digest: &str,
) -> Result<ProductDrainFirstApplyRow, sqlx::Error> {
    let product = canonical.product_preimage();
    let drain = canonical.drain_preimage();
    sqlx::query_as(PRODUCT_DRAIN_FIRST_APPLY)
        .bind(product.operation_id.as_str())
        .bind(drain.key.intent_id.as_str())
        .bind(product.scope.tenant_id.as_str())
        .bind(product.scope.installation_id.as_str())
        .bind(product.scope.deployment_id.as_str())
        .bind(i64::try_from(product.expected_revision.get()).unwrap())
        .bind(product.slot.guild_id.to_string())
        .bind(product.slot.ruleset_key.as_str())
        .bind(product.expected_target.guild_id.to_string())
        .bind(product.expected_target.ruleset_key.as_str())
        .bind(i64::from(product.expected_target.version.get()))
        .bind(product.expected_target.content_hash.to_hex())
        .bind(i64::try_from(product.expected_target.binding_revision.get()).unwrap())
        .bind(product.expected_target.binding_fingerprint.as_str())
        .bind(product_mutation_kind_tag(product.mutation_kind))
        .bind(product.product_semantic_request_digest.as_str())
        .bind(product_mutation_request_bytes)
        .bind(product_mutation_digest)
        .bind(drain_intent_request_bytes)
        .bind(drain_intent_digest)
        .fetch_one(executor)
        .await
}

async fn call_product_drain_first_apply_with_slot_guild(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
    slot_guild_id: &str,
) -> Result<ProductDrainFirstApplyRow, sqlx::Error> {
    let product = canonical.product_preimage();
    let drain = canonical.drain_preimage();
    sqlx::query_as(PRODUCT_DRAIN_FIRST_APPLY)
        .bind(product.operation_id.as_str())
        .bind(drain.key.intent_id.as_str())
        .bind(product.scope.tenant_id.as_str())
        .bind(product.scope.installation_id.as_str())
        .bind(product.scope.deployment_id.as_str())
        .bind(i64::try_from(product.expected_revision.get()).unwrap())
        .bind(slot_guild_id)
        .bind(product.slot.ruleset_key.as_str())
        .bind(product.expected_target.guild_id.to_string())
        .bind(product.expected_target.ruleset_key.as_str())
        .bind(i64::from(product.expected_target.version.get()))
        .bind(product.expected_target.content_hash.to_hex())
        .bind(i64::try_from(product.expected_target.binding_revision.get()).unwrap())
        .bind(product.expected_target.binding_fingerprint.as_str())
        .bind(product_mutation_kind_tag(product.mutation_kind))
        .bind(product.product_semantic_request_digest.as_str())
        .bind(canonical.product_mutation_request_bytes())
        .bind(canonical.product_mutation_digest().as_str())
        .bind(canonical.drain_intent_request_bytes())
        .bind(canonical.drain_intent_digest().as_str())
        .fetch_one(executor)
        .await
}

async fn committed_product_drain_first_apply(
    pool: &PgPool,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) -> Result<ProductDrainFirstApplyRow, sqlx::Error> {
    let mut transaction = begin_product_drain_first_apply(pool).await;
    let result = call_product_drain_first_apply(&mut *transaction, canonical).await;
    match result {
        Ok(row) => {
            transaction.commit().await?;
            Ok(row)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

fn product_mutation_kind_tag(
    kind: automation_runtime_controller::RuntimeProductMutationKindV2,
) -> &'static str {
    match kind {
        automation_runtime_controller::RuntimeProductMutationKindV2::Apply => "apply",
        automation_runtime_controller::RuntimeProductMutationKindV2::Supersede => "supersede",
        automation_runtime_controller::RuntimeProductMutationKindV2::Cancel => "cancel",
        automation_runtime_controller::RuntimeProductMutationKindV2::AuthorityChange => {
            "authority_change"
        }
        automation_runtime_controller::RuntimeProductMutationKindV2::Teardown => "teardown",
    }
}

fn product_drain_canonical_with(
    snapshot: &RuntimeDeploymentSnapshotV1,
    product_operation_id: &str,
    drain_intent_id: &str,
    semantic_digest: &str,
) -> automation_runtime_controller::RuntimeCanonicalProductDrainV2 {
    let preimage = automation_runtime_controller::RuntimeProductMutationPreimageV2 {
        operation_id: automation_runtime_controller::RuntimeProductOperationIdV2::parse(
            product_operation_id,
        )
        .unwrap(),
        scope: automation_runtime_controller::RuntimeDeploymentScopeV1::from_identity(
            &snapshot.identity,
        ),
        expected_revision: snapshot.revision,
        slot: automation_runtime_controller::RuntimeServingSlotV2::from_target(&snapshot.target),
        expected_target: snapshot.target.clone(),
        mutation_kind: automation_runtime_controller::RuntimeProductMutationKindV2::Teardown,
        product_semantic_request_digest:
            automation_runtime_controller::RuntimeProductSemanticRequestDigestV2::parse(
                semantic_digest,
            )
            .unwrap(),
    };
    automation_runtime_controller::RuntimeCanonicalProductDrainV2::new(
        preimage,
        automation_runtime_controller::RuntimeDrainIntentIdV2::parse(drain_intent_id).unwrap(),
    )
    .unwrap()
}

fn assert_complete_product_drain_row(
    row: &ProductDrainFirstApplyRow,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) {
    let product = canonical.product_preimage();
    let drain = canonical.drain_preimage();
    let drain_guild_id = drain.key.slot.guild_id.to_string();
    let locked_snapshot = row.locked_snapshot.as_ref().unwrap();
    let observed_at = row.observed_at.as_ref().unwrap();
    assert_eq!(
        row.product_tenant_id.as_deref(),
        Some(product.scope.tenant_id.as_str())
    );
    assert_eq!(
        row.product_installation_id.as_deref(),
        Some(product.scope.installation_id.as_str())
    );
    assert_eq!(
        row.product_deployment_id.as_deref(),
        Some(product.scope.deployment_id.as_str())
    );
    assert_eq!(
        row.product_expected_revision,
        Some(i64::try_from(product.expected_revision.get()).unwrap())
    );
    assert_eq!(
        row.product_operation_id.as_deref(),
        Some(product.operation_id.as_str())
    );
    assert_eq!(
        row.product_mutation_request_bytes.as_deref(),
        Some(canonical.product_mutation_request_bytes())
    );
    assert_eq!(
        row.product_mutation_digest.as_deref(),
        Some(canonical.product_mutation_digest().as_str())
    );
    assert_eq!(
        row.drain_tenant_id.as_deref(),
        Some(drain.key.scope.tenant_id.as_str())
    );
    assert_eq!(
        row.drain_installation_id.as_deref(),
        Some(drain.key.scope.installation_id.as_str())
    );
    assert_eq!(
        row.drain_deployment_id.as_deref(),
        Some(drain.key.scope.deployment_id.as_str())
    );
    assert_eq!(
        row.drain_slot_guild_id.as_deref(),
        Some(drain_guild_id.as_str())
    );
    assert_eq!(
        row.drain_slot_ruleset_key.as_deref(),
        Some(drain.key.slot.ruleset_key.as_str())
    );
    assert_eq!(
        row.drain_expected_revision,
        Some(i64::try_from(drain.key.expected_revision.get()).unwrap())
    );
    assert_eq!(
        row.drain_intent_id.as_deref(),
        Some(drain.key.intent_id.as_str())
    );
    assert_eq!(
        row.drain_intent_request_bytes.as_deref(),
        Some(canonical.drain_intent_request_bytes())
    );
    assert_eq!(
        row.drain_intent_digest.as_deref(),
        Some(canonical.drain_intent_digest().as_str())
    );
    assert_eq!(row.intent_revision, Some(1));
    assert_eq!(row.intent_state.as_deref(), Some("pending"));
    assert!(row.product_expected_target.is_some());
    assert!(observed_at.timestamp_micros() > 0);
    assert_eq!(
        locked_snapshot.0["identity"]["deployment_id"],
        product.scope.deployment_id.as_str()
    );
    assert_eq!(
        locked_snapshot.0["identity"]["tenant_id"],
        product.scope.tenant_id.as_str()
    );
    assert_eq!(
        locked_snapshot.0["identity"]["installation_id"],
        product.scope.installation_id.as_str()
    );
}

fn assert_empty_product_drain_row(row: &ProductDrainFirstApplyRow) {
    assert!(
        row.locked_snapshot.is_none()
            && row.observed_at.is_none()
            && row.product_tenant_id.is_none()
            && row.product_installation_id.is_none()
            && row.product_deployment_id.is_none()
            && row.product_expected_revision.is_none()
            && row.product_operation_id.is_none()
            && row.product_expected_target.is_none()
            && row.product_mutation_request_bytes.is_none()
            && row.product_mutation_digest.is_none()
            && row.drain_tenant_id.is_none()
            && row.drain_installation_id.is_none()
            && row.drain_deployment_id.is_none()
            && row.drain_slot_guild_id.is_none()
            && row.drain_slot_ruleset_key.is_none()
            && row.drain_expected_revision.is_none()
            && row.drain_intent_id.is_none()
            && row.drain_intent_request_bytes.is_none()
            && row.drain_intent_digest.is_none()
            && row.intent_revision.is_none()
            && row.intent_state.is_none()
    );
}

async fn assert_product_drain_first_apply_gates_clear(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
) {
    let values = sqlx::query_scalar::<_, Vec<Option<String>>>(
        "SELECT ARRAY[\
            NULLIF(pg_catalog.current_setting($1, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($2, TRUE), ''),\
            NULLIF(pg_catalog.current_setting($3, TRUE), '')\
         ]",
    )
    .bind(PRODUCT_DRAIN_FIRST_APPLY_GUCS[0])
    .bind(PRODUCT_DRAIN_FIRST_APPLY_GUCS[1])
    .bind(PRODUCT_DRAIN_FIRST_APPLY_GUCS[2])
    .fetch_one(executor)
    .await
    .unwrap();
    assert_eq!(values, vec![None, None, None]);
}

async fn product_drain_row_counts(pool: &PgPool) -> (i64, i64) {
    sqlx::query_as(
        "SELECT \
            (SELECT pg_catalog.count(*) FROM public.runtime_product_operations_v2),\
            (SELECT pg_catalog.count(*) FROM public.runtime_drain_intents_v2)",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn product_drain_ready_snapshot(
    database: &IsolatedDatabase,
    controller: &str,
) -> RuntimeDeploymentSnapshotV1 {
    let session = gateway_ready_session(database, controller).await;
    assert_eq!(
        session.snapshot().phase,
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady
    );
    session.snapshot().clone()
}

async fn product_drain_live_transition_error(
    database: &IsolatedDatabase,
    session: &RuntimeConvergenceSessionV1,
) -> sqlx::Error {
    let gateway_ready = gateway_ready_attestation(database, session).await;
    let guard = session.execution_guard().unwrap();
    let mut transaction = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut transaction,
        &guard,
        serde_json::to_value(&gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap();
    let input = certification_input(&guard, gateway_ready, &prepared);
    let error = raw_certify_commit(
        &mut transaction,
        &input,
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    transaction.rollback().await.unwrap();
    error
}

async fn insert_product_only(
    pool: &PgPool,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) {
    set_product_drain_row_triggers(pool, false).await;
    let product = canonical.product_preimage();
    sqlx::query(
        "INSERT INTO public.runtime_product_operations_v2 \
         (product_operation_id, tenant_id, installation_id, deployment_id, expected_revision, \
          expected_target_guild_id, expected_target_ruleset_key, expected_target_version, \
          expected_target_content_hash, expected_target_binding_revision, \
          expected_target_binding_fingerprint, product_mutation_request_bytes, \
          product_mutation_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(product.operation_id.as_str())
    .bind(product.scope.tenant_id.as_str())
    .bind(product.scope.installation_id.as_str())
    .bind(product.scope.deployment_id.as_str())
    .bind(i64::try_from(product.expected_revision.get()).unwrap())
    .bind(product.expected_target.guild_id.to_string())
    .bind(product.expected_target.ruleset_key.as_str())
    .bind(i64::from(product.expected_target.version.get()))
    .bind(product.expected_target.content_hash.to_hex())
    .bind(i64::try_from(product.expected_target.binding_revision.get()).unwrap())
    .bind(product.expected_target.binding_fingerprint.as_str())
    .bind(canonical.product_mutation_request_bytes())
    .bind(canonical.product_mutation_digest().as_str())
    .execute(pool)
    .await
    .unwrap();
    set_product_drain_row_triggers(pool, true).await;
}

async fn insert_drain_only(
    pool: &PgPool,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) {
    sqlx::query(
        "ALTER TABLE public.runtime_drain_intents_v2 \
         DROP CONSTRAINT runtime_drain_intents_v2_product_fk",
    )
    .execute(pool)
    .await
    .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    for trigger in [
        "runtime_drain_intents_v2_reject_row_mutation",
        "runtime_drain_intents_v2_assert_slot_writer_fence_symmetry",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.runtime_drain_intents_v2 DISABLE TRIGGER {trigger}"
        ))
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    let drain = canonical.drain_preimage();
    sqlx::query(
        "INSERT INTO public.runtime_drain_intents_v2 \
         (drain_intent_id, tenant_id, installation_id, deployment_id, slot_guild_id, \
          slot_ruleset_key, expected_revision, product_operation_id, product_mutation_digest, \
          drain_intent_request_bytes, drain_intent_digest, intent_revision, intent_state) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,1,'pending')",
    )
    .bind(drain.key.intent_id.as_str())
    .bind(drain.key.scope.tenant_id.as_str())
    .bind(drain.key.scope.installation_id.as_str())
    .bind(drain.key.scope.deployment_id.as_str())
    .bind(drain.key.slot.guild_id.to_string())
    .bind(drain.key.slot.ruleset_key.as_str())
    .bind(i64::try_from(drain.key.expected_revision.get()).unwrap())
    .bind(drain.key.product_operation_id.as_str())
    .bind(drain.key.product_mutation_digest.as_str())
    .bind(canonical.drain_intent_request_bytes())
    .bind(canonical.drain_intent_digest().as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    for trigger in [
        "runtime_drain_intents_v2_assert_slot_writer_fence_symmetry",
        "runtime_drain_intents_v2_reject_row_mutation",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.runtime_drain_intents_v2 ENABLE TRIGGER {trigger}"
        ))
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn install_recursive_product_insert_trigger(pool: &PgPool) {
    sqlx::raw_sql(
        "CREATE FUNCTION public.starring_test_product_drain_second_insert() \
         RETURNS TRIGGER LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog \
         AS $function$ \
         BEGIN \
             INSERT INTO public.runtime_product_operations_v2 (\
                 product_operation_id, tenant_id, installation_id, deployment_id, \
                 expected_revision, expected_target_guild_id, expected_target_ruleset_key, \
                 expected_target_version, expected_target_content_hash, \
                 expected_target_binding_revision, expected_target_binding_fingerprint, \
                 product_mutation_request_bytes, product_mutation_digest\
             ) VALUES (\
                 NEW.product_operation_id, NEW.tenant_id, NEW.installation_id, NEW.deployment_id, \
                 NEW.expected_revision, NEW.expected_target_guild_id, \
                 NEW.expected_target_ruleset_key, NEW.expected_target_version, \
                 NEW.expected_target_content_hash, NEW.expected_target_binding_revision, \
                 NEW.expected_target_binding_fingerprint, NEW.product_mutation_request_bytes, \
                 NEW.product_mutation_digest\
             ); \
             RETURN NEW; \
         END; \
         $function$; \
         CREATE TRIGGER starring_test_product_drain_second_insert \
         AFTER INSERT ON public.runtime_product_operations_v2 \
         FOR EACH ROW EXECUTE FUNCTION public.starring_test_product_drain_second_insert();",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn wait_for_product_drain_first_apply_lock(
    pool: &PgPool,
    waiter_pid: i32,
    blocker_pid: i32,
) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let blocked = sqlx::query_scalar::<_, bool>(
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
            .bind(waiter_pid)
            .bind(blocker_pid)
            .fetch_one(pool)
            .await
            .unwrap();
            if blocked {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

fn assert_database_error(error: &sqlx::Error, code: &str, message: &str) {
    let sqlx::Error::Database(database) = error else {
        panic!("{error:?}");
    };
    assert_eq!(database.code().as_deref(), Some(code));
    assert_eq!(database.message(), message);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_first_apply_is_atomic_replayable_divergent_and_closed() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let session = gateway_ready_session(&database, "runtime-first-apply-controller").await;
    let snapshot = session.snapshot().clone();
    let canonical = canonical_product_drain(&snapshot);

    let mut invalid_transaction = begin_product_drain_first_apply(&database.owner_pool).await;
    let mut invalid_product_bytes = canonical.product_mutation_request_bytes().to_vec();
    invalid_product_bytes.push(0);
    let invalid = call_product_drain_first_apply_with_roots(
        &mut *invalid_transaction,
        &canonical,
        &invalid_product_bytes,
        canonical.product_mutation_digest().as_str(),
        canonical.drain_intent_request_bytes(),
        canonical.drain_intent_digest().as_str(),
    )
    .await
    .unwrap_err();
    assert_database_error(
        &invalid,
        "RX002",
        "runtime_product_drain_first_apply_input_invalid",
    );
    invalid_transaction.rollback().await.unwrap();
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));

    let mut mismatched_slot_transaction =
        begin_product_drain_first_apply(&database.owner_pool).await;
    let mismatched_slot = call_product_drain_first_apply_with_slot_guild(
        &mut *mismatched_slot_transaction,
        &canonical,
        "9200102",
    )
    .await
    .unwrap_err();
    assert_database_error(
        &mismatched_slot,
        "RX002",
        "runtime_product_mutation_builder_input_invalid",
    );
    mismatched_slot_transaction.rollback().await.unwrap();
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));

    let mut transaction = begin_product_drain_first_apply(&database.owner_pool).await;
    sqlx::query("SET LOCAL search_path = pg_temp, public")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let inserted = call_product_drain_first_apply(&mut *transaction, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    assert_eq!(
        inserted.locked_snapshot.as_ref().unwrap().0["revision"],
        canonical.product_preimage().expected_revision.get()
    );
    assert_complete_product_drain_row(&inserted, &canonical);
    assert_product_drain_first_apply_gates_clear(&mut *transaction).await;
    for statement in [
        "INSERT INTO public.runtime_product_operations_v2 DEFAULT VALUES",
        "INSERT INTO public.runtime_drain_intents_v2 DEFAULT VALUES",
    ] {
        sqlx::query("SAVEPOINT product_drain_first_apply_gate_probe")
            .execute(&mut *transaction)
            .await
            .unwrap();
        let leaked_gate = sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap_err();
        assert_database_error(
            &leaked_gate,
            "23514",
            "runtime_product_drain_mutation_rejected",
        );
        sqlx::query("ROLLBACK TO SAVEPOINT product_drain_first_apply_gate_probe")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("RELEASE SAVEPOINT product_drain_first_apply_gate_probe")
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();

    let blocked_live = product_drain_live_transition_error(&database, &session).await;
    assert_database_error(
        &blocked_live,
        "RX007",
        "runtime_execution_product_drain_pending",
    );
    let pending_snapshot = product_drain_snapshot(&database.owner_pool).await;
    assert_eq!(
        pending_snapshot.phase,
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady
    );
    assert_eq!(
        pending_snapshot.revision,
        canonical.product_preimage().expected_revision
    );

    let replayed = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(replayed.outcome_name, "replayed");
    assert_eq!(
        replayed.locked_snapshot.as_ref().unwrap().0["revision"],
        pending_snapshot.revision.get()
    );
    assert_complete_product_drain_row(&replayed, &canonical);

    let changed_identifier_proposal = product_drain_canonical_with(
        &snapshot,
        "11112222333344445555666677778888",
        "88887777666655554444333322221111",
        canonical
            .product_preimage()
            .product_semantic_request_digest
            .as_str(),
    );
    let changed_identifier =
        committed_product_drain_first_apply(&database.owner_pool, &changed_identifier_proposal)
            .await
            .unwrap();
    assert_eq!(changed_identifier.outcome_name, "diverged");
    assert_complete_product_drain_row(&changed_identifier, &canonical);

    let mut changed_target_snapshot = snapshot.clone();
    changed_target_snapshot.target.version =
        changed_target_snapshot.target.version.next().unwrap();
    let changed_target_proposal = canonical_product_drain(&changed_target_snapshot);
    let changed_target =
        committed_product_drain_first_apply(&database.owner_pool, &changed_target_proposal)
            .await
            .unwrap();
    assert_eq!(changed_target.outcome_name, "diverged");
    assert_complete_product_drain_row(&changed_target, &canonical);

    let diverged_proposal = product_drain_canonical_with(
        &snapshot,
        "22223333444455556666777788889999",
        "99998888777766665555444433332222",
        &"9".repeat(64),
    );
    let diverged = committed_product_drain_first_apply(&database.owner_pool, &diverged_proposal)
        .await
        .unwrap();
    assert_eq!(diverged.outcome_name, "diverged");
    assert_complete_product_drain_row(&diverged, &canonical);
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (1, 1));

    let mut conflicting_snapshot = snapshot.clone();
    conflicting_snapshot.revision = conflicting_snapshot.revision.next().unwrap();
    let conflicting = product_drain_canonical_with(
        &conflicting_snapshot,
        canonical.product_preimage().operation_id.as_str(),
        canonical.drain_preimage().key.intent_id.as_str(),
        canonical
            .product_preimage()
            .product_semantic_request_digest
            .as_str(),
    );
    let slot_conflict =
        committed_product_drain_first_apply(&database.owner_pool, &conflicting)
            .await
            .unwrap();
    assert_eq!(slot_conflict.outcome_name, "slot_conflict");
    assert_empty_product_drain_row(&slot_conflict);
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (1, 1));

    let read_committed_error =
        call_product_drain_first_apply(&database.owner_pool, &canonical)
            .await
            .unwrap_err();
    assert_database_error(
        &read_committed_error,
        "RX004",
        "runtime_product_drain_first_apply_isolation_invalid",
    );

    let mut executor_transaction =
        begin_product_drain_first_apply(&database.executor_pool).await;
    let denied = call_product_drain_first_apply(&mut *executor_transaction, &canonical)
        .await
        .unwrap_err();
    assert_sqlstate(&denied, "42501");
    executor_transaction.rollback().await.unwrap();
    let (schema_usage, function_execute) = sqlx::query_as::<_, (bool, bool)>(
        "SELECT \
            pg_catalog.has_schema_privilege(\
                pg_catalog.to_regrole($1), 'starring_runtime_private_v2', 'USAGE'\
            ),\
            pg_catalog.has_function_privilege(\
                pg_catalog.to_regrole($1), pg_catalog.to_regprocedure($2), 'EXECUTE'\
            )",
    )
    .bind(&database.role)
    .bind(PRODUCT_DRAIN_FIRST_APPLY_IDENTITY)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert!(!schema_usage);
    assert!(!function_execute);

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_first_apply_rejects_partial_and_recursive_second_insert() {
    let server = PostgresTestServer::start();

    let recursive_database = isolated_database(server.connect_options()).await;
    let recursive_snapshot =
        product_drain_ready_snapshot(&recursive_database, "runtime-recursive-controller").await;
    let recursive_canonical = canonical_product_drain(&recursive_snapshot);
    install_recursive_product_insert_trigger(&recursive_database.owner_pool).await;
    let recursive_error = committed_product_drain_first_apply(
        &recursive_database.owner_pool,
        &recursive_canonical,
    )
    .await
    .unwrap_err();
    assert_database_error(
        &recursive_error,
        "23514",
        "runtime_product_drain_mutation_rejected",
    );
    assert_eq!(
        product_drain_row_counts(&recursive_database.owner_pool).await,
        (0, 0)
    );
    cleanup(recursive_database).await;

    let partial_database = isolated_database(server.connect_options()).await;
    let partial_snapshot =
        product_drain_ready_snapshot(&partial_database, "runtime-partial-controller").await;
    let partial_canonical = canonical_product_drain(&partial_snapshot);
    insert_product_only(&partial_database.owner_pool, &partial_canonical).await;
    let corrupt =
        committed_product_drain_first_apply(&partial_database.owner_pool, &partial_canonical)
            .await
            .unwrap();
    assert_eq!(corrupt.outcome_name, "persistence_corrupt");
    assert_eq!(
        product_drain_row_counts(&partial_database.owner_pool).await,
        (1, 0)
    );
    cleanup(partial_database).await;

    let drain_only_database = isolated_database(server.connect_options()).await;
    let drain_only_snapshot =
        product_drain_ready_snapshot(&drain_only_database, "runtime-drain-only-controller").await;
    let drain_only_canonical = canonical_product_drain(&drain_only_snapshot);
    insert_drain_only(&drain_only_database.owner_pool, &drain_only_canonical).await;
    let corrupt =
        committed_product_drain_first_apply(&drain_only_database.owner_pool, &drain_only_canonical)
            .await
            .unwrap_err();
    assert_database_error(
        &corrupt,
        "RX004",
        "runtime_execution_product_drain_state_invalid",
    );
    assert_eq!(
        product_drain_row_counts(&drain_only_database.owner_pool).await,
        (0, 1)
    );
    cleanup(drain_only_database).await;

    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_first_apply_rejects_persisted_roots_outside_locked_deployment_truth() {
    let server = PostgresTestServer::start();

    let target_database = isolated_database(server.connect_options()).await;
    let target_snapshot =
        product_drain_ready_snapshot(&target_database, "runtime-root-target-controller").await;
    let expected = canonical_product_drain(&target_snapshot);
    let mut different_target_snapshot = target_snapshot.clone();
    different_target_snapshot.target.version =
        different_target_snapshot.target.version.next().unwrap();
    let different_target = canonical_product_drain(&different_target_snapshot);
    seed_canonical_product_drain(&target_database.owner_pool, &different_target).await;
    let corrupt = committed_product_drain_first_apply(&target_database.owner_pool, &expected)
        .await
        .unwrap();
    assert_eq!(corrupt.outcome_name, "persistence_corrupt");
    assert_complete_product_drain_row(&corrupt, &different_target);
    assert_eq!(
        product_drain_row_counts(&target_database.owner_pool).await,
        (1, 1)
    );
    cleanup(target_database).await;

    let revision_database = isolated_database(server.connect_options()).await;
    let revision_snapshot =
        product_drain_ready_snapshot(&revision_database, "runtime-root-revision-controller").await;
    let mut future_revision_snapshot = revision_snapshot.clone();
    future_revision_snapshot.revision = future_revision_snapshot.revision.next().unwrap();
    let future_revision = canonical_product_drain(&future_revision_snapshot);
    seed_canonical_product_drain(&revision_database.owner_pool, &future_revision).await;
    let corrupt =
        committed_product_drain_first_apply(&revision_database.owner_pool, &future_revision)
            .await
            .unwrap();
    assert_eq!(corrupt.outcome_name, "persistence_corrupt");
    assert_complete_product_drain_row(&corrupt, &future_revision);
    assert_eq!(
        product_drain_row_counts(&revision_database.owner_pool).await,
        (1, 1)
    );
    cleanup(revision_database).await;

    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_first_apply_normalizes_serializable_exact_and_divergent_races() {
    let server = PostgresTestServer::start();

    let exact_database = isolated_database(server.connect_options()).await;
    let exact_snapshot =
        product_drain_ready_snapshot(&exact_database, "runtime-exact-race-controller").await;
    let exact_canonical = canonical_product_drain(&exact_snapshot);
    let mut holder = begin_product_drain_first_apply(&exact_database.owner_pool).await;
    let holder_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let inserted = call_product_drain_first_apply(&mut *holder, &exact_canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let mut waiter = begin_product_drain_first_apply(&exact_database.owner_pool).await;
    let waiter_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *waiter)
        .await
        .unwrap();
    let (exact_waiter_result, ()) = tokio::join!(
        call_product_drain_first_apply(&mut *waiter, &exact_canonical),
        async {
            wait_for_product_drain_first_apply_lock(
                &exact_database.owner_pool,
                waiter_pid,
                holder_pid,
            )
            .await;
            holder.commit().await.unwrap();
        }
    );
    waiter.rollback().await.unwrap();
    let exact_race_error = exact_waiter_result.unwrap_err();
    assert_sqlstate(&exact_race_error, "40001");
    let replayed =
        committed_product_drain_first_apply(&exact_database.owner_pool, &exact_canonical)
            .await
            .unwrap();
    assert_eq!(replayed.outcome_name, "replayed");
    assert_eq!(
        product_drain_row_counts(&exact_database.owner_pool).await,
        (1, 1)
    );
    cleanup(exact_database).await;

    let divergent_database = isolated_database(server.connect_options()).await;
    let divergent_snapshot = product_drain_ready_snapshot(
        &divergent_database,
        "runtime-divergent-race-controller",
    )
    .await;
    let winner = canonical_product_drain(&divergent_snapshot);
    let loser = product_drain_canonical_with(
        &divergent_snapshot,
        "3333444455556666777788889999aaaa",
        "aaaa9999888877776666555544443333",
        &"8".repeat(64),
    );
    let mut divergent_holder =
        begin_product_drain_first_apply(&divergent_database.owner_pool).await;
    let divergent_holder_pid =
        sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
            .fetch_one(&mut *divergent_holder)
            .await
            .unwrap();
    let inserted = call_product_drain_first_apply(&mut *divergent_holder, &winner)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let mut divergent_waiter =
        begin_product_drain_first_apply(&divergent_database.owner_pool).await;
    let divergent_waiter_pid =
        sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
            .fetch_one(&mut *divergent_waiter)
            .await
            .unwrap();
    let (divergent_waiter_result, ()) = tokio::join!(
        call_product_drain_first_apply(&mut *divergent_waiter, &loser),
        async {
            wait_for_product_drain_first_apply_lock(
                &divergent_database.owner_pool,
                divergent_waiter_pid,
                divergent_holder_pid,
            )
            .await;
            divergent_holder.commit().await.unwrap();
        }
    );
    divergent_waiter.rollback().await.unwrap();
    let divergent_race_error = divergent_waiter_result.unwrap_err();
    assert_sqlstate(&divergent_race_error, "40001");
    let diverged =
        committed_product_drain_first_apply(&divergent_database.owner_pool, &loser)
            .await
            .unwrap();
    assert_eq!(diverged.outcome_name, "diverged");
    assert_complete_product_drain_row(&diverged, &winner);
    assert_eq!(
        product_drain_row_counts(&divergent_database.owner_pool).await,
        (1, 1)
    );
    cleanup(divergent_database).await;

    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_first_apply_rechecks_writer_fence_and_slot_after_lock_waits() {
    let server = PostgresTestServer::start();

    let fence_database = isolated_database(server.connect_options()).await;
    let fence_snapshot =
        product_drain_ready_snapshot(&fence_database, "runtime-fence-race-controller").await;
    let fence_canonical = canonical_product_drain(&fence_snapshot);
    let mut fence_holder = fence_database.owner_pool.begin().await.unwrap();
    let fence_holder_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *fence_holder)
        .await
        .unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *fence_holder)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence DISABLE TRIGGER USER")
        .execute(&mut *fence_holder)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_writer_fence \
         SET fence_state = 'closed', fence_generation = 2, \
             cutover_lease_epoch_high_water = 1, \
             cutover_coordinator_id = '0123456789abcdeffedcba9876543210', \
             cutover_expires_at = pg_catalog.clock_timestamp() + INTERVAL '1 hour' \
         WHERE singleton",
    )
    .execute(&mut *fence_holder)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence ENABLE TRIGGER USER")
        .execute(&mut *fence_holder)
        .await
        .unwrap();
    let mut fence_waiter = begin_product_drain_first_apply(&fence_database.owner_pool).await;
    let fence_waiter_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *fence_waiter)
        .await
        .unwrap();
    let (fence_waiter_result, ()) = tokio::join!(
        call_product_drain_first_apply(&mut *fence_waiter, &fence_canonical),
        async {
            wait_for_product_drain_first_apply_lock(
                &fence_database.owner_pool,
                fence_waiter_pid,
                fence_holder_pid,
            )
            .await;
            fence_holder.commit().await.unwrap();
        }
    );
    fence_waiter.rollback().await.unwrap();
    let stale_fence = fence_waiter_result.unwrap_err();
    assert_sqlstate(&stale_fence, "40001");
    let fresh_fence =
        committed_product_drain_first_apply(&fence_database.owner_pool, &fence_canonical)
            .await
            .unwrap_err();
    assert_database_error(
        &fresh_fence,
        "RX005",
        "runtime_product_drain_first_apply_writer_fenced",
    );
    assert_eq!(
        product_drain_row_counts(&fence_database.owner_pool).await,
        (0, 0)
    );
    cleanup(fence_database).await;

    let slot_database = isolated_database(server.connect_options()).await;
    let slot_snapshot =
        product_drain_ready_snapshot(&slot_database, "runtime-slot-race-controller").await;
    let slot_canonical = canonical_product_drain(&slot_snapshot);
    sqlx::query("ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER")
        .execute(&slot_database.owner_pool)
        .await
        .unwrap();
    let mut slot_holder = slot_database.owner_pool.begin().await.unwrap();
    let slot_holder_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *slot_holder)
        .await
        .unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock_shared(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *slot_holder)
    .await
    .unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended(\
                pg_catalog.concat('starring-runtime-serving-slot-v1:', $1, ':', $2), 0\
            )\
         )",
    )
    .bind(slot_snapshot.target.guild_id.to_string())
    .bind(slot_snapshot.target.ruleset_key.as_str())
    .execute(&mut *slot_holder)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET revision = revision + 1 \
         WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3",
    )
    .bind(slot_snapshot.identity.tenant_id.as_str())
    .bind(slot_snapshot.identity.installation_id.as_str())
    .bind(slot_snapshot.identity.deployment_id.as_str())
    .execute(&mut *slot_holder)
    .await
    .unwrap();
    let mut slot_waiter = begin_product_drain_first_apply(&slot_database.owner_pool).await;
    let slot_waiter_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *slot_waiter)
        .await
        .unwrap();
    let (slot_waiter_result, ()) = tokio::join!(
        call_product_drain_first_apply(&mut *slot_waiter, &slot_canonical),
        async {
            wait_for_product_drain_first_apply_lock(
                &slot_database.owner_pool,
                slot_waiter_pid,
                slot_holder_pid,
            )
            .await;
            slot_holder.commit().await.unwrap();
        }
    );
    slot_waiter.rollback().await.unwrap();
    let stale_slot = slot_waiter_result.unwrap_err();
    assert_sqlstate(&stale_slot, "40001");
    sqlx::query("ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER")
        .execute(&slot_database.owner_pool)
        .await
        .unwrap();
    let fresh_slot =
        committed_product_drain_first_apply(&slot_database.owner_pool, &slot_canonical)
            .await
            .unwrap_err();
    assert_database_error(
        &fresh_slot,
        "RX001",
        "runtime_product_drain_first_apply_deployment_mismatch",
    );
    assert_eq!(
        product_drain_row_counts(&slot_database.owner_pool).await,
        (0, 0)
    );
    cleanup(slot_database).await;

    drop(server);
}

async fn product_drain_first_apply_catalog_fingerprint(pool: &PgPool) -> String {
    sqlx::query_scalar(
        "WITH contract(value) AS (\
            SELECT pg_catalog.concat_ws(\
                '|', namespace.nspname, function_row.oid::TEXT, \
                function_row.proowner::TEXT, COALESCE(function_row.proacl::TEXT, ''), \
                pg_catalog.pg_get_functiondef(function_row.oid)\
            ) \
            FROM pg_catalog.pg_proc AS function_row \
            INNER JOIN pg_catalog.pg_namespace AS namespace \
              ON namespace.oid = function_row.pronamespace \
            WHERE namespace.nspname = 'starring_runtime_private_v2' \
               OR function_row.oid IN (\
                    pg_catalog.to_regprocedure(\
                        'public.reject_runtime_product_drain_mutation()'\
                    ),\
                    pg_catalog.to_regprocedure(\
                        'public.starring_runtime_execution_schema_manifest_v1()'\
                    ),\
                    pg_catalog.to_regprocedure(\
                        'public.starring_runtime_execution_database_readiness_v1()'\
                    )\
               ) \
            UNION ALL \
            SELECT pg_catalog.concat_ws(\
                '|', trigger_row.tgrelid::TEXT, trigger_row.tgname, \
                pg_catalog.pg_get_triggerdef(trigger_row.oid)\
            ) \
            FROM pg_catalog.pg_trigger AS trigger_row \
            WHERE trigger_row.tgrelid IN (\
                pg_catalog.to_regclass('public.runtime_product_operations_v2'),\
                pg_catalog.to_regclass('public.runtime_drain_intents_v2')\
            ) AND NOT trigger_row.tgisinternal\
        ) \
        SELECT pg_catalog.encode(\
            pg_catalog.sha256(pg_catalog.convert_to(\
                pg_catalog.string_agg(value, E'\\n' ORDER BY value), 'UTF8'\
            )), 'hex'\
        ) FROM contract",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn public_runtime_function_acl_fingerprint(pool: &PgPool) -> String {
    sqlx::query_scalar(
        "SELECT pg_catalog.encode(\
            pg_catalog.sha256(pg_catalog.convert_to(\
                pg_catalog.string_agg(\
                    pg_catalog.concat_ws(\
                        '|', function_row.oid::TEXT, function_row.proowner::TEXT, \
                        COALESCE(function_row.proacl::TEXT, '')\
                    ), E'\\n' ORDER BY function_row.oid\
                ), 'UTF8'\
            )), 'hex'\
        ) \
        FROM pg_catalog.pg_proc AS function_row \
        INNER JOIN pg_catalog.pg_namespace AS namespace \
          ON namespace.oid = function_row.pronamespace \
        WHERE namespace.nspname = 'public'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_first_apply_rerun_rolls_back_without_catalog_drift() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let before = product_drain_first_apply_catalog_fingerprint(&database.owner_pool).await;
    let mut transaction = database.owner_pool.begin().await.unwrap();
    let error = sqlx::raw_sql(PRODUCT_DRAIN_FIRST_APPLY_MIGRATION)
        .execute(&mut *transaction)
        .await
        .unwrap_err();
    transaction.rollback().await.unwrap();
    assert_sqlstate(&error, "RE001");
    assert_eq!(
        product_drain_first_apply_catalog_fingerprint(&database.owner_pool).await,
        before
    );
    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_first_apply_upgrade_preserves_fifteen_executor_capabilities() {
    let server = PostgresTestServer::start();
    let base = server.connect_options();
    let suffix = unique_suffix();
    let database_name = format!("st_re_fa_db_{suffix}");
    let executor_role = format!("st_re_fa_role_{suffix}");
    assert!(canonical_identifier(&database_name));
    assert!(canonical_identifier(&executor_role));
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    administrator
        .execute(format!("CREATE DATABASE {database_name}").as_str())
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(base.clone().database(&database_name))
        .await
        .unwrap();
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 202_607_240_008)
    {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }
    administrator
        .execute(
            format!(
                "CREATE ROLE {executor_role} LOGIN NOINHERIT NOSUPERUSER NOCREATEDB \
                 NOCREATEROLE NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4"
            )
            .as_str(),
        )
        .await
        .unwrap();
    for function in EXECUTOR_FUNCTIONS {
        pool.execute(
            format!("GRANT EXECUTE ON FUNCTION {function} TO {executor_role}").as_str(),
        )
        .await
        .unwrap();
    }
    assert_eq!(EXECUTOR_FUNCTIONS.len(), 15);
    let before = public_runtime_function_acl_fingerprint(&pool).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::raw_sql(PRODUCT_DRAIN_FIRST_APPLY_MIGRATION)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(public_runtime_function_acl_fingerprint(&pool).await, before);
    let (capability_count, schema_usage, function_execute, manifest_valid) =
        sqlx::query_as::<_, (i64, bool, bool, bool)>(
            "SELECT \
                (SELECT pg_catalog.count(*) \
                 FROM pg_catalog.unnest($1::TEXT[]) AS expected(identity) \
                 WHERE pg_catalog.has_function_privilege(\
                    pg_catalog.to_regrole($2), \
                    pg_catalog.to_regprocedure(expected.identity), 'EXECUTE'\
                 )),\
                pg_catalog.has_schema_privilege(\
                    pg_catalog.to_regrole($2), 'starring_runtime_private_v2', 'USAGE'\
                ),\
                pg_catalog.has_function_privilege(\
                    pg_catalog.to_regrole($2), pg_catalog.to_regprocedure($3), 'EXECUTE'\
                ),\
                public.starring_runtime_execution_schema_manifest_v1()",
        )
        .bind(EXECUTOR_FUNCTIONS.as_slice())
        .bind(&executor_role)
        .bind(PRODUCT_DRAIN_FIRST_APPLY_IDENTITY)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(capability_count, 15);
    assert!(!schema_usage);
    assert!(!function_execute);
    assert!(manifest_valid);

    pool.close().await;
    administrator
        .execute(format!("DROP DATABASE {database_name} WITH (FORCE)").as_str())
        .await
        .unwrap();
    administrator
        .execute(format!("DROP ROLE {executor_role}").as_str())
        .await
        .unwrap();
    drop(server);
}
