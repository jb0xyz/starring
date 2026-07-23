async fn reopen_applied_activation(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
) -> Result<(), sqlx::Error> {
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut **transaction)
        .await?;
    let changed = sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'approved' \
         WHERE id = $1 AND state = 'applied' AND product_revision = 4",
    )
    .bind(&fixture.activation_id)
    .execute(&mut **transaction)
    .await?;
    assert_eq!(changed.rows_affected(), 1);
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn seed_competing_product_activation(
    pool: &PgPool,
    source: &Fixture,
    label: &str,
) -> Fixture {
    let unique = suffix();
    let promotion_id = digest(&format!("competing-promotion:{label}:{unique}"));
    let promotion_request_digest = digest(&format!("competing-promotion-request:{label}:{unique}"));
    let activation_id = format!("competing_{label}_{unique}");
    let payload_digest = digest(&format!("competing-payload:{label}:{unique}"));
    let context_digest = digest(&format!("competing-context:{label}:{unique}"));
    let (
        principal_id,
        requester_id,
        created_at,
        expires_at,
        linked_at,
        Json(mut promotion_record),
        Json(mut approval_context),
    ) = sqlx::query_as::<
        _,
        (
            String,
            String,
            DateTime<Utc>,
            DateTime<Utc>,
            DateTime<Utc>,
            Json<Value>,
            Json<Value>,
        ),
    >(
        "SELECT promotion.principal_id, activation.requester_id, \
          activation.created_at, activation.expires_at, activation.linked_at, \
          promotion.record, activation.approval_context \
         FROM public.authoring_promotions AS promotion \
         INNER JOIN public.activation_requests AS activation \
          ON activation.promotion_id = promotion.id \
         WHERE promotion.id = $1 AND activation.id = $2",
    )
    .bind(&source.promotion_id)
    .bind(&source.activation_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let target_content_hash = promotion_record["stage"]["publication"]["content_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let context = &mut approval_context["context"];
    context["promotion_id"] = json!(&promotion_id);
    context["promotion_request_digest"] = json!(&promotion_request_digest);
    context["approval_payload_digest"] = json!(&payload_digest);
    context["approval_context_digest"] = json!(&context_digest);
    promotion_record["id"] = json!(&promotion_id);
    promotion_record["request_digest"] = json!(&promotion_request_digest);
    promotion_record["stage"]["activation"]["request_id"] = json!(&activation_id);
    promotion_record["stage"]["activation"]["approval_context"] = context.clone();

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    insert_activation_pending_promotion(
        &mut transaction,
        &promotion_id,
        &promotion_request_digest,
        &source.tenant_id,
        &source.installation_id,
        &principal_id,
        &promotion_record,
    )
    .await;
    let inserted = sqlx::query(
        "INSERT INTO public.activation_requests \
         (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, state, created_at, expires_at, authority_kind, link_state_name, \
          approval_context, link_state, promotion_id, promotion_request_digest, \
          approval_payload_digest, approval_context_digest, linked_at) \
         VALUES ($1, $2, $3, 1, $4, $5, 1, 'pending', $6, $7, \
          'product_authoring', 'linked', $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(&activation_id)
    .bind(&source.guild_id)
    .bind(&source.ruleset_key)
    .bind(&target_content_hash)
    .bind(&requester_id)
    .bind(created_at)
    .bind(expires_at)
    .bind(Json(&approval_context))
    .bind(Json(json!({"state": "linked", "linked_at": linked_at})))
    .bind(&promotion_id)
    .bind(&promotion_request_digest)
    .bind(&payload_digest)
    .bind(&context_digest)
    .bind(linked_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(inserted.rows_affected(), 1);
    sqlx::query("SELECT pg_catalog.set_config('starring.product_approval_gate', $1, TRUE)")
        .bind(&context_digest)
        .execute(&mut *transaction)
        .await
        .unwrap();
    let approved = sqlx::query(
        "INSERT INTO public.activation_request_approvals \
         (request_id, tenant_id, installation_id, approver_id, approved_at, \
          approval_payload_digest) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&activation_id)
    .bind(&source.tenant_id)
    .bind(&source.installation_id)
    .bind(&source.actor.user_id)
    .bind(linked_at + TimeDelta::seconds(1))
    .bind(&payload_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(approved.rows_affected(), 1);
    let transitioned = sqlx::query(
        "UPDATE public.activation_requests SET state = 'approved', product_revision = 2 \
         WHERE id = $1 AND state = 'pending' AND product_revision = 1",
    )
    .bind(&activation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(transitioned.rows_affected(), 1);
    transaction.commit().await.unwrap();

    Fixture {
        tenant_id: source.tenant_id.clone(),
        installation_id: source.installation_id.clone(),
        promotion_id,
        activation_id,
        actor: source.actor.clone(),
        application_id: source.application_id.clone(),
        guild_id: source.guild_id.clone(),
        ruleset_key: source.ruleset_key.clone(),
        payload_digest,
        authority_digest: source.authority_digest.clone(),
        observation_digest: source.observation_digest.clone(),
    }
}

async fn supersede_runtime_generation(pool: &PgPool, fixture: &Fixture, deployment_id: &str) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let changed = sqlx::query(
        "UPDATE public.runtime_deployments \
         SET phase = 'superseded', revision = revision + 1, \
          superseded_at = GREATEST(pg_catalog.clock_timestamp(), requested_at), \
          snapshot = pg_catalog.jsonb_set(snapshot, '{phase}', '\"superseded\"'::JSONB), \
          updated_at = pg_catalog.clock_timestamp() \
         WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3 \
          AND guild_id = $4 AND ruleset_key = $5 AND phase = 'requested'",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(deployment_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn set_existing_runtime_phase(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    deployment_id: &str,
    phase: &str,
) -> Result<(), sqlx::Error> {
    if phase == "requested" {
        return Ok(());
    }
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut **transaction)
        .await?;
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut **transaction)
        .await?;
    let changed = sqlx::query(
        "UPDATE public.runtime_deployments \
         SET phase = $4, revision = revision + 1, \
          live_attestation_id = CASE WHEN $4 = 'live' THEN $5 ELSE NULL END, \
          live_at = CASE WHEN $4 = 'live' \
           THEN GREATEST(pg_catalog.clock_timestamp(), requested_at) ELSE NULL END, \
          snapshot = pg_catalog.jsonb_set(snapshot, '{phase}', pg_catalog.to_jsonb($4::TEXT)), \
          updated_at = pg_catalog.clock_timestamp() \
         WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3 \
          AND guild_id = $6 AND ruleset_key = $7 AND phase = 'requested'",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(deployment_id)
    .bind(phase)
    .bind(digest(&format!("runtime-attestation:{deployment_id}")))
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut **transaction)
    .await?;
    assert_eq!(changed.rows_affected(), 1);
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn insert_newer_requested_runtime(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    source_deployment_id: &str,
) -> Result<String, sqlx::Error> {
    let unique = suffix();
    let promotion_id = digest(&format!("coexisting-promotion:{unique}"));
    let promotion_request_digest = digest(&format!("coexisting-promotion-request:{unique}"));
    let activation_id = format!("coexisting_activation_{unique}");
    let deployment_id = format!("coexisting-deployment-{unique}");
    let desired_target_digest = digest(&format!("coexisting-target:{unique}"));
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         SELECT (pg_catalog.jsonb_populate_record(\
          NULL::public.authoring_promotions, \
          pg_catalog.to_jsonb(promotion) || pg_catalog.jsonb_build_object(\
           'id', $2, \
           'request_digest', $3, \
           'record', pg_catalog.jsonb_set(\
            pg_catalog.jsonb_set(\
             promotion.record, '{id}', pg_catalog.to_jsonb($2::TEXT)), \
            '{request_digest}', pg_catalog.to_jsonb($3::TEXT))\
          ))).* \
         FROM public.authoring_promotions AS promotion WHERE promotion.id = $1",
    )
    .bind(&fixture.promotion_id)
    .bind(&promotion_id)
    .bind(&promotion_request_digest)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO public.activation_requests \
         SELECT (pg_catalog.jsonb_populate_record(\
          NULL::public.activation_requests, \
          pg_catalog.to_jsonb(activation) || pg_catalog.jsonb_build_object(\
           'id', $2, \
           'promotion_id', $3, \
           'promotion_request_digest', $4, \
           'approval_context', pg_catalog.jsonb_set(\
            pg_catalog.jsonb_set(\
             activation.approval_context, \
             '{context,promotion_id}', pg_catalog.to_jsonb($3::TEXT)), \
            '{context,promotion_request_digest}', pg_catalog.to_jsonb($4::TEXT))\
          ))).* \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&activation_id)
    .bind(&promotion_id)
    .bind(&promotion_request_digest)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO public.runtime_deployments \
         SELECT (pg_catalog.jsonb_populate_record(\
          NULL::public.runtime_deployments, \
          pg_catalog.to_jsonb(deployment) || pg_catalog.jsonb_build_object(\
           'deployment_id', $2, \
           'promotion_id', $3, \
           'activation_request_id', $4, \
           'desired_target_digest', $5, \
           'runtime_generation', 2, \
           'revision', 1, \
           'phase', 'requested', \
           'live_attestation_id', NULL, \
           'live_at', NULL, \
           'snapshot', pg_catalog.jsonb_set(\
            deployment.snapshot, '{phase}', '\"requested\"'::JSONB), \
           'updated_at', deployment.requested_at\
          ))).* \
         FROM public.runtime_deployments AS deployment \
         WHERE deployment.deployment_id = $1",
    )
    .bind(source_deployment_id)
    .bind(&deployment_id)
    .bind(&promotion_id)
    .bind(&activation_id)
    .bind(&desired_target_digest)
    .execute(&mut **transaction)
    .await?;
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut **transaction)
        .await?;
    Ok(deployment_id)
}

const EXISTING_RUNTIME_PRODUCT_STATE_SQL: &str = "SELECT pg_catalog.jsonb_build_object(\
  'activation', (\
   SELECT pg_catalog.to_jsonb(activation) \
   FROM public.activation_requests AS activation WHERE activation.id = $3), \
  'installation', (\
   SELECT pg_catalog.to_jsonb(installation) \
   FROM public.automation_installations AS installation \
   WHERE installation.tenant_id = $1 AND installation.installation_id = $2), \
  'active', (\
   SELECT pg_catalog.to_jsonb(active) \
   FROM public.automation_ruleset_activations AS active \
   WHERE active.guild_id = $4 AND active.ruleset_key = $5), \
  'deployments', (\
   SELECT COALESCE(\
    pg_catalog.jsonb_agg(pg_catalog.to_jsonb(deployment) \
     ORDER BY deployment.runtime_generation, deployment.deployment_id), \
    '[]'::JSONB) \
   FROM public.runtime_deployments AS deployment \
   WHERE deployment.tenant_id = $1 AND deployment.installation_id = $2), \
  'receipts', (\
   SELECT COALESCE(\
    pg_catalog.jsonb_agg(pg_catalog.to_jsonb(receipt) ORDER BY receipt.receipt_id), \
    '[]'::JSONB) \
   FROM public.product_action_receipts AS receipt \
   WHERE receipt.tenant_id = $1 AND receipt.installation_id = $2), \
  'aliases', (\
   SELECT COALESCE(\
    pg_catalog.jsonb_agg(pg_catalog.to_jsonb(alias) \
     ORDER BY alias.idempotency_key_digest), \
    '[]'::JSONB) \
   FROM public.product_action_receipt_idempotency_aliases AS alias \
   WHERE alias.tenant_id = $1 AND alias.installation_id = $2), \
  'audits', (\
   SELECT COALESCE(\
    pg_catalog.jsonb_agg(pg_catalog.to_jsonb(audit) ORDER BY audit.event_id), \
    '[]'::JSONB) \
   FROM public.product_audit_events AS audit \
   WHERE audit.tenant_id = $1 AND audit.installation_id = $2), \
  'evidence', (\
   SELECT COALESCE(\
    pg_catalog.jsonb_agg(pg_catalog.to_jsonb(evidence) ORDER BY evidence.event_id), \
    '[]'::JSONB) \
   FROM public.product_action_receipt_audit_evidence AS evidence \
   WHERE evidence.tenant_id = $1 AND evidence.installation_id = $2))";

async fn existing_runtime_product_state(pool: &PgPool, fixture: &Fixture) -> Json<Value> {
    sqlx::query_scalar(EXISTING_RUNTIME_PRODUCT_STATE_SQL)
        .bind(&fixture.tenant_id)
        .bind(&fixture.installation_id)
        .bind(&fixture.activation_id)
        .bind(&fixture.guild_id)
        .bind(&fixture.ruleset_key)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn existing_runtime_product_state_in(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
) -> Json<Value> {
    sqlx::query_scalar(EXISTING_RUNTIME_PRODUCT_STATE_SQL)
        .bind(&fixture.tenant_id)
        .bind(&fixture.installation_id)
        .bind(&fixture.activation_id)
        .bind(&fixture.guild_id)
        .bind(&fixture.ruleset_key)
        .fetch_one(&mut **transaction)
        .await
        .unwrap()
}

async fn product_apply_operation_state(
    pool: &PgPool,
    fixture: &Fixture,
    operation: &Operation,
) -> Json<Value> {
    sqlx::query_scalar(
        "SELECT pg_catalog.jsonb_build_object(\
          'promotion', (\
           SELECT pg_catalog.to_jsonb(promotion) \
           FROM public.authoring_promotions AS promotion WHERE promotion.id = $2), \
          'activation', (\
           SELECT pg_catalog.to_jsonb(activation) \
           FROM public.activation_requests AS activation WHERE activation.id = $1), \
          'deployments', (\
           SELECT COALESCE(\
            pg_catalog.jsonb_agg(pg_catalog.to_jsonb(deployment) \
             ORDER BY deployment.runtime_generation, deployment.deployment_id), \
            '[]'::JSONB) \
           FROM public.runtime_deployments AS deployment \
           WHERE deployment.activation_request_id = $1 OR deployment.deployment_id = $3), \
          'receipts', (\
           SELECT COALESCE(\
            pg_catalog.jsonb_agg(pg_catalog.to_jsonb(receipt) ORDER BY receipt.receipt_id), \
            '[]'::JSONB) \
           FROM public.product_action_receipts AS receipt WHERE receipt.receipt_id = $4), \
          'aliases', (\
           SELECT COALESCE(\
            pg_catalog.jsonb_agg(pg_catalog.to_jsonb(alias) \
             ORDER BY alias.idempotency_key_digest), \
            '[]'::JSONB) \
           FROM public.product_action_receipt_idempotency_aliases AS alias \
           WHERE alias.receipt_id = $4), \
          'audits', (\
           SELECT COALESCE(\
            pg_catalog.jsonb_agg(pg_catalog.to_jsonb(audit) ORDER BY audit.event_id), \
            '[]'::JSONB) \
           FROM public.product_audit_events AS audit \
           WHERE audit.event_id = $5 OR audit.receipt_id = $4 OR audit.request_id = $6), \
          'evidence', (\
           SELECT COALESCE(\
            pg_catalog.jsonb_agg(pg_catalog.to_jsonb(evidence) ORDER BY evidence.event_id), \
            '[]'::JSONB) \
           FROM public.product_action_receipt_audit_evidence AS evidence \
           WHERE evidence.receipt_id = $4 OR evidence.event_id = $5))",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.promotion_id)
    .bind(&operation.deployment_id)
    .bind(&operation.receipt_id)
    .bind(&operation.audit_event_id)
    .bind(&operation.request_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn assert_closed_apply_result(result: &LockRow, outcome: &str) {
    assert_eq!(result.outcome, outcome);
    assert!(!result.exact_replay);
    assert!(!result.requires_commit);
    assert!(result.resulting_revision.is_none());
    assert!(result.resulting_state.is_none());
    assert!(result.deployment_id.is_none());
    assert!(result.desired_target_digest.is_none());
    assert!(result.locked_projection.is_none());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_apply_waits_at_serving_slot_before_deployment_lock_and_retains_both() {
    let database = isolated_database("apply_serving_slot_order").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await.unwrap();
        let fixture = seed_fixture(&database.pool).await;
        let applied_operation = Operation::new("serving-slot-seed");
        complete_apply(&database.pool, &fixture, &applied_operation).await;
        let mut reopen = database.pool.begin().await?;
        reopen_applied_activation(&mut reopen, &fixture).await?;
        reopen.commit().await?;
        let operation = Operation::new("serving-slot-order");
        let durable_before = existing_runtime_product_state(&database.pool, &fixture).await;
        let mut slot = database.pool.begin().await?;
        sqlx::query(
            "SELECT pg_catalog.pg_advisory_xact_lock(\
             pg_catalog.hashtextextended(\
              pg_catalog.concat('starring-runtime-serving-slot-v1:', $1, ':', $2), 0))",
        )
        .bind(&fixture.guild_id)
        .bind(&fixture.ruleset_key)
        .execute(&mut *slot)
        .await?;

        let (started_sender, started_receiver) = futures::channel::oneshot::channel();
        let (locked_sender, locked_receiver) = futures::channel::oneshot::channel();
        let (release_sender, release_receiver) = futures::channel::oneshot::channel();
        let apply_pool = database.pool.clone();
        let apply_fixture = fixture.clone();
        let apply_operation = operation.clone();
        let apply = tokio::spawn(async move {
            let mut transaction = begin_serializable(&apply_pool).await;
            let process_id = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
                .fetch_one(&mut *transaction)
                .await?;
            let _ = started_sender.send(process_id);
            let mut call = Call::valid(&apply_fixture);
            call.expected_revision = 4;
            let locked =
                lock_apply(&mut transaction, &apply_fixture, &apply_operation, &call).await;
            match locked {
                Ok(locked) => {
                    let _ = locked_sender.send(Ok(locked.outcome));
                    let _ = release_receiver.await;
                    transaction.rollback().await?;
                }
                Err(error) => {
                    let _ = locked_sender.send(Err(error));
                    transaction.rollback().await?;
                }
            }
            Ok::<_, sqlx::Error>(())
        });
        let process_id = started_receiver.await.unwrap();
        wait_for_advisory_lock_wait(&database.pool, process_id).await;

        let mut row_probe = database.pool.begin().await?;
        let unlocked_deployment = sqlx::query_scalar::<_, String>(
            "SELECT deployment_id FROM public.runtime_deployments \
             WHERE deployment_id = $1 FOR UPDATE NOWAIT",
        )
        .bind(&applied_operation.deployment_id)
        .fetch_one(&mut *row_probe)
        .await?;
        assert_eq!(unlocked_deployment, applied_operation.deployment_id);
        row_probe.rollback().await?;

        slot.rollback().await?;
        assert_eq!(locked_receiver.await.unwrap()?, "runtime_pending_conflict");

        let mut contention = database.pool.begin().await?;
        let slot_available = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.pg_try_advisory_xact_lock(\
             pg_catalog.hashtextextended(\
              pg_catalog.concat('starring-runtime-serving-slot-v1:', $1, ':', $2), 0))",
        )
        .bind(&fixture.guild_id)
        .bind(&fixture.ruleset_key)
        .fetch_one(&mut *contention)
        .await?;
        assert!(!slot_available);
        let row_error = sqlx::query(
            "SELECT deployment_id FROM public.runtime_deployments \
             WHERE deployment_id = $1 FOR UPDATE NOWAIT",
        )
        .bind(&applied_operation.deployment_id)
        .execute(&mut *contention)
        .await
        .expect_err("Product Apply must retain deployment row locks");
        assert!(matches!(
            row_error,
            sqlx::Error::Database(database_error)
                if database_error.code().as_deref() == Some("55P03")
        ));
        contention.rollback().await?;

        let _ = release_sender.send(());
        apply.await.unwrap()?;

        let mut released = database.pool.begin().await?;
        let slot_available = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.pg_try_advisory_xact_lock(\
             pg_catalog.hashtextextended(\
              pg_catalog.concat('starring-runtime-serving-slot-v1:', $1, ':', $2), 0))",
        )
        .bind(&fixture.guild_id)
        .bind(&fixture.ruleset_key)
        .fetch_one(&mut *released)
        .await?;
        assert!(slot_available);
        let released_deployment = sqlx::query_scalar::<_, String>(
            "SELECT deployment_id FROM public.runtime_deployments \
             WHERE deployment_id = $1 FOR UPDATE NOWAIT",
        )
        .bind(&applied_operation.deployment_id)
        .fetch_one(&mut *released)
        .await?;
        assert_eq!(released_deployment, applied_operation.deployment_id);
        released.rollback().await?;
        assert_eq!(
            existing_runtime_product_state(&database.pool, &fixture).await,
            durable_before
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn concurrent_first_product_insert_returns_serialization_failure() {
    let database = isolated_database("apply_first_insert").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await.unwrap();
        let base_fixture = seed_fixture(&database.pool).await;
        let base_operation = Operation::new("first-insert-base");
        complete_apply(&database.pool, &base_fixture, &base_operation).await;
        supersede_runtime_generation(&database.pool, &base_fixture, &base_operation.deployment_id)
            .await;
        let first_fixture =
            seed_competing_product_activation(&database.pool, &base_fixture, "first").await;
        let second_fixture =
            seed_competing_product_activation(&database.pool, &base_fixture, "second").await;
        let first_operation = Operation::new("first-insert-winner");
        let second_operation = Operation::new("first-insert-stale");
        let second_before =
            product_apply_operation_state(&database.pool, &second_fixture, &second_operation).await;

        let first_call = Call::valid(&first_fixture);
        let mut first = begin_serializable(&database.pool).await;
        let first_lock =
            lock_apply(&mut first, &first_fixture, &first_operation, &first_call).await?;
        assert_eq!(first_lock.outcome, "ready");
        let first_prepared = prepare_requested_deployment(&first_lock);

        let (started_sender, started_receiver) = futures::channel::oneshot::channel();
        let second_pool = database.pool.clone();
        let second_task_fixture = second_fixture.clone();
        let second_task_operation = second_operation.clone();
        let second = tokio::spawn(async move {
            let second_call = Call::valid(&second_task_fixture);
            let mut transaction = begin_serializable(&second_pool).await;
            let (process_id, snapshot_revision) = sqlx::query_as::<_, (i32, i64)>(
                "SELECT pg_catalog.pg_backend_pid(), activation.product_revision \
                 FROM public.activation_requests AS activation WHERE activation.id = $1",
            )
            .bind(&second_task_fixture.activation_id)
            .fetch_one(&mut *transaction)
            .await?;
            assert_eq!(snapshot_revision, 2);
            let _ = started_sender.send(process_id);
            let locked = lock_apply(
                &mut transaction,
                &second_task_fixture,
                &second_task_operation,
                &second_call,
            )
            .await?;
            let lock_outcome = locked.outcome.clone();
            assert_eq!(lock_outcome, "ready");
            let prepared = prepare_requested_deployment(&locked);
            let error = finalize_apply(
                &mut transaction,
                &second_task_fixture,
                &second_task_operation,
                &second_call,
                &locked,
                finalize_projection(
                    prepared.desired_target_digest(),
                    prepared.previous_runtime_json(),
                    prepared.snapshot_json(),
                ),
            )
            .await
            .expect_err("stale first insert must force a serialization retry");
            let error_code = error
                .as_database_error()
                .and_then(|database_error| database_error.code())
                .expect("stale first insert must return a PostgreSQL code")
                .into_owned();
            transaction.rollback().await?;
            Ok::<_, sqlx::Error>((lock_outcome, error_code))
        });

        let second_process_id = started_receiver.await.unwrap();
        wait_for_advisory_lock_wait(&database.pool, second_process_id).await;
        let first_finalized = finalize_apply(
            &mut first,
            &first_fixture,
            &first_operation,
            &first_call,
            &first_lock,
            finalize_projection(
                first_prepared.desired_target_digest(),
                first_prepared.previous_runtime_json(),
                first_prepared.snapshot_json(),
            ),
        )
        .await?;
        assert_eq!(first_finalized.outcome, "ok");
        assert_eq!(first_finalized.resulting_revision, Some(4));
        assert_eq!(first_finalized.resulting_state.as_deref(), Some("applied"));
        first.commit().await?;

        let (second_lock_outcome, second_error_code) = second.await.unwrap()?;
        assert_eq!(second_lock_outcome, "ready");
        assert_eq!(second_error_code, "40001");
        assert_ne!(second_error_code, "23505");

        let winning_deployment = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT runtime_generation, phase, activation_request_id \
             FROM public.runtime_deployments WHERE deployment_id = $1",
        )
        .bind(&first_operation.deployment_id)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(
            winning_deployment,
            (
                2,
                "requested".to_string(),
                first_fixture.activation_id.clone()
            )
        );

        let mut retry = begin_serializable(&database.pool).await;
        let retry_result = lock_apply(
            &mut retry,
            &second_fixture,
            &second_operation,
            &Call::valid(&second_fixture),
        )
        .await?;
        assert_closed_apply_result(&retry_result, "runtime_pending_conflict");
        retry.rollback().await?;
        assert_eq!(
            product_apply_operation_state(&database.pool, &second_fixture, &second_operation).await,
            second_before
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_apply_classifies_durable_runtime_phases_without_mutation() {
    let database = isolated_database("apply_runtime_phase").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await.unwrap();
        for (label, phase, expected_outcome) in [
            ("requested", "requested", "runtime_pending_conflict"),
            (
                "awaiting",
                "awaiting_gateway_ready",
                "runtime_drain_required",
            ),
            ("live", "live", "runtime_drain_required"),
        ] {
            let fixture = seed_fixture(&database.pool).await;
            let applied_operation = Operation::new(&format!("{label}-phase-seed"));
            complete_apply(&database.pool, &fixture, &applied_operation).await;
            let durable_before = existing_runtime_product_state(&database.pool, &fixture).await;
            let operation = Operation::new(&format!("{label}-phase-classify"));
            let mut transaction = begin_serializable(&database.pool).await;
            reopen_applied_activation(&mut transaction, &fixture).await?;
            set_existing_runtime_phase(
                &mut transaction,
                &fixture,
                &applied_operation.deployment_id,
                phase,
            )
            .await?;
            let transient_before =
                existing_runtime_product_state_in(&mut transaction, &fixture).await;

            if phase == "awaiting_gateway_ready" {
                let precedence_operation = Operation::new("drain-precedence");
                let mut stale_call = Call::valid(&fixture);
                stale_call.expected_revision = 3;
                let precedence = lock_apply(
                    &mut transaction,
                    &fixture,
                    &precedence_operation,
                    &stale_call,
                )
                .await?;
                assert_closed_apply_result(&precedence, "revision_conflict");
            }

            let mut call = Call::valid(&fixture);
            call.expected_revision = 4;
            let classified = lock_apply(&mut transaction, &fixture, &operation, &call).await?;
            assert_closed_apply_result(&classified, expected_outcome);
            assert_eq!(
                existing_runtime_product_state_in(&mut transaction, &fixture).await,
                transient_before
            );
            transaction.rollback().await?;
            assert_eq!(
                existing_runtime_product_state(&database.pool, &fixture).await,
                durable_before
            );
        }
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_apply_classifies_the_latest_relevant_runtime_phase() {
    let database = isolated_database("apply_latest_phase").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await.unwrap();
        let fixture = seed_fixture(&database.pool).await;
        let applied_operation = Operation::new("latest-phase-seed");
        complete_apply(&database.pool, &fixture, &applied_operation).await;
        let durable_before = existing_runtime_product_state(&database.pool, &fixture).await;
        let operation = Operation::new("latest-phase-classify");
        let mut transaction = begin_serializable(&database.pool).await;
        reopen_applied_activation(&mut transaction, &fixture).await?;
        set_existing_runtime_phase(
            &mut transaction,
            &fixture,
            &applied_operation.deployment_id,
            "live",
        )
        .await?;
        let newer_deployment_id = insert_newer_requested_runtime(
            &mut transaction,
            &fixture,
            &applied_operation.deployment_id,
        )
        .await?;
        let generations = sqlx::query_as::<_, (String, i64, String)>(
            "SELECT deployment_id, runtime_generation, phase \
             FROM public.runtime_deployments \
             WHERE guild_id = $1 AND ruleset_key = $2 \
             ORDER BY runtime_generation, deployment_id",
        )
        .bind(&fixture.guild_id)
        .bind(&fixture.ruleset_key)
        .fetch_all(&mut *transaction)
        .await?;
        assert_eq!(
            generations,
            vec![
                (
                    applied_operation.deployment_id.clone(),
                    1,
                    "live".to_string()
                ),
                (newer_deployment_id, 2, "requested".to_string())
            ]
        );
        let transient_before = existing_runtime_product_state_in(&mut transaction, &fixture).await;
        let mut call = Call::valid(&fixture);
        call.expected_revision = 4;
        let classified = lock_apply(&mut transaction, &fixture, &operation, &call).await?;
        assert_closed_apply_result(&classified, "runtime_pending_conflict");
        assert_eq!(
            existing_runtime_product_state_in(&mut transaction, &fixture).await,
            transient_before
        );
        transaction.rollback().await?;
        assert_eq!(
            existing_runtime_product_state(&database.pool, &fixture).await,
            durable_before
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}
