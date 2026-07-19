#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn fresh_apply_exact_replay_and_semantic_conflict_are_atomic() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("fresh");
    let mut transaction = begin_serializable(&pool).await;
    let call = Call::valid(&fixture);
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    assert!(!lock.exact_replay);
    assert!(!lock.requires_commit);
    assert_eq!(lock.resulting_revision, Some(2));
    assert_eq!(lock.resulting_state.as_deref(), Some("approved"));
    assert_eq!(
        lock.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert!(lock.desired_target_digest.is_none());
    let unchanged = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $2) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(unchanged, ("approved".to_string(), 2, 0, 0));
    let prepared = prepare_requested_deployment(&lock);
    let finalized = finalize_apply(
        &mut transaction,
        &fixture,
        &operation,
        &call,
        &lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(finalized.outcome, "ok");
    assert_eq!(finalized.resulting_revision, Some(4));
    assert_eq!(finalized.resulting_state.as_deref(), Some("applied"));
    assert!(!finalized.exact_replay);
    assert_eq!(finalized.guild_id.as_deref(), Some(&*fixture.guild_id));
    assert_eq!(
        finalized.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert_eq!(
        finalized.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    transaction.commit().await.unwrap();

    let persisted = sqlx::query_as::<_, (String, i64, String, i64, i16, String, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, deployment.phase, \
          deployment.policy_revision, deployment.desired_target_digest_version, \
          deployment.desired_target_digest, \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $2 AND endpoint_domain = 'product_apply_v1'), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $2 AND action = 'promotion.apply'), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
           WHERE receipt_id = $2 AND endpoint_domain = 'product_apply_v1') \
         FROM public.activation_requests AS activation \
         INNER JOIN public.runtime_deployments AS deployment \
          ON deployment.activation_request_id = activation.id \
         WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (
            "applied".to_string(),
            4,
            "requested".to_string(),
            1,
            1,
            prepared.desired_target_digest().to_string(),
            1,
            1,
            1
        )
    );

    let scope = deployment_scope(&prepared);
    let status = PostgresRuntimeConvergence::new(pool.clone())
        .status(&scope)
        .await
        .unwrap();
    assert_eq!(&status.snapshot, prepared.snapshot());
    let error = sqlx::query(
        "UPDATE public.activation_requests SET state = 'approved' WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .execute(&pool)
    .await
    .expect_err("Applied product activation record must be immutable");
    assert!(matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("23514")
                && database.constraint()
                    == Some("product_activation_applied_record_immutable")
    ));
    let error = sqlx::query(
        "UPDATE public.activation_requests SET applied_by = $2 WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .bind("forged-product-actor")
    .execute(&pool)
    .await
    .expect_err("Applied product activation evidence must be immutable");
    assert!(matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("23514")
                && database.constraint()
                    == Some("product_activation_applied_record_immutable")
    ));
    let exact_lineage = sqlx::query_scalar::<_, bool>(
        "SELECT public.starring_product_ruleset_slot_exact_v1($1, $2, $3, $4, 1)",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(exact_lineage);

    let mut replay_transaction = begin_serializable(&pool).await;
    let replay_call = Call::valid(&fixture);
    let replay = lock_apply(&mut replay_transaction, &fixture, &operation, &replay_call)
        .await
        .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(replay.resulting_revision, Some(4));
    assert_eq!(replay.resulting_state.as_deref(), Some("applied"));
    assert_eq!(
        replay.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    assert!(replay.locked_projection.is_none());
    replay_transaction.commit().await.unwrap();

    let mut conflict_operation = operation.clone();
    conflict_operation.semantic_digest = digest("different-apply-semantics");
    let mut conflict_transaction = begin_serializable(&pool).await;
    let conflict = lock_apply(
        &mut conflict_transaction,
        &fixture,
        &conflict_operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(conflict.outcome, "idempotency_conflict");
    conflict_transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn successful_finalize_clears_runtime_mutation_clock() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("runtime-clock");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    let finalized = finalize_apply(
        &mut transaction,
        &fixture,
        &operation,
        &call,
        &lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(finalized.outcome, "ok");
    assert_runtime_mutation_clock_cleared(&mut transaction).await;
    transaction.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn exact_replay_survives_rejected_direct_pointer_change() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("replay-after-pointer-change");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    let finalized = finalize_apply(
        &mut transaction,
        &fixture,
        &operation,
        &call,
        &lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(finalized.outcome, "ok");
    transaction.commit().await.unwrap();

    let next_content_hash =
        "91d936ba08910497f8f31e16e7f2b1ffce5ee9447a4636d47ddddc5c79fb0103".to_string();
    let mut pointer_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.set_config(\
          'starring.product_approval_context_digest', approval_context_digest, TRUE) \
         FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 2, 1, \
          pg_catalog.jsonb_build_object('version', 2, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $3, $4)",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&next_content_hash)
    .bind(&fixture.actor.user_id)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_ruleset_heads SET next_version = 3 \
         WHERE guild_id = $1 AND ruleset_key = $2 AND next_version = 2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    let changed = sqlx::query(
        "UPDATE public.automation_ruleset_activations SET active_version = 2 \
         WHERE guild_id = $1 AND ruleset_key = $2 AND active_version = 1",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    let error = pointer_transaction
        .commit()
        .await
        .expect_err("direct product pointer change must roll back");
    assert!(matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("23514")
                && database.constraint() == Some("product_ruleset_slot_pointer_exact")
    ));
    let active_version = sqlx::query_scalar::<_, i64>(
        "SELECT active_version FROM public.automation_ruleset_activations \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_version, 1);

    let mut delete_transaction = pool.begin().await.unwrap();
    let deleted = sqlx::query(
        "DELETE FROM public.automation_ruleset_activations \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *delete_transaction)
    .await
    .unwrap();
    assert_eq!(deleted.rows_affected(), 1);
    let error = delete_transaction
        .commit()
        .await
        .expect_err("product pointer deletion must roll back");
    assert!(matches!(
        error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("23514")
                && database.constraint()
                    == Some("product_ruleset_slot_pointer_delete_forbidden")
    ));

    let mut replay_transaction = begin_serializable(&pool).await;
    let replay = lock_apply(
        &mut replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(replay.resulting_revision, Some(4));
    assert_eq!(replay.resulting_state.as_deref(), Some("applied"));
    assert_eq!(
        replay.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    assert!(replay.locked_projection.is_none());
    replay_transaction.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn deferred_pointer_invariant_checks_the_final_transaction_pointer() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    complete_apply(&pool, &fixture, &Operation::new("final-pointer")).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 2, 1, \
          pg_catalog.jsonb_build_object('version', 2, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $3, $4)",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind("91d936ba08910497f8f31e16e7f2b1ffce5ee9447a4636d47ddddc5c79fb0103")
    .bind(&fixture.actor.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_ruleset_heads SET next_version = 3 \
         WHERE guild_id = $1 AND ruleset_key = $2 AND next_version = 2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    for version in [2_i64, 1] {
        let changed = sqlx::query(
            "UPDATE public.automation_ruleset_activations SET active_version = $3 \
             WHERE guild_id = $1 AND ruleset_key = $2",
        )
        .bind(&fixture.guild_id)
        .bind(&fixture.ruleset_key)
        .bind(version)
        .execute(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(changed.rows_affected(), 1);
    }
    transaction.commit().await.unwrap();
    let active_version = sqlx::query_scalar::<_, i64>(
        "SELECT active_version FROM public.automation_ruleset_activations \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_version, 1);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn one_bit_wrong_desired_digest_is_rejected_without_mutation() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("wrong-runtime-digest");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    let wrong_digest = one_bit_wrong_digest(prepared.desired_target_digest());
    assert_ne!(wrong_digest, prepared.desired_target_digest());
    let finalized = finalize_apply(
        &mut transaction,
        &fixture,
        &operation,
        &call,
        &lock,
        finalize_projection(
            &wrong_digest,
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(finalized.outcome, "invalid_runtime_projection");
    assert_eq!(finalized.resulting_revision, None);
    assert_eq!(finalized.resulting_state, None);
    assert!(!finalized.exact_replay);
    assert_eq!(finalized.guild_id, None);
    assert_eq!(finalized.deployment_id, None);
    assert_eq!(finalized.desired_target_digest, None);
    assert_apply_unmutated(&mut transaction, &fixture, &operation).await;
    transaction.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn malformed_runtime_projection_shapes_are_stable_and_atomic() {
    let pool = pool().await;
    for case in [
        "top-scalar",
        "top-array",
        "identity-array",
        "missing-nullable",
        "revision-string",
        "version-string",
        "generation-string",
        "notices-object",
    ] {
        let fixture = seed_fixture(&pool).await;
        let operation = Operation::new(case);
        let call = Call::valid(&fixture);
        let mut transaction = begin_serializable(&pool).await;
        let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
            .await
            .unwrap();
        assert_eq!(lock.outcome, "ready");
        let prepared = prepare_requested_deployment(&lock);
        let mut snapshot = prepared.snapshot_json().clone();
        let mut notices = json!([]);
        match case {
            "top-scalar" => snapshot = json!(1),
            "top-array" => snapshot = json!([]),
            "identity-array" => snapshot["identity"] = json!([]),
            "missing-nullable" => {
                let object = snapshot.as_object_mut().unwrap();
                assert_eq!(object.remove("controller_lease"), Some(Value::Null));
                assert_eq!(object.insert("unknown".to_string(), Value::Null), None);
            }
            "revision-string" => snapshot["revision"] = json!("1"),
            "version-string" => snapshot["target"]["version"] = json!("1"),
            "generation-string" => snapshot["runtime_generation"] = json!("1"),
            "notices-object" => notices = json!({}),
            _ => unreachable!(),
        }
        let finalized = finalize_apply(
            &mut transaction,
            &fixture,
            &operation,
            &call,
            &lock,
            FinalizeProjection {
                desired_target_digest: prepared.desired_target_digest(),
                previous_runtime: prepared.previous_runtime_json(),
                snapshot: &snapshot,
                notices: &notices,
            },
        )
        .await
        .unwrap();
        assert_eq!(finalized.outcome, "invalid_runtime_projection", "{case}");
        assert_eq!(finalized.resulting_revision, None, "{case}");
        assert_eq!(finalized.resulting_state, None, "{case}");
        assert!(!finalized.exact_replay, "{case}");
        assert_eq!(finalized.guild_id, None, "{case}");
        assert_eq!(finalized.deployment_id, None, "{case}");
        assert_eq!(finalized.desired_target_digest, None, "{case}");
        assert_apply_unmutated(&mut transaction, &fixture, &operation).await;
        transaction.commit().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn exact_replay_with_wrong_payload_fails_closed() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("replay-payload");
    complete_apply(&pool, &fixture, &operation).await;
    let mut context = ApplyLockContext::single(&fixture, &operation);
    context.expected_payload_digest = digest(&format!("wrong:payload:{}", fixture.activation_id));
    let mut transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "payload_mismatch");
    assert!(!replay.exact_replay);
    assert!(!replay.requires_commit);
    assert_eq!(replay.resulting_revision, None);
    assert_eq!(replay.resulting_state, None);
    assert_eq!(replay.deployment_id, None);
    assert_eq!(replay.desired_target_digest, None);
    transaction.rollback().await.unwrap();
    let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $2), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
           WHERE receipt_id = $2), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $2)",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 1, 1));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn applied_replay_rejects_tampered_revision_and_disposition_evidence() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("applied-replay-forensic");
    complete_apply(&pool, &fixture, &operation).await;

    let mut corruption = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *corruption)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET product_revision = 5 \
         WHERE id = $1 AND state = 'applied' AND product_revision = 4",
    )
    .bind(&fixture.activation_id)
    .execute(&mut *corruption)
    .await
    .unwrap();
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut *corruption)
        .await
        .unwrap();
    corruption.commit().await.unwrap();
    let mut revision_transaction = begin_serializable(&pool).await;
    let revision = lock_apply(
        &mut revision_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(revision.outcome, "indeterminate");
    assert!(!revision.exact_replay);
    revision_transaction.rollback().await.unwrap();

    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("applied-replay-disposition");
    complete_apply(&pool, &fixture, &operation).await;

    let mut corruption = pool.begin().await.unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.product_action_receipts \
         DISABLE TRIGGER product_action_receipts_reject_mutation; \
         ALTER TABLE public.product_action_receipt_audit_evidence \
         DISABLE TRIGGER product_action_receipt_audit_evidence_reject_mutation",
    )
    .execute(&mut *corruption)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.product_action_receipts SET http_disposition_class = 4 \
         WHERE receipt_id = $1",
    )
    .bind(&operation.receipt_id)
    .execute(&mut *corruption)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.product_action_receipt_audit_evidence \
         SET http_disposition_class = 4 WHERE receipt_id = $1",
    )
    .bind(&operation.receipt_id)
    .execute(&mut *corruption)
    .await
    .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.product_action_receipts \
         ENABLE TRIGGER product_action_receipts_reject_mutation; \
         ALTER TABLE public.product_action_receipt_audit_evidence \
         ENABLE TRIGGER product_action_receipt_audit_evidence_reject_mutation",
    )
    .execute(&mut *corruption)
    .await
    .unwrap();
    corruption.commit().await.unwrap();

    let mut disposition_transaction = begin_serializable(&pool).await;
    let disposition = lock_apply(
        &mut disposition_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(disposition.outcome, "indeterminate");
    assert!(!disposition.exact_replay);
    disposition_transaction.rollback().await.unwrap();
}
