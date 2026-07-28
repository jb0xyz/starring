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
    let semantic_digest = digest(&format!(
        "product-apply-epoch-semantic:{}",
        snapshot.identity.deployment_id.as_str()
    ));
    product_apply_drain_for_exact_root(
        snapshot,
        &operation_id[..32],
        &intent_id[..32],
        &semantic_digest,
    )
}

fn product_apply_drain_for_exact_root(
    snapshot: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
    operation_id: &str,
    intent_id: &str,
    semantic_digest: &str,
) -> automation_runtime_controller::RuntimeCanonicalProductDrainV2 {
    let preimage = automation_runtime_controller::RuntimeProductMutationPreimageV2 {
        operation_id: automation_runtime_controller::RuntimeProductOperationIdV2::parse(
            operation_id,
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
            automation_runtime_controller::RuntimeProductSemanticRequestDigestV2::parse(
                semantic_digest,
            )
            .unwrap(),
    };
    automation_runtime_controller::RuntimeCanonicalProductDrainV2::new(
        preimage,
        automation_runtime_controller::RuntimeDrainIntentIdV2::parse(intent_id).unwrap(),
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

fn digest_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

fn product_apply_consume_guard(
    deployment: &RuntimeDeployment,
    controller_id: &ControllerId,
    fencing_token: FencingToken,
    now: DateTime<Utc>,
) -> CommandGuardV1 {
    CommandGuardV1 {
        expected_revision: deployment.revision(),
        controller_id: controller_id.clone(),
        fencing_token,
        runtime_generation: deployment.runtime_generation(),
        now,
    }
}

fn product_apply_consume_awaiting_snapshot(
    prepared: &PreparedRequestedDeploymentV1,
) -> (
    automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
    String,
) {
    let mut deployment = RuntimeDeployment::restore(prepared.snapshot().clone()).unwrap();
    let requested_at = deployment.snapshot().requested_at;
    let controller_id = ControllerId::parse("product-apply-consume-controller").unwrap();
    let fencing_token = FencingToken::new(1).unwrap();
    let process_instance_id = ProcessInstanceId::parse("product-apply-consume-process").unwrap();
    let at = |milliseconds| requested_at + TimeDelta::milliseconds(milliseconds);
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token,
            now: at(1),
            expires_at: at(60_000),
        })
        .unwrap();
    let target = deployment.target().clone();
    let runtime_generation = deployment.runtime_generation();
    let previous_runtime = deployment.snapshot().previous_runtime;
    deployment
        .accept_preflight(
            &product_apply_consume_guard(&deployment, &controller_id, fencing_token, at(2)),
            PreflightAttestationV1 {
                target: target.clone(),
                runtime_generation,
                observed_runtime: previous_runtime.clone(),
                checked_at: at(2),
            },
        )
        .unwrap();
    deployment
        .request_drain(&product_apply_consume_guard(
            &deployment,
            &controller_id,
            fencing_token,
            at(3),
        ))
        .unwrap();
    deployment
        .accept_drain(
            &product_apply_consume_guard(&deployment, &controller_id, fencing_token, at(4)),
            DrainAttestationV1 {
                previous_runtime,
                target_runtime_generation: runtime_generation,
                drained_at: at(4),
            },
        )
        .unwrap();
    deployment
        .begin_activation(&product_apply_consume_guard(
            &deployment,
            &controller_id,
            fencing_token,
            at(5),
        ))
        .unwrap();
    deployment
        .accept_activation(
            &product_apply_consume_guard(&deployment, &controller_id, fencing_token, at(6)),
            ActivationAttestationV1 {
                activation_request_id: deployment.identity().activation_request_id.clone(),
                target: target.clone(),
                runtime_generation,
                kind: ActivationOutcomeKindV1::Activated,
                activated_at: at(6),
            },
        )
        .unwrap();
    deployment
        .begin_panel_reconciliation(&product_apply_consume_guard(
            &deployment,
            &controller_id,
            fencing_token,
            at(7),
        ))
        .unwrap();
    deployment
        .accept_panel_certificate(
            &product_apply_consume_guard(&deployment, &controller_id, fencing_token, at(8)),
            PanelCertificateV1 {
                certificate_id: PanelCertificateId::parse("product-apply-consume-panel").unwrap(),
                report_digest: PanelReportDigestV1::parse(digest(
                    "product-apply-consume-panel-report",
                ))
                .unwrap(),
                target,
                runtime_generation,
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
                reconciled_at: at(8),
            },
        )
        .unwrap();
    let mut snapshot = deployment.snapshot();
    snapshot.controller_lease = None;
    RuntimeDeployment::restore(snapshot.clone()).unwrap();
    (snapshot, controller_id.as_str().to_string())
}

async fn persist_product_apply_consume_source(
    pool: &PgPool,
    snapshot: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
    last_controller_id: &str,
) {
    let panel_time = snapshot.panel_certificate.as_ref().unwrap().reconciled_at;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let changed = sqlx::query(
        "UPDATE public.runtime_deployments \
         SET snapshot = $1, revision = $2, phase = 'awaiting_gateway_ready', \
             controller_id = NULL, controller_fencing_token = NULL, \
             controller_acquired_at = NULL, controller_lease_expires_at = NULL, \
             last_fencing_token = $3, last_controller_id = $4, \
             next_retry_at = NULL, last_stable_error_code = NULL, \
             live_attestation_id = NULL, live_at = NULL, blocked_at = NULL, \
             superseded_at = NULL, cancelled_at = NULL, \
             convergence_attempt_no = 1, last_failure_attempt_no = NULL, \
             updated_at = $5 \
         WHERE tenant_id = $6 AND installation_id = $7 AND deployment_id = $8",
    )
    .bind(Json(serde_json::to_value(snapshot).unwrap()))
    .bind(i64::try_from(snapshot.revision.get()).unwrap())
    .bind(i64::try_from(snapshot.last_fencing_token.unwrap().get()).unwrap())
    .bind(last_controller_id)
    .bind(panel_time)
    .bind(snapshot.identity.tenant_id.as_str())
    .bind(snapshot.identity.installation_id.as_str())
    .bind(snapshot.identity.deployment_id.as_str())
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

struct ProductApplyConsumeAcknowledgement {
    intent_revision: i64,
    state_bytes: Vec<u8>,
    state_digest: String,
    acknowledged_at: DateTime<Utc>,
}

fn product_apply_consume_acknowledgement(
    snapshot: &automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
    canonical: &automation_runtime_controller::RuntimeCanonicalProductDrainV2,
) -> ProductApplyConsumeAcknowledgement {
    let operation = automation_runtime_controller::RuntimeProductDrainOperationV2::new(
        snapshot,
        canonical.clone(),
    )
    .unwrap();
    let root = automation_runtime_controller::RuntimePersistedProductDrainRootV2::from_persisted(
        operation.product_operation_scope().scope().clone(),
        operation.product_operation_scope().expected_revision(),
        operation.product_operation_id(),
        operation.drain_intent_scope().scope().clone(),
        operation.drain_intent_scope().slot().clone(),
        operation.drain_intent_scope().expected_revision(),
        operation.drain_intent_id(),
        &operation.canonical().product_preimage().expected_target,
        operation.product_mutation_request_bytes(),
        operation.product_mutation_digest(),
        operation.drain_intent_request_bytes(),
        operation.drain_intent_digest(),
    )
    .unwrap();
    let process_instance_id = ProcessInstanceId::parse("product-apply-consume-process").unwrap();
    let key = &canonical.drain_preimage().key;
    let acknowledged_at =
        snapshot.panel_certificate.as_ref().unwrap().reconciled_at + TimeDelta::milliseconds(10);
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
            lease_epoch: NonZeroU64::new(1).unwrap(),
            expected_build_revision: automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                "product-apply-consume-build",
            )
            .unwrap(),
        },
        NonZeroU64::new(2).unwrap(),
        process_instance_id,
        ControllerId::parse("product-apply-consume-controller").unwrap(),
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
                "1234567890abcdef1234567890abcdef",
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
    let intent = automation_runtime_controller::RuntimeDrainIntentV2::route_absent_acknowledged_from_persisted(
        &root,
        intent_revision,
        acknowledgement,
    )
    .unwrap();
    let state =
        automation_runtime_controller::RuntimeCanonicalDrainIntentStateV2::from_intent(intent)
            .unwrap();
    ProductApplyConsumeAcknowledgement {
        intent_revision: i64::try_from(intent_revision.get()).unwrap(),
        state_bytes: state.state_bytes().to_vec(),
        state_digest: digest_bytes(state.state_bytes()),
        acknowledged_at,
    }
}

