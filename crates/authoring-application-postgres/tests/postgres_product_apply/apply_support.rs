async fn lock_apply(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    operation: &Operation,
    call: &Call,
) -> Result<LockRow, sqlx::Error> {
    let context = ApplyLockContext::single(fixture, operation);
    lock_apply_with_context(transaction, fixture, operation, call, &context).await
}

async fn lock_apply_with_context(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    operation: &Operation,
    call: &Call,
    context: &ApplyLockContext,
) -> Result<LockRow, sqlx::Error> {
    sqlx::query_as::<_, LockRow>(
        "SELECT outcome, exact_replay, requires_commit, resulting_revision, resulting_state, \
         deployment_id, desired_target_digest, locked_projection \
         FROM public.starring_product_apply_lock_v1(\
          $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
          $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30)",
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
    .fetch_one(&mut **transaction)
    .await
}

#[derive(Deserialize)]
struct LockedApplyProjectionV1 {
    requested_at: DateTime<Utc>,
    runtime_generation: RuntimeGeneration,
    previous_runtime: Option<RuntimeProcessIdentityV1>,
    operation: LockedApplyOperationV1,
    server: LockedApplyServerV1,
}

#[derive(Deserialize)]
struct LockedApplyOperationV1 {
    deployment_id: DeploymentId,
}

#[derive(Deserialize)]
struct LockedApplyServerV1 {
    scope: LockedApplyScopeV1,
    activation: LockedApplyActivationV1,
    authority: LockedApplyAuthorityV1,
    target: LockedApplyTargetV1,
}

#[derive(Deserialize)]
struct LockedApplyScopeV1 {
    tenant_id: TenantId,
    installation_id: InstallationId,
    promotion_id: PromotionId,
}

#[derive(Deserialize)]
struct LockedApplyActivationV1 {
    request_id: ActivationRequestId,
}

#[derive(Deserialize)]
struct LockedApplyAuthorityV1 {
    revision: u64,
    binding_revision: BindingRevision,
    binding_fingerprint: ResourceBindingFingerprint,
}

#[derive(Deserialize)]
struct LockedApplyTargetV1 {
    guild_id: GuildId,
    ruleset_key: RuleSetKey,
    version: RuleSetVersionId,
    content_hash: RuleSetContentHash,
}

fn prepare_requested_deployment(lock: &LockRow) -> PreparedRequestedDeploymentV1 {
    let projection: LockedApplyProjectionV1 = serde_json::from_value(
        lock.locked_projection
            .as_ref()
            .expect("fresh lock projection")
            .0
            .clone(),
    )
    .expect("locked apply projection must decode");
    prepare_requested_deployment_v1(
        EnqueueDeploymentV1 {
            identity: RuntimeDeploymentIdentityV1 {
                deployment_id: projection.operation.deployment_id,
                tenant_id: projection.server.scope.tenant_id,
                installation_id: projection.server.scope.installation_id,
                promotion_id: projection.server.scope.promotion_id,
                activation_request_id: projection.server.activation.request_id,
            },
            target: RuntimeDeploymentTargetV1 {
                guild_id: projection.server.target.guild_id,
                ruleset_key: projection.server.target.ruleset_key,
                version: projection.server.target.version,
                content_hash: projection.server.target.content_hash,
                binding_revision: projection.server.authority.binding_revision,
                binding_fingerprint: projection.server.authority.binding_fingerprint,
            },
            runtime_generation: projection.runtime_generation,
            previous_runtime: projection.previous_runtime,
            installation_authority_revision: projection.server.authority.revision,
        },
        projection.requested_at,
    )
    .expect("locked apply projection must prepare")
}

fn deployment_scope(prepared: &PreparedRequestedDeploymentV1) -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: prepared.snapshot().identity.tenant_id.clone(),
        installation_id: prepared.snapshot().identity.installation_id.clone(),
        deployment_id: prepared.snapshot().identity.deployment_id.clone(),
    }
}

fn one_bit_wrong_digest(value: &str) -> String {
    let mut bytes = value.as_bytes().to_vec();
    let index = bytes.len() - 1;
    bytes[index] = match bytes[index] {
        b'0' => b'1',
        b'1' => b'0',
        b'2' => b'3',
        b'3' => b'2',
        b'4' => b'5',
        b'5' => b'4',
        b'6' => b'7',
        b'7' => b'6',
        b'8' => b'9',
        b'9' => b'8',
        b'a' => b'b',
        b'b' => b'a',
        b'c' => b'd',
        b'd' => b'c',
        b'e' => b'f',
        b'f' => b'e',
        _ => panic!("digest must be lowercase hexadecimal"),
    };
    String::from_utf8(bytes).unwrap()
}

