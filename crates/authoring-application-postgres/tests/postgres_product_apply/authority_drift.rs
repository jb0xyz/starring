#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn authority_drift_preserves_recorded_replay_and_blocks_fresh_apply() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("authority-drift-replay");
    let prepared = complete_apply(&pool, &fixture, &operation).await;
    let guild_id = fixture.guild_id.parse::<u64>().unwrap();
    let mut next_resource_bindings = ResourceBindingMap::default();
    next_resource_bindings.channel_bindings.insert(
        ResourceKey("community_hub".to_string()),
        ChannelId(guild_id + 1_000_000_000),
    );
    next_resource_bindings.role_bindings.insert(
        ResourceKey("automation_operator".to_string()),
        RoleId(guild_id + 2_000_000_000),
    );
    let next_binding_fingerprint = resource_binding_fingerprint_v2(&next_resource_bindings);
    let stored_next_resource_bindings = json!({
        "role_bindings": &next_resource_bindings.role_bindings,
        "channel_bindings": &next_resource_bindings.channel_bindings
    });
    let next_authority_digest = digest(&format!("authority:v2:{}", fixture.installation_id));
    let mut authority_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 2, $2, 2, $3, $4, 2, 1, 3600, $5, $6, $7)",
    )
    .bind(&fixture.installation_id)
    .bind(&fixture.tenant_id)
    .bind(Json(&stored_next_resource_bindings))
    .bind(next_binding_fingerprint.as_str())
    .bind(&next_authority_digest)
    .bind(&fixture.actor.principal_id)
    .bind(digest(&format!(
        "authority-request:v2:{}",
        fixture.installation_id
    )))
    .execute(&mut *authority_transaction)
    .await
    .unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_installations \
         SET current_authority_revision = 2, updated_at = pg_catalog.clock_timestamp() \
         WHERE installation_id = $1 AND tenant_id = $2 AND current_authority_revision = 1",
    )
    .bind(&fixture.installation_id)
    .bind(&fixture.tenant_id)
    .execute(&mut *authority_transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    authority_transaction.commit().await.unwrap();

    let mut stale_replay_transaction = begin_serializable(&pool).await;
    let stale_replay = lock_apply(
        &mut stale_replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(stale_replay.outcome, "authority_mismatch");
    assert!(!stale_replay.exact_replay);
    assert!(!stale_replay.requires_commit);
    stale_replay_transaction.rollback().await.unwrap();

    let current_authority = AuthorityHead {
        revision: 2,
        digest: next_authority_digest,
    };
    let current_context = apply_context_at_authority(&fixture, &operation, &current_authority);
    let mut replay_transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &current_context,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(
        replay.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    replay_transaction.commit().await.unwrap();

    let fresh_operation = Operation::new("authority-drift-fresh");
    let mut fresh_transaction = begin_serializable(&pool).await;
    let fresh = lock_apply(
        &mut fresh_transaction,
        &fixture,
        &fresh_operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(fresh.outcome, "authority_mismatch");
    assert!(!fresh.exact_replay);
    assert!(!fresh.requires_commit);
    assert!(fresh.locked_projection.is_none());
    fresh_transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn stale_applied_replay_cannot_commit_a_rotated_idempotency_alias() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("stale-replay-alias");
    complete_apply(&pool, &fixture, &operation).await;
    let (bindings, fingerprint) = authority_binding_material(&pool, &fixture).await;
    advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 1,
            resource_bindings: &bindings,
            binding_fingerprint: &fingerprint,
            policy_revision: 2,
            required_approvals: 1,
            activation_ttl_seconds: 3_600,
        },
    )
    .await;
    let rotated_digest = digest(&format!("rotated-alias:{}", operation.request_id));
    let rotated_key_id = TEST_DECISION_KEY_V2_ID.to_string();
    let rotated_key_fingerprint = test_decision_key_fingerprint(97);
    let mut stale_context = ApplyLockContext::single(&fixture, &operation);
    stale_context.active_idempotency_digest = rotated_digest.clone();
    stale_context.idempotency_candidates =
        vec![rotated_digest.clone(), operation.idempotency_digest.clone()];
    stale_context.candidate_key_ids = vec![rotated_key_id.clone(), operation.key_id.clone()];
    stale_context.candidate_key_fingerprints = vec![
        rotated_key_fingerprint.clone(),
        operation.key_fingerprint.clone(),
    ];
    stale_context.active_key_id = rotated_key_id;

    let mut transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &stale_context,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "authority_mismatch");
    assert!(!replay.exact_replay);
    assert!(!replay.requires_commit);
    transaction.commit().await.unwrap();

    let aliases = sqlx::query_as::<_, (i64, i64)>(
        "SELECT pg_catalog.count(*), \
          pg_catalog.count(*) FILTER (WHERE idempotency_key_digest = $2) \
         FROM public.product_action_receipt_idempotency_aliases \
         WHERE receipt_id = $1",
    )
    .bind(&operation.receipt_id)
    .bind(&rotated_digest)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(aliases, (1, 0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn replay_rechecks_discord_freshness_after_the_final_receipt_lock() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("replay-final-freshness");
    complete_apply(&pool, &fixture, &operation).await;
    let rotated_digest = digest(&format!("freshness-alias:{}", operation.request_id));
    let mut context = ApplyLockContext::single(&fixture, &operation);
    context.active_idempotency_digest = rotated_digest.clone();
    context.idempotency_candidates =
        vec![rotated_digest.clone(), operation.idempotency_digest.clone()];
    context.candidate_key_ids = vec![TEST_DECISION_KEY_V2_ID.to_string(), operation.key_id.clone()];
    context.candidate_key_fingerprints = vec![
        test_decision_key_fingerprint(97),
        operation.key_fingerprint.clone(),
    ];
    context.active_key_id = TEST_DECISION_KEY_V2_ID.to_string();
    let observed_at = Utc::now() - TimeDelta::milliseconds(20);
    let call = Call {
        expected_revision: 2,
        capability: "apply".to_string(),
        session_digest: fixture.actor.session_digest.clone(),
        observed_at,
        expires_at: observed_at + TimeDelta::seconds(2),
        effective_permissions: "32".to_string(),
        guild_owner: false,
    };
    let expires_at = call.expires_at;
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT receipt_id FROM public.product_action_receipts \
         WHERE receipt_id = $1 FOR UPDATE",
    )
    .bind(&operation.receipt_id)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    let replay_pool = pool.clone();
    let replay_fixture = fixture.clone();
    let replay_operation = operation.clone();
    let replay = tokio::spawn(async move {
        let mut transaction = begin_serializable(&replay_pool).await;
        let locked = lock_apply_with_context(
            &mut transaction,
            &replay_fixture,
            &replay_operation,
            &call,
            &context,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        locked
    });
    let mut reached_final_lock = false;
    for _ in 0..100 {
        reached_final_lock = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
              SELECT 1 FROM pg_catalog.pg_stat_activity \
              WHERE datname = pg_catalog.current_database() \
               AND pid <> pg_catalog.pg_backend_pid() \
               AND wait_event_type = 'Lock' \
               AND query LIKE '%starring_product_apply_lock_v1%')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if reached_final_lock {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(reached_final_lock);
    let remaining = (expires_at - Utc::now())
        .to_std()
        .unwrap_or(std::time::Duration::ZERO);
    tokio::time::sleep(remaining + std::time::Duration::from_millis(100)).await;
    blocker.commit().await.unwrap();
    let replay = replay.await.unwrap();
    assert_eq!(replay.outcome, "authorization_stale");
    assert!(!replay.exact_replay);
    assert!(!replay.requires_commit);
    let aliases = sqlx::query_as::<_, (i64, i64)>(
        "SELECT pg_catalog.count(*), \
          pg_catalog.count(*) FILTER (WHERE idempotency_key_digest = $2) \
         FROM public.product_action_receipt_idempotency_aliases \
         WHERE receipt_id = $1",
    )
    .bind(&operation.receipt_id)
    .bind(&rotated_digest)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(aliases, (1, 0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn baseline_drift_is_durably_superseded_and_exactly_replayed() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let competing_hash = set_competing_active_baseline(&pool, &fixture).await;
    let operation = Operation::new("superseded-baseline");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(locked.outcome, "superseded");
    assert!(!locked.exact_replay);
    assert!(locked.requires_commit);
    assert_eq!(locked.resulting_revision, Some(4));
    assert_eq!(locked.resulting_state.as_deref(), Some("superseded"));
    assert!(locked.deployment_id.is_none());
    assert!(locked.desired_target_digest.is_none());
    assert!(locked.locked_projection.is_none());
    transaction.commit().await.unwrap();

    let persisted = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&persisted, "superseded_baseline_drift");
    let termination = &persisted.termination.as_ref().unwrap().0;
    assert_eq!(termination["kind"], "superseded");
    assert_eq!(termination["reason"]["reason"], "active_baseline_drift");
    assert_eq!(termination["reason"]["expected"]["state"], "absent");
    assert_eq!(termination["reason"]["observed"]["state"], "exact");
    assert_eq!(termination["reason"]["observed"]["version"], 2);
    assert_eq!(
        termination["reason"]["observed"]["content_hash"],
        competing_hash
    );
    assert_eq!(persisted.audit_authority_revision, Some(1));
    assert_eq!(persisted.audit_policy_revision, Some(1));
    assert_eq!(persisted.audit_baseline_version, Some(2));
    assert_eq!(
        persisted.audit_baseline_hash.as_deref(),
        Some(&*competing_hash)
    );

    let mut replay_transaction = begin_serializable(&pool).await;
    let replay = lock_apply(
        &mut replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "superseded");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(replay.resulting_revision, Some(4));
    assert_eq!(replay.resulting_state.as_deref(), Some("superseded"));
    assert!(replay.deployment_id.is_none());
    assert!(replay.desired_target_digest.is_none());
    assert!(replay.locked_projection.is_none());
    replay_transaction.commit().await.unwrap();

    let replayed = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&replayed, "superseded_baseline_drift");
    assert_eq!(replayed.termination.unwrap().0, termination.clone());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn terminal_replay_rejects_mismatched_baseline_and_clock_evidence() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    set_competing_active_baseline(&pool, &fixture).await;
    let operation = Operation::new("terminal-evidence-tamper");
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "superseded");
    transaction.commit().await.unwrap();
    let original = terminal_persistence(&pool, &fixture, &operation)
        .await
        .termination
        .unwrap()
        .0;

    sqlx::query(
        "UPDATE public.activation_requests \
         SET termination = pg_catalog.jsonb_set(\
          termination, '{reason,expected}', \
          pg_catalog.jsonb_build_object(\
           'state', 'exact', 'version', 1, 'content_hash', target_content_hash)) \
         WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .execute(&pool)
    .await
    .unwrap();
    let mut baseline_transaction = begin_serializable(&pool).await;
    let baseline = lock_apply(
        &mut baseline_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(baseline.outcome, "indeterminate");
    assert!(!baseline.exact_replay);
    baseline_transaction.rollback().await.unwrap();

    sqlx::query("UPDATE public.activation_requests SET termination = $2 WHERE id = $1")
        .bind(&fixture.activation_id)
        .bind(Json(&original))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests AS activation \
         SET termination = pg_catalog.jsonb_set(\
          activation.termination, '{at}', pg_catalog.to_jsonb(receipt.completed_at + INTERVAL '1 second')) \
         FROM public.product_action_receipts AS receipt \
         WHERE activation.id = $1 AND receipt.receipt_id = $2",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .execute(&pool)
    .await
    .unwrap();
    let mut clock_transaction = begin_serializable(&pool).await;
    let clock = lock_apply(
        &mut clock_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(clock.outcome, "indeterminate");
    assert!(!clock.exact_replay);
    clock_transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn binding_drift_uses_current_authority_and_historical_replay_evidence() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let guild_id = fixture.guild_id.parse::<u64>().unwrap();
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        ResourceKey("community_hub".to_string()),
        ChannelId(guild_id + 1_000_000_000),
    );
    bindings.role_bindings.insert(
        ResourceKey("automation_operator".to_string()),
        RoleId(guild_id + 2_000_000_000),
    );
    let fingerprint = resource_binding_fingerprint_v2(&bindings);
    let stored_bindings = json!({
        "role_bindings": &bindings.role_bindings,
        "channel_bindings": &bindings.channel_bindings
    });
    let authority = advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 2,
            resource_bindings: &stored_bindings,
            binding_fingerprint: fingerprint.as_str(),
            policy_revision: 1,
            required_approvals: 1,
            activation_ttl_seconds: 3_600,
        },
    )
    .await;
    let operation = Operation::new("superseded-binding");
    let context = apply_context_at_authority(&fixture, &operation, &authority);
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "superseded");
    assert!(!locked.exact_replay);
    assert!(locked.requires_commit);
    assert_eq!(locked.resulting_revision, Some(4));
    assert_eq!(locked.resulting_state.as_deref(), Some("superseded"));
    assert!(locked.deployment_id.is_none());
    assert!(locked.desired_target_digest.is_none());
    assert!(locked.locked_projection.is_none());
    transaction.commit().await.unwrap();

    let persisted = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&persisted, "superseded_binding_drift");
    let termination = &persisted.termination.as_ref().unwrap().0;
    assert_eq!(termination["reason"]["reason"], "binding_drift");
    assert_eq!(termination["reason"]["expected_revision"], 1);
    assert_eq!(termination["reason"]["observed_revision"], 2);
    assert!(termination["reason"]["observed_fingerprint"].is_null());
    assert_eq!(persisted.audit_authority_revision, Some(2));
    assert_eq!(
        persisted.audit_binding_fingerprint.as_deref(),
        Some(fingerprint.as_str())
    );
    assert_eq!(persisted.audit_policy_revision, Some(1));
    assert!(persisted.audit_baseline_version.is_none());
    assert!(persisted.audit_baseline_hash.is_none());

    let mut stale_replay_transaction = begin_serializable(&pool).await;
    let stale_replay = lock_apply(
        &mut stale_replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(stale_replay.outcome, "authority_mismatch");
    assert!(!stale_replay.exact_replay);
    assert!(!stale_replay.requires_commit);
    stale_replay_transaction.rollback().await.unwrap();

    let mut replay_transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "superseded");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(replay.resulting_revision, Some(4));
    assert_eq!(replay.resulting_state.as_deref(), Some("superseded"));
    assert!(replay.locked_projection.is_none());
    replay_transaction.commit().await.unwrap();

    let replayed = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&replayed, "superseded_binding_drift");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn policy_drift_persists_exact_expected_and_observed_policy() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let (bindings, fingerprint) = authority_binding_material(&pool, &fixture).await;
    let authority = advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 1,
            resource_bindings: &bindings,
            binding_fingerprint: &fingerprint,
            policy_revision: 2,
            required_approvals: 1,
            activation_ttl_seconds: 1_800,
        },
    )
    .await;
    let operation = Operation::new("superseded-policy");
    let context = apply_context_at_authority(&fixture, &operation, &authority);
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "superseded");
    assert!(!locked.exact_replay);
    assert!(locked.requires_commit);
    assert_eq!(locked.resulting_revision, Some(4));
    assert_eq!(locked.resulting_state.as_deref(), Some("superseded"));
    assert!(locked.deployment_id.is_none());
    assert!(locked.desired_target_digest.is_none());
    assert!(locked.locked_projection.is_none());
    transaction.commit().await.unwrap();

    let persisted = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&persisted, "superseded_policy_drift");
    let termination = &persisted.termination.as_ref().unwrap().0;
    assert_eq!(termination["reason"]["reason"], "policy_drift");
    assert_eq!(termination["reason"]["expected_revision"], 1);
    assert_eq!(termination["reason"]["observed_revision"], 2);
    assert_eq!(termination["reason"]["expected_required_approvals"], 1);
    assert_eq!(termination["reason"]["observed_required_approvals"], 1);
    assert_eq!(termination["reason"]["expected_ttl_seconds"], 3_600);
    assert_eq!(termination["reason"]["observed_ttl_seconds"], 1_800);
    assert_eq!(persisted.audit_authority_revision, Some(2));
    assert_eq!(
        persisted.audit_binding_fingerprint.as_deref(),
        Some(&*fingerprint)
    );
    assert_eq!(persisted.audit_policy_revision, Some(2));
    assert!(persisted.audit_baseline_version.is_none());
    assert!(persisted.audit_baseline_hash.is_none());
}

