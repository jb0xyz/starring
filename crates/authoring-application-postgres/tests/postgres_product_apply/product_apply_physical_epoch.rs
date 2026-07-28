const PRODUCT_DRAIN_FIRST_APPLY_FOR_EPOCH_TEST: &str = "SELECT outcome_name FROM \
    starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20\
    )";

const PRODUCT_APPLY_BEGIN_RUNTIME_DRAIN_FOR_EPOCH_TEST: &str = "SELECT \
    outcome, product_operation_id, drain_intent_id, writer_epoch_before, \
    writer_epoch_after, pending_drain_intent_id, pending_product_operation_id, \
    pending_tenant_id, pending_installation_id, pending_deployment_id, \
    pending_expected_revision \
    FROM public.starring_product_apply_begin_runtime_drain_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,\
        $21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32\
    )";

#[derive(Debug, sqlx::FromRow)]
struct ProductApplyBeginRuntimeDrainRow {
    outcome: String,
    product_operation_id: Option<String>,
    drain_intent_id: Option<String>,
    writer_epoch_before: Option<i64>,
    writer_epoch_after: Option<i64>,
    pending_drain_intent_id: Option<String>,
    pending_product_operation_id: Option<String>,
    pending_tenant_id: Option<String>,
    pending_installation_id: Option<String>,
    pending_deployment_id: Option<String>,
    pending_expected_revision: Option<i64>,
}

struct ExpectedProductApplyRuntimeDrain<'a> {
    outcome: &'a str,
    operation_id: &'a str,
    intent_id: &'a str,
    epoch_before: i64,
    epoch_after: i64,
    fixture: &'a Fixture,
    deployment_id: &'a str,
}

impl ProductApplyBeginRuntimeDrainRow {
    fn assert_absent(&self, expected_epoch: i64) {
        assert_eq!(self.outcome, "absent");
        assert!(self.product_operation_id.is_none());
        assert!(self.drain_intent_id.is_none());
        assert_eq!(self.writer_epoch_before, Some(expected_epoch));
        assert_eq!(self.writer_epoch_after, Some(expected_epoch));
        assert!(self.pending_drain_intent_id.is_none());
        assert!(self.pending_product_operation_id.is_none());
        assert!(self.pending_tenant_id.is_none());
        assert!(self.pending_installation_id.is_none());
        assert!(self.pending_deployment_id.is_none());
        assert!(self.pending_expected_revision.is_none());
    }

    fn assert_present(&self, expected: ExpectedProductApplyRuntimeDrain<'_>) {
        assert_eq!(self.outcome, expected.outcome);
        assert_eq!(
            self.product_operation_id.as_deref(),
            Some(expected.operation_id)
        );
        assert_eq!(self.drain_intent_id.as_deref(), Some(expected.intent_id));
        assert_eq!(self.writer_epoch_before, Some(expected.epoch_before));
        assert_eq!(self.writer_epoch_after, Some(expected.epoch_after));
        assert_eq!(
            self.pending_drain_intent_id.as_deref(),
            Some(expected.intent_id)
        );
        assert_eq!(
            self.pending_product_operation_id.as_deref(),
            Some(expected.operation_id)
        );
        assert_eq!(
            self.pending_tenant_id.as_deref(),
            Some(expected.fixture.tenant_id.as_str())
        );
        assert_eq!(
            self.pending_installation_id.as_deref(),
            Some(expected.fixture.installation_id.as_str())
        );
        assert_eq!(
            self.pending_deployment_id.as_deref(),
            Some(expected.deployment_id)
        );
        assert_eq!(self.pending_expected_revision, Some(2));
    }
}