async fn assert_runtime_mutation_clock_cleared(transaction: &mut Transaction<'_, Postgres>) {
    let configured = sqlx::query_scalar::<_, Option<String>>(
        "SELECT NULLIF(\
         pg_catalog.current_setting('starring.runtime_mutation_clock', TRUE), '')",
    )
    .fetch_one(&mut **transaction)
    .await
    .unwrap();
    assert!(configured.is_none());
    sqlx::query("SAVEPOINT assert_runtime_mutation_clock_cleared")
        .execute(&mut **transaction)
        .await
        .unwrap();
    let error = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT public.starring_runtime_current_mutation_clock()",
    )
    .fetch_one(&mut **transaction)
    .await
    .expect_err("cleared runtime mutation clock cannot be reused");
    assert!(matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
    ));
    sqlx::query("ROLLBACK TO SAVEPOINT assert_runtime_mutation_clock_cleared")
        .execute(&mut **transaction)
        .await
        .unwrap();
    sqlx::query("RELEASE SAVEPOINT assert_runtime_mutation_clock_cleared")
        .execute(&mut **transaction)
        .await
        .unwrap();
}

struct FinalizeProjection<'a> {
    desired_target_digest: &'a str,
    previous_runtime: Option<&'a Value>,
    snapshot: &'a Value,
    notices: &'a Value,
}

fn finalize_projection<'a>(
    desired_target_digest: &'a str,
    previous_runtime: Option<&'a Value>,
    snapshot: &'a Value,
) -> FinalizeProjection<'a> {
    static EMPTY_NOTICES: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| json!([]));
    FinalizeProjection {
        desired_target_digest,
        previous_runtime,
        snapshot,
        notices: &EMPTY_NOTICES,
    }
}

async fn finalize_apply(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    operation: &Operation,
    call: &Call,
    lock: &LockRow,
    projection_input: FinalizeProjection<'_>,
) -> Result<FinalizeRow, sqlx::Error> {
    let FinalizeProjection {
        desired_target_digest,
        previous_runtime,
        snapshot,
        notices,
    } = projection_input;
    let projection = lock
        .locked_projection
        .as_ref()
        .expect("fresh lock projection");
    let previous_runtime = previous_runtime.cloned().unwrap_or(Value::Null);
    sqlx::query_as::<_, FinalizeRow>(
        "SELECT outcome, resulting_revision, resulting_state, exact_replay, guild_id, \
         deployment_id, desired_target_digest \
         FROM public.starring_product_apply_finalize_v1(\
          $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
          $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, \
          $31, $32, $33, $34, $35)",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.promotion_id)
    .bind(call.expected_revision)
    .bind(&fixture.payload_digest)
    .bind(&fixture.actor.principal_id)
    .bind(&call.session_digest)
    .bind(&fixture.actor.session_subject)
    .bind(&fixture.actor.user_id)
    .bind(&fixture.application_id)
    .bind(&fixture.guild_id)
    .bind(&call.capability)
    .bind(1_i64)
    .bind(&fixture.authority_digest)
    .bind(&fixture.observation_digest)
    .bind(call.observed_at)
    .bind(call.expires_at)
    .bind(&call.effective_permissions)
    .bind(call.guild_owner)
    .bind(&operation.request_id)
    .bind(&operation.idempotency_digest)
    .bind(vec![operation.idempotency_digest.clone()])
    .bind(vec![operation.key_id.clone()])
    .bind(vec![operation.key_fingerprint.clone()])
    .bind(&operation.key_id)
    .bind(&operation.semantic_digest)
    .bind(&operation.receipt_id)
    .bind(&operation.audit_event_id)
    .bind(&operation.apply_attempt_id)
    .bind(&operation.deployment_id)
    .bind(projection)
    .bind(desired_target_digest)
    .bind(Json(previous_runtime))
    .bind(Json(snapshot))
    .bind(Json(notices))
    .fetch_one(&mut **transaction)
    .await
}

async fn assert_apply_unmutated(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    operation: &Operation,
) {
    let unchanged = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
          (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations \
           WHERE guild_id = $2 AND ruleset_key = $3), \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $4), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $4), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
           WHERE receipt_id = $4) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&operation.receipt_id)
    .fetch_one(&mut **transaction)
    .await
    .unwrap();
    assert_eq!(unchanged, ("approved".to_string(), 2, 0, 0, 0, 0, 0));
}

async fn complete_apply(
    pool: &PgPool,
    fixture: &Fixture,
    operation: &Operation,
) -> PreparedRequestedDeploymentV1 {
    let call = Call::valid(fixture);
    let mut transaction = begin_serializable(pool).await;
    let lock = lock_apply(&mut transaction, fixture, operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    let finalized = finalize_apply(
        &mut transaction,
        fixture,
        operation,
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
    transaction.commit().await.unwrap();
    prepared
}

fn is_serialization_failure(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("40001")
    )
}
