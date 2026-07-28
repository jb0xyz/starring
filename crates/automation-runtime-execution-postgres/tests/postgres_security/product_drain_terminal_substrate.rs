const TERMINAL_RESULT_DEPLOYMENT_ID: &str = "result.Deployment:v2-01";

#[derive(Clone, Debug, sqlx::FromRow)]
struct ProductDrainTerminalSourceRow {
    drain_intent_id: String,
    product_operation_id: String,
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    slot_guild_id: String,
    slot_ruleset_key: String,
    expected_revision: i64,
    product_mutation_digest: String,
    drain_intent_digest: String,
    source_intent_revision: i64,
    source_state_bytes: Vec<u8>,
    source_state_digest: String,
    source_epoch: i64,
    installation_authority_revision: i64,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct ProductDrainTerminalResultRow {
    intent_revision: i64,
    canonical_state_bytes: Vec<u8>,
    canonical_state_digest: String,
}

async fn seed_product_drain_terminal_source(
    database: &IsolatedDatabase,
    recovery_id: &str,
) -> ProductDrainTerminalSourceRow {
    seed_claimable_deployment(&database.owner_pool).await;
    let mut revision_transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER")
        .execute(&mut *revision_transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET revision = 8, \
             snapshot = pg_catalog.jsonb_set(\
                 pg_catalog.jsonb_set(\
                     snapshot, '{revision}', '8'::JSONB, FALSE\
                 ), \
                 '{last_fencing_token}', '1'::JSONB, FALSE\
             ), \
             last_fencing_token = 1, \
             last_controller_id = 'terminal-substrate-baseline-controller', \
             convergence_attempt_no = 1 \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .execute(&mut *revision_transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER")
        .execute(&mut *revision_transaction)
        .await
        .unwrap();
    revision_transaction.commit().await.unwrap();
    let Json(snapshot) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT snapshot FROM public.runtime_deployments \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(snapshot).unwrap();
    assert_eq!(snapshot.revision.get(), 8);
    let canonical = canonical_product_drain(&snapshot);
    seed_canonical_product_drain(&database.owner_pool, &canonical).await;
    let fixture = pending_drain_execution_fixture(database, recovery_id).await;
    let claim = committed_pending_drain_claim(&database.executor_pool, &fixture)
        .await
        .unwrap();
    let mut acknowledgement_fixture = fixture;
    acknowledgement_fixture.minimum_database_now = database_now(&database.owner_pool).await;
    let mut acknowledgement_transaction = database.executor_pool.begin().await.unwrap();
    prepare_pending_drain_write(&mut acknowledgement_transaction)
        .await
        .unwrap();
    let acknowledgement = call_pending_drain_execution(
        &mut *acknowledgement_transaction,
        &acknowledgement_fixture,
        "acknowledge",
        &claim.terminal_digest,
    )
    .await
    .unwrap();
    acknowledgement_transaction.commit().await.unwrap();
    assert_eq!(
        acknowledgement.terminal_outcome_name,
        "route_absent_acknowledged"
    );
    sqlx::query_as::<_, ProductDrainTerminalSourceRow>(
        "SELECT drain.drain_intent_id, drain.product_operation_id, \
                drain.tenant_id, drain.installation_id, drain.deployment_id, \
                drain.slot_guild_id, drain.slot_ruleset_key, \
                drain.expected_revision, drain.product_mutation_digest, \
                drain.drain_intent_digest, \
                drain.intent_revision AS source_intent_revision, \
                drain.canonical_state_bytes AS source_state_bytes, \
                drain.canonical_state_digest AS source_state_digest, \
                fence.writer_epoch AS source_epoch, \
                deployment.installation_authority_revision \
         FROM public.runtime_drain_intents_v2 AS drain \
         INNER JOIN public.runtime_slot_writer_fences_v2 AS fence \
             ON fence.slot_guild_id = drain.slot_guild_id \
             AND fence.slot_ruleset_key = drain.slot_ruleset_key \
         INNER JOIN public.runtime_deployments AS deployment \
             ON deployment.deployment_id = drain.deployment_id \
         WHERE drain.drain_intent_id = $1",
    )
    .bind(canonical.drain_preimage().key.intent_id.as_str())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap()
}

async fn call_product_drain_terminal_transition(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    source: &ProductDrainTerminalSourceRow,
    terminal_kind: &str,
    requested_resulting_revision: i64,
    terminal_time: DateTime<Utc>,
) -> Result<ProductDrainTerminalResultRow, sqlx::Error> {
    sqlx::query_as::<_, ProductDrainTerminalResultRow>(
        "SELECT intent_revision, canonical_state_bytes, canonical_state_digest \
         FROM starring_runtime_private_v2.\
         starring_runtime_product_drain_terminal_transition_v2(\
            $1,$2,$3,$4,$5,$6\
         )",
    )
    .bind(&source.drain_intent_id)
    .bind(source.source_intent_revision)
    .bind(&source.source_state_digest)
    .bind(terminal_kind)
    .bind(requested_resulting_revision)
    .bind(terminal_time)
    .fetch_one(executor)
    .await
}

async fn call_product_drain_terminal_release(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    source: &ProductDrainTerminalSourceRow,
    source_state_bytes: &[u8],
    source_state_digest: &str,
    result: &ProductDrainTerminalResultRow,
    terminal_kind: &str,
    terminal_time: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT starring_runtime_private_v2.\
         starring_runtime_slot_writer_fence_terminal_release_v2(\
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16\
         )",
    )
    .bind(&source.slot_guild_id)
    .bind(&source.slot_ruleset_key)
    .bind(source.source_epoch)
    .bind(&source.drain_intent_id)
    .bind(&source.product_operation_id)
    .bind(&source.tenant_id)
    .bind(&source.installation_id)
    .bind(&source.deployment_id)
    .bind(source.expected_revision)
    .bind(source.source_intent_revision)
    .bind(source_state_bytes)
    .bind(source_state_digest)
    .bind(result.intent_revision)
    .bind(&result.canonical_state_digest)
    .bind(terminal_kind)
    .bind(terminal_time)
    .fetch_one(executor)
    .await
}

async fn transition_and_release_product_drain(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source: &ProductDrainTerminalSourceRow,
    terminal_kind: &str,
    terminal_time: DateTime<Utc>,
) -> (ProductDrainTerminalResultRow, i64) {
    let requested_resulting_revision = if terminal_kind == "consumed" {
        1_i64
    } else {
        source.expected_revision + 1
    };
    let result = call_product_drain_terminal_transition(
        &mut **transaction,
        source,
        terminal_kind,
        requested_resulting_revision,
        terminal_time,
    )
    .await
    .unwrap();
    let successor_epoch = call_product_drain_terminal_release(
        &mut **transaction,
        source,
        &source.source_state_bytes,
        &source.source_state_digest,
        &result,
        terminal_kind,
        terminal_time,
    )
    .await
    .unwrap();
    (result, successor_epoch)
}

async fn insert_product_drain_terminal_action(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source: &ProductDrainTerminalSourceRow,
    result: &ProductDrainTerminalResultRow,
    successor_epoch: i64,
    terminal_kind: &str,
    terminal_time: DateTime<Utc>,
) -> Vec<u8> {
    let terminal_action_id = if terminal_kind == "consumed" {
        "a".repeat(64)
    } else {
        "3".repeat(64)
    };
    let product_action_idempotency_digest = if terminal_kind == "consumed" {
        "b".repeat(64)
    } else {
        "4".repeat(64)
    };
    let product_action_semantic_request_digest = if terminal_kind == "consumed" {
        "c".repeat(64)
    } else {
        "5".repeat(64)
    };
    let cancellation_reason_digest = if terminal_kind == "cancelled" {
        Some("6".repeat(64))
    } else {
        None
    };
    let Json(mut source_result_snapshot) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT snapshot FROM public.runtime_deployments \
         WHERE deployment_id = $1",
    )
    .bind(&source.deployment_id)
    .fetch_one(&mut **transaction)
    .await
    .unwrap();
    let source_result_deployment_revision = source.expected_revision + 1;
    source_result_snapshot["revision"] = json!(source_result_deployment_revision);
    let source_result_snapshot_bytes = serde_json::to_vec(&source_result_snapshot).unwrap();
    let source_result_snapshot_digest =
        sqlx::query_scalar::<_, String>("SELECT pg_catalog.encode(pg_catalog.sha256($1), 'hex')")
            .bind(&source_result_snapshot_bytes)
            .fetch_one(&mut **transaction)
            .await
            .unwrap();
    let result_deployment_id = if terminal_kind == "consumed" {
        Some(TERMINAL_RESULT_DEPLOYMENT_ID)
    } else {
        None
    };
    let result_deployment_revision = if terminal_kind == "consumed" {
        Some(1_i64)
    } else {
        None
    };
    let result_deployment_snapshot_bytes = if terminal_kind == "consumed" {
        let mut result_snapshot = source_result_snapshot.clone();
        result_snapshot["identity"]["deployment_id"] = json!(TERMINAL_RESULT_DEPLOYMENT_ID);
        result_snapshot["revision"] = json!(1);
        Some(serde_json::to_vec(&result_snapshot).unwrap())
    } else {
        None
    };
    let result_deployment_snapshot_digest =
        if let Some(snapshot_bytes) = result_deployment_snapshot_bytes.as_ref() {
            Some(
                sqlx::query_scalar::<_, String>(
                    "SELECT pg_catalog.encode(pg_catalog.sha256($1), 'hex')",
                )
                .bind(snapshot_bytes)
                .fetch_one(&mut **transaction)
                .await
                .unwrap(),
            )
        } else {
            None
        };
    let product_receipt_id = "f".repeat(64);
    let product_audit_event_id = "1".repeat(64);
    let authority_observation_digest = "2".repeat(64);
    let action = "runtime_drain.terminal";
    let endpoint_domain = "runtime_drain_terminal_test";
    let request_digest = product_action_semantic_request_digest.clone();
    sqlx::query(
        "INSERT INTO public.product_action_receipts (\
            receipt_id, tenant_id, installation_id, principal_id, endpoint_domain, \
            idempotency_key_digest, request_digest, target_resource_type, \
            target_resource_id, resulting_revision, resulting_state, result_code, \
            http_disposition_class, completed_at\
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,'runtime_drain',$8,$9,$10,'ok',2,$11)",
    )
    .bind(&product_receipt_id)
    .bind(&source.tenant_id)
    .bind(&source.installation_id)
    .bind(PRINCIPAL)
    .bind(endpoint_domain)
    .bind(&product_action_idempotency_digest)
    .bind(&request_digest)
    .bind(&source.drain_intent_id)
    .bind(result.intent_revision)
    .bind(terminal_kind)
    .bind(terminal_time)
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_audit_events (\
            event_id, tenant_id, installation_id, principal_id, \
            session_subject_digest, action, target_resource_type, \
            target_resource_id, request_id, receipt_id, \
            authority_observation_digest, effective_permission_bits, \
            authority_observed_at, installation_authority_revision, \
            resulting_state, result_code, occurred_at\
         ) VALUES (\
            $1,$2,$3,$4,pg_catalog.decode(pg_catalog.repeat('0',64),'hex'),\
            $5,'runtime_drain',$6,$7,$8,$9,0,$10,$11,$12,'ok',$10\
         )",
    )
    .bind(&product_audit_event_id)
    .bind(&source.tenant_id)
    .bind(&source.installation_id)
    .bind(PRINCIPAL)
    .bind(action)
    .bind(&source.drain_intent_id)
    .bind(format!("runtime.drain.terminal.{terminal_kind}"))
    .bind(&product_receipt_id)
    .bind(&authority_observation_digest)
    .bind(terminal_time)
    .bind(source.installation_authority_revision)
    .bind(terminal_kind)
    .execute(&mut **transaction)
    .await
    .unwrap();
    let projection = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT starring_runtime_private_v2.\
         starring_runtime_product_drain_terminal_projection_v2(\
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,\
            $15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27\
         )",
    )
    .bind(terminal_kind)
    .bind(&terminal_action_id)
    .bind(&product_action_idempotency_digest)
    .bind(&product_action_semantic_request_digest)
    .bind(cancellation_reason_digest.as_deref())
    .bind(&source.product_operation_id)
    .bind(&source.product_mutation_digest)
    .bind(&source.drain_intent_id)
    .bind(&source.drain_intent_digest)
    .bind(source.source_intent_revision)
    .bind(&source.source_state_digest)
    .bind(result.intent_revision)
    .bind(&result.canonical_state_bytes)
    .bind(&result.canonical_state_digest)
    .bind(source.expected_revision)
    .bind(source_result_deployment_revision)
    .bind(&source_result_snapshot_digest)
    .bind(result_deployment_id)
    .bind(result_deployment_revision)
    .bind(result_deployment_snapshot_digest.as_deref())
    .bind(source.source_epoch)
    .bind(successor_epoch)
    .bind(&product_receipt_id)
    .bind(&product_audit_event_id)
    .bind(&authority_observation_digest)
    .bind(source.installation_authority_revision)
    .bind(terminal_time)
    .fetch_one(&mut **transaction)
    .await
    .unwrap();
    let projection_digest =
        sqlx::query_scalar::<_, String>("SELECT pg_catalog.encode(pg_catalog.sha256($1), 'hex')")
            .bind(&projection)
            .fetch_one(&mut **transaction)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_product_drain_terminal_actions_v2 (\
            terminal_action_id, terminal_kind, drain_intent_id, \
            product_operation_id, product_mutation_digest, drain_intent_digest, \
            product_action_idempotency_digest, \
            product_action_semantic_request_digest, cancellation_reason_digest, \
            source_intent_revision, source_canonical_state_digest, \
            result_intent_revision, result_canonical_state_digest, \
            source_deployment_revision, source_result_deployment_revision, \
            source_result_deployment_snapshot_digest, \
            source_result_deployment_snapshot_bytes, result_deployment_id, \
            result_deployment_revision, result_deployment_snapshot_digest, \
            result_deployment_snapshot_bytes, \
            source_slot_writer_epoch, successor_slot_writer_epoch, \
            terminal_database_time, product_receipt_id, product_audit_event_id, \
            authority_observation_digest, installation_authority_revision, \
            terminal_projection_bytes, terminal_projection_digest\
         ) VALUES (\
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,\
            $15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30\
         )",
    )
    .bind(&terminal_action_id)
    .bind(terminal_kind)
    .bind(&source.drain_intent_id)
    .bind(&source.product_operation_id)
    .bind(&source.product_mutation_digest)
    .bind(&source.drain_intent_digest)
    .bind(&product_action_idempotency_digest)
    .bind(&product_action_semantic_request_digest)
    .bind(cancellation_reason_digest.as_deref())
    .bind(source.source_intent_revision)
    .bind(&source.source_state_digest)
    .bind(result.intent_revision)
    .bind(&result.canonical_state_digest)
    .bind(source.expected_revision)
    .bind(source_result_deployment_revision)
    .bind(&source_result_snapshot_digest)
    .bind(&source_result_snapshot_bytes)
    .bind(result_deployment_id)
    .bind(result_deployment_revision)
    .bind(result_deployment_snapshot_digest.as_deref())
    .bind(result_deployment_snapshot_bytes.as_deref())
    .bind(source.source_epoch)
    .bind(successor_epoch)
    .bind(terminal_time)
    .bind(&product_receipt_id)
    .bind(&product_audit_event_id)
    .bind(&authority_observation_digest)
    .bind(source.installation_authority_revision)
    .bind(&projection)
    .bind(&projection_digest)
    .execute(&mut **transaction)
    .await
    .unwrap();
    projection
}

