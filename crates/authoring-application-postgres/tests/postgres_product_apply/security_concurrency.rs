#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn apply_key_rotation_requires_coverage_and_promotes_new_alias() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("key-rotation");
    let prepared = complete_apply(&pool, &fixture, &operation).await;
    let new_digest = digest(&format!("idempotency:v2:{}", operation.request_id));
    let new_key_id = TEST_DECISION_KEY_V2_ID.to_string();
    let new_key_fingerprint = test_decision_key_fingerprint(97);
    let mut new_only = ApplyLockContext::single(&fixture, &operation);
    new_only.active_idempotency_digest = new_digest.clone();
    new_only.idempotency_candidates = vec![new_digest.clone()];
    new_only.candidate_key_ids = vec![new_key_id.clone()];
    new_only.candidate_key_fingerprints = vec![new_key_fingerprint.clone()];
    new_only.active_key_id = new_key_id.clone();

    let mut incomplete_transaction = begin_serializable(&pool).await;
    let incomplete = lock_apply_with_context(
        &mut incomplete_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &new_only,
    )
    .await
    .unwrap();
    assert_eq!(incomplete.outcome, "idempotency_keyring_incomplete");
    assert!(!incomplete.exact_replay);
    assert!(!incomplete.requires_commit);
    incomplete_transaction.rollback().await.unwrap();

    let mut rotating = new_only.clone();
    rotating
        .idempotency_candidates
        .push(operation.idempotency_digest.clone());
    rotating.candidate_key_ids.push(operation.key_id.clone());
    rotating
        .candidate_key_fingerprints
        .push(operation.key_fingerprint.clone());
    let mut rotation_transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut rotation_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &rotating,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    rotation_transaction.commit().await.unwrap();

    let aliases = sqlx::query_as::<_, (i64, i64)>(
        "SELECT \
          pg_catalog.count(*), \
          pg_catalog.count(*) FILTER (WHERE idempotency_key_digest = $5 \
           AND idempotency_digest_key_id = $6 \
           AND idempotency_digest_key_fingerprint = $7) \
         FROM public.product_action_receipt_idempotency_aliases \
         WHERE tenant_id = $1 AND installation_id = $2 AND principal_id = $3 \
          AND endpoint_domain = 'product_apply_v1' AND receipt_id = $4",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.actor.principal_id)
    .bind(&operation.receipt_id)
    .bind(&new_digest)
    .bind(&new_key_id)
    .bind(&new_key_fingerprint)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(aliases, (2, 1));

    let mut new_only_transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut new_only_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &new_only,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    new_only_transaction.commit().await.unwrap();

    let retained = sqlx::query_as::<_, (i32, i32, bool)>(
        "SELECT deleted_receipts, deleted_aliases, backlog_remaining \
         FROM public.starring_purge_product_action_receipts_v1(1)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retained, (0, 0, false));
    let retained_counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
           WHERE receipt_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
           WHERE receipt_id = $1)",
    )
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retained_counts, (1, 2, 1, 1));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn finalize_failure_rolls_back_pointer_activation_and_runtime() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("rollback");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    sqlx::query(
        "INSERT INTO public.product_action_receipts \
         (receipt_id, tenant_id, installation_id, principal_id, endpoint_domain, \
          idempotency_key_digest, request_digest, target_resource_type, target_resource_id, \
          resulting_state, result_code, http_disposition_class) \
         VALUES ($1, $2, $3, $4, 'test_collision_v1', $5, $6, \
          'test_collision', $7, 'collision', 'collision', 4)",
    )
    .bind(&operation.receipt_id)
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.actor.principal_id)
    .bind(digest("rollback-collision-idempotency"))
    .bind(digest("rollback-collision-request"))
    .bind(&fixture.promotion_id)
    .execute(&pool)
    .await
    .unwrap();
    let error = finalize_apply(
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
    .expect_err("receipt collision must abort the atomic finalizer");
    assert!(
        is_serialization_failure(&error)
            || matches!(
                &error,
                sqlx::Error::Database(database)
                    if database.code().as_deref() == Some("23505")
            )
    );
    transaction.rollback().await.unwrap();
    let rolled_back = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
          (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations \
           WHERE guild_id = $2 AND ruleset_key = $3), \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = activation.id) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rolled_back, ("approved".to_string(), 2, 0, 0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn stale_revision_authority_session_and_capability_fail_closed() {
    let pool = pool().await;

    let revision_fixture = seed_fixture(&pool).await;
    let operation = Operation::new("stale-revision");
    let mut revision_call = Call::valid(&revision_fixture);
    revision_call.expected_revision = 1;
    let mut transaction = begin_serializable(&pool).await;
    let revision = lock_apply(
        &mut transaction,
        &revision_fixture,
        &operation,
        &revision_call,
    )
    .await
    .unwrap();
    assert_eq!(revision.outcome, "revision_conflict");
    transaction.rollback().await.unwrap();

    let capability_fixture = seed_fixture(&pool).await;
    let mut capability_call = Call::valid(&capability_fixture);
    capability_call.effective_permissions = "0".to_string();
    let mut transaction = begin_serializable(&pool).await;
    let capability = lock_apply(
        &mut transaction,
        &capability_fixture,
        &Operation::new("capability"),
        &capability_call,
    )
    .await
    .unwrap();
    assert_eq!(capability.outcome, "invalid_input");
    transaction.rollback().await.unwrap();

    let observation_fixture = seed_fixture(&pool).await;
    let mut observation_call = Call::valid(&observation_fixture);
    observation_call.observed_at = Utc::now() - TimeDelta::seconds(10);
    observation_call.expires_at = observation_call.observed_at + TimeDelta::seconds(5);
    let mut transaction = begin_serializable(&pool).await;
    let observation = lock_apply(
        &mut transaction,
        &observation_fixture,
        &Operation::new("observation"),
        &observation_call,
    )
    .await
    .unwrap();
    assert_eq!(observation.outcome, "authorization_stale");
    transaction.rollback().await.unwrap();

    let authority_fixture = seed_fixture(&pool).await;
    let mut wrong_authority = authority_fixture.clone();
    wrong_authority.authority_digest = digest("wrong-authority");
    let mut transaction = begin_serializable(&pool).await;
    let authority = lock_apply(
        &mut transaction,
        &wrong_authority,
        &Operation::new("authority"),
        &Call::valid(&wrong_authority),
    )
    .await
    .unwrap();
    assert_eq!(authority.outcome, "authority_mismatch");
    transaction.rollback().await.unwrap();

    let session_fixture = seed_fixture(&pool).await;
    sqlx::query(
        "UPDATE public.product_auth_sessions \
         SET revoked_at = pg_catalog.clock_timestamp(), revocation_reason = 'security_test' \
         WHERE session_digest = $1",
    )
    .bind(&session_fixture.actor.session_digest)
    .execute(&pool)
    .await
    .unwrap();
    let mut transaction = begin_serializable(&pool).await;
    let session = lock_apply(
        &mut transaction,
        &session_fixture,
        &Operation::new("session"),
        &Call::valid(&session_fixture),
    )
    .await
    .unwrap();
    assert_eq!(session.outcome, "authorization_stale");
    transaction.rollback().await.unwrap();
}

async fn apply_with_serializable_retry(
    pool: PgPool,
    fixture: Fixture,
    operation: Operation,
) -> Result<bool, sqlx::Error> {
    for _ in 0..4 {
        let mut transaction = begin_serializable(&pool).await;
        let call = Call::valid(&fixture);
        let lock = match lock_apply(&mut transaction, &fixture, &operation, &call).await {
            Ok(lock) => lock,
            Err(error) if is_serialization_failure(&error) => {
                transaction.rollback().await.ok();
                continue;
            }
            Err(error) => return Err(error),
        };
        if lock.exact_replay {
            match transaction.commit().await {
                Ok(()) => return Ok(false),
                Err(error) if is_serialization_failure(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        assert_eq!(lock.outcome, "ready");
        let prepared = prepare_requested_deployment(&lock);
        let finalized = match finalize_apply(
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
        {
            Ok(finalized) => finalized,
            Err(error) if is_serialization_failure(&error) => {
                transaction.rollback().await.ok();
                continue;
            }
            Err(error) => return Err(error),
        };
        assert_eq!(finalized.outcome, "ok");
        match transaction.commit().await {
            Ok(()) => return Ok(true),
            Err(error) if is_serialization_failure(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    panic!("serializable apply did not converge within its bounded retry budget")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn concurrent_same_apply_converges_to_one_deployment_and_one_receipt() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("concurrent");
    let first = tokio::spawn(apply_with_serializable_retry(
        pool.clone(),
        fixture.clone(),
        operation.clone(),
    ));
    let second = tokio::spawn(apply_with_serializable_retry(
        pool.clone(),
        fixture.clone(),
        operation.clone(),
    ));
    let (first, second) = tokio::join!(first, second);
    let first = first.unwrap().unwrap();
    let second = second.unwrap().unwrap();
    assert_ne!(first, second);
    let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $2 AND endpoint_domain = 'product_apply_v1'), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $2 AND action = 'promotion.apply'), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
           WHERE receipt_id = $2 AND action = 'promotion.apply')",
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
async fn deferred_invariant_and_security_contract_reject_bypass() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let mut bypass = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *bypass)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'applied', applied_at = pg_catalog.clock_timestamp(), applied_by = $2, \
          completion_kind = 'already_active', activation_notices = '[]'::JSONB, \
          product_revision = product_revision + 1 \
         WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.actor.user_id)
    .execute(&mut *bypass)
    .await
    .unwrap();
    let bypass_error = bypass
        .commit()
        .await
        .expect_err("Applied without an exact runtime deployment must fail at commit");
    let sqlx::Error::Database(database) = bypass_error else {
        panic!("expected deferred invariant database error");
    };
    assert_eq!(
        database.constraint(),
        Some("atomic_product_apply_runtime_request_exact")
    );
    let state = sqlx::query_as::<_, (String, i64)>(
        "SELECT state, product_revision FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, ("approved".to_string(), 2));

    let function_security = sqlx::query_as::<_, (bool, bool, bool, bool)>(
        "SELECT \
          lock_function.prosecdef, \
          lock_function.proconfig @> ARRAY['search_path=pg_catalog']::TEXT[], \
          finalize_function.prosecdef, \
          finalize_function.proconfig @> ARRAY['search_path=pg_catalog']::TEXT[] \
         FROM pg_catalog.pg_proc AS lock_function \
         CROSS JOIN pg_catalog.pg_proc AS finalize_function \
         WHERE lock_function.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)') \
          AND finalize_function.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(function_security, (true, true, true, true));
    let public_execute = sqlx::query_as::<_, (bool, bool)>(
        "SELECT \
          EXISTS (SELECT 1 FROM pg_catalog.aclexplode(lock_function.proacl) AS privilege \
           WHERE privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE'), \
          EXISTS (SELECT 1 FROM pg_catalog.aclexplode(finalize_function.proacl) AS privilege \
           WHERE privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE') \
         FROM pg_catalog.pg_proc AS lock_function \
         CROSS JOIN pg_catalog.pg_proc AS finalize_function \
         WHERE lock_function.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)') \
          AND finalize_function.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(public_execute, (false, false));

    let operation = Operation::new("isolation");
    let call = Call::valid(&fixture);
    let mut read_committed = pool.begin().await.unwrap();
    let isolation = lock_apply(&mut read_committed, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(isolation.outcome, "invalid_input");
    read_committed.rollback().await.unwrap();

    let oversized_operation = Operation::new("oversized");
    let oversized_call = Call::valid(&fixture);
    let mut oversized_transaction = begin_serializable(&pool).await;
    let oversized_lock = lock_apply(
        &mut oversized_transaction,
        &fixture,
        &oversized_operation,
        &oversized_call,
    )
    .await
    .unwrap();
    assert_eq!(oversized_lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&oversized_lock);
    let mut oversized_snapshot = prepared.snapshot_json().clone();
    oversized_snapshot["oversized"] = Value::String("x".repeat(300_000));
    let oversized = finalize_apply(
        &mut oversized_transaction,
        &fixture,
        &oversized_operation,
        &oversized_call,
        &oversized_lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            &oversized_snapshot,
        ),
    )
    .await
    .unwrap();
    assert_eq!(oversized.outcome, "invalid_runtime_projection");
    oversized_transaction.commit().await.unwrap();
    let unchanged = sqlx::query_as::<_, (String, i64)>(
        "SELECT state, product_revision FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unchanged, ("approved".to_string(), 2));

    let null_rows = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.starring_product_apply_lock_v1(\
          NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::BIGINT, NULL::TEXT, NULL::TEXT, \
          NULL::BYTEA, NULL::BYTEA, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::TEXT, \
          NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TIMESTAMPTZ, NULL::TIMESTAMPTZ, \
          NULL::TEXT, NULL::BOOLEAN, NULL::TEXT, NULL::TEXT, NULL::TEXT[], NULL::TEXT[], \
          NULL::TEXT[], NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::TEXT)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(null_rows, 0);
}
