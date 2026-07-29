#[derive(sqlx::FromRow)]
struct PendingApplicationDrainV2 {
    drain_intent_id: String,
    product_operation_id: String,
    product_mutation_request_bytes: Vec<u8>,
    product_mutation_digest: String,
    drain_intent_request_bytes: Vec<u8>,
    drain_intent_digest: String,
    expected_revision: i64,
    canonical_state_bytes: Vec<u8>,
    canonical_state_digest: String,
}

struct AcknowledgedApplicationDrainV2 {
    drain_intent_id: String,
    product_operation_id: String,
    intent_revision: i64,
    state_bytes: Vec<u8>,
    state_digest: String,
    acknowledged_at: DateTime<Utc>,
}

async fn seed_competing_product_control_fixture(pool: &PgPool, source: &Fixture) -> Fixture {
    let Json(mut record_value) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT record FROM public.authoring_promotions WHERE id = $1",
    )
    .bind(source.promotion_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap();
    let ruleset_key = RuleSetKey::parse(
        record_value["intent"]["authority"]["ruleset_key"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let source_content_hash = automation_ruleset::RuleSetContentHash::parse_hex(
        record_value["stage"]["publication"]["content_hash"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let definition: automation_state::InteractionRuleSet = serde_json::from_value(json!({
        "version": 1,
        "panels": [{
            "key": "runtime_drain_e2e_panel",
            "channel": "community_hub",
            "content": "Runtime drain successor",
            "buttons": []
        }],
        "modals": [],
        "rules": []
    }))
    .unwrap();
    let target_content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap();
    assert_ne!(target_content_hash, source_content_hash);
    let unique = suffix();
    let scope_digest = canonical_digest(
        IDEMPOTENCY_SCOPE_DOMAIN,
        &json!({
            "tenant_id": source.tenant_id.as_str(),
            "principal_id": record_value["intent"]["authority"]["principal_id"],
            "idempotency_key": format!("runtime-drain-successor-{unique}")
        }),
    );
    let promotion_id = PromotionId::parse(&scope_digest).unwrap();
    record_value["id"] = json!(promotion_id.as_str());
    record_value["intent"]["idempotency_scope_digest"] = json!(scope_digest);
    record_value["intent"]["definition"] = serde_json::to_value(&definition).unwrap();
    record_value["intent"]["preview"]["summary"]["panels"] = json!(1);
    record_value["intent"]["expected_registry_content_hash"] = json!(target_content_hash.to_hex());
    record_value["intent"]["evidence"]["candidate_ruleset_hash"] =
        json!(target_content_hash.to_hex());
    let intent: PromotionIntentV1 = serde_json::from_value(record_value["intent"].clone()).unwrap();
    let promotion_request_digest =
        PromotionRequestDigest::parse(&canonical_digest(PROMOTION_REQUEST_DOMAIN, &intent))
            .unwrap();
    record_value["request_digest"] = json!(promotion_request_digest.as_str());
    record_value["stage"]["publication"]["version"] = json!(2);
    record_value["stage"]["publication"]["content_hash"] = json!(target_content_hash.to_hex());
    record_value["stage"]["publication"]["disposition"] = json!("created");
    let activation_id = canonical_digest(
        ACTIVATION_REQUEST_DOMAIN,
        &json!({
            "promotion_id": promotion_id,
            "promotion_request_digest": promotion_request_digest,
            "version": 2,
            "schema_version": CURRENT_RULESET_SCHEMA_VERSION,
            "content_hash": target_content_hash
        }),
    );
    record_value["stage"]["activation"]["request_id"] = json!(&activation_id);
    record_value["stage"]["activation"]["target"]["version"] = json!(2);
    record_value["stage"]["activation"]["target"]["content_hash"] =
        json!(target_content_hash.to_hex());
    record_value["stage"]["activation"]["observed_active"] = json!({
        "version": 1,
        "content_hash": source_content_hash.to_hex()
    });
    let mut context = serde_json::from_value::<
        automation_ruleset_activation::ProductApprovalContextV1,
    >(record_value["stage"]["activation"]["approval_context"].clone())
    .unwrap();
    context.promotion_id =
        automation_ruleset_activation::ActivationPromotionId::parse(promotion_id.as_str()).unwrap();
    context.promotion_request_digest =
        automation_ruleset_activation::ActivationDigest::parse(promotion_request_digest.as_str())
            .unwrap();
    context.approval_payload_digest =
        automation_ruleset_activation::ActivationDigest::parse(&"0".repeat(64)).unwrap();
    context.approval_context_digest =
        automation_ruleset_activation::ActivationDigest::parse(&"0".repeat(64)).unwrap();
    context.baseline = automation_ruleset_activation::ExpectedActiveBaselineV1::Exact {
        version: RuleSetVersionId::FIRST,
        content_hash: source_content_hash,
    };
    record_value["stage"]["activation"]["approval_context"] =
        serde_json::to_value(&context).unwrap();
    let provisional: PromotionRecordV1 = serde_json::from_value(record_value.clone()).unwrap();
    let payload = provisional.product_approval_payload().unwrap();
    let payload_digest = approval_payload_digest_v1(&payload).unwrap();
    context.approval_payload_digest =
        automation_ruleset_activation::ActivationDigest::parse(payload_digest.as_str()).unwrap();
    let activation_target = automation_ruleset_activation::ActivationTarget {
        guild_id: source.guild_id,
        ruleset_key: ruleset_key.clone(),
        version: RuleSetVersionId::new(2).unwrap(),
        content_hash: target_content_hash,
    };
    context.approval_context_digest =
        automation_ruleset_activation::product_approval_context_digest_v1(
            &automation_ruleset_activation::ActivationRequestId::parse(&activation_id).unwrap(),
            &activation_target,
            record_value["stage"]["activation"]["requester"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .map(UserId)
                .unwrap(),
            &context,
        );
    record_value["stage"]["activation"]["approval_context"] =
        serde_json::to_value(&context).unwrap();
    let record: PromotionRecordV1 = serde_json::from_value(record_value.clone()).unwrap();
    record.validate().unwrap();
    let payload = record.product_approval_payload().unwrap();
    assert_eq!(
        approval_payload_digest_v1(&payload).unwrap(),
        context.approval_payload_digest
    );
    let activation_created_at = record_value["stage"]["activation"]["created_at"]
        .as_str()
        .unwrap()
        .parse::<DateTime<Utc>>()
        .unwrap();
    let activation_expires_at = record_value["stage"]["activation"]["expires_at"]
        .as_str()
        .unwrap()
        .parse::<DateTime<Utc>>()
        .unwrap();
    let linked_at = record_value["stage"]["activation"]["link_state_at_journal"]
        .as_str()
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .unwrap_or(record.updated_at);
    let principal_id = record.intent.authority.principal_id.as_str().to_string();
    let requester_id = record.intent.authority.requester.to_string();
    let approval_context =
        automation_ruleset_activation::ActivationApprovalContextV1::ProductAuthoring {
            context: Box::new(context.clone()),
        };
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let head = sqlx::query(
        "UPDATE public.automation_ruleset_heads \
         SET next_version = 3 \
         WHERE guild_id = $1 AND ruleset_key = $2 AND next_version = 2",
    )
    .bind(source.guild_id.to_string())
    .bind(ruleset_key.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(head.rows_affected(), 1);
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 2, $3, $4, $5, $6)",
    )
    .bind(source.guild_id.to_string())
    .bind(ruleset_key.as_str())
    .bind(i64::from(CURRENT_RULESET_SCHEMA_VERSION.get()))
    .bind(Json(serde_json::to_value(&definition).unwrap()))
    .bind(target_content_hash.to_hex())
    .bind(&requester_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    insert_activation_pending_promotion(
        &mut transaction,
        promotion_id.as_str(),
        promotion_request_digest.as_str(),
        source.tenant_id.as_str(),
        source.installation_id.as_str(),
        &principal_id,
        &serde_json::to_value(&record).unwrap(),
    )
    .await;
    sqlx::query(
        "INSERT INTO public.activation_requests \
         (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, state, created_at, expires_at, authority_kind, link_state_name, \
          approval_context, link_state, promotion_id, promotion_request_digest, \
          approval_payload_digest, approval_context_digest, linked_at, tenant_id, \
          installation_id, product_revision, observed_active_version, \
          observed_active_hash) \
         VALUES ($1, $2, $3, 2, $4, $5, 1, 'pending', $6, $7, \
          'product_authoring', 'linked', $8, $9, $10, $11, $12, $13, $14, $15, $16, 1, 1, $17)",
    )
    .bind(&activation_id)
    .bind(source.guild_id.to_string())
    .bind(ruleset_key.as_str())
    .bind(target_content_hash.to_hex())
    .bind(&requester_id)
    .bind(activation_created_at)
    .bind(activation_expires_at)
    .bind(Json(serde_json::to_value(&approval_context).unwrap()))
    .bind(Json(json!({"state": "linked", "linked_at": linked_at})))
    .bind(promotion_id.as_str())
    .bind(promotion_request_digest.as_str())
    .bind(payload_digest.as_str())
    .bind(context.approval_context_digest.as_str())
    .bind(linked_at)
    .bind(source.tenant_id.as_str())
    .bind(source.installation_id.as_str())
    .bind(source_content_hash.to_hex())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let mut fixture = source.clone();
    fixture.promotion_id = promotion_id;
    fixture.activation_id = activation_id;
    fixture.payload_digest = payload_digest.to_string();
    fixture.payload = payload;
    fixture
}

async fn advance_application_source_to_awaiting_gateway_ready(
    pool: &PgPool,
    fixture: &Fixture,
    exact: &ExactDeploymentSelectorV1,
) -> automation_runtime_convergence::RuntimeDeploymentSnapshotV1 {
    let runtime = PostgresRuntimeConvergence::new(pool.clone());
    let scope = product_runtime_scope(fixture, exact);
    let requested = runtime.status(&scope).await.unwrap();
    let claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: ControllerId::parse(format!(
                "product-drain-e2e-controller-{}",
                suffix()
            ))
            .unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let mut revision = advance_product_runtime_to_ready(&runtime, &scope, &claim).await;
    let guard = ProductRuntimeMutationGuard::from_claim(&scope, &claim);
    revision = mutate_product_runtime(
        &runtime,
        &guard,
        revision,
        DeploymentMutationV1::BeginPanelReconciliation,
    )
    .await;
    let process_instance_id =
        ProcessInstanceId::parse(format!("product-drain-e2e-process-{}", suffix())).unwrap();
    let panel_report_digest = sha256_hex(&format!("product-drain-e2e-panel:{}", suffix()));
    mutate_product_runtime(
        &runtime,
        &guard,
        revision,
        DeploymentMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse(format!(
                "product-drain-e2e-certificate-{}",
                suffix()
            ))
            .unwrap(),
            report_digest: automation_runtime_convergence::PanelReportDigestV1::parse(
                panel_report_digest,
            )
            .unwrap(),
            target: claim.snapshot.target.clone(),
            runtime_generation: claim.snapshot.runtime_generation,
            process_instance_id,
            declared_count: 0,
            installed_count: 0,
            unchanged_count: 0,
            skipped_transient_count: 0,
            skipped_unresolved_channel_count: 0,
            failed_count: 0,
            ambiguous_outcome_count: 0,
            stale_message_cleanup_pending_count: 0,
            orphan_message_cleanup_pending_count: 0,
            reposted_old_message_cleanup_pending_count: 0,
            reconciled_at: claim.acquired_at,
        }),
    )
    .await;
    let mut snapshot = runtime.status(&scope).await.unwrap().snapshot;
    assert_eq!(
        snapshot.phase.kind(),
        automation_runtime_convergence::RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady
    );
    snapshot.controller_lease = None;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let released = sqlx::query(
        "UPDATE public.runtime_deployments \
         SET snapshot = $1, controller_id = NULL, controller_fencing_token = NULL, \
             controller_acquired_at = NULL, controller_lease_expires_at = NULL \
         WHERE tenant_id = $2 AND installation_id = $3 AND deployment_id = $4 \
           AND revision = $5 AND phase = 'awaiting_gateway_ready'",
    )
    .bind(Json(serde_json::to_value(&snapshot).unwrap()))
    .bind(snapshot.identity.tenant_id.as_str())
    .bind(snapshot.identity.installation_id.as_str())
    .bind(snapshot.identity.deployment_id.as_str())
    .bind(i64::try_from(snapshot.revision.get()).unwrap())
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(released.rows_affected(), 1);
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    snapshot
}

async fn load_pending_application_drain(
    pool: &PgPool,
    source: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
) -> PendingApplicationDrainV2 {
    let pending = sqlx::query_as::<_, PendingApplicationDrainV2>(
        "SELECT drain.drain_intent_id, drain.product_operation_id, \
          product.product_mutation_request_bytes, product.product_mutation_digest, \
          drain.drain_intent_request_bytes, drain.drain_intent_digest, \
          drain.expected_revision, drain.canonical_state_bytes, \
          drain.canonical_state_digest \
         FROM public.runtime_drain_intents_v2 AS drain \
         INNER JOIN public.runtime_product_operations_v2 AS product \
           ON product.product_operation_id = drain.product_operation_id \
         WHERE drain.tenant_id = $1 AND drain.installation_id = $2 \
           AND drain.deployment_id = $3 AND drain.intent_state = 'pending'",
    )
    .bind(source.identity.tenant_id.as_str())
    .bind(source.identity.installation_id.as_str())
    .bind(source.identity.deployment_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        pending.expected_revision,
        i64::try_from(source.revision.get()).unwrap()
    );
    assert_eq!(
        pending.canonical_state_digest,
        runtime_drain_digest_bytes(&pending.canonical_state_bytes)
    );
    pending
}

fn persisted_application_drain_root(
    source: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
    pending: &PendingApplicationDrainV2,
) -> automation_runtime_controller::RuntimePersistedProductDrainRootV2 {
    let scope =
        automation_runtime_controller::RuntimeDeploymentScopeV1::from_identity(&source.identity);
    let revision =
        DeploymentRevision::new(u64::try_from(pending.expected_revision).unwrap()).unwrap();
    automation_runtime_controller::RuntimePersistedProductDrainRootV2::from_persisted(
        scope.clone(),
        revision,
        &automation_runtime_controller::RuntimeProductOperationIdV2::parse(
            &pending.product_operation_id,
        )
        .unwrap(),
        scope,
        automation_runtime_controller::RuntimeServingSlotV2::from_target(&source.target),
        revision,
        &automation_runtime_controller::RuntimeDrainIntentIdV2::parse(&pending.drain_intent_id)
            .unwrap(),
        &source.target,
        &pending.product_mutation_request_bytes,
        &automation_runtime_controller::RuntimeProductMutationDigestV2::parse(
            &pending.product_mutation_digest,
        )
        .unwrap(),
        &pending.drain_intent_request_bytes,
        &automation_runtime_controller::RuntimeDrainIntentDigestV2::parse(
            &pending.drain_intent_digest,
        )
        .unwrap(),
    )
    .unwrap()
}

fn runtime_drain_digest_bytes(bytes: &[u8]) -> String {
    lower_hex(&Sha256::digest(bytes))
}

async fn acknowledge_application_drain(
    pool: &PgPool,
    source: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
    pending: &PendingApplicationDrainV2,
) -> AcknowledgedApplicationDrainV2 {
    let root = persisted_application_drain_root(source, pending);
    let pending_state =
        automation_runtime_controller::RuntimeCanonicalDrainIntentStateV2::from_persisted(
            &root,
            NonZeroU64::MIN,
            "pending",
            &pending.canonical_state_bytes,
        )
        .unwrap();
    let key = pending_state.intent().key();
    let pending_marked_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT pending_marked_at FROM public.runtime_slot_writer_fences_v2 \
         WHERE pending_drain_intent_id = $1",
    )
    .bind(&pending.drain_intent_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let acknowledged_at =
        DateTime::from_timestamp_micros(pending_marked_at.timestamp_micros()).unwrap();
    let process_instance_id =
        ProcessInstanceId::parse(format!("product-drain-e2e-owner-{}", suffix())).unwrap();
    let seal = automation_runtime_controller::RuntimeDrainClaimSealWitnessV2::new(
        key,
        process_instance_id.clone(),
        NonZeroU64::new(7).unwrap(),
        None,
        NonZeroU64::new(8).unwrap(),
    )
    .unwrap();
    let claim = automation_runtime_controller::RuntimeDrainClaimV2::new(
        key,
        automation_runtime_controller::RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: automation_runtime_controller::GatewayShardIdV1::parse("shard:0")
                .unwrap(),
            process_instance_id: process_instance_id.clone(),
            lease_epoch: NonZeroU64::MIN,
            expected_build_revision: automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                "product-drain-e2e-build",
            )
            .unwrap(),
        },
        NonZeroU64::new(2).unwrap(),
        process_instance_id,
        ControllerId::parse(format!("product-drain-e2e-owner-controller-{}", suffix())).unwrap(),
        FencingToken::new(2).unwrap(),
        NonZeroU64::new(3).unwrap(),
        NonZeroU64::new(4).unwrap(),
        acknowledged_at + TimeDelta::seconds(60),
        automation_runtime_controller::RuntimeDrainClaimProgressV2::claimed(seal),
    )
    .unwrap();
    let acknowledgement = automation_runtime_controller::RuntimeRouteAbsentAcknowledgementV2::new(
        key,
        claim,
        None,
        automation_runtime_controller::RuntimeRouteMutationProvenanceV2::Ordinary {
            barrier_id: automation_runtime_controller::RuntimeBarrierIdV1::parse(
                &sha256_hex(&format!("product-drain-e2e-barrier:{}", suffix()))[..32],
            )
            .unwrap(),
            pause: automation_runtime_controller::RuntimeBarrierPauseWitnessV2 {
                coordinator_generation: NonZeroU64::new(5).unwrap(),
                connection_epoch: NonZeroU64::new(6).unwrap(),
                paused_admission_revision: NonZeroU64::new(7).unwrap(),
                pause_sequence:
                    automation_runtime_controller::RuntimeGatewayAdmissionSequenceV2::new(
                        NonZeroU64::new(9).unwrap(),
                    ),
            },
        },
        NonZeroU64::new(10).unwrap(),
        automation_runtime_controller::RuntimeDrainCertificationResolutionV2::no_operation_reserved(
        ),
        acknowledged_at,
    )
    .unwrap();
    let intent_revision = NonZeroU64::new(2).unwrap();
    let intent =
        automation_runtime_controller::RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
            &root,
            intent_revision,
            acknowledgement,
        )
        .unwrap();
    let state =
        automation_runtime_controller::RuntimeCanonicalDrainIntentStateV2::from_intent(intent)
            .unwrap();
    automation_runtime_controller::RuntimeRouteAbsentDrainIntentSourceV2::from_acknowledged(
        state.intent().clone(),
    )
    .unwrap();
    let acknowledged = AcknowledgedApplicationDrainV2 {
        drain_intent_id: pending.drain_intent_id.clone(),
        product_operation_id: pending.product_operation_id.clone(),
        intent_revision: i64::try_from(intent_revision.get()).unwrap(),
        state_bytes: state.state_bytes().to_vec(),
        state_digest: runtime_drain_digest_bytes(state.state_bytes()),
        acknowledged_at,
    };
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let changed = sqlx::query(
        "UPDATE public.runtime_drain_intents_v2 \
         SET intent_revision = $2, intent_state = 'route_absent_acknowledged', \
             canonical_state_bytes = $3, canonical_state_digest = $4 \
         WHERE drain_intent_id = $1 AND intent_revision = 1 AND intent_state = 'pending'",
    )
    .bind(&acknowledged.drain_intent_id)
    .bind(acknowledged.intent_revision)
    .bind(&acknowledged.state_bytes)
    .bind(&acknowledged.state_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    acknowledged
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_control_application_consumes_acknowledged_runtime_drain_and_replays_exactly() {
    let database = isolated_product_control_database("consume_e2e").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let pool = &database.pool;
        let source = seed_fixture(pool).await;
        let decisions = product_decisions(pool);
        approve_fixture(pool, &source, &decisions).await;
        let authentication = PostgresAuthentication::new(pool.clone());
        let authority = authority_adapter(source.clone());
        let deployments = PostgresProductDeploymentStatuses::new(pool.clone());
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let source_applied = application
            .apply(
                &source.credential,
                &source.csrf,
                &ProductRequestIdV1::parse(&format!("apply.drain.source.{}", suffix())).unwrap(),
                &selector(&source),
                apply_command(&source, &format!("apply-drain-source-{}", suffix())),
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
        let approval_key = format!("approve-drain-target-{}", suffix());
        application
            .approve(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("approve.drain.target.{}", suffix())).unwrap(),
                &selector(&target),
                approval_command(&target, &approval_key),
            )
            .await
            .unwrap();
        let apply_key = format!("apply-drain-target-{}", suffix());
        let first_error = application
            .apply(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("apply.drain.target.first.{}", suffix()))
                    .unwrap(),
                &selector(&target),
                apply_command(&target, &apply_key),
            )
            .await
            .unwrap_err();
        assert_eq!(
            first_error,
            ProductApplicationError::Control(ProductControlPortError::RuntimeDrainRequired)
        );
        let pending = load_pending_application_drain(pool, &source_snapshot).await;
        let pending_replay_error = application
            .apply(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("apply.drain.target.pending.{}", suffix()))
                    .unwrap(),
                &selector(&target),
                apply_command(&target, &apply_key),
            )
            .await
            .unwrap_err();
        assert_eq!(
            pending_replay_error,
            ProductApplicationError::Control(ProductControlPortError::RuntimeDrainRequired)
        );
        let pending_epoch = sqlx::query_scalar::<_, i64>(
            "SELECT writer_epoch FROM public.runtime_slot_writer_fences_v2 \
             WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
        )
        .bind(source_snapshot.target.guild_id.to_string())
        .bind(source_snapshot.target.ruleset_key.as_str())
        .fetch_one(pool)
        .await
        .unwrap();
        let acknowledged = acknowledge_application_drain(pool, &source_snapshot, &pending).await;
        let applied = application
            .apply(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("apply.drain.target.consume.{}", suffix()))
                    .unwrap(),
                &selector(&target),
                apply_command(&target, &apply_key),
            )
            .await
            .unwrap();
        assert!(!applied.exact_replay());
        assert_eq!(applied.status(), ProductStatusV1::RuntimePending);
        let result_deployment_id = applied
            .exact_deployment()
            .deployment_reference()
            .to_string();
        let replayed = application
            .apply(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!("apply.drain.target.replay.{}", suffix()))
                    .unwrap(),
                &selector(&target),
                apply_command(&target, &apply_key),
            )
            .await
            .unwrap();
        assert!(replayed.exact_replay());
        assert_eq!(replayed.status(), applied.status());
        assert_eq!(replayed.exact_deployment(), applied.exact_deployment());
        let durable = sqlx::query_as::<
            _,
            (
                String,
                i64,
                String,
                i64,
                i64,
                Option<String>,
                String,
                i64,
                String,
                String,
                i64,
            ),
        >(
            "SELECT source.phase, source.revision, drain.intent_state, \
              drain.intent_revision, fence.writer_epoch, fence.pending_drain_intent_id, \
              action.terminal_kind, action.source_slot_writer_epoch, \
              action.result_deployment_id, result.phase, activation.product_revision \
             FROM public.runtime_deployments AS source \
             INNER JOIN public.runtime_drain_intents_v2 AS drain \
               ON drain.drain_intent_id = $2 \
             INNER JOIN public.runtime_slot_writer_fences_v2 AS fence \
               ON fence.slot_guild_id = drain.slot_guild_id \
               AND fence.slot_ruleset_key = drain.slot_ruleset_key \
             INNER JOIN public.runtime_product_drain_terminal_actions_v2 AS action \
               ON action.drain_intent_id = drain.drain_intent_id \
             INNER JOIN public.runtime_deployments AS result \
               ON result.deployment_id = action.result_deployment_id \
             INNER JOIN public.activation_requests AS activation \
               ON activation.id = result.activation_request_id \
             WHERE source.deployment_id = $1",
        )
        .bind(source_snapshot.identity.deployment_id.as_str())
        .bind(&acknowledged.drain_intent_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(durable.0, "superseded");
        assert_eq!(
            durable.1,
            i64::try_from(source_snapshot.revision.get()).unwrap() + 1
        );
        assert_eq!(durable.2, "consumed");
        assert_eq!(durable.3, acknowledged.intent_revision + 1);
        assert_eq!(durable.4, pending_epoch + 1);
        assert!(durable.5.is_none());
        assert_eq!(durable.6, "consumed");
        assert_eq!(durable.7, pending_epoch);
        assert_eq!(durable.8, result_deployment_id);
        assert_eq!(durable.9, "requested");
        assert_eq!(durable.10, 4);
        let terminal_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_product_drain_terminal_actions_v2 \
             WHERE drain_intent_id = $1 AND product_operation_id = $2",
        )
        .bind(&acknowledged.drain_intent_id)
        .bind(&acknowledged.product_operation_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(terminal_count, 1);
        let source_result = sqlx::query_scalar::<_, String>(
            "SELECT snapshot #>> '{phase,phase}' \
             FROM public.runtime_deployments WHERE deployment_id = $1",
        )
        .bind(source_snapshot.identity.deployment_id.as_str())
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(source_result, "superseded");
        assert_eq!(
            acknowledged.state_digest,
            runtime_drain_digest_bytes(&acknowledged.state_bytes)
        );
        assert!(acknowledged.acknowledged_at <= Utc::now());
        let progressed = advance_application_source_to_awaiting_gateway_ready(
            pool,
            &target,
            applied.exact_deployment(),
        )
        .await;
        assert_eq!(
            progressed.phase.kind(),
            automation_runtime_convergence::RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady
        );
        let progressed_replay = application
            .apply(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!(
                    "apply.drain.target.progressed-replay.{}",
                    suffix()
                ))
                .unwrap(),
                &selector(&target),
                apply_command(&target, &apply_key),
            )
            .await
            .unwrap();
        assert!(progressed_replay.exact_replay());
        assert_eq!(
            progressed_replay.exact_deployment(),
            applied.exact_deployment()
        );
        let replay_snapshots = sqlx::query_as::<_, (String, String)>(
            "SELECT current.snapshot #>> '{phase,phase}', \
              pg_catalog.convert_from(action.result_deployment_snapshot_bytes, 'UTF8')::JSONB \
                #>> '{phase,phase}' \
             FROM public.runtime_product_drain_terminal_actions_v2 AS action \
             INNER JOIN public.runtime_deployments AS current \
               ON current.deployment_id = action.result_deployment_id \
             WHERE action.drain_intent_id = $1",
        )
        .bind(&acknowledged.drain_intent_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(replay_snapshots.0, "awaiting_gateway_ready");
        assert_eq!(replay_snapshots.1, "requested");
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
        let corrupt_replay = application
            .apply(
                &target.credential,
                &target.csrf,
                &ProductRequestIdV1::parse(&format!(
                    "apply.drain.target.corrupt-replay.{}",
                    suffix()
                ))
                .unwrap(),
                &selector(&target),
                apply_command(&target, &apply_key),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            corrupt_replay,
            ProductApplicationError::Control(ProductControlPortError::Backend(_))
        ));
        let unchanged = sqlx::query_as::<_, (i64, i64)>(
            "SELECT drain.intent_revision, activation.product_revision \
             FROM public.runtime_drain_intents_v2 AS drain \
             INNER JOIN public.runtime_deployments AS result \
               ON result.deployment_id = $2 \
             INNER JOIN public.activation_requests AS activation \
               ON activation.id = result.activation_request_id \
             WHERE drain.drain_intent_id = $1",
        )
        .bind(&acknowledged.drain_intent_id)
        .bind(&result_deployment_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(unchanged.0, acknowledged.intent_revision + 1);
        assert_eq!(unchanged.1, 4);
    }
    drop_isolated_product_control_database(database).await;
}
