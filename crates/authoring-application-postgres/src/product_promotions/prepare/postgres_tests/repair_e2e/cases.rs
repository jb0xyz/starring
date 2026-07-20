use authoring_promotion::PromotionStageV1;

use crate::product_promotions::row::ProductPromotionLegacyRepairStageV1;

use super::super::*;
use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn legacy_repair_recovers_linked_and_unlinked_pending_rows() {
    let name = "starring_product_promotion_repair_pending_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let artifact = preview_ready_artifact().await;
    let ring = keyring();
    let linked = create_legacy_fixture(
        &pool,
        &ring,
        artifact.clone(),
        "repair-linked-pending-key",
        "repair-linked-pending-seed",
        true,
        30,
    )
    .await;
    progress_legacy_activation_to_approved(&pool, &linked).await;
    let unlinked = create_legacy_fixture(
        &pool,
        &ring,
        artifact,
        "repair-unlinked-pending-key",
        "repair-unlinked-pending-seed",
        false,
        30,
    )
    .await;
    for (fixture, request_id, expected_state) in [
        (&linked, "repair-linked-pending-request", "approved"),
        (&unlinked, "repair-unlinked-pending-request", "pending"),
    ] {
        let input = repair_input(&pool, &ring, fixture, request_id).await;
        let row = direct_repair(&pool, &input).await.unwrap();
        assert_eq!(row.outcome_code, "recovered");
        if expected_state == "approved" {
            let request = &row.activation_projection.as_ref().unwrap().0["request"];
            assert_eq!(request["state"], "approved");
            assert_eq!(request["approvals"].as_array().unwrap().len(), 1);
            assert_eq!(request["approvals"][0]["approver"], "1002");
            assert_eq!(
                request["approvals"][0]["approval_payload_digest"],
                request["approval_context"]["context"]["approval_payload_digest"]
            );
        }
        assert_recovered(decode_repair(row, &ring, &input), "activation_pending", 3);
        assert_eq!(
            repair_write_state(
                &pool,
                fixture.case.plan.promotion_id.as_str(),
                &input.digests.receipt_id,
            )
            .await,
            RepairWriteState {
                promotion_stage: "activation_pending".to_string(),
                promotion_revision: 3,
                admission_count: 1,
                activation_state: expected_state.to_string(),
                link_state: "linked".to_string(),
                receipt_count: 1,
                alias_count: 1,
                audit_count: 1,
                evidence_count: 1,
            }
        );
    }
    drop_temporary_database(administrator, pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn legacy_repair_expires_linked_and_unlinked_rows_then_replays_exactly() {
    let name = "starring_product_promotion_repair_expired_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let artifact = preview_ready_artifact().await;
    let ring = keyring();
    let linked = create_legacy_fixture(
        &pool,
        &ring,
        artifact.clone(),
        "repair-linked-expired-key",
        "repair-linked-expired-seed",
        true,
        1,
    )
    .await;
    let unlinked = create_legacy_fixture(
        &pool,
        &ring,
        artifact,
        "repair-unlinked-expired-key",
        "repair-unlinked-expired-seed",
        false,
        1,
    )
    .await;
    progress_legacy_activation_to_approved(&pool, &linked).await;
    wait_until_expired(&[&linked, &unlinked]).await;
    for (fixture, request_id, expected_link) in [
        (&linked, "repair-linked-expired-request", "linked"),
        (&unlinked, "repair-unlinked-expired-request", "unlinked"),
    ] {
        let input = repair_input(&pool, &ring, fixture, request_id).await;
        let first = direct_repair(&pool, &input).await.unwrap();
        assert_eq!(first.outcome_code, "recovered");
        if expected_link == "linked" {
            let request = &first.activation_projection.as_ref().unwrap().0["request"];
            assert_eq!(request["state"], "expired");
            assert_eq!(request["approvals"].as_array().unwrap().len(), 1);
            assert_eq!(request["approvals"][0]["approver"], "1002");
        }
        assert_recovered(decode_repair(first, &ring, &input), "expired", 4);
        let second = direct_repair(&pool, &input).await.unwrap();
        assert_eq!(second.outcome_code, "final_replay_required");
        assert!(second.promotion_record.is_some());
        assert!(second.admission_evidence.is_some());
        assert!(second.admission_digest.is_some());
        assert!(second.activation_projection.is_none());
        assert!(second.receipt_projection.is_none());
        assert!(second.audit_evidence_projection.is_none());
        assert!(matches!(
            decode_repair(second, &ring, &input),
            ProductPromotionLegacyRepairStageV1::FinalReplayRequired(_)
        ));
        let adapter = PostgresProductPromotions::new(pool.clone(), ring.clone()).unwrap();
        let replay = adapter
            .execute_replay_stage_v1(&input.access, &input.context, &input.digests)
            .await
            .unwrap();
        let ProductPromotionReplayStageV1::FinalExact(finalized) = replay else {
            panic!("repaired expired promotion did not replay exactly")
        };
        assert!(matches!(
            finalized.admitted.record.stage,
            PromotionStageV1::Expired { .. }
        ));
        assert_eq!(
            repair_write_state(
                &pool,
                fixture.case.plan.promotion_id.as_str(),
                &input.digests.receipt_id,
            )
            .await,
            RepairWriteState {
                promotion_stage: "expired".to_string(),
                promotion_revision: 4,
                admission_count: 1,
                activation_state: "expired".to_string(),
                link_state: expected_link.to_string(),
                receipt_count: 1,
                alias_count: 2,
                audit_count: 1,
                evidence_count: 1,
            }
        );
    }
    drop_temporary_database(administrator, pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn concurrent_legacy_repair_has_one_winner_and_one_replay_signal() {
    let name = "starring_product_promotion_repair_concurrent_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let ring = keyring();
    let fixture = create_legacy_fixture(
        &pool,
        &ring,
        preview_ready_artifact().await,
        "repair-concurrent-key",
        "repair-concurrent-seed",
        false,
        30,
    )
    .await;
    let first_input = repair_input(&pool, &ring, &fixture, "repair-concurrent-request-one").await;
    let second_input = repair_input(&pool, &ring, &fixture, "repair-concurrent-request-two").await;
    assert_eq!(
        first_input.digests.receipt_id,
        second_input.digests.receipt_id
    );
    let (first, second) = tokio::join!(
        direct_repair(&pool, &first_input),
        direct_repair(&pool, &second_input)
    );
    let first = first.unwrap();
    let second = second.unwrap();
    let mut outcomes = [first.outcome_code.as_str(), second.outcome_code.as_str()];
    outcomes.sort_unstable();
    assert_eq!(outcomes, ["final_replay_required", "recovered"]);
    for (row, input) in [(first, &first_input), (second, &second_input)] {
        match row.outcome_code.as_str() {
            "recovered" => {
                assert_recovered(decode_repair(row, &ring, input), "activation_pending", 3)
            }
            "final_replay_required" => assert!(matches!(
                decode_repair(row, &ring, input),
                ProductPromotionLegacyRepairStageV1::FinalReplayRequired(_)
            )),
            _ => panic!("unexpected concurrent repair outcome"),
        }
    }
    assert_eq!(
        repair_write_state(
            &pool,
            fixture.case.plan.promotion_id.as_str(),
            &first_input.digests.receipt_id,
        )
        .await,
        RepairWriteState {
            promotion_stage: "activation_pending".to_string(),
            promotion_revision: 3,
            admission_count: 1,
            activation_state: "pending".to_string(),
            link_state: "linked".to_string(),
            receipt_count: 1,
            alias_count: 2,
            audit_count: 1,
            evidence_count: 1,
        }
    );
    drop_temporary_database(administrator, pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn legacy_repair_rejects_target_context_link_and_admission_tampering_without_writes() {
    let name = "starring_product_promotion_repair_tamper_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let ring = keyring();
    let fixture = create_legacy_fixture(
        &pool,
        &ring,
        preview_ready_artifact().await,
        "repair-tamper-key",
        "repair-tamper-seed",
        true,
        30,
    )
    .await;
    let input = repair_input(&pool, &ring, &fixture, "repair-tamper-request").await;
    for statement in [
        "UPDATE public.activation_requests SET target_content_hash = pg_catalog.repeat('0', 64) WHERE promotion_id = $1",
        "UPDATE public.activation_requests SET approval_context_digest = pg_catalog.repeat('0', 64), approval_context = pg_catalog.jsonb_set(approval_context, '{context,approval_context_digest}', pg_catalog.to_jsonb(pg_catalog.repeat('0', 64))) WHERE promotion_id = $1",
        "UPDATE public.activation_requests SET link_state = link_state || '{\"unexpected\":true}'::JSONB WHERE promotion_id = $1",
    ] {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("ALTER TABLE public.activation_requests DISABLE TRIGGER USER")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(statement)
            .bind(fixture.case.plan.promotion_id.as_str())
            .execute(&mut *transaction)
            .await
            .unwrap();
        let row = direct_repair(&mut *transaction, &input).await.unwrap();
        assert_eq!(row.outcome_code, "persistence_corrupt");
        assert!(row.promotion_record.is_none());
        assert!(row.admission_evidence.is_none());
        assert!(row.admission_digest.is_none());
        assert!(row.activation_projection.is_none());
        assert!(row.receipt_projection.is_none());
        assert!(row.audit_evidence_projection.is_none());
        let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipts WHERE receipt_id = $1), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases WHERE receipt_id = $1), \
             (SELECT pg_catalog.count(*) FROM public.product_audit_events WHERE receipt_id = $1), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence WHERE receipt_id = $1)",
        )
        .bind(&input.digests.receipt_id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(counts, (0, 0, 0, 0));
        transaction.rollback().await.unwrap();
    }
    let mut definition_transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.automation_ruleset_versions DISABLE TRIGGER USER")
        .execute(&mut *definition_transaction)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions DROP CONSTRAINT arv_content_integrity",
    )
    .execute(&mut *definition_transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.automation_ruleset_versions AS version \
         SET definition = version.definition || '{\"repair_tampered\":true}'::JSONB \
         FROM public.activation_requests AS activation \
         WHERE activation.promotion_id = $1 \
           AND version.guild_id = activation.guild_id \
           AND version.ruleset_key = activation.ruleset_key \
           AND version.version = activation.target_version",
    )
    .bind(fixture.case.plan.promotion_id.as_str())
    .execute(&mut *definition_transaction)
    .await
    .unwrap();
    let row = direct_repair(&mut *definition_transaction, &input)
        .await
        .unwrap();
    assert_eq!(row.outcome_code, "persistence_corrupt");
    assert!(row.promotion_record.is_none());
    assert!(row.admission_evidence.is_none());
    assert!(row.admission_digest.is_none());
    assert!(row.activation_projection.is_none());
    assert!(row.receipt_projection.is_none());
    assert!(row.audit_evidence_projection.is_none());
    let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts WHERE receipt_id = $1), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases WHERE receipt_id = $1), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events WHERE receipt_id = $1), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence WHERE receipt_id = $1)",
    )
    .bind(&input.digests.receipt_id)
    .fetch_one(&mut *definition_transaction)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0, 0, 0));
    definition_transaction.rollback().await.unwrap();
    let mut admission_transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.authoring_promotions DISABLE TRIGGER USER")
        .execute(&mut *admission_transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET product_admission_format_version = 1, product_admission_digest = $2, \
             product_admission = $3 WHERE id = $1",
    )
    .bind(fixture.case.plan.promotion_id.as_str())
    .bind(&fixture.case.admission.digest)
    .bind(sqlx::types::Json(admission_evidence(&fixture)))
    .execute(&mut *admission_transaction)
    .await
    .unwrap();
    let row = direct_repair(&mut *admission_transaction, &input)
        .await
        .unwrap();
    assert_eq!(row.outcome_code, "persistence_corrupt");
    let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts WHERE receipt_id = $1), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases WHERE receipt_id = $1), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events WHERE receipt_id = $1), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence WHERE receipt_id = $1)",
    )
    .bind(&input.digests.receipt_id)
    .fetch_one(&mut *admission_transaction)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0, 0, 0));
    admission_transaction.rollback().await.unwrap();
    drop_temporary_database(administrator, pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn legacy_repair_refuses_prepared_and_published_rows_without_writes() {
    let name = "starring_product_promotion_repair_nonfinal_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let ring = keyring();
    for (stage, suffix) in [("prepared", "prepared"), ("published", "published")] {
        let fixture = create_legacy_fixture(
            &pool,
            &ring,
            preview_ready_artifact().await,
            &format!("repair-{suffix}-key"),
            &format!("repair-{suffix}-seed"),
            true,
            30,
        )
        .await;
        let input = repair_input(&pool, &ring, &fixture, &format!("repair-{suffix}-request")).await;
        rewrite_legacy_promotion_stage(&pool, &fixture, stage).await;
        let row = direct_repair(&pool, &input).await.unwrap();
        assert_eq!(row.outcome_code, "persistence_corrupt");
        assert!(row.promotion_record.is_none());
        assert!(row.admission_evidence.is_none());
        assert!(row.admission_digest.is_none());
        assert!(row.activation_projection.is_none());
        assert!(row.receipt_projection.is_none());
        assert!(row.audit_evidence_projection.is_none());
        let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipts WHERE receipt_id = $1), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases WHERE receipt_id = $1), \
             (SELECT pg_catalog.count(*) FROM public.product_audit_events WHERE receipt_id = $1), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence WHERE receipt_id = $1)",
        )
        .bind(&input.digests.receipt_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counts, (0, 0, 0, 0));
    }
    drop_temporary_database(administrator, pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn legacy_repair_preserves_superseded_and_withdrawn_activation_states() {
    let name = "starring_product_promotion_repair_terminal_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let ring = keyring();
    for state in ["superseded", "withdrawn"] {
        let fixture = create_legacy_fixture(
            &pool,
            &ring,
            preview_ready_artifact().await,
            &format!("repair-{state}-key"),
            &format!("repair-{state}-seed"),
            true,
            30,
        )
        .await;
        progress_legacy_activation_to_terminal(&pool, &fixture, state).await;
        let input = repair_input(&pool, &ring, &fixture, &format!("repair-{state}-request")).await;
        let row = direct_repair(&pool, &input).await.unwrap();
        assert_eq!(row.outcome_code, "recovered");
        let request = &row.activation_projection.as_ref().unwrap().0["request"];
        assert_eq!(request["state"], state);
        assert_eq!(request["termination"]["kind"], state);
        assert_recovered(decode_repair(row, &ring, &input), "activation_pending", 3);
    }
    drop_temporary_database(administrator, pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn repaired_pending_receipt_replays_after_later_owner_expiry() {
    let name = "starring_product_promotion_repair_later_expiry_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let ring = keyring();
    let fixture = create_legacy_fixture(
        &pool,
        &ring,
        preview_ready_artifact().await,
        "repair-later-expiry-key",
        "repair-later-expiry-seed",
        true,
        1,
    )
    .await;
    let input = repair_input(&pool, &ring, &fixture, "repair-later-expiry-request").await;
    let repaired = direct_repair(&pool, &input).await.unwrap();
    assert_eq!(repaired.outcome_code, "recovered");
    let receipt = &repaired.receipt_projection.as_ref().unwrap().0;
    assert_eq!(receipt["resulting_revision"], 3);
    assert_eq!(receipt["resulting_state"], "activation_pending");
    match decode_repair(repaired, &ring, &input) {
        ProductPromotionLegacyRepairStageV1::Finalized(_) => {}
        _ => panic!("pending legacy repair did not finalize"),
    }
    wait_until_expired(&[&fixture]).await;
    expire_repaired_promotion(&pool, &fixture).await;
    let adapter = PostgresProductPromotions::new(pool.clone(), ring.clone()).unwrap();
    let replay = adapter
        .execute_replay_stage_v1(&input.access, &input.context, &input.digests)
        .await
        .unwrap();
    let ProductPromotionReplayStageV1::FinalExact(finalized) = replay else {
        panic!("later-expired repaired promotion did not replay exactly")
    };
    assert!(matches!(
        finalized.admitted.record.stage,
        PromotionStageV1::Expired { .. }
    ));
    assert_eq!(finalized.admitted.record.revision.get(), 4);
    let persisted_receipt = sqlx::query_as::<_, (Option<i64>, String)>(
        "SELECT resulting_revision, resulting_state FROM public.product_action_receipts \
         WHERE receipt_id = $1",
    )
    .bind(&input.digests.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted_receipt,
        (Some(3), "activation_pending".to_string())
    );
    drop_temporary_database(administrator, pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn deterministic_receipt_collision_rolls_back_legacy_repair() {
    let name = "starring_product_promotion_repair_collision_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let ring = keyring();
    let fixture = create_legacy_fixture(
        &pool,
        &ring,
        preview_ready_artifact().await,
        "repair-collision-key",
        "repair-collision-seed",
        false,
        30,
    )
    .await;
    let input = repair_input(&pool, &ring, &fixture, "repair-collision-request").await;
    let mut collision = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.product_action_receipts DISABLE TRIGGER USER")
        .execute(&mut *collision)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.product_action_receipts (\
         receipt_id, tenant_id, installation_id, principal_id, endpoint_domain, \
         idempotency_key_digest, idempotency_digest_key_id, \
         idempotency_digest_key_fingerprint, request_digest, target_resource_type, \
         target_resource_id, resulting_revision, resulting_state, result_code, \
         http_disposition_class, completed_at) \
         VALUES ($1, 'tenant', 'installation', 'principal', 'product_promote_v1', \
         $2, $3, $4, $5, 'authoring_promotion', $6, 3, 'activation_pending', \
         'promotion_recovered', 2, pg_catalog.clock_timestamp())",
    )
    .bind(&input.digests.receipt_id)
    .bind("0".repeat(64))
    .bind(&input.digests.active_key_id)
    .bind(&input.digests.active_key_fingerprint)
    .bind(&input.digests.semantic_request)
    .bind("f".repeat(64))
    .execute(&mut *collision)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.product_action_receipts ENABLE TRIGGER USER")
        .execute(&mut *collision)
        .await
        .unwrap();
    collision.commit().await.unwrap();
    let row = direct_repair(&pool, &input).await.unwrap();
    assert_eq!(row.outcome_code, "persistence_corrupt");
    assert!(row.promotion_record.is_none());
    assert!(row.admission_evidence.is_none());
    assert!(row.admission_digest.is_none());
    assert!(row.activation_projection.is_none());
    assert!(row.receipt_projection.is_none());
    assert!(row.audit_evidence_projection.is_none());
    assert_eq!(
        repair_write_state(
            &pool,
            fixture.case.plan.promotion_id.as_str(),
            &input.digests.receipt_id,
        )
        .await,
        RepairWriteState {
            promotion_stage: "activation_pending".to_string(),
            promotion_revision: 3,
            admission_count: 0,
            activation_state: "pending".to_string(),
            link_state: "unlinked".to_string(),
            receipt_count: 1,
            alias_count: 0,
            audit_count: 0,
            evidence_count: 0,
        }
    );
    drop_temporary_database(administrator, pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn repaired_receipt_replays_after_its_digest_key_becomes_retired() {
    let name = "starring_product_promotion_repair_rotation_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let old_ring = legacy_keyring();
    let fixture = create_legacy_fixture(
        &pool,
        &old_ring,
        preview_ready_artifact().await,
        "repair-rotation-key",
        "repair-rotation-seed",
        true,
        30,
    )
    .await;
    let repair = repair_input(&pool, &old_ring, &fixture, "repair-rotation-request").await;
    let row = direct_repair(&pool, &repair).await.unwrap();
    assert_eq!(row.outcome_code, "recovered");
    assert_recovered(
        decode_repair(row, &old_ring, &repair),
        "activation_pending",
        3,
    );
    let rotated = keyring();
    let access = access_args(database_now(&pool).await, &SESSION_DIGEST);
    let context = admission_context(
        "repair-rotation-replay",
        fixture.case.plan.intent.authority.session_generation,
    );
    let digests = promotion_digests(
        &rotated,
        &fixture.case.plan,
        &fixture.secret,
        &SESSION_DIGEST,
    );
    let adapter = PostgresProductPromotions::new(pool.clone(), rotated).unwrap();
    let replay = adapter
        .execute_replay_stage_v1(&access, &context, &digests)
        .await
        .unwrap();
    let ProductPromotionReplayStageV1::FinalExact(finalized) = replay else {
        panic!("retired-key receipt did not replay exactly")
    };
    assert_eq!(
        finalized
            .admitted
            .admission
            .payload
            .idempotency_digest_key_id,
        "retired-v1"
    );
    drop_temporary_database(administrator, pool, name).await;
}