async fn assert_product_drain_terminal_result(
    database: &IsolatedDatabase,
    source: &ProductDrainTerminalSourceRow,
    terminal_kind: &str,
    projection: &[u8],
) {
    let Json(drain) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.to_jsonb(drain) \
         FROM public.runtime_drain_intents_v2 AS drain \
         WHERE drain.drain_intent_id = $1",
    )
    .bind(&source.drain_intent_id)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(drain["intent_state"], json!(terminal_kind));
    assert_eq!(
        drain["intent_revision"],
        json!(source.source_intent_revision + 1)
    );
    let canonical_state: Value = serde_json::from_slice(&source_state_bytes(&drain)).unwrap();
    if terminal_kind == "consumed" {
        assert_eq!(canonical_state["state"]["resulting_revision"], json!(1));
    } else {
        assert!(canonical_state["state"].get("resulting_revision").is_none());
    }
    let Json(fence) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.to_jsonb(fence) \
         FROM public.runtime_slot_writer_fences_v2 AS fence \
         WHERE fence.slot_guild_id = $1 AND fence.slot_ruleset_key = $2",
    )
    .bind(&source.slot_guild_id)
    .bind(&source.slot_ruleset_key)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(fence["writer_epoch"], json!(source.source_epoch + 1));
    for field in [
        "pending_drain_intent_id",
        "pending_product_operation_id",
        "pending_tenant_id",
        "pending_installation_id",
        "pending_deployment_id",
        "pending_expected_revision",
        "pending_marked_at",
    ] {
        assert!(fence[field].is_null(), "{field}");
    }
    let Json(action) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.to_jsonb(action) \
         FROM public.runtime_product_drain_terminal_actions_v2 AS action \
         WHERE action.drain_intent_id = $1",
    )
    .bind(&source.drain_intent_id)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(action["source_deployment_revision"], json!(8));
    assert_eq!(action["source_result_deployment_revision"], json!(9));
    if terminal_kind == "consumed" {
        assert_eq!(
            action["result_deployment_id"],
            json!(TERMINAL_RESULT_DEPLOYMENT_ID)
        );
        assert_eq!(action["result_deployment_revision"], json!(1));
    } else {
        assert!(action["result_deployment_id"].is_null());
        assert!(action["result_deployment_revision"].is_null());
        assert!(action["result_deployment_snapshot_digest"].is_null());
    }
    let exact = sqlx::query_scalar::<_, bool>(
        "SELECT starring_runtime_private_v2.\
         starring_runtime_product_drain_terminal_action_exact_v2(\
            action, product, drain\
         ) \
         FROM public.runtime_product_drain_terminal_actions_v2 AS action \
         INNER JOIN public.runtime_product_operations_v2 AS product \
             ON product.product_operation_id = action.product_operation_id \
         INNER JOIN public.runtime_drain_intents_v2 AS drain \
             ON drain.drain_intent_id = action.drain_intent_id \
         WHERE action.drain_intent_id = $1",
    )
    .bind(&source.drain_intent_id)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert!(exact);
    let stored_projection = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT terminal_projection_bytes \
         FROM public.runtime_product_drain_terminal_actions_v2 \
         WHERE drain_intent_id = $1",
    )
    .bind(&source.drain_intent_id)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(stored_projection, projection);
    let candidate = sqlx::query_as::<_, (i64, Option<String>)>(
        "SELECT active_pending_count, selected_drain_intent_id \
         FROM starring_runtime_private_v2.\
         starring_runtime_pending_drain_candidate_v2()",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(candidate.0, 0);
    assert!(candidate.1.is_none());
}

