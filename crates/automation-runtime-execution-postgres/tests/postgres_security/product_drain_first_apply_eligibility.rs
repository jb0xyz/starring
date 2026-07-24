const PRODUCT_DRAIN_FIRST_APPLY_ELIGIBILITY_MIGRATION: &str = include_str!(
    "../../../../migrations/202607240011_harden_product_drain_first_apply_eligibility.sql"
);

async fn advance_product_drain_deployment_to_live_with_lease(
    database: &IsolatedDatabase,
    session: &RuntimeConvergenceSessionV1,
    lease_milliseconds: i64,
) -> RuntimeDeploymentSnapshotV1 {
    let gateway_ready = gateway_ready_attestation(database, session).await;
    let guard = session.execution_guard().unwrap();
    let mut transaction = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut transaction,
        &guard,
        serde_json::to_value(&gateway_ready).unwrap(),
        lease_milliseconds,
    )
    .await
    .unwrap();
    let input = certification_input(&guard, gateway_ready, &prepared);
    let outcome = raw_certify_commit(&mut transaction, &input, lease_milliseconds)
        .await
        .unwrap();
    assert_eq!(outcome, "applied");
    transaction.commit().await.unwrap();
    product_drain_snapshot(&database.owner_pool).await
}

async fn restore_awaiting_gateway_ready_projection(
    pool: &PgPool,
    snapshot: &RuntimeDeploymentSnapshotV1,
) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET snapshot = $1, revision = $2, phase = 'awaiting_gateway_ready', \
             live_attestation_id = NULL, live_at = NULL, \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE tenant_id = $3 AND installation_id = $4 AND deployment_id = $5",
    )
    .bind(Json(serde_json::to_value(snapshot).unwrap()))
    .bind(i64::try_from(snapshot.revision.get()).unwrap())
    .bind(snapshot.identity.tenant_id.as_str())
    .bind(snapshot.identity.installation_id.as_str())
    .bind(snapshot.identity.deployment_id.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn serving_lease_image(pool: &PgPool) -> Option<Json<Value>> {
    sqlx::query_scalar(
        "SELECT pg_catalog.to_jsonb(lease) \
         FROM public.runtime_serving_leases AS lease \
         WHERE lease.guild_id = $1 AND lease.ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_optional(pool)
    .await
    .unwrap()
}

async fn disconnect_product_drain_serving_lease(database: &IsolatedDatabase) {
    let lease = sqlx::query_as::<_, (String, String, String, String, String, i64, i64, i64)>(
        "SELECT tenant_id, installation_id, deployment_id, attestation_id, \
                process_instance_id, runtime_generation, lease_epoch, revision \
         FROM public.runtime_serving_leases \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let connected = sqlx::query_scalar::<_, bool>(
        "SELECT connected \
         FROM public.starring_runtime_serving_disconnect_v1(\
            $1,$2,$3,$4,$5,$6,$7,$8\
         )",
    )
    .bind(lease.0)
    .bind(lease.1)
    .bind(lease.2)
    .bind(lease.3)
    .bind(lease.4)
    .bind(lease.5)
    .bind(lease.6)
    .bind(lease.7)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert!(!connected);
    transaction.commit().await.unwrap();
}

async fn delete_product_drain_serving_lease(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_serving_leases \
         DISABLE TRIGGER runtime_serving_leases_reject_delete",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM public.runtime_serving_leases \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_serving_leases \
         ENABLE TRIGGER runtime_serving_leases_reject_delete",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn corrupt_product_drain_serving_binding(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_serving_leases \
         DISABLE TRIGGER runtime_serving_leases_validate_transition",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_serving_leases \
         SET binding_fingerprint = $3 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind("f".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_serving_leases \
         ENABLE TRIGGER runtime_serving_leases_validate_transition",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn insert_newer_awaiting_product_drain_head(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) {
    for relation in [
        "public.authoring_promotions",
        "public.activation_requests",
        "public.runtime_deployments",
    ] {
        sqlx::query(&format!("ALTER TABLE {relation} DISABLE TRIGGER USER"))
            .execute(&mut **transaction)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO public.authoring_promotions (\
            id, record_format_version, revision, stage, request_digest, tenant_id, \
            principal_id, record, installation_id, product_admission_format_version, \
            product_admission_digest, product_admission\
         ) \
         SELECT $1, promotion.record_format_version, promotion.revision, promotion.stage, $2, \
                promotion.tenant_id, promotion.principal_id, \
                pg_catalog.jsonb_set(\
                    pg_catalog.jsonb_set(\
                        promotion.record, '{id}', pg_catalog.to_jsonb($1::TEXT)\
                    ), \
                    '{request_digest}', pg_catalog.to_jsonb($2::TEXT)\
                ), \
                promotion.installation_id, promotion.product_admission_format_version, \
                promotion.product_admission_digest, promotion.product_admission \
         FROM public.authoring_promotions AS promotion \
         WHERE promotion.id = $3",
    )
    .bind("b".repeat(64))
    .bind("c".repeat(64))
    .bind(PROMOTION)
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.activation_requests (\
            id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
            required_approvals, state, created_at, expires_at, apply_attempt_id, \
            apply_attempt_no, apply_lease_until, last_apply_error, observed_active_version, \
            observed_active_hash, applied_at, applied_by, completion_kind, \
            activation_notices, rejected_at, rejected_by, rejection_reason, authority_kind, \
            link_state_name, approval_context, link_state, promotion_id, \
            promotion_request_digest, approval_payload_digest, approval_context_digest, \
            linked_at, termination, tenant_id, installation_id, product_revision\
         ) \
         SELECT $1, request.guild_id, request.ruleset_key, request.target_version, \
                request.target_content_hash, request.requester_id, \
                request.required_approvals, request.state, request.created_at, \
                request.expires_at, request.apply_attempt_id, request.apply_attempt_no, \
                request.apply_lease_until, request.last_apply_error, \
                request.observed_active_version, request.observed_active_hash, \
                request.applied_at, request.applied_by, request.completion_kind, \
                request.activation_notices, request.rejected_at, request.rejected_by, \
                request.rejection_reason, request.authority_kind, request.link_state_name, \
                pg_catalog.jsonb_set(\
                    pg_catalog.jsonb_set(\
                        pg_catalog.jsonb_set(\
                            request.approval_context, \
                            '{context,promotion_id}', \
                            pg_catalog.to_jsonb($2::TEXT)\
                        ), \
                        '{context,promotion_request_digest}', \
                        pg_catalog.to_jsonb($3::TEXT)\
                    ), \
                    '{context,approval_context_digest}', \
                    pg_catalog.to_jsonb($4::TEXT)\
                ), \
                request.link_state, $2, $3, request.approval_payload_digest, $4, \
                request.linked_at, request.termination, request.tenant_id, \
                request.installation_id, request.product_revision + 1 \
         FROM public.activation_requests AS request \
         WHERE request.id = $5",
    )
    .bind("runtime-eligibility-new-head")
    .bind("b".repeat(64))
    .bind("c".repeat(64))
    .bind("d".repeat(64))
    .bind(ACTIVATION)
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "WITH mutation_clock AS (\
            SELECT pg_catalog.clock_timestamp() AS value\
         ) \
         INSERT INTO public.runtime_deployments (\
            deployment_id, tenant_id, installation_id, promotion_id, \
            activation_request_id, installation_authority_revision, guild_id, ruleset_key, \
            target_version, target_content_hash, binding_revision, binding_fingerprint, \
            desired_target_digest, runtime_generation, previous_runtime, requested_at, \
            snapshot_format_version, snapshot, revision, phase, controller_id, \
            controller_fencing_token, controller_acquired_at, controller_lease_expires_at, \
            last_fencing_token, next_retry_at, last_stable_error_code, live_attestation_id, \
            live_at, blocked_at, superseded_at, cancelled_at, created_at, updated_at, \
            policy_revision, desired_target_digest_version, convergence_attempt_no, \
            last_failure_attempt_no, last_controller_id\
         ) \
         SELECT $1, deployment.tenant_id, deployment.installation_id, $2, $3, \
                deployment.installation_authority_revision, deployment.guild_id, \
                deployment.ruleset_key, deployment.target_version, \
                deployment.target_content_hash, deployment.binding_revision, \
                deployment.binding_fingerprint, $4, deployment.runtime_generation + 1, \
                NULL, mutation_clock.value, 1, deployment.snapshot, 1, \
                'awaiting_gateway_ready', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, \
                NULL, NULL, NULL, NULL, mutation_clock.value, mutation_clock.value, \
                deployment.policy_revision, 1, 0, NULL, NULL \
         FROM public.runtime_deployments AS deployment \
         CROSS JOIN mutation_clock \
         WHERE deployment.deployment_id = $5",
    )
    .bind("runtime-eligibility-new-head")
    .bind("b".repeat(64))
    .bind("runtime-eligibility-new-head")
    .bind("e".repeat(64))
    .bind(DEPLOYMENT)
    .execute(&mut **transaction)
    .await
    .unwrap();
}

async fn assert_product_drain_first_apply_eligible_without_commit(
    database: &IsolatedDatabase,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) {
    let guild_id = canonical.product_preimage().slot.guild_id.to_string();
    let ruleset_key = canonical.product_preimage().slot.ruleset_key.as_str();
    let fence_before = slot_writer_fence_row(&database.owner_pool, &guild_id, ruleset_key).await;
    let lease_before = serving_lease_image(&database.owner_pool).await;
    let mut transaction = begin_product_drain_first_apply(&database.owner_pool).await;
    let inserted = call_product_drain_first_apply(&mut *transaction, canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    assert_complete_product_drain_row(&inserted, canonical);
    transaction.rollback().await.unwrap();
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));
    assert_eq!(
        slot_writer_fence_row(&database.owner_pool, &guild_id, ruleset_key).await,
        fence_before
    );
    assert_eq!(serving_lease_image(&database.owner_pool).await, lease_before);
}

async fn assert_product_drain_first_apply_ineligible_without_mutation(
    database: &IsolatedDatabase,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) {
    let guild_id = canonical.product_preimage().slot.guild_id.to_string();
    let ruleset_key = canonical.product_preimage().slot.ruleset_key.as_str();
    let fence_before = slot_writer_fence_row(&database.owner_pool, &guild_id, ruleset_key).await;
    let lease_before = serving_lease_image(&database.owner_pool).await;
    let error = committed_product_drain_first_apply(&database.owner_pool, canonical)
        .await
        .unwrap_err();
    assert_database_error(
        &error,
        "RX001",
        "runtime_product_drain_first_apply_deployment_mismatch",
    );
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));
    assert_eq!(
        slot_writer_fence_row(&database.owner_pool, &guild_id, ruleset_key).await,
        fence_before
    );
    assert_eq!(serving_lease_image(&database.owner_pool).await, lease_before);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_first_apply_fresh_eligibility_is_phase_and_lease_exact() {
    let server = PostgresTestServer::start();

    let awaiting_database = isolated_database(server.connect_options()).await;
    let awaiting_session =
        gateway_ready_session(&awaiting_database, "runtime-eligibility-awaiting").await;
    let awaiting_snapshot = awaiting_session.snapshot().clone();
    advance_product_drain_deployment_to_live_with_lease(
        &awaiting_database,
        &awaiting_session,
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await;
    restore_awaiting_gateway_ready_projection(
        &awaiting_database.owner_pool,
        &awaiting_snapshot,
    )
    .await;
    let awaiting_canonical = canonical_product_drain(&awaiting_snapshot);
    assert_product_drain_first_apply_ineligible_without_mutation(
        &awaiting_database,
        &awaiting_canonical,
    )
    .await;
    disconnect_product_drain_serving_lease(&awaiting_database).await;
    assert_product_drain_first_apply_eligible_without_commit(
        &awaiting_database,
        &awaiting_canonical,
    )
    .await;
    cleanup(awaiting_database).await;

    let expired_database = isolated_database(server.connect_options()).await;
    let expired_session =
        gateway_ready_session(&expired_database, "runtime-eligibility-expired").await;
    let expired_awaiting_snapshot = expired_session.snapshot().clone();
    let expired_live_snapshot = advance_product_drain_deployment_to_live_with_lease(
        &expired_database,
        &expired_session,
        1_000,
    )
    .await;
    let expires_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT expires_at FROM public.runtime_serving_leases \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .fetch_one(&expired_database.owner_pool)
    .await
    .unwrap();
    wait_for_database_time(
        &expired_database.owner_pool,
        expires_at + TimeDelta::milliseconds(1),
    )
    .await;
    let expired_live_canonical = canonical_product_drain(&expired_live_snapshot);
    assert_product_drain_first_apply_eligible_without_commit(
        &expired_database,
        &expired_live_canonical,
    )
    .await;
    restore_awaiting_gateway_ready_projection(
        &expired_database.owner_pool,
        &expired_awaiting_snapshot,
    )
    .await;
    let expired_awaiting_canonical = canonical_product_drain(&expired_awaiting_snapshot);
    assert_product_drain_first_apply_eligible_without_commit(
        &expired_database,
        &expired_awaiting_canonical,
    )
    .await;
    cleanup(expired_database).await;

    let live_database = isolated_database(server.connect_options()).await;
    let live_session = gateway_ready_session(&live_database, "runtime-eligibility-live").await;
    let live_snapshot = advance_product_drain_deployment_to_live_with_lease(
        &live_database,
        &live_session,
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await;
    let live_canonical = canonical_product_drain(&live_snapshot);
    assert_product_drain_first_apply_eligible_without_commit(&live_database, &live_canonical).await;
    let live_fence_before = slot_writer_fence_row(
        &live_database.owner_pool,
        &GUILD.to_string(),
        RULESET,
    )
    .await;
    let live_lease_before = serving_lease_image(&live_database.owner_pool).await;
    let mut newer_head = begin_product_drain_first_apply(&live_database.owner_pool).await;
    insert_newer_awaiting_product_drain_head(&mut newer_head).await;
    let newer_head_error = call_product_drain_first_apply(&mut *newer_head, &live_canonical)
        .await
        .unwrap_err();
    assert_database_error(
        &newer_head_error,
        "RX001",
        "runtime_product_drain_first_apply_deployment_mismatch",
    );
    newer_head.rollback().await.unwrap();
    assert_eq!(product_drain_row_counts(&live_database.owner_pool).await, (0, 0));
    assert_eq!(
        slot_writer_fence_row(
            &live_database.owner_pool,
            &GUILD.to_string(),
            RULESET,
        )
        .await,
        live_fence_before
    );
    assert_eq!(
        serving_lease_image(&live_database.owner_pool).await,
        live_lease_before
    );
    disconnect_product_drain_serving_lease(&live_database).await;
    assert_product_drain_first_apply_eligible_without_commit(&live_database, &live_canonical).await;
    corrupt_product_drain_serving_binding(&live_database.owner_pool).await;
    assert_product_drain_first_apply_ineligible_without_mutation(&live_database, &live_canonical)
        .await;
    delete_product_drain_serving_lease(&live_database.owner_pool).await;
    assert_product_drain_first_apply_ineligible_without_mutation(&live_database, &live_canonical)
        .await;
    cleanup(live_database).await;

    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn product_drain_first_apply_eligibility_upgrade_preserves_pending_roots_and_replay() {
    let server = PostgresTestServer::start();
    let (database_name, administrator, pool) =
        pre_slot_writer_fence_database(&server, "st_re_fa_eligibility").await;
    let mut slot_fence_upgrade = pool.begin().await.unwrap();
    sqlx::raw_sql(SLOT_WRITER_FENCE_MIGRATION)
        .execute(&mut *slot_fence_upgrade)
        .await
        .unwrap();
    slot_fence_upgrade.commit().await.unwrap();
    seed_claimable_deployment(&pool).await;
    let mut awaiting_projection = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER")
        .execute(&mut *awaiting_projection)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET phase = 'awaiting_gateway_ready', \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .execute(&mut *awaiting_projection)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER")
        .execute(&mut *awaiting_projection)
        .await
        .unwrap();
    awaiting_projection.commit().await.unwrap();
    let snapshot = product_drain_snapshot(&pool).await;
    let canonical = canonical_product_drain(&snapshot);
    let inserted = committed_product_drain_first_apply(&pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    assert_eq!(product_drain_row_counts(&pool).await, (1, 1));
    let guild_id = GUILD.to_string();
    let pending_fence = slot_writer_fence_row(&pool, &guild_id, RULESET).await;
    assert_eq!(pending_fence.2, 2);

    let mut eligibility_upgrade = pool.begin().await.unwrap();
    sqlx::raw_sql(PRODUCT_DRAIN_FIRST_APPLY_ELIGIBILITY_MIGRATION)
        .execute(&mut *eligibility_upgrade)
        .await
        .unwrap();
    eligibility_upgrade.commit().await.unwrap();
    assert_eq!(slot_writer_fence_manifests(&pool).await, (true, true, true, true));
    assert_eq!(product_drain_row_counts(&pool).await, (1, 1));
    assert_eq!(
        slot_writer_fence_row(&pool, &guild_id, RULESET).await,
        pending_fence
    );
    let replayed = committed_product_drain_first_apply(&pool, &canonical)
        .await
        .unwrap();
    assert_eq!(replayed.outcome_name, "replayed");
    assert_complete_product_drain_row(&replayed, &canonical);
    let mut replay_with_newer_head = begin_product_drain_first_apply(&pool).await;
    sqlx::query("ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER")
        .execute(&mut *replay_with_newer_head)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments \
         SET phase = 'cancelled', cancelled_at = pg_catalog.clock_timestamp(), \
             updated_at = pg_catalog.clock_timestamp() \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .execute(&mut *replay_with_newer_head)
    .await
    .unwrap();
    insert_newer_awaiting_product_drain_head(&mut replay_with_newer_head).await;
    let replayed_with_newer_head =
        call_product_drain_first_apply(&mut *replay_with_newer_head, &canonical)
            .await
            .unwrap();
    assert_eq!(replayed_with_newer_head.outcome_name, "replayed");
    assert_complete_product_drain_row(&replayed_with_newer_head, &canonical);
    replay_with_newer_head.rollback().await.unwrap();

    let mut rerun = pool.begin().await.unwrap();
    let rerun_error = sqlx::raw_sql(PRODUCT_DRAIN_FIRST_APPLY_ELIGIBILITY_MIGRATION)
        .execute(&mut *rerun)
        .await
        .unwrap_err();
    rerun.rollback().await.unwrap();
    assert_database_error(
        &rerun_error,
        "RE001",
        "runtime_product_drain_first_apply_eligibility_preflight_drift",
    );
    assert_eq!(slot_writer_fence_manifests(&pool).await, (true, true, true, true));
    assert_eq!(product_drain_row_counts(&pool).await, (1, 1));
    assert_eq!(
        slot_writer_fence_row(&pool, &guild_id, RULESET).await,
        pending_fence
    );

    drop_slot_writer_fence_test_database(&database_name, administrator, pool).await;
    drop(server);
}