async fn persist_product_apply_consume_acknowledgement(
    pool: &PgPool,
    intent_id: &str,
    acknowledgement: &ProductApplyConsumeAcknowledgement,
) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let changed = sqlx::query(
        "UPDATE public.runtime_drain_intents_v2 \
         SET intent_revision = $2, intent_state = 'route_absent_acknowledged', \
             canonical_state_bytes = $3, canonical_state_digest = $4 \
         WHERE drain_intent_id = $1 AND intent_state = 'pending'",
    )
    .bind(intent_id)
    .bind(acknowledgement.intent_revision)
    .bind(&acknowledgement.state_bytes)
    .bind(&acknowledgement.state_digest)
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

#[derive(Debug, sqlx::FromRow)]
struct ProductApplyConsumeRuntimeDrainRow {
    outcome_name: String,
    preparation_ready: bool,
    exact_replay: bool,
    requires_commit: bool,
    preparation_token: Option<String>,
    locked_product_projection: Option<Json<Value>>,
    source_deployment_snapshot: Option<Json<Value>>,
    source_acknowledged_at: Option<DateTime<Utc>>,
    source_deployment_revision: Option<i64>,
    source_result_deployment_revision: Option<i64>,
    source_result_deployment_snapshot: Option<Json<Value>>,
    source_result_deployment_snapshot_digest: Option<String>,
    result_deployment_id: Option<String>,
    result_deployment_revision: Option<i64>,
    result_deployment_snapshot: Option<Json<Value>>,
    result_deployment_snapshot_digest: Option<String>,
    product_resulting_revision: Option<i64>,
    product_resulting_state: Option<String>,
    result_intent_revision: Option<i64>,
    result_intent_state: Option<String>,
    source_slot_epoch: Option<i64>,
    successor_slot_epoch: Option<i64>,
    terminal_action_id: Option<String>,
    terminal_database_time: Option<DateTime<Utc>>,
}