fn source_state_bytes(drain: &Value) -> Vec<u8> {
    let encoded = drain["canonical_state_bytes"].as_str().unwrap();
    let encoded = encoded.strip_prefix("\\x").unwrap();
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn acknowledged_microseconds(source_state_bytes: &[u8]) -> i64 {
    let state = serde_json::from_slice::<Value>(source_state_bytes).unwrap();
    state["state"]["acknowledgement"]["acknowledged_at_unix_microseconds"]
        .as_i64()
        .unwrap()
}

fn replace_acknowledged_microseconds(source_state_bytes: &[u8], replacement: i64) -> Vec<u8> {
    let original = acknowledged_microseconds(source_state_bytes);
    let original_fragment = format!("\"acknowledged_at_unix_microseconds\":{original}");
    let replacement_fragment = format!("\"acknowledged_at_unix_microseconds\":{replacement}");
    let source = String::from_utf8(source_state_bytes.to_vec()).unwrap();
    assert_eq!(source.matches(&original_fragment).count(), 1);
    source
        .replacen(&original_fragment, &replacement_fragment, 1)
        .into_bytes()
}

async fn assert_product_drain_terminal_clock_and_revision_guards(
    database: &IsolatedDatabase,
    source: &ProductDrainTerminalSourceRow,
    terminal_kind: &str,
) {
    let acknowledgement_microseconds = acknowledged_microseconds(&source.source_state_bytes);
    let regressed_time =
        DateTime::<Utc>::from_timestamp_micros(acknowledgement_microseconds - 1).unwrap();
    let mut regressed = database.owner_pool.begin().await.unwrap();
    let requested_resulting_revision = if terminal_kind == "consumed" {
        1_i64
    } else {
        source.expected_revision + 1
    };
    let error = call_product_drain_terminal_transition(
        &mut *regressed,
        source,
        terminal_kind,
        requested_resulting_revision,
        regressed_time,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "runtime_product_drain_terminal_transition_causal_clock_invalid"
    );
    regressed.rollback().await.unwrap();

    if terminal_kind == "consumed" {
        let mut wrong_revision = database.owner_pool.begin().await.unwrap();
        let terminal_time =
            sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
                .fetch_one(&mut *wrong_revision)
                .await
                .unwrap();
        let error = call_product_drain_terminal_transition(
            &mut *wrong_revision,
            source,
            terminal_kind,
            2,
            terminal_time,
        )
        .await
        .unwrap_err();
        assert_sqlstate(&error, "RX002");
        wrong_revision.rollback().await.unwrap();
    }

    let mut independent_release = database.owner_pool.begin().await.unwrap();
    let terminal_time =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *independent_release)
            .await
            .unwrap();
    let result = call_product_drain_terminal_transition(
        &mut *independent_release,
        source,
        terminal_kind,
        requested_resulting_revision,
        terminal_time,
    )
    .await
    .unwrap();
    let forged_source_state_bytes = replace_acknowledged_microseconds(
        &source.source_state_bytes,
        terminal_time.timestamp_micros() + 1,
    );
    let forged_source_state_digest =
        sqlx::query_scalar::<_, String>("SELECT pg_catalog.encode(pg_catalog.sha256($1), 'hex')")
            .bind(&forged_source_state_bytes)
            .fetch_one(&mut *independent_release)
            .await
            .unwrap();
    let error = call_product_drain_terminal_release(
        &mut *independent_release,
        source,
        &forged_source_state_bytes,
        &forged_source_state_digest,
        &result,
        terminal_kind,
        terminal_time,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "runtime_slot_writer_fence_terminal_release_causal_clock_invalid"
    );
    independent_release.rollback().await.unwrap();
}

