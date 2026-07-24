const PRODUCT_DRAIN_FIRST_APPLY_FOR_EPOCH_TEST: &str = "SELECT outcome_name FROM \
    starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20\
    )";

async fn product_slot_writer_epoch(pool: &PgPool, fixture: &Fixture) -> i64 {
    sqlx::query_scalar(
        "SELECT writer_epoch FROM public.runtime_slot_writer_fences_v2 \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn product_slot_writer_epoch_in(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
) -> i64 {
    sqlx::query_scalar(
        "SELECT writer_epoch FROM public.runtime_slot_writer_fences_v2 \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .fetch_one(&mut **transaction)
    .await
    .unwrap()
}

fn product_apply_drain_for_epoch_test(
    snapshot: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
) -> automation_runtime_controller::RuntimeCanonicalProductDrainV2 {
    let operation_id = digest(&format!(
        "product-apply-epoch-operation:{}",
        snapshot.identity.deployment_id.as_str()
    ));
    let intent_id = digest(&format!(
        "product-apply-epoch-intent:{}",
        snapshot.identity.deployment_id.as_str()
    ));
    let preimage = automation_runtime_controller::RuntimeProductMutationPreimageV2 {
        operation_id: automation_runtime_controller::RuntimeProductOperationIdV2::parse(
            &operation_id[..32],
        )
        .unwrap(),
        scope: automation_runtime_controller::RuntimeDeploymentScopeV1::from_identity(
            &snapshot.identity,
        ),
        expected_revision: snapshot.revision,
        slot: automation_runtime_controller::RuntimeServingSlotV2::from_target(&snapshot.target),
        expected_target: snapshot.target.clone(),
        mutation_kind: automation_runtime_controller::RuntimeProductMutationKindV2::Apply,
        product_semantic_request_digest:
            automation_runtime_controller::RuntimeProductSemanticRequestDigestV2::parse(digest(
                &format!(
                    "product-apply-epoch-semantic:{}",
                    snapshot.identity.deployment_id.as_str()
                ),
            ))
            .unwrap(),
    };
    automation_runtime_controller::RuntimeCanonicalProductDrainV2::new(
        preimage,
        automation_runtime_controller::RuntimeDrainIntentIdV2::parse(&intent_id[..32]).unwrap(),
    )
    .unwrap()
}

async fn install_product_apply_pending_drain(
    pool: &PgPool,
    snapshot: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
) {
    let canonical = product_apply_drain_for_epoch_test(snapshot);
    let product = canonical.product_preimage();
    let drain = canonical.drain_preimage();
    let mut transaction = begin_serializable(pool).await;
    let outcome = sqlx::query_scalar::<_, String>(PRODUCT_DRAIN_FIRST_APPLY_FOR_EPOCH_TEST)
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
        .bind("apply")
        .bind(product.product_semantic_request_digest.as_str())
        .bind(canonical.product_mutation_request_bytes())
        .bind(canonical.product_mutation_digest().as_str())
        .bind(canonical.drain_intent_request_bytes())
        .bind(canonical.drain_intent_digest().as_str())
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(outcome, "inserted");
    transaction.commit().await.unwrap();
}

async fn terminalize_product_apply_pending_deployment(
    pool: &PgPool,
    fixture: &Fixture,
    deployment_id: &str,
) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let changed = sqlx::query(
        "UPDATE public.runtime_deployments \
         SET phase = 'cancelled', revision = revision + 1, \
          controller_id = NULL, controller_fencing_token = NULL, \
          controller_acquired_at = NULL, controller_lease_expires_at = NULL, \
          live_attestation_id = NULL, live_at = NULL, superseded_at = NULL, \
          cancelled_at = GREATEST(pg_catalog.clock_timestamp(), requested_at), \
          snapshot = pg_catalog.jsonb_set(snapshot, '{phase}', \
           pg_catalog.jsonb_build_object(\
            'phase', 'cancelled', \
            'reason', 'product_apply_epoch_fixture', \
            'cancelled_at', GREATEST(pg_catalog.clock_timestamp(), requested_at))), \
          updated_at = GREATEST(pg_catalog.clock_timestamp(), requested_at) \
         WHERE tenant_id = $1 AND installation_id = $2 AND deployment_id = $3 \
          AND guild_id = $4 AND ruleset_key = $5 \
          AND phase = 'awaiting_gateway_ready'",
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

async fn product_pending_drain_state(pool: &PgPool, fixture: &Fixture) -> (i64, String, i64, i64) {
    sqlx::query_as(
        "SELECT fence.writer_epoch, fence.pending_drain_intent_id, \
          (SELECT pg_catalog.count(*) FROM public.runtime_product_operations_v2 \
           WHERE tenant_id = $3 AND installation_id = $4), \
          (SELECT pg_catalog.count(*) FROM public.runtime_drain_intents_v2 \
           WHERE tenant_id = $3 AND installation_id = $4) \
         FROM public.runtime_slot_writer_fences_v2 AS fence \
         WHERE fence.slot_guild_id = $1 AND fence.slot_ruleset_key = $2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn successful_product_apply_advances_epoch_once_and_replay_does_not_advance() {
    let database = isolated_database("apply_epoch_success").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await?;
        let fixture = seed_fixture(&database.pool).await;
        let operation = Operation::new("physical-epoch-success");
        assert_eq!(product_slot_writer_epoch(&database.pool, &fixture).await, 1);

        complete_apply(&database.pool, &fixture, &operation).await;
        assert_eq!(product_slot_writer_epoch(&database.pool, &fixture).await, 2);

        let mut replay_transaction = begin_serializable(&database.pool).await;
        let replay = lock_apply(
            &mut replay_transaction,
            &fixture,
            &operation,
            &Call::valid(&fixture),
        )
        .await?;
        assert_eq!(replay.outcome, "ok");
        assert!(replay.exact_replay);
        assert!(replay.requires_commit);
        assert_eq!(
            product_slot_writer_epoch_in(&mut replay_transaction, &fixture).await,
            2
        );
        replay_transaction.commit().await?;
        assert_eq!(product_slot_writer_epoch(&database.pool, &fixture).await, 2);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn rolled_back_product_apply_finalize_restores_writer_epoch() {
    let database = isolated_database("apply_epoch_rollback").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await?;
        let fixture = seed_fixture(&database.pool).await;
        let operation = Operation::new("physical-epoch-rollback");
        let call = Call::valid(&fixture);
        let durable_before = existing_runtime_product_state(&database.pool, &fixture).await;
        assert_eq!(product_slot_writer_epoch(&database.pool, &fixture).await, 1);

        let mut transaction = begin_serializable(&database.pool).await;
        let lock = lock_apply(&mut transaction, &fixture, &operation, &call).await?;
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
        .await?;
        assert_eq!(finalized.outcome, "ok");
        assert_eq!(
            product_slot_writer_epoch_in(&mut transaction, &fixture).await,
            2
        );
        transaction.rollback().await?;

        assert_eq!(product_slot_writer_epoch(&database.pool, &fixture).await, 1);
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
async fn pending_drain_preserves_replay_and_blocks_fresh_product_apply() {
    let database = isolated_database("apply_epoch_pending").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await?;
        let fixture = seed_fixture(&database.pool).await;
        let applied_operation = Operation::new("physical-epoch-pending-seed");
        let prepared = complete_apply(&database.pool, &fixture, &applied_operation).await;
        assert_eq!(product_slot_writer_epoch(&database.pool, &fixture).await, 2);

        let mut transition = database.pool.begin().await?;
        set_existing_runtime_phase(
            &mut transition,
            &fixture,
            &applied_operation.deployment_id,
            "awaiting_gateway_ready",
        )
        .await?;
        transition.commit().await?;

        let mut drain_snapshot = prepared.snapshot().clone();
        drain_snapshot.revision =
            automation_runtime_convergence::DeploymentRevision::new(2).unwrap();
        drain_snapshot.phase =
            automation_runtime_convergence::RuntimeDeploymentPhaseV1::AwaitingGatewayReady;
        install_product_apply_pending_drain(&database.pool, &drain_snapshot).await;
        let pending_state = product_pending_drain_state(&database.pool, &fixture).await;
        assert_eq!(pending_state.0, 3);
        assert_eq!(pending_state.2, 1);
        assert_eq!(pending_state.3, 1);

        terminalize_product_apply_pending_deployment(
            &database.pool,
            &fixture,
            &applied_operation.deployment_id,
        )
        .await;
        assert_eq!(
            product_pending_drain_state(&database.pool, &fixture).await,
            pending_state
        );

        let mut replay_transaction = begin_serializable(&database.pool).await;
        let replay = lock_apply(
            &mut replay_transaction,
            &fixture,
            &applied_operation,
            &Call::valid(&fixture),
        )
        .await?;
        assert_eq!(replay.outcome, "ok");
        assert!(replay.exact_replay);
        assert_eq!(
            product_slot_writer_epoch_in(&mut replay_transaction, &fixture).await,
            3
        );
        replay_transaction.commit().await?;

        let mut reopen = database.pool.begin().await?;
        reopen_applied_activation(&mut reopen, &fixture).await?;
        reopen.commit().await?;

        let fresh_operation = Operation::new("physical-epoch-pending-fresh");
        let durable_before = existing_runtime_product_state(&database.pool, &fixture).await;
        let mut fresh_transaction = begin_serializable(&database.pool).await;
        let mut fresh_call = Call::valid(&fixture);
        fresh_call.expected_revision = 4;
        let blocked = lock_apply(
            &mut fresh_transaction,
            &fixture,
            &fresh_operation,
            &fresh_call,
        )
        .await?;
        assert_closed_apply_result(&blocked, "runtime_drain_required");
        assert_eq!(
            product_slot_writer_epoch_in(&mut fresh_transaction, &fixture).await,
            3
        );
        assert_eq!(
            existing_runtime_product_state_in(&mut fresh_transaction, &fixture).await,
            durable_before
        );
        fresh_transaction.rollback().await?;
        assert_eq!(
            product_pending_drain_state(&database.pool, &fixture).await,
            pending_state
        );
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