async fn begin_product_apply_runtime_drain_in(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    operation: &Operation,
    call: &Call,
    proposed_operation_id: &str,
    proposed_intent_id: &str,
) -> Result<ProductApplyBeginRuntimeDrainRow, sqlx::Error> {
    let context = ApplyLockContext::single(fixture, operation);
    sqlx::query_as::<_, ProductApplyBeginRuntimeDrainRow>(
        PRODUCT_APPLY_BEGIN_RUNTIME_DRAIN_FOR_EPOCH_TEST,
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.promotion_id)
    .bind(call.expected_revision)
    .bind(&context.expected_payload_digest)
    .bind(&fixture.actor.principal_id)
    .bind(&call.session_digest)
    .bind(&fixture.actor.session_subject)
    .bind(&fixture.actor.user_id)
    .bind(&fixture.application_id)
    .bind(&fixture.guild_id)
    .bind(&call.capability)
    .bind(context.expected_authority_revision)
    .bind(&context.expected_authority_digest)
    .bind(&fixture.observation_digest)
    .bind(call.observed_at)
    .bind(call.expires_at)
    .bind(&call.effective_permissions)
    .bind(call.guild_owner)
    .bind(&operation.request_id)
    .bind(&context.active_idempotency_digest)
    .bind(&context.idempotency_candidates)
    .bind(&context.candidate_key_ids)
    .bind(&context.candidate_key_fingerprints)
    .bind(&context.active_key_id)
    .bind(&operation.semantic_digest)
    .bind(&operation.receipt_id)
    .bind(&operation.audit_event_id)
    .bind(&operation.apply_attempt_id)
    .bind(&operation.deployment_id)
    .bind(proposed_operation_id)
    .bind(proposed_intent_id)
    .fetch_one(&mut **transaction)
    .await
}

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
    let mut transaction = begin_serializable(pool).await;
    let outcome = apply_product_pending_drain_in(&mut transaction, &canonical)
        .await
        .unwrap();
    assert_eq!(outcome, "inserted");
    transaction.commit().await.unwrap();
}