async fn assert_product_drain_terminal_table_shape_guards(
    database: &IsolatedDatabase,
    terminal_kind: &str,
) {
    let overrides = if terminal_kind == "consumed" {
        vec![
            "pg_catalog.jsonb_build_object(\
                'terminal_action_id', pg_catalog.repeat('7', 64), \
                'result_deployment_id', NULL, \
                'result_deployment_revision', NULL, \
                'result_deployment_snapshot_digest', NULL\
             )",
            "pg_catalog.jsonb_build_object(\
                'terminal_action_id', pg_catalog.repeat('8', 64), \
                'result_deployment_revision', 2\
             )",
            "pg_catalog.jsonb_build_object(\
                'terminal_action_id', pg_catalog.repeat('9', 64), \
                'result_deployment_id', 'invalid deployment'\
             )",
        ]
    } else {
        vec![
            "pg_catalog.jsonb_build_object(\
                'terminal_action_id', pg_catalog.repeat('9', 64), \
                'cancellation_reason_digest', NULL\
             )",
        ]
    };
    for override_expression in overrides {
        let statement = format!(
            "INSERT INTO public.runtime_product_drain_terminal_actions_v2 \
             SELECT (pg_catalog.jsonb_populate_record(\
                NULL::public.runtime_product_drain_terminal_actions_v2, \
                pg_catalog.to_jsonb(action) || {override_expression}\
             )).* \
             FROM public.runtime_product_drain_terminal_actions_v2 AS action \
             LIMIT 1"
        );
        let error = sqlx::query(&statement)
            .execute(&database.owner_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "23514");
    }
}