#[derive(Clone, Copy)]
enum ArtifactCorruption {
    Definition,
    Oversized,
}

#[derive(Clone, Copy)]
enum IntegrityDrift {
    Baseline,
    Binding,
    Policy,
}

async fn corrupt_target_artifact(
    pool: &PgPool,
    fixture: &Fixture,
    corruption: ArtifactCorruption,
) {
    let definition = match corruption {
        ArtifactCorruption::Definition => json!({
            "version": 2,
            "panels": [],
            "modals": [],
            "rules": []
        }),
        ArtifactCorruption::Oversized => json!({
            "version": 1,
            "panels": [],
            "modals": [],
            "rules": [],
            "padding": "x".repeat(524_289)
        }),
    };
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         DISABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         DROP CONSTRAINT arv_content_integrity",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let changed = sqlx::query(
        "UPDATE public.automation_ruleset_versions SET definition = $4 \
         WHERE guild_id = $1 AND ruleset_key = $2 AND version = $3",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(1_i64)
    .bind(Json(definition))
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         ADD CONSTRAINT arv_content_integrity CHECK (canonical_content_hash IS NOT NULL \
          AND canonical_content_hash = content_hash) NOT VALID",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         ENABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn integrity_drift_context(
    pool: &PgPool,
    fixture: &Fixture,
    operation: &Operation,
    drift: IntegrityDrift,
) -> ApplyLockContext {
    match drift {
        IntegrityDrift::Baseline => {
            set_competing_active_baseline(pool, fixture).await;
            ApplyLockContext::single(fixture, operation)
        }
        IntegrityDrift::Binding => {
            let guild_id = fixture.guild_id.parse::<u64>().unwrap();
            let mut bindings = ResourceBindingMap::default();
            bindings.channel_bindings.insert(
                ResourceKey("community_hub".to_string()),
                ChannelId(guild_id + 1_000_000_000),
            );
            bindings.role_bindings.insert(
                ResourceKey("automation_operator".to_string()),
                RoleId(guild_id + 2_000_000_000),
            );
            let fingerprint = resource_binding_fingerprint_v2(&bindings);
            let stored = json!({
                "role_bindings": &bindings.role_bindings,
                "channel_bindings": &bindings.channel_bindings
            });
            let authority = advance_authority(
                pool,
                fixture,
                AuthorityAdvance {
                    binding_revision: 2,
                    resource_bindings: &stored,
                    binding_fingerprint: fingerprint.as_str(),
                    policy_revision: 1,
                    required_approvals: 1,
                    activation_ttl_seconds: 3_600,
                },
            )
            .await;
            apply_context_at_authority(fixture, operation, &authority)
        }
        IntegrityDrift::Policy => {
            let (bindings, fingerprint) = authority_binding_material(pool, fixture).await;
            let authority = advance_authority(
                pool,
                fixture,
                AuthorityAdvance {
                    binding_revision: 1,
                    resource_bindings: &bindings,
                    binding_fingerprint: &fingerprint,
                    policy_revision: 2,
                    required_approvals: 1,
                    activation_ttl_seconds: 3_600,
                },
            )
            .await;
            apply_context_at_authority(fixture, operation, &authority)
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn corrupted_or_oversized_target_preempts_every_drift_without_product_mutation() {
    for corruption in [ArtifactCorruption::Definition, ArtifactCorruption::Oversized] {
        for drift in [
            IntegrityDrift::Baseline,
            IntegrityDrift::Binding,
            IntegrityDrift::Policy,
        ] {
            let database = isolated_database("artifact_integrity_drift").await;
            MIGRATOR.run(&database.pool).await.unwrap();
            {
                let pool = &database.pool;
                let fixture = seed_fixture(pool).await;
                corrupt_target_artifact(pool, &fixture, corruption).await;
                let operation = Operation::new("artifact-integrity-drift");
                let context = integrity_drift_context(pool, &fixture, &operation, drift).await;
                let mut transaction = begin_serializable(pool).await;
                let result = lock_apply_with_context(
                    &mut transaction,
                    &fixture,
                    &operation,
                    &Call::valid(&fixture),
                    &context,
                )
                .await;
                match result {
                    Ok(locked) => {
                        assert_eq!(locked.outcome, "target_mismatch");
                        assert!(!locked.exact_replay);
                        assert!(!locked.requires_commit);
                        assert!(locked.locked_projection.is_none());
                    }
                    Err(error) => assert_eq!(
                        error
                            .as_database_error()
                            .and_then(|database| database.code())
                            .as_deref(),
                        Some("PZ012")
                    ),
                }
                transaction.rollback().await.unwrap();
                let persisted = sqlx::query_as::<
                    _,
                    (String, i64, i64, Option<Json<Value>>, i64, i64, i64, i64),
                >(
                    "SELECT activation.state, activation.product_revision, \
                     activation.apply_attempt_no, activation.termination, \
                     (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
                      WHERE activation_request_id = activation.id), \
                     (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
                      WHERE receipt_id = $2), \
                     (SELECT pg_catalog.count(*) FROM public.product_audit_events \
                      WHERE receipt_id = $2), \
                     (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
                      WHERE receipt_id = $2) \
                     FROM public.activation_requests AS activation WHERE activation.id = $1",
                )
                .bind(&fixture.activation_id)
                .bind(&operation.receipt_id)
                .fetch_one(pool)
                .await
                .unwrap();
                assert_eq!(
                    persisted,
                    ("approved".to_string(), 2, 0, None, 0, 0, 0, 0)
                );
            }
            drop_isolated_database(database).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn missing_target_with_policy_drift_remains_target_mismatch_without_supersession() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let (bindings, fingerprint) = authority_binding_material(&pool, &fixture).await;
    let authority = advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 1,
            resource_bindings: &bindings,
            binding_fingerprint: &fingerprint,
            policy_revision: 2,
            required_approvals: 1,
            activation_ttl_seconds: 3_600,
        },
    )
    .await;
    let mut corruption = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         DISABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *corruption)
    .await
    .unwrap();
    let deleted = sqlx::query(
        "DELETE FROM public.automation_ruleset_versions \
         WHERE guild_id = $1 AND ruleset_key = $2 AND version = 1",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *corruption)
    .await
    .unwrap();
    assert_eq!(deleted.rows_affected(), 1);
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         ENABLE TRIGGER automation_ruleset_versions_reject_mutation",
    )
    .execute(&mut *corruption)
    .await
    .unwrap();
    corruption.commit().await.unwrap();

    let operation = Operation::new("missing-target-policy-drift");
    let context = apply_context_at_authority(&fixture, &operation, &authority);
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "target_mismatch");
    assert!(!locked.exact_replay);
    assert!(!locked.requires_commit);
    assert!(locked.resulting_revision.is_none());
    assert!(locked.resulting_state.is_none());
    assert!(locked.deployment_id.is_none());
    assert!(locked.desired_target_digest.is_none());
    assert!(locked.locked_projection.is_none());
    transaction.commit().await.unwrap();

    let persisted = sqlx::query_as::<
        _,
        (
            String,
            i64,
            i64,
            Option<Json<Value>>,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
        ),
    >(
        "SELECT activation.state, activation.product_revision, activation.apply_attempt_no, \
          activation.termination, \
          (SELECT head.next_version FROM public.automation_ruleset_heads AS head \
           WHERE head.guild_id = $2 AND head.ruleset_key = $3), \
          (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations AS active \
           WHERE active.guild_id = $2 AND active.ruleset_key = $3), \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
           WHERE deployment.activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
           WHERE receipt.receipt_id = $4), \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_idempotency_aliases AS alias \
           WHERE alias.receipt_id = $4), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
           WHERE audit.receipt_id = $4), \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_audit_evidence AS evidence \
           WHERE evidence.receipt_id = $4) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, "approved");
    assert_eq!(persisted.1, 2);
    assert_eq!(persisted.2, 0);
    assert!(persisted.3.is_none());
    assert_eq!(persisted.4, 2);
    assert_eq!((persisted.5, persisted.6), (0, 0));
    assert_eq!(
        (persisted.7, persisted.8, persisted.9, persisted.10),
        (0, 0, 0, 0)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn existing_runtime_deployment_blocks_fresh_drift_supersession() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let applied_operation = Operation::new("malformed-runtime-seed");
    complete_apply(&pool, &fixture, &applied_operation).await;
    let mut corruption = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
    .execute(&mut *corruption)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET state = 'approved' \
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
    let (bindings, fingerprint) = authority_binding_material(&pool, &fixture).await;
    let authority = advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 1,
            resource_bindings: &bindings,
            binding_fingerprint: &fingerprint,
            policy_revision: 2,
            required_approvals: 1,
            activation_ttl_seconds: 3_600,
        },
    )
    .await;
    let operation = Operation::new("malformed-runtime-drift");
    let context = apply_context_at_authority(&fixture, &operation, &authority);
    let mut call = Call::valid(&fixture);
    call.expected_revision = 4;
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply_with_context(&mut transaction, &fixture, &operation, &call, &context)
        .await
        .unwrap();
    assert_eq!(locked.outcome, "indeterminate");
    assert!(!locked.exact_replay);
    assert!(!locked.requires_commit);
    transaction.commit().await.unwrap();

    let unchanged = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
           WHERE deployment.activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
           WHERE receipt.receipt_id = $2) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unchanged, ("approved".to_string(), 4, 1, 0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn supersession_rolls_back_as_one_atomic_unit() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    set_competing_active_baseline(&pool, &fixture).await;
    let operation = Operation::new("superseded-rollback");
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "superseded");
    assert!(!locked.exact_replay);
    assert!(locked.requires_commit);
    transaction.rollback().await.unwrap();

    let rolled_back = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, activation.apply_attempt_no, \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
           WHERE deployment.activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
           WHERE receipt.receipt_id = $2), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
           WHERE audit.receipt_id = $2), \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_audit_evidence AS evidence \
           WHERE evidence.receipt_id = $2) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rolled_back, ("approved".to_string(), 2, 0, 0, 0, 0, 0));
    let active_version = sqlx::query_scalar::<_, i64>(
        "SELECT active_version FROM public.automation_ruleset_activations \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_version, 2);
}
