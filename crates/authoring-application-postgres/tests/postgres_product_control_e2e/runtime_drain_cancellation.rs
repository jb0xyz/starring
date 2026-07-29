fn cancellation_product_control(
    pool: &PgPool,
) -> authoring_application_postgres::PostgresProductControl {
    let keyring = ProductDecisionDigestKeyringV1::new(
        ProductDecisionDigestKeyV1::from_bytes(
            "product-e2e-v1",
            std::array::from_fn(|index| 41_u8.wrapping_add(index as u8)),
        )
        .unwrap(),
        [ProductDecisionDigestKeyV1::from_bytes(
            "product-e2e-v2",
            std::array::from_fn(|index| 97_u8.wrapping_add(index as u8)),
        )
        .unwrap()],
    )
    .unwrap();
    authoring_application_postgres::PostgresProductControl::new(
        product_decision_pools(pool),
        pool.clone(),
        pool.clone(),
        keyring,
    )
    .unwrap()
}

fn rotated_cancellation_product_control(
    pool: &PgPool,
) -> authoring_application_postgres::PostgresProductControl {
    let keyring = ProductDecisionDigestKeyringV1::new(
        ProductDecisionDigestKeyV1::from_bytes(
            "product-e2e-v3",
            std::array::from_fn(|index| 173_u8.wrapping_add(index as u8)),
        )
        .unwrap(),
        [
            ProductDecisionDigestKeyV1::from_bytes(
                "product-e2e-v1",
                std::array::from_fn(|index| 41_u8.wrapping_add(index as u8)),
            )
            .unwrap(),
            ProductDecisionDigestKeyV1::from_bytes(
                "product-e2e-v2",
                std::array::from_fn(|index| 97_u8.wrapping_add(index as u8)),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    authoring_application_postgres::PostgresProductControl::new(
        product_decision_pools(pool),
        pool.clone(),
        pool.clone(),
        keyring,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_control_cancels_acknowledged_runtime_drain_and_replays_exactly() {
    let database = isolated_product_control_database("cancel_drain_e2e").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let pool = &database.pool;
        let source = seed_fixture(pool).await;
        let decisions = cancellation_product_control(pool);
        let authentication = PostgresAuthentication::new(pool.clone());
        let authority = authority_adapter(source.clone());
        let deployments = PostgresProductDeploymentStatuses::new(pool.clone());
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        application
            .approve(
                &source.credential,
                &source.csrf,
                &ProductRequestIdV1::parse(&format!("cancel.source.approve.{}", suffix())).unwrap(),
                &selector(&source),
                approval_command(&source, &format!("cancel-source-approve-{}", suffix())),
            )
            .await
            .unwrap();
        let source_applied = application
            .apply(
                &source.credential,
                &source.csrf,
                &ProductRequestIdV1::parse(&format!("cancel.source.apply.{}", suffix())).unwrap(),
                &selector(&source),
                apply_command(&source, &format!("cancel-source-{}", suffix())),
            )
            .await
            .unwrap();
        let source_snapshot = advance_application_source_to_awaiting_gateway_ready(
            pool,
            &source,
            source_applied.exact_deployment(),
        )
        .await;
        let target = seed_competing_product_control_fixture(pool, &source).await;
        application
            .approve(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("cancel.target.approve.{}", suffix())).unwrap(),
                &selector(&target),
                approval_command(&target, &format!("cancel-approve-{}", suffix())),
            )
            .await
            .unwrap();
        let apply_key = format!("cancel-target-apply-{}", suffix());
        let pending_error = application
            .apply(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("cancel.target.pending.{}", suffix())).unwrap(),
                &selector(&target),
                apply_command(&target, &apply_key),
            )
            .await
            .unwrap_err();
        assert_eq!(
            pending_error,
            ProductApplicationError::Control(ProductControlPortError::RuntimeDrainRequired)
        );
        let pending = load_pending_application_drain(pool, &source_snapshot).await;
        let acknowledged = acknowledge_application_drain(pool, &source_snapshot, &pending).await;
        let cancellation_key = format!("cancel-lifecycle-{}", suffix());
        let cancellation = authoring_application::CancelProductLifecycleMutationV1 {
            promotion: PromotionSelectorV1::new(target.promotion_id.clone()),
            expected_payload_digest: ApprovalPayloadDigestV1::parse(&target.payload_digest)
                .unwrap(),
            expected_revision: ProductRevisionV1::new(2).unwrap(),
            drain_selector: authoring_application::ProductDrainSelectorV1::from_server_projection(
                acknowledged.drain_intent_id.clone(),
                u64::try_from(acknowledged.intent_revision).unwrap(),
                acknowledged.state_digest.clone(),
                acknowledged.product_operation_id.clone(),
                source_snapshot.revision.get(),
            )
            .unwrap(),
            idempotency_key: ProductIdempotencyKeyV1::parse(&cancellation_key).unwrap(),
            reason: authoring_application::ProductLifecycleCancellationReasonV1::parse(
                "Operator cancelled pending rollout",
            )
            .unwrap(),
        };
        let first = application
            .cancel_product_lifecycle(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("cancel.target.first.{}", suffix())).unwrap(),
                &selector(&target),
                cancellation.clone(),
            )
            .await
            .unwrap();
        assert!(!first.exact_replay());
        assert_eq!(
            first.resulting_runtime_deployment_revision().get(),
            source_snapshot.revision.get() + 1
        );
        assert_eq!(
            first.terminal_intent_revision().get(),
            u64::try_from(acknowledged.intent_revision).unwrap() + 1
        );
        let rotated_decisions = rotated_cancellation_product_control(pool);
        let rotated_authority = authority_adapter(target.clone());
        let rotated_application = ProductControlApplication::new(
            &authentication,
            &rotated_authority,
            &rotated_decisions,
            &deployments,
        );
        let replay = rotated_application
            .cancel_product_lifecycle(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("cancel.target.replay.{}", suffix())).unwrap(),
                &selector(&target),
                cancellation.clone(),
            )
            .await
            .unwrap();
        assert!(replay.exact_replay());
        assert_eq!(
            replay.resulting_runtime_deployment_revision(),
            first.resulting_runtime_deployment_revision()
        );
        assert_eq!(
            replay.terminal_intent_revision(),
            first.terminal_intent_revision()
        );
        let durable = sqlx::query_as::<_, (String, i64, i64, Option<String>, String, i64, i64)>(
            "SELECT drain.intent_state, drain.intent_revision, fence.writer_epoch, \
              fence.pending_drain_intent_id, action.terminal_kind, source.revision, \
              activation.product_revision \
             FROM public.runtime_drain_intents_v2 AS drain \
             INNER JOIN public.runtime_slot_writer_fences_v2 AS fence \
               ON fence.slot_guild_id = drain.slot_guild_id \
               AND fence.slot_ruleset_key = drain.slot_ruleset_key \
             INNER JOIN public.runtime_product_drain_terminal_actions_v2 AS action \
               ON action.drain_intent_id = drain.drain_intent_id \
             INNER JOIN public.runtime_deployments AS source \
               ON source.deployment_id = drain.deployment_id \
             INNER JOIN public.activation_requests AS activation \
               ON activation.promotion_id = $2 \
               AND activation.tenant_id = drain.tenant_id \
               AND activation.installation_id = drain.installation_id \
             WHERE drain.drain_intent_id = $1",
        )
        .bind(&acknowledged.drain_intent_id)
        .bind(target.promotion_id.as_str())
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(durable.0, "cancelled");
        assert_eq!(durable.1, acknowledged.intent_revision + 1);
        assert!(durable.3.is_none());
        assert_eq!(durable.4, "cancelled");
        assert_eq!(
            durable.5,
            i64::try_from(source_snapshot.revision.get()).unwrap() + 1
        );
        assert_eq!(durable.6, 2);
        let mut tamper = pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *tamper)
            .await
            .unwrap();
        let deleted = sqlx::query(
            "DELETE FROM public.runtime_product_drain_terminal_actions_v2 \
             WHERE drain_intent_id = $1",
        )
        .bind(&acknowledged.drain_intent_id)
        .execute(&mut *tamper)
        .await
        .unwrap();
        assert_eq!(deleted.rows_affected(), 1);
        sqlx::query("SET LOCAL session_replication_role = origin")
            .execute(&mut *tamper)
            .await
            .unwrap();
        tamper.commit().await.unwrap();
        let corrupt_replay = rotated_application
            .cancel_product_lifecycle(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("cancel.target.corrupt-replay.{}", suffix()))
                    .unwrap(),
                &selector(&target),
                cancellation,
            )
            .await
            .unwrap_err();
        assert!(matches!(
            corrupt_replay,
            ProductApplicationError::Control(ProductControlPortError::Backend(_))
        ));
        let unchanged = sqlx::query_as::<_, (String, i64, i64)>(
            "SELECT drain.intent_state, drain.intent_revision, source.revision \
             FROM public.runtime_drain_intents_v2 AS drain \
             INNER JOIN public.runtime_deployments AS source \
               ON source.deployment_id = drain.deployment_id \
             WHERE drain.drain_intent_id = $1",
        )
        .bind(&acknowledged.drain_intent_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(unchanged.0, "cancelled");
        assert_eq!(unchanged.1, acknowledged.intent_revision + 1);
        assert_eq!(
            unchanged.2,
            i64::try_from(source_snapshot.revision.get()).unwrap() + 1
        );
    }
    drop_isolated_product_control_database(database).await;
}