async fn exercise_product_drain_terminal_kind(
    database: &IsolatedDatabase,
    terminal_kind: &str,
    verify_missing_journal_rollback: bool,
) {
    let source = seed_product_drain_terminal_source(
        database,
        if terminal_kind == "consumed" {
            "41414141414141414141414141414141"
        } else {
            "42424242424242424242424242424242"
        },
    )
    .await;
    assert_eq!(source.expected_revision, 8);
    assert_eq!(source.source_epoch, 2);
    assert_product_drain_terminal_clock_and_revision_guards(database, &source, terminal_kind).await;
    if verify_missing_journal_rollback {
        let mut incomplete = database.owner_pool.begin().await.unwrap();
        let incomplete_time =
            sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
                .fetch_one(&mut *incomplete)
                .await
                .unwrap();
        transition_and_release_product_drain(
            &mut incomplete,
            &source,
            terminal_kind,
            incomplete_time,
        )
        .await;
        let error = incomplete.commit().await.unwrap_err();
        assert_sqlstate(&error, "23514");
        let persisted = sqlx::query_as::<_, (String, i64, i64, Option<String>)>(
            "SELECT drain.intent_state, drain.intent_revision, fence.writer_epoch, \
                    fence.pending_drain_intent_id \
             FROM public.runtime_drain_intents_v2 AS drain \
             INNER JOIN public.runtime_slot_writer_fences_v2 AS fence \
                 ON fence.slot_guild_id = drain.slot_guild_id \
                 AND fence.slot_ruleset_key = drain.slot_ruleset_key \
             WHERE drain.drain_intent_id = $1",
        )
        .bind(&source.drain_intent_id)
        .fetch_one(&database.owner_pool)
        .await
        .unwrap();
        assert_eq!(persisted.0, "route_absent_acknowledged");
        assert_eq!(persisted.1, source.source_intent_revision);
        assert_eq!(persisted.2, source.source_epoch);
        assert_eq!(
            persisted.3.as_deref(),
            Some(source.drain_intent_id.as_str())
        );
    }
    let mut terminal = database.owner_pool.begin().await.unwrap();
    let terminal_time =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *terminal)
            .await
            .unwrap();
    let (result, successor_epoch) =
        transition_and_release_product_drain(&mut terminal, &source, terminal_kind, terminal_time)
            .await;
    let projection = insert_product_drain_terminal_action(
        &mut terminal,
        &source,
        &result,
        successor_epoch,
        terminal_kind,
        terminal_time,
    )
    .await;
    terminal.commit().await.unwrap();
    assert_product_drain_terminal_result(database, &source, terminal_kind, &projection).await;
    assert_product_drain_terminal_table_shape_guards(database, terminal_kind).await;
    for statement in [
        "UPDATE public.runtime_product_drain_terminal_actions_v2 \
         SET terminal_projection_digest = terminal_projection_digest",
        "DELETE FROM public.runtime_product_drain_terminal_actions_v2",
        "TRUNCATE public.runtime_product_drain_terminal_actions_v2",
    ] {
        let error = sqlx::query(statement)
            .execute(&database.owner_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "23514");
    }
    let denied = sqlx::query("SELECT * FROM public.runtime_product_drain_terminal_actions_v2")
        .fetch_all(&database.executor_pool)
        .await
        .unwrap_err();
    assert_sqlstate(&denied, "42501");
    let private_denied = sqlx::query(
        "SELECT starring_runtime_private_v2.\
         starring_runtime_product_drain_terminal_transition_v2(\
            $1,$2,$3,'consumed',1,pg_catalog.clock_timestamp()\
         )",
    )
    .bind(&source.drain_intent_id)
    .bind(source.source_intent_revision)
    .bind(&source.source_state_digest)
    .fetch_all(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&private_denied, "42501");
    let public_terminal_capability_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_proc AS function_row \
         WHERE function_row.pronamespace = pg_catalog.to_regnamespace('public') \
             AND function_row.proname IN (\
                 'starring_product_apply_consume_runtime_drain_v2', \
                 'starring_product_cancel_runtime_drain_v2'\
             )",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(public_terminal_capability_count, 1);
    let runtime_executor_can_consume = sqlx::query_scalar::<_, bool>(
        "SELECT pg_catalog.has_function_privilege(\
             CURRENT_USER, function_row.oid, 'EXECUTE'\
         ) \
         FROM pg_catalog.pg_proc AS function_row \
         WHERE function_row.pronamespace = pg_catalog.to_regnamespace('public') \
             AND function_row.proname = \
                 'starring_product_apply_consume_runtime_drain_v2'",
    )
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    assert!(!runtime_executor_can_consume);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn product_drain_terminal_substrate_is_exact_private_and_atomic() {
    let server = PostgresTestServer::start();
    let mut consumed_database = isolated_database(server.connect_options()).await;
    exercise_product_drain_terminal_kind(&consumed_database, "consumed", true).await;
    assert_cross_runtime_readiness(&mut consumed_database).await;
    cleanup(consumed_database).await;
    let cancelled_database = isolated_database(server.connect_options()).await;
    exercise_product_drain_terminal_kind(&cancelled_database, "cancelled", false).await;
    cleanup(cancelled_database).await;
    drop(server);
}