struct ProductApplyConsumeCommitProjection {
    preparation_token: String,
    source_result_snapshot_bytes: Vec<u8>,
    source_result_snapshot_digest: String,
    result_deployment_snapshot_bytes: Vec<u8>,
    result_deployment_snapshot_digest: String,
    desired_target_digest: String,
    activation_notices_bytes: Vec<u8>,
}

struct ProductApplyConsumeCall<'a> {
    source_deployment_id: &'a str,
    source_deployment_revision: i64,
    product_operation_id: &'a str,
    drain_intent_id: &'a str,
    terminal_action_id: &'a str,
    acknowledgement: &'a ProductApplyConsumeAcknowledgement,
}

async fn consume_product_apply_runtime_drain_in(
    transaction: &mut Transaction<'_, Postgres>,
    phase: &str,
    fixture: &Fixture,
    operation: &Operation,
    call: &Call,
    consume: &ProductApplyConsumeCall<'_>,
    projection: Option<&ProductApplyConsumeCommitProjection>,
) -> Result<ProductApplyConsumeRuntimeDrainRow, sqlx::Error> {
    let context = ApplyLockContext::single(fixture, operation);
    let empty = Vec::<u8>::new();
    let preparation_token = projection
        .map(|value| value.preparation_token.as_str())
        .unwrap_or("");
    let source_result_snapshot_bytes = projection
        .map(|value| value.source_result_snapshot_bytes.as_slice())
        .unwrap_or(empty.as_slice());
    let source_result_snapshot_digest = projection
        .map(|value| value.source_result_snapshot_digest.as_str())
        .unwrap_or("");
    let result_deployment_snapshot_bytes = projection
        .map(|value| value.result_deployment_snapshot_bytes.as_slice())
        .unwrap_or(empty.as_slice());
    let result_deployment_snapshot_digest = projection
        .map(|value| value.result_deployment_snapshot_digest.as_str())
        .unwrap_or("");
    let desired_target_digest = projection
        .map(|value| value.desired_target_digest.as_str())
        .unwrap_or("");
    let activation_notices_bytes = projection
        .map(|value| value.activation_notices_bytes.as_slice())
        .unwrap_or(empty.as_slice());
    sqlx::query_as::<_, ProductApplyConsumeRuntimeDrainRow>(
        "SELECT outcome_name, preparation_ready, exact_replay, requires_commit, \
            preparation_token, locked_product_projection, source_deployment_snapshot, \
            source_acknowledged_at, source_deployment_revision, \
            source_result_deployment_revision, source_result_deployment_snapshot, \
            source_result_deployment_snapshot_digest, result_deployment_id, \
            result_deployment_revision, result_deployment_snapshot, \
            result_deployment_snapshot_digest, product_resulting_revision, \
            product_resulting_state, result_intent_revision, result_intent_state, \
            source_slot_epoch, successor_slot_epoch, terminal_action_id, \
            terminal_database_time \
         FROM public.starring_product_apply_consume_runtime_drain_v2(\
            requested_phase => $1, expected_tenant_id => $2, \
            expected_installation_id => $3, expected_promotion_id => $4, \
            expected_product_revision => $5, expected_payload_digest => $6, \
            expected_principal_id => $7, expected_product_session_digest => $8, \
            session_subject_digest => $9, expected_acting_user_id => $10, \
            expected_discord_application_id => $11, expected_guild_id => $12, \
            expected_capability => $13, expected_authority_revision => $14, \
            expected_authority_payload_digest => $15, \
            expected_authority_observation_digest => $16, \
            expected_authority_observed_at => $17, \
            expected_authority_expires_at => $18, \
            expected_effective_permission_bits => $19, expected_guild_owner => $20, \
            product_request_id => $21, active_idempotency_key_digest => $22, \
            idempotency_key_digest_candidates => $23, \
            idempotency_digest_key_id_candidates => $24, \
            idempotency_digest_key_fingerprint_candidates => $25, \
            idempotency_digest_key_id => $26, semantic_request_digest => $27, \
            new_receipt_id => $28, new_audit_event_id => $29, \
            new_apply_attempt_id => $30, new_deployment_id => $31, \
            expected_drain_intent_id => $32, expected_source_intent_revision => $33, \
            expected_source_state_bytes => $34, expected_source_state_digest => $35, \
            expected_product_operation_id => $36, expected_source_deployment_id => $37, \
            expected_source_deployment_revision => $38, \
            proposed_terminal_action_id => $39, expected_preparation_token => $40, \
            prepared_source_result_snapshot_bytes => $41, \
            prepared_source_result_snapshot_digest => $42, \
            prepared_result_deployment_snapshot_bytes => $43, \
            prepared_result_deployment_snapshot_digest => $44, \
            prepared_desired_target_digest => $45, \
            prepared_activation_notices_bytes => $46)",
    )
    .bind(phase)
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
    .bind(consume.drain_intent_id)
    .bind(consume.acknowledgement.intent_revision)
    .bind(&consume.acknowledgement.state_bytes)
    .bind(&consume.acknowledgement.state_digest)
    .bind(consume.product_operation_id)
    .bind(consume.source_deployment_id)
    .bind(consume.source_deployment_revision)
    .bind(consume.terminal_action_id)
    .bind(preparation_token)
    .bind(source_result_snapshot_bytes)
    .bind(source_result_snapshot_digest)
    .bind(result_deployment_snapshot_bytes)
    .bind(result_deployment_snapshot_digest)
    .bind(desired_target_digest)
    .bind(activation_notices_bytes)
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
async fn product_apply_consumes_acknowledged_runtime_drain_and_replays_exactly() {
    let database = isolated_database("apply_consume").await;
    let outcome = async {
        MIGRATOR.run(&database.pool).await?;
        let fixture = seed_fixture(&database.pool).await;
        let source_operation = Operation::new("consume-source");
        let source_prepared = complete_apply(&database.pool, &fixture, &source_operation).await;
        let (source_snapshot, last_controller_id) =
            product_apply_consume_awaiting_snapshot(&source_prepared);
        persist_product_apply_consume_source(
            &database.pool,
            &source_snapshot,
            &last_controller_id,
        )
        .await;
        let fixture =
            seed_competing_product_activation(&database.pool, &fixture, "consume-result").await;

        let operation = Operation::new("consume-result");
        let call = Call::valid(&fixture);
        let product_operation_id = &digest("consume-product-operation")[..32];
        let drain_intent_id = &digest("consume-drain-intent")[..32];
        let terminal_action_id = digest("consume-terminal-action");
        let mut create = begin_serializable(&database.pool).await;
        let classified = lock_apply(
            &mut create,
            &fixture,
            &operation,
            &call,
        )
        .await?;
        assert_eq!(classified.outcome, "runtime_drain_required");
        let absent = begin_product_apply_runtime_drain_in(
            &mut create,
            &fixture,
            &operation,
            &call,
            "",
            "",
        )
        .await?;
        assert_eq!(absent.outcome, "absent");
        let inserted = begin_product_apply_runtime_drain_in(
            &mut create,
            &fixture,
            &operation,
            &call,
            product_operation_id,
            drain_intent_id,
        )
        .await?;
        assert_eq!(inserted.outcome, "inserted");
        assert_eq!(
            inserted.pending_expected_revision,
            Some(i64::try_from(source_snapshot.revision.get()).unwrap())
        );
        create.commit().await?;

        let canonical = product_apply_drain_for_exact_root(
            &source_snapshot,
            product_operation_id,
            drain_intent_id,
            &operation.semantic_digest,
        );
        let acknowledgement =
            product_apply_consume_acknowledgement(&source_snapshot, &canonical);
        persist_product_apply_consume_acknowledgement(
            &database.pool,
            drain_intent_id,
            &acknowledgement,
        )
        .await;
        let consume = ProductApplyConsumeCall {
            source_deployment_id: &source_operation.deployment_id,
            source_deployment_revision: i64::try_from(source_snapshot.revision.get()).unwrap(),
            product_operation_id,
            drain_intent_id,
            terminal_action_id: &terminal_action_id,
            acknowledgement: &acknowledgement,
        };

        let mut transaction = begin_serializable(&database.pool).await;
        let prepared = consume_product_apply_runtime_drain_in(
            &mut transaction,
            "prepare",
            &fixture,
            &operation,
            &call,
            &consume,
            None,
        )
        .await?;
        assert_eq!(prepared.outcome_name, "drain_pending");
        assert!(prepared.preparation_ready);
        assert!(!prepared.exact_replay);
        assert!(prepared.requires_commit);
        assert_eq!(
            prepared.source_acknowledged_at,
            Some(acknowledgement.acknowledged_at)
        );
        assert_eq!(
            prepared.source_deployment_revision,
            Some(i64::try_from(source_snapshot.revision.get()).unwrap())
        );
        let preparation_token = prepared.preparation_token.clone().unwrap();
        let terminal_time = prepared.terminal_database_time.unwrap();
        assert!(terminal_time >= acknowledgement.acknowledged_at);
        let lock = LockRow {
            outcome: "ready".to_string(),
            exact_replay: false,
            requires_commit: false,
            resulting_revision: None,
            resulting_state: None,
            deployment_id: None,
            desired_target_digest: None,
            locked_projection: prepared.locked_product_projection.clone(),
        };
        let result_prepared = prepare_requested_deployment(&lock);
        let mut source_deployment = RuntimeDeployment::restore(
            prepared
                .source_deployment_snapshot
                .as_ref()
                .map(|snapshot| {
                    serde_json::from_value(snapshot.0.clone()).unwrap()
                })
                .unwrap(),
        )
        .unwrap();
        let permit =
            ProductDrainSourceSupersessionPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
                &source_deployment,
                source_deployment.revision(),
                acknowledgement.acknowledged_at,
            )
            .unwrap();
        source_deployment
            .supersede_product_drain_source(
                permit,
                SupersedingDeploymentV1 {
                    identity: result_prepared.snapshot().identity.clone(),
                    target: result_prepared.snapshot().target.clone(),
                    runtime_generation: result_prepared.snapshot().runtime_generation,
                },
                "correlated Product apply".to_string(),
                terminal_time,
            )
            .unwrap();
        let source_result_snapshot_bytes =
            serde_json::to_vec(&source_deployment.snapshot()).unwrap();
        let result_deployment_snapshot_bytes =
            serde_json::to_vec(result_prepared.snapshot()).unwrap();
        let commit_projection = ProductApplyConsumeCommitProjection {
            preparation_token,
            source_result_snapshot_digest: digest_bytes(&source_result_snapshot_bytes),
            source_result_snapshot_bytes,
            result_deployment_snapshot_digest: digest_bytes(
                &result_deployment_snapshot_bytes,
            ),
            result_deployment_snapshot_bytes,
            desired_target_digest: result_prepared.desired_target_digest().to_string(),
            activation_notices_bytes: b"[]".to_vec(),
        };
        let expected_source_result_snapshot =
            serde_json::from_slice::<Value>(&commit_projection.source_result_snapshot_bytes)
                .unwrap();
        let expected_result_snapshot =
            serde_json::from_slice::<Value>(&commit_projection.result_deployment_snapshot_bytes)
                .unwrap();
        let applied = consume_product_apply_runtime_drain_in(
            &mut transaction,
            "commit",
            &fixture,
            &operation,
            &call,
            &consume,
            Some(&commit_projection),
        )
        .await?;
        assert_eq!(applied.outcome_name, "applied");
        assert!(!applied.preparation_ready);
        assert!(!applied.exact_replay);
        assert!(!applied.requires_commit);
        assert_eq!(applied.product_resulting_revision, Some(4));
        assert_eq!(applied.product_resulting_state.as_deref(), Some("applied"));
        assert_eq!(
            applied.source_result_deployment_revision,
            Some(consume.source_deployment_revision + 1)
        );
        assert_eq!(
            applied
                .source_result_deployment_snapshot
                .as_ref()
                .map(|snapshot| &snapshot.0),
            Some(&expected_source_result_snapshot)
        );
        assert_eq!(
            applied
                .source_result_deployment_snapshot_digest
                .as_deref(),
            Some(commit_projection.source_result_snapshot_digest.as_str())
        );
        assert_eq!(
            applied.result_deployment_id.as_deref(),
            Some(operation.deployment_id.as_str())
        );
        assert_eq!(applied.result_deployment_revision, Some(1));
        assert_eq!(
            applied
                .result_deployment_snapshot
                .as_ref()
                .map(|snapshot| &snapshot.0),
            Some(&expected_result_snapshot)
        );
        assert_eq!(
            applied.result_deployment_snapshot_digest.as_deref(),
            Some(
                commit_projection
                    .result_deployment_snapshot_digest
                    .as_str()
            )
        );
        assert_eq!(applied.result_intent_state.as_deref(), Some("consumed"));
        assert_eq!(
            applied.result_intent_revision,
            Some(acknowledgement.intent_revision + 1)
        );
        assert_eq!(
            applied.successor_slot_epoch,
            applied.source_slot_epoch.map(|epoch| epoch + 1)
        );
        assert_eq!(
            applied.terminal_action_id.as_deref(),
            Some(terminal_action_id.as_str())
        );
        assert_eq!(applied.terminal_database_time, Some(terminal_time));
        transaction.commit().await?;

        let durable = sqlx::query_as::<_, (String, i64, String, i64, i64, Option<String>, i64)>(
            "SELECT source.phase, source.revision, drain.intent_state, \
                    drain.intent_revision, fence.writer_epoch, \
                    fence.pending_drain_intent_id, \
                    (SELECT pg_catalog.count(*) \
                     FROM public.runtime_product_drain_terminal_actions_v2) \
             FROM public.runtime_deployments AS source \
             INNER JOIN public.runtime_drain_intents_v2 AS drain \
                ON drain.drain_intent_id = $2 \
             INNER JOIN public.runtime_slot_writer_fences_v2 AS fence \
                ON fence.slot_guild_id = drain.slot_guild_id \
                AND fence.slot_ruleset_key = drain.slot_ruleset_key \
             WHERE source.deployment_id = $1",
        )
        .bind(&source_operation.deployment_id)
        .bind(drain_intent_id)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(durable.0, "superseded");
        assert_eq!(durable.1, consume.source_deployment_revision + 1);
        assert_eq!(durable.2, "consumed");
        assert_eq!(durable.3, acknowledgement.intent_revision + 1);
        assert_eq!(durable.4, applied.successor_slot_epoch.unwrap());
        assert!(durable.5.is_none());
        assert_eq!(durable.6, 1);

        let mut progress = database.pool.begin().await?;
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *progress)
            .await?;
        let progressed = sqlx::query(
            "UPDATE public.runtime_deployments \
             SET revision = revision + 1, \
                 snapshot = pg_catalog.jsonb_set(\
                    snapshot, '{revision}', \
                    pg_catalog.to_jsonb(revision + 1), FALSE\
                 ) \
             WHERE deployment_id = $1",
        )
        .bind(&operation.deployment_id)
        .execute(&mut *progress)
        .await?;
        assert_eq!(progressed.rows_affected(), 1);
        sqlx::query("SET LOCAL session_replication_role = origin")
            .execute(&mut *progress)
            .await?;
        progress.commit().await?;

        let mut replay_transaction = begin_serializable(&database.pool).await;
        let replayed = consume_product_apply_runtime_drain_in(
            &mut replay_transaction,
            "commit",
            &fixture,
            &operation,
            &call,
            &consume,
            Some(&commit_projection),
        )
        .await?;
        assert_eq!(replayed.outcome_name, "replayed");
        assert!(!replayed.preparation_ready);
        assert!(replayed.exact_replay);
        assert!(!replayed.requires_commit);
        assert_eq!(replayed.product_resulting_revision, Some(4));
        assert_eq!(
            replayed
                .source_result_deployment_snapshot
                .as_ref()
                .map(|snapshot| &snapshot.0),
            Some(&expected_source_result_snapshot)
        );
        assert_eq!(
            replayed
                .source_result_deployment_snapshot_digest
                .as_deref(),
            Some(commit_projection.source_result_snapshot_digest.as_str())
        );
        assert_eq!(
            replayed
                .result_deployment_snapshot
                .as_ref()
                .map(|snapshot| &snapshot.0),
            Some(&expected_result_snapshot)
        );
        assert_eq!(
            replayed.result_deployment_snapshot_digest.as_deref(),
            Some(
                commit_projection
                    .result_deployment_snapshot_digest
                    .as_str()
            )
        );
        assert_eq!(
            replayed.terminal_action_id.as_deref(),
            Some(terminal_action_id.as_str())
        );
        assert_eq!(replayed.terminal_database_time, Some(terminal_time));
        replay_transaction.commit().await?;
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
        let expected_intent_id = canonical
            .drain_preimage()
            .key
            .intent_id
            .as_str()
            .to_string();
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