async fn apply_product_pending_drain_in(
    transaction: &mut Transaction<'_, Postgres>,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) -> Result<String, sqlx::Error> {
    let product = canonical.product_preimage();
    let drain = canonical.drain_preimage();
    sqlx::query_scalar::<_, String>(PRODUCT_DRAIN_FIRST_APPLY_FOR_EPOCH_TEST)
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
        .fetch_one(&mut **transaction)
        .await
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

async fn product_pending_drain_state(
    pool: &PgPool,
    fixture: &Fixture,
) -> (i64, Option<String>, i64, i64) {
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
async fn product_apply_runtime_drain_observes_rolls_back_inserts_and_adopts_exactly() {
    let database = isolated_database("apply_drain_public").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await?;
        let fixture = seed_fixture(&database.pool).await;
        let applied_operation = Operation::new("public-drain-seed");
        complete_apply(&database.pool, &fixture, &applied_operation).await;

        let mut transition = database.pool.begin().await?;
        set_existing_runtime_phase(
            &mut transition,
            &fixture,
            &applied_operation.deployment_id,
            "awaiting_gateway_ready",
        )
        .await?;
        reopen_applied_activation(&mut transition, &fixture).await?;
        transition.commit().await?;

        let fresh_operation = Operation::new("public-drain-fresh");
        let mut call = Call::valid(&fixture);
        call.expected_revision = 4;
        assert_eq!(product_slot_writer_epoch(&database.pool, &fixture).await, 2);

        let mut observation = begin_serializable(&database.pool).await;
        let absent = begin_product_apply_runtime_drain_in(
            &mut observation,
            &fixture,
            &fresh_operation,
            &call,
            "",
            "",
        )
        .await?;
        absent.assert_absent(2);
        observation.commit().await?;
        assert_eq!(
            product_pending_drain_state(&database.pool, &fixture).await,
            (2, None, 0, 0)
        );

        let rolled_back_operation_id = &digest("public-drain-rolled-back-operation")[..32];
        let rolled_back_intent_id = &digest("public-drain-rolled-back-intent")[..32];
        let mut rolled_back = begin_serializable(&database.pool).await;
        let inserted = begin_product_apply_runtime_drain_in(
            &mut rolled_back,
            &fixture,
            &fresh_operation,
            &call,
            rolled_back_operation_id,
            rolled_back_intent_id,
        )
        .await?;
        inserted.assert_present(ExpectedProductApplyRuntimeDrain {
            outcome: "inserted",
            operation_id: rolled_back_operation_id,
            intent_id: rolled_back_intent_id,
            epoch_before: 2,
            epoch_after: 3,
            fixture: &fixture,
            deployment_id: &applied_operation.deployment_id,
        });
        rolled_back.rollback().await?;
        assert_eq!(
            product_pending_drain_state(&database.pool, &fixture).await,
            (2, None, 0, 0)
        );

        let operation_id = &digest("public-drain-committed-operation")[..32];
        let intent_id = &digest("public-drain-committed-intent")[..32];
        let mut creation = begin_serializable(&database.pool).await;
        let absent_after_rollback = begin_product_apply_runtime_drain_in(
            &mut creation,
            &fixture,
            &fresh_operation,
            &call,
            "",
            "",
        )
        .await?;
        absent_after_rollback.assert_absent(2);
        let inserted = begin_product_apply_runtime_drain_in(
            &mut creation,
            &fixture,
            &fresh_operation,
            &call,
            operation_id,
            intent_id,
        )
        .await?;
        inserted.assert_present(ExpectedProductApplyRuntimeDrain {
            outcome: "inserted",
            operation_id,
            intent_id,
            epoch_before: 2,
            epoch_after: 3,
            fixture: &fixture,
            deployment_id: &applied_operation.deployment_id,
        });
        creation.commit().await?;

        let persisted = product_pending_drain_state(&database.pool, &fixture).await;
        assert_eq!(persisted, (3, Some(intent_id.to_string()), 1, 1));

        let mut replay = begin_serializable(&database.pool).await;
        let adopted = begin_product_apply_runtime_drain_in(
            &mut replay,
            &fixture,
            &fresh_operation,
            &call,
            "",
            "",
        )
        .await?;
        adopted.assert_present(ExpectedProductApplyRuntimeDrain {
            outcome: "replayed",
            operation_id,
            intent_id,
            epoch_before: 3,
            epoch_after: 3,
            fixture: &fixture,
            deployment_id: &applied_operation.deployment_id,
        });
        replay.commit().await?;
        assert_eq!(
            product_pending_drain_state(&database.pool, &fixture).await,
            persisted
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
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

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn drain_first_apply_wins_slot_epoch_race_and_product_apply_retries_closed() {
    let database = isolated_database("apply_epoch_race").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await?;
        let fixture = seed_fixture(&database.pool).await;
        let applied_operation = Operation::new("physical-epoch-race-seed");
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
        reopen_applied_activation(&mut transition, &fixture).await?;
        transition.commit().await?;

        let mut drain_snapshot = prepared.snapshot().clone();
        drain_snapshot.revision =
            automation_runtime_convergence::DeploymentRevision::new(2).unwrap();
        drain_snapshot.phase =
            automation_runtime_convergence::RuntimeDeploymentPhaseV1::AwaitingGatewayReady;
        let canonical = product_apply_drain_for_epoch_test(&drain_snapshot);
        let expected_intent_id = canonical.drain_preimage().key.intent_id.as_str().to_string();
        let fresh_operation = Operation::new("physical-epoch-race-fresh");

        let mut drain_transaction = begin_serializable(&database.pool).await;
        let drain_outcome =
            apply_product_pending_drain_in(&mut drain_transaction, &canonical).await?;
        assert_eq!(drain_outcome, "inserted");
        assert_eq!(
            product_slot_writer_epoch_in(&mut drain_transaction, &fixture).await,
            3
        );

        let (started_sender, started_receiver) = futures::channel::oneshot::channel();
        let apply_pool = database.pool.clone();
        let apply_fixture = fixture.clone();
        let apply_operation = fresh_operation.clone();
        let apply = tokio::spawn(async move {
            let mut transaction = begin_serializable(&apply_pool).await;
            let process_id = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
                .fetch_one(&mut *transaction)
                .await?;
            let _ = started_sender.send(process_id);
            let mut call = Call::valid(&apply_fixture);
            call.expected_revision = 4;
            let result =
                lock_apply(&mut transaction, &apply_fixture, &apply_operation, &call).await;
            transaction.rollback().await?;
            result
        });

        let process_id = started_receiver.await.unwrap();
        wait_for_advisory_lock_wait(&database.pool, process_id).await;
        assert_eq!(product_slot_writer_epoch(&database.pool, &fixture).await, 2);

        drain_transaction.commit().await?;
        let stale_error = apply
            .await
            .unwrap()
            .expect_err("stale Product Apply must retry after drain first-apply commits");
        assert!(is_serialization_failure(&stale_error));

        let pending_state = product_pending_drain_state(&database.pool, &fixture).await;
        assert_eq!(pending_state, (3, Some(expected_intent_id), 1, 1));

        terminalize_product_apply_pending_deployment(
            &database.pool,
            &fixture,
            &applied_operation.deployment_id,
        )
        .await;
        let durable_before = existing_runtime_product_state(&database.pool, &fixture).await;

        let mut retry_transaction = begin_serializable(&database.pool).await;
        let mut retry_call = Call::valid(&fixture);
        retry_call.expected_revision = 4;
        let blocked = lock_apply(
            &mut retry_transaction,
            &fixture,
            &fresh_operation,
            &retry_call,
        )
        .await?;
        assert_closed_apply_result(&blocked, "runtime_drain_required");
        assert_eq!(
            product_slot_writer_epoch_in(&mut retry_transaction, &fixture).await,
            3
        );
        assert_eq!(
            existing_runtime_product_state_in(&mut retry_transaction, &fixture).await,
            durable_before
        );
        retry_transaction.rollback().await?;

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
