const SELECTOR_INSTALLATION: &str = "runtime-execution-selector-installation";
const SELECTOR_GUILD: &str = "9200102";
const SELECTOR_RULESET: &str = "runtime_execution_selector_ruleset";
const SELECTOR_PROMOTION: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SELECTOR_ACTIVATION: &str = "runtime_execution_selector_activation";
const SELECTOR_DEPLOYMENT: &str = "runtime-execution-selector-deployment";
const EXECUTION_SELECTOR_SLOT_WRITER_EPOCH_MIGRATION: &str = include_str!(
    "../../../../migrations/202607240014_fence_runtime_execution_selector_slot_writer_epoch.sql"
);
const EXECUTION_SELECTOR_SLOT_WRITER_CATALOG: [&str; 6] = [
    "public.starring_runtime_execution_claim_next_v1(text,bigint)",
    "public.starring_runtime_execution_recover_stale_live_v1()",
    "public.starring_runtime_execution_schema_manifest_v1()",
    "public.starring_runtime_execution_database_readiness_v1()",
    "starring_runtime_private_v2.starring_runtime_slot_writer_fence_lock_v2(text,text)",
    "starring_runtime_private_v2.starring_runtime_slot_writer_fence_begin_unsafe_v2(text,text,bigint)",
];

type ExecutionSelectorCatalogRow = (String, i64, i64, String, String);

fn pre_enable_executor_functions() -> &'static [&'static str] {
    &PRE_INGRESS_ACK_EXECUTOR_FUNCTIONS
}

async fn raw_selector_claim(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    controller_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut **transaction)
        .await?;
    sqlx::query_scalar(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_claim_next_v1($1, 300000)",
    )
    .bind(controller_id)
    .fetch_optional(&mut **transaction)
    .await
}

async fn raw_selector_recover(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut **transaction)
        .await?;
    sqlx::query_scalar(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_recover_stale_live_v1()",
    )
    .fetch_optional(&mut **transaction)
    .await
}

async fn apply_execution_selector_epoch_prerequisites(
    server: &PostgresTestServer,
    database_name: &str,
    pool: PgPool,
) -> PgPool {
    let mut applied_versions = Vec::new();
    for migration in MIGRATOR
        .iter()
        .filter(|migration| (202_607_240_010..=202_607_240_012).contains(&migration.version))
    {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        applied_versions.push(migration.version);
    }
    let pool = reopen_execution_slot_writer_migration_pool(server, database_name, pool).await;
    let migration = MIGRATOR
        .iter()
        .find(|migration| migration.version == 202_607_240_013)
        .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::raw_sql(migration.sql.as_ref())
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    applied_versions.push(migration.version);
    assert_eq!(
        applied_versions,
        vec![
            202_607_240_010,
            202_607_240_011,
            202_607_240_012,
            202_607_240_013
        ]
    );
    pool
}

async fn create_execution_selector_role(pool: &PgPool, role: &str, can_login: bool) {
    assert!(canonical_identifier(role));
    let login = if can_login { "LOGIN" } else { "NOLOGIN" };
    pool.execute(
        format!(
            "CREATE ROLE {role} {login} NOINHERIT NOSUPERUSER NOCREATEDB \
             NOCREATEROLE NOREPLICATION NOBYPASSRLS"
        )
        .as_str(),
    )
    .await
    .unwrap();
    for capability in pre_enable_executor_functions() {
        pool.execute(format!("GRANT EXECUTE ON FUNCTION {capability} TO {role}").as_str())
            .await
            .unwrap();
    }
}

async fn execution_selector_capability_image(
    pool: &PgPool,
) -> Vec<(String, i64, i64, String)> {
    let mut image = Vec::new();
    for identity in pre_enable_executor_functions() {
        image.push(
            sqlx::query_as(
                "SELECT $1::TEXT, function_row.oid::BIGINT, \
                        function_row.proowner::BIGINT, \
                        COALESCE(function_row.proacl::TEXT, '') \
                 FROM pg_catalog.pg_proc AS function_row \
                 WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
            )
            .bind(identity)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    image
}

async fn assert_execution_selector_capabilities(
    pool: &PgPool,
    role: &str,
) -> Vec<(String, i64, i64, String)> {
    let image = execution_selector_capability_image(pool).await;
    assert_eq!(image.len(), pre_enable_executor_functions().len());
    assert_eq!(
        image
            .iter()
            .map(|row| row.3.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        1
    );
    for identity in pre_enable_executor_functions() {
        assert!(
            sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
            )
            .bind(role)
            .bind(identity)
            .fetch_one(pool)
            .await
            .unwrap()
        );
    }
    image
}

async fn execution_selector_catalog_image(pool: &PgPool) -> Vec<ExecutionSelectorCatalogRow> {
    let mut image = Vec::new();
    for identity in EXECUTION_SELECTOR_SLOT_WRITER_CATALOG {
        image.push(
            sqlx::query_as(
                "SELECT $1::TEXT, function_row.oid::BIGINT, \
                        function_row.proowner::BIGINT, \
                        COALESCE(function_row.proacl::TEXT, ''), \
                        pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to( \
                            pg_catalog.pg_get_functiondef(function_row.oid), \
                            'UTF8' \
                        )), 'hex') \
                 FROM pg_catalog.pg_proc AS function_row \
                 WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
            )
            .bind(identity)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    image
}

fn execution_selector_catalog_contract(
    image: &[ExecutionSelectorCatalogRow],
) -> Vec<(String, i64, i64, String)> {
    image
        .iter()
        .map(|row| (row.0.clone(), row.1, row.2, row.3.clone()))
        .collect()
}

async fn selector_deployment_snapshot(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    deployment_id: &str,
) -> Json<Value> {
    sqlx::query_scalar(
        "SELECT deployment.snapshot \
         FROM public.runtime_deployments AS deployment \
         WHERE deployment.deployment_id = $1",
    )
    .bind(deployment_id)
    .fetch_one(executor)
    .await
    .unwrap()
}

async fn selector_slot_epoch(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    guild_id: &str,
    ruleset_key: &str,
) -> i64 {
    sqlx::query_scalar(
        "SELECT fence.writer_epoch \
         FROM public.runtime_slot_writer_fences_v2 AS fence \
         WHERE fence.slot_guild_id = $1 AND fence.slot_ruleset_key = $2",
    )
    .bind(guild_id)
    .bind(ruleset_key)
    .fetch_one(executor)
    .await
    .unwrap()
}

fn selector_requested_snapshot(requested_at: DateTime<Utc>) -> RuntimeDeploymentSnapshotV1 {
    let identity = serde_json::from_value::<RuntimeDeploymentIdentityV1>(json!({
        "deployment_id": SELECTOR_DEPLOYMENT,
        "tenant_id": TENANT,
        "installation_id": SELECTOR_INSTALLATION,
        "promotion_id": SELECTOR_PROMOTION,
        "activation_request_id": SELECTOR_ACTIVATION
    }))
    .unwrap();
    let target = serde_json::from_value::<RuntimeDeploymentTargetV1>(json!({
        "guild_id": SELECTOR_GUILD,
        "ruleset_key": SELECTOR_RULESET,
        "version": 1,
        "content_hash": CONTENT_HASH,
        "binding_revision": 1,
        "binding_fingerprint": BINDING_FINGERPRINT
    }))
    .unwrap();
    RuntimeDeployment::request(
        identity,
        target,
        RuntimeGeneration::FIRST,
        None,
        requested_at,
    )
    .unwrap()
    .snapshot()
}

async fn insert_selector_activation_pending_promotion(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request_digest: &str,
    record: &Value,
) {
    let mut prepared = record.clone();
    prepared["revision"] = json!(1);
    prepared["stage"] = json!({"state": "prepared"});
    let mut published = record.clone();
    published["revision"] = json!(2);
    published["stage"] = json!({
        "state": "published",
        "publication": record["stage"]["publication"].clone()
    });
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, installation_id, \
          principal_id, record) VALUES ($1, 1, 1, 'prepared', $2, $3, $4, $5, $6)",
    )
    .bind(SELECTOR_PROMOTION)
    .bind(request_digest)
    .bind(TENANT)
    .bind(SELECTOR_INSTALLATION)
    .bind(PRINCIPAL)
    .bind(Json(&prepared))
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 2, stage = 'published', record = $2 WHERE id = $1",
    )
    .bind(SELECTOR_PROMOTION)
    .bind(Json(&published))
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 3, stage = 'activation_pending', record = $2 WHERE id = $1",
    )
    .bind(SELECTOR_PROMOTION)
    .bind(Json(record))
    .execute(&mut **transaction)
    .await
    .unwrap();
}

async fn seed_second_claimable_deployment(pool: &PgPool) {
    let now = database_now(pool).await;
    let expires_at = now + TimeDelta::hours(1);
    let linked_at = now + TimeDelta::seconds(1);
    let request_digest = "8".repeat(64);
    let approval_payload_digest = "9".repeat(64);
    let approval_context_digest = "a".repeat(64);
    let approval_context = json!({
        "promotion_id": SELECTOR_PROMOTION,
        "promotion_request_digest": request_digest,
        "approval_payload_digest": approval_payload_digest,
        "approval_context_digest": approval_context_digest,
        "binding": {
            "revision": 1,
            "required_bindings": [],
            "fingerprint": BINDING_FINGERPRINT
        },
        "baseline": { "state": "absent" },
        "policy": {
            "revision": 1,
            "required_approvals": 1,
            "ttl_seconds": 3600,
            "digest": "2".repeat(64)
        }
    });
    let mut promotion =
        promotion_record(now, expires_at, &request_digest, &approval_context);
    promotion["id"] = json!(SELECTOR_PROMOTION);
    promotion["intent"]["authority"]["installation_id"] = json!(SELECTOR_INSTALLATION);
    promotion["intent"]["authority"]["guild_id"] = json!(SELECTOR_GUILD);
    promotion["intent"]["authority"]["ruleset_key"] = json!(SELECTOR_RULESET);
    promotion["stage"]["activation"]["request_id"] = json!(SELECTOR_ACTIVATION);
    promotion["stage"]["activation"]["target"]["guild_id"] = json!(SELECTOR_GUILD);
    promotion["stage"]["activation"]["target"]["ruleset_key"] = json!(SELECTOR_RULESET);

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, \
          ruleset_key, lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(SELECTOR_INSTALLATION)
    .bind(TENANT)
    .bind("9200302")
    .bind(SELECTOR_GUILD)
    .bind(SELECTOR_RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, 1, '{}'::JSONB, $3, 1, 1, 3600, $4, $5, $6)",
    )
    .bind(SELECTOR_INSTALLATION)
    .bind(TENANT)
    .bind(BINDING_FINGERPRINT)
    .bind("b".repeat(64))
    .bind(PRINCIPAL)
    .bind("c".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_heads \
         (guild_id, ruleset_key, next_version) VALUES ($1, $2, 2)",
    )
    .bind(SELECTOR_GUILD)
    .bind(SELECTOR_RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 1, 1, \
          pg_catalog.jsonb_build_object('version', 1, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $3, $4)",
    )
    .bind(SELECTOR_GUILD)
    .bind(SELECTOR_RULESET)
    .bind(CONTENT_HASH)
    .bind("9200201")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) VALUES ($1, $2, 1)",
    )
    .bind(SELECTOR_GUILD)
    .bind(SELECTOR_RULESET)
    .execute(&mut *transaction)
    .await
    .unwrap();
    insert_selector_activation_pending_promotion(
        &mut transaction,
        &request_digest,
        &promotion,
    )
    .await;
    sqlx::query(
        "INSERT INTO public.activation_requests \
         (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, state, created_at, expires_at, authority_kind, link_state_name, \
          approval_context, link_state, promotion_id, promotion_request_digest, \
          approval_payload_digest, approval_context_digest) \
         VALUES ($1, $2, $3, 1, $4, $5, 1, 'pending', $6, $7, 'product_authoring', \
                 'unlinked', $8, '{\"state\":\"unlinked\"}'::JSONB, $9, $10, $11, $12)",
    )
    .bind(SELECTOR_ACTIVATION)
    .bind(SELECTOR_GUILD)
    .bind(SELECTOR_RULESET)
    .bind(CONTENT_HASH)
    .bind("9200401")
    .bind(now)
    .bind(expires_at)
    .bind(Json(json!({
        "authority": "product_authoring",
        "context": approval_context
    })))
    .bind(SELECTOR_PROMOTION)
    .bind(&request_digest)
    .bind(&approval_payload_digest)
    .bind(&approval_context_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests \
         SET link_state_name = 'linked', link_state = $2, linked_at = $3 \
         WHERE id = $1",
    )
    .bind(SELECTOR_ACTIVATION)
    .bind(Json(json!({ "state": "linked", "linked_at": linked_at })))
    .bind(linked_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'applied', applied_at = $2, applied_by = $3, \
             completion_kind = 'already_active', activation_notices = '[]'::JSONB \
         WHERE id = $1",
    )
    .bind(SELECTOR_ACTIVATION)
    .bind(linked_at)
    .bind("9200501")
    .execute(&mut *transaction)
    .await
    .unwrap();

    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let requested_at =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    let snapshot = selector_requested_snapshot(requested_at);
    let desired_target_digest = runtime_desired_target_digest_v1(
        &snapshot.identity,
        &snapshot.target,
        snapshot.runtime_generation.get(),
        1,
        snapshot.previous_runtime.as_ref(),
    );
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', $1, TRUE)")
        .bind(requested_at.to_rfc3339())
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.runtime_deployments \
         (deployment_id, tenant_id, installation_id, promotion_id, activation_request_id, \
          installation_authority_revision, guild_id, ruleset_key, target_version, \
          target_content_hash, binding_revision, binding_fingerprint, desired_target_digest, \
          runtime_generation, requested_at, snapshot_format_version, snapshot, revision, phase, \
          created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, 1, $6, $7, 1, $8, 1, $9, $10, 1, $11, \
                 1, $12, 1, 'requested', $11, $11)",
    )
    .bind(SELECTOR_DEPLOYMENT)
    .bind(TENANT)
    .bind(SELECTOR_INSTALLATION)
    .bind(SELECTOR_PROMOTION)
    .bind(SELECTOR_ACTIVATION)
    .bind(SELECTOR_GUILD)
    .bind(SELECTOR_RULESET)
    .bind(CONTENT_HASH)
    .bind(BINDING_FINGERPRINT)
    .bind(desired_target_digest.as_str())
    .bind(requested_at)
    .bind(Json(serde_json::to_value(&snapshot).unwrap()))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', '', TRUE)")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn claim_selector_epoch_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let request = RuntimeClaimNextExecutionV1 {
        controller_id: ControllerId::parse("runtime-selector-claim-controller").unwrap(),
        lease_for: Duration::from_secs(300),
    };
    let guild_id = GUILD.to_string();
    let baseline_epoch = selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let baseline_snapshot =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;

    let mut rollback = database.owner_pool.begin().await.unwrap();
    assert_eq!(
        raw_selector_claim(&mut rollback, request.controller_id.as_str())
            .await
            .unwrap()
            .as_deref(),
        Some("applied")
    );
    assert_eq!(
        selector_slot_epoch(&mut *rollback, &guild_id, RULESET).await,
        baseline_epoch + 1
    );
    assert_ne!(
        selector_deployment_snapshot(&mut *rollback, DEPLOYMENT).await,
        baseline_snapshot
    );
    rollback.rollback().await.unwrap();
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        baseline_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        baseline_snapshot
    );

    let applied = adapter
        .claim_next_execution(request.clone())
        .await
        .unwrap()
        .unwrap();
    let applied_epoch = selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let applied_snapshot =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;
    assert_eq!(applied_epoch, baseline_epoch + 1);
    assert_ne!(applied_snapshot, baseline_snapshot);

    let replayed = adapter
        .claim_next_execution(request.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replayed, applied);
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        applied_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        applied_snapshot
    );

    let gateway_ready = selector_gateway_ready_session(
        database,
        &adapter,
        request.controller_id.as_str(),
        "runtime-selector-claim-panel",
        "runtime-selector-claim-process",
    )
    .await;
    let replay_expected = gateway_ready.current_execution_receipt().unwrap();
    let ready_epoch = selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let canonical = canonical_product_drain(gateway_ready.snapshot());
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let pending_epoch = selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let pending_snapshot =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;
    let pending_counts = product_drain_row_counts(&database.owner_pool).await;
    assert_eq!(pending_epoch, ready_epoch + 1);

    let pending_replay = adapter
        .claim_next_execution(request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending_replay, replay_expected);
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        pending_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        pending_snapshot
    );

    let other_controller = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse("runtime-selector-other-controller").unwrap(),
            lease_for: Duration::from_secs(300),
        })
        .await
        .unwrap();
    assert!(other_controller.is_none());
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        pending_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        pending_snapshot
    );
    assert_eq!(
        product_drain_row_counts(&database.owner_pool).await,
        pending_counts
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn claim_selector_skips_pending_candidate(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let first_session = selector_gateway_ready_session(
        database,
        &adapter,
        "runtime-selector-skip-pending-controller",
        "runtime-selector-skip-pending-panel",
        "runtime-selector-skip-pending-process",
    )
    .await;
    seed_second_claimable_deployment(&database.owner_pool).await;
    let canonical = canonical_product_drain(first_session.snapshot());
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let first_epoch =
        selector_slot_epoch(&database.owner_pool, &GUILD.to_string(), RULESET).await;
    let second_epoch =
        selector_slot_epoch(&database.owner_pool, SELECTOR_GUILD, SELECTOR_RULESET).await;
    let first_image =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;

    let claimed = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse("runtime-selector-skip-controller").unwrap(),
            lease_for: Duration::from_secs(300),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        claimed.snapshot.identity.deployment_id.as_str(),
        SELECTOR_DEPLOYMENT
    );
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &GUILD.to_string(), RULESET).await,
        first_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        first_image
    );
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, SELECTOR_GUILD, SELECTOR_RULESET).await,
        second_epoch + 1
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn selector_gateway_ready_session(
    database: &IsolatedDatabase,
    adapter: &PostgresRuntimeExecutionV1,
    controller_id: &str,
    certificate_id: &str,
    process_instance_id: &str,
) -> RuntimeConvergenceSessionV1 {
    let mut session =
        claimed_session(adapter, controller_id, Duration::from_secs(300)).await;
    advance_to_activation_applying(&database.owner_pool, adapter, &mut session).await;
    let activation = ActivationAttestationV1 {
        activation_request_id: session.snapshot().identity.activation_request_id.clone(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        kind: ActivationOutcomeKindV1::Activated,
        activated_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptActivation(activation),
    )
    .await;
    mutate_applied(
        adapter,
        &mut session,
        RuntimeConvergenceMutationV1::BeginPanelReconciliation,
    )
    .await;
    let certificate = PanelCertificateV1 {
        certificate_id: PanelCertificateId::parse(certificate_id).unwrap(),
        report_digest: PanelReportDigestV1::parse(CERTIFICATION_REPORT).unwrap(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        process_instance_id: ProcessInstanceId::parse(process_instance_id).unwrap(),
        declared_count: 1,
        installed_count: 1,
        unchanged_count: 0,
        skipped_transient_count: 0,
        skipped_unresolved_channel_count: 0,
        failed_count: 0,
        ambiguous_outcome_count: 0,
        stale_message_cleanup_pending_count: 0,
        orphan_message_cleanup_pending_count: 0,
        reposted_old_message_cleanup_pending_count: 0,
        reconciled_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate),
    )
    .await;
    session
}

async fn selector_stale_live_session(
    database: &IsolatedDatabase,
    adapter: &PostgresRuntimeExecutionV1,
    controller_id: &str,
    certificate_id: &str,
    process_instance_id: &str,
) -> (RuntimeConvergenceSessionV1, DateTime<Utc>) {
    let mut session = selector_gateway_ready_session(
        database,
        adapter,
        controller_id,
        certificate_id,
        process_instance_id,
    )
    .await;
    let gateway_ready = gateway_ready_attestation(database, &session).await;
    let request = session
        .begin_certification(
            gateway_ready,
            adapter_certification_metadata(),
            Duration::from_secs(1),
        )
        .unwrap();
    let applied = RuntimeExecutionConvergencePort::certify_live(adapter, request)
        .await
        .unwrap();
    let expires_at = applied.serving.expires_at;
    session.apply_certification(applied).unwrap();
    (session, expires_at)
}

async fn recovery_selector_epoch_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let (_, expires_at) = selector_stale_live_session(
        database,
        &adapter,
        "runtime-selector-recovery-controller",
        "runtime-selector-recovery-panel",
        "runtime-selector-recovery-process",
    )
    .await;
    wait_for_database_time(&database.owner_pool, expires_at).await;
    let guild_id = GUILD.to_string();
    let baseline_epoch =
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let baseline_snapshot =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;

    let mut rollback = database.owner_pool.begin().await.unwrap();
    assert_eq!(
        raw_selector_recover(&mut rollback)
            .await
            .unwrap()
            .as_deref(),
        Some("applied")
    );
    assert_eq!(
        selector_slot_epoch(&mut *rollback, &guild_id, RULESET).await,
        baseline_epoch + 1
    );
    assert_ne!(
        selector_deployment_snapshot(&mut *rollback, DEPLOYMENT).await,
        baseline_snapshot
    );
    rollback.rollback().await.unwrap();
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        baseline_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        baseline_snapshot
    );

    let recovered = RuntimeExecutionConvergencePort::recover_next_stale_live(&adapter)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        recovered.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    let recovered_epoch =
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let recovered_snapshot =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;
    assert_eq!(recovered_epoch, baseline_epoch + 1);
    assert_ne!(recovered_snapshot, baseline_snapshot);

    assert!(
        RuntimeExecutionConvergencePort::recover_next_stale_live(&adapter)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        recovered_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        recovered_snapshot
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn recovery_selector_pending_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let (session, expires_at) = selector_stale_live_session(
        database,
        &adapter,
        "runtime-selector-pending-recovery-controller",
        "runtime-selector-pending-recovery-panel",
        "runtime-selector-pending-recovery-process",
    )
    .await;
    wait_for_database_time(&database.owner_pool, expires_at).await;
    let canonical = canonical_product_drain(session.snapshot());
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let guild_id = GUILD.to_string();
    let pending_epoch =
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let pending_snapshot =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;
    let pending_counts = product_drain_row_counts(&database.owner_pool).await;

    assert!(
        RuntimeExecutionConvergencePort::recover_next_stale_live(&adapter)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        pending_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        pending_snapshot
    );
    assert_eq!(
        product_drain_row_counts(&database.owner_pool).await,
        pending_counts
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn recovery_selector_skips_pending_candidate(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    seed_second_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let (first_session, first_expiry) = selector_stale_live_session(
        database,
        &adapter,
        "runtime-selector-first-live-controller",
        "runtime-selector-first-live-panel",
        "runtime-selector-first-live-process",
    )
    .await;
    let (_, second_expiry) = selector_stale_live_session(
        database,
        &adapter,
        "runtime-selector-second-live-controller",
        "runtime-selector-second-live-panel",
        "runtime-selector-second-live-process",
    )
    .await;
    wait_for_database_time(
        &database.owner_pool,
        std::cmp::max(first_expiry, second_expiry),
    )
    .await;
    let canonical = canonical_product_drain(first_session.snapshot());
    let inserted = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");
    let first_epoch =
        selector_slot_epoch(&database.owner_pool, &GUILD.to_string(), RULESET).await;
    let second_epoch =
        selector_slot_epoch(&database.owner_pool, SELECTOR_GUILD, SELECTOR_RULESET).await;
    let first_image =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;

    let recovered = RuntimeExecutionConvergencePort::recover_next_stale_live(&adapter)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.snapshot.identity.deployment_id.as_str(),
        SELECTOR_DEPLOYMENT
    );
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &GUILD.to_string(), RULESET).await,
        first_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        first_image
    );
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, SELECTOR_GUILD, SELECTOR_RULESET).await,
        second_epoch + 1
    );
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn assert_selector_slot_locks_available(
    pool: &PgPool,
    guild_id: &str,
    ruleset_key: &str,
) {
    let mut probe = pool.begin().await.unwrap();
    let advisory_available = sqlx::query_scalar::<_, bool>(TRY_RUNTIME_SERVING_SLOT_LOCK)
        .bind(guild_id)
        .bind(ruleset_key)
        .fetch_one(&mut *probe)
        .await
        .unwrap();
    assert!(advisory_available);
    let physical_epoch = sqlx::query_scalar::<_, i64>(
        "SELECT fence.writer_epoch \
         FROM public.runtime_slot_writer_fences_v2 AS fence \
         WHERE fence.slot_guild_id = $1 AND fence.slot_ruleset_key = $2 \
         FOR UPDATE NOWAIT",
    )
    .bind(guild_id)
    .bind(ruleset_key)
    .fetch_one(&mut *probe)
    .await
    .unwrap();
    assert!(physical_epoch >= 1);
    probe.rollback().await.unwrap();
}

async fn claim_selector_releases_rejected_candidate_locks(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let mut blocker = database.owner_pool.begin().await.unwrap();
    let locked = sqlx::query_scalar::<_, String>(
        "SELECT deployment.deployment_id \
         FROM public.runtime_deployments AS deployment \
         WHERE deployment.deployment_id = $1 \
         FOR UPDATE",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    assert_eq!(locked, DEPLOYMENT);

    let mut selector = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *selector)
        .await
        .unwrap();
    assert!(
        raw_selector_claim(&mut selector, "runtime-selector-rejected-claim")
            .await
            .unwrap()
            .is_none()
    );
    assert_selector_slot_locks_available(&database.owner_pool, &GUILD.to_string(), RULESET).await;
    selector.rollback().await.unwrap();
    blocker.rollback().await.unwrap();
}

async fn recovery_selector_releases_rejected_candidate_locks(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let (_, expires_at) = selector_stale_live_session(
        database,
        &adapter,
        "runtime-selector-rejected-recovery-controller",
        "runtime-selector-rejected-recovery-panel",
        "runtime-selector-rejected-recovery-process",
    )
    .await;
    wait_for_database_time(&database.owner_pool, expires_at).await;

    let mut blocker = database.owner_pool.begin().await.unwrap();
    let locked = sqlx::query_scalar::<_, String>(
        "SELECT deployment.deployment_id \
         FROM public.runtime_deployments AS deployment \
         WHERE deployment.deployment_id = $1 \
         FOR UPDATE",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    assert_eq!(locked, DEPLOYMENT);

    let mut selector = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *selector)
        .await
        .unwrap();
    assert!(raw_selector_recover(&mut selector)
        .await
        .unwrap()
        .is_none());
    assert_selector_slot_locks_available(&database.owner_pool, &GUILD.to_string(), RULESET).await;
    selector.rollback().await.unwrap();
    blocker.rollback().await.unwrap();
}

async fn claim_selector_writer_wins_drain_race(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let baseline_snapshot: RuntimeDeploymentSnapshotV1 = serde_json::from_value(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT)
            .await
            .0,
    )
    .unwrap();
    let canonical = canonical_product_drain(&baseline_snapshot);
    let guild_id = GUILD.to_string();
    let baseline_epoch =
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;

    let mut writer = database.owner_pool.begin().await.unwrap();
    let writer_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *writer)
        .await
        .unwrap();
    assert_eq!(
        raw_selector_claim(&mut writer, "runtime-selector-writer-first")
            .await
            .unwrap()
            .as_deref(),
        Some("applied")
    );
    assert_eq!(
        selector_slot_epoch(&mut *writer, &guild_id, RULESET).await,
        baseline_epoch + 1
    );

    let mut drain = begin_product_drain_first_apply(&database.owner_pool).await;
    let drain_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *drain)
        .await
        .unwrap();
    let (drain_result, ()) = tokio::join!(
        call_product_drain_first_apply(&mut *drain, &canonical),
        async {
            wait_for_product_drain_first_apply_lock(
                &database.owner_pool,
                drain_pid,
                writer_pid,
            )
            .await;
            writer.commit().await.unwrap();
        }
    );
    drain.rollback().await.unwrap();
    assert_sqlstate(&drain_result.unwrap_err(), "40001");
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        baseline_epoch + 1
    );
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));

    let stale_retry = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap_err();
    assert_database_error(
        &stale_retry,
        "RX001",
        "runtime_product_drain_first_apply_deployment_mismatch",
    );
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        baseline_epoch + 1
    );
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn claim_selector_drain_wins_writer_race(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let request = RuntimeClaimNextExecutionV1 {
        controller_id: ControllerId::parse("runtime-selector-drain-first").unwrap(),
        lease_for: Duration::from_secs(300),
    };
    adapter
        .claim_next_execution(request.clone())
        .await
        .unwrap()
        .unwrap();
    let gateway_ready = selector_gateway_ready_session(
        database,
        &adapter,
        request.controller_id.as_str(),
        "runtime-selector-drain-first-panel",
        "runtime-selector-drain-first-process",
    )
    .await;
    let replay_expected = gateway_ready.current_execution_receipt().unwrap();
    let canonical = canonical_product_drain(gateway_ready.snapshot());
    let guild_id = GUILD.to_string();
    let baseline_epoch =
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let baseline_image =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;

    let mut drain = begin_product_drain_first_apply(&database.owner_pool).await;
    let drain_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *drain)
        .await
        .unwrap();
    let inserted = call_product_drain_first_apply(&mut *drain, &canonical)
        .await
        .unwrap();
    assert_eq!(inserted.outcome_name, "inserted");

    let mut writer = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *writer)
        .await
        .unwrap();
    let _: String = sqlx::query_scalar(
        "SELECT public.starring_runtime_execution_database_identity_v1()",
    )
    .fetch_one(&mut *writer)
    .await
    .unwrap();
    let writer_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *writer)
        .await
        .unwrap();
    let (writer_result, ()) = tokio::join!(
        raw_selector_claim(&mut writer, request.controller_id.as_str()),
        async {
            wait_for_product_drain_first_apply_lock(
                &database.owner_pool,
                writer_pid,
                drain_pid,
            )
            .await;
            drain.commit().await.unwrap();
        }
    );
    writer.rollback().await.unwrap();
    assert_sqlstate(&writer_result.unwrap_err(), "40001");
    let pending_epoch =
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    assert_eq!(pending_epoch, baseline_epoch + 1);
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        baseline_image
    );
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (1, 1));

    let replayed = adapter
        .claim_next_execution(request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replayed, replay_expected);
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        pending_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        baseline_image
    );
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (1, 1));
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn recovery_selector_writer_wins_drain_race(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let (session, expires_at) = selector_stale_live_session(
        database,
        &adapter,
        "runtime-selector-recovery-writer-first",
        "runtime-selector-recovery-writer-first-panel",
        "runtime-selector-recovery-writer-first-process",
    )
    .await;
    wait_for_database_time(&database.owner_pool, expires_at).await;
    let canonical = canonical_product_drain(session.snapshot());
    let guild_id = GUILD.to_string();
    let baseline_epoch =
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;

    let mut writer = database.owner_pool.begin().await.unwrap();
    let writer_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *writer)
        .await
        .unwrap();
    assert_eq!(
        raw_selector_recover(&mut writer)
            .await
            .unwrap()
            .as_deref(),
        Some("applied")
    );
    assert_eq!(
        selector_slot_epoch(&mut *writer, &guild_id, RULESET).await,
        baseline_epoch + 1
    );

    let mut drain = begin_product_drain_first_apply(&database.owner_pool).await;
    let drain_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *drain)
        .await
        .unwrap();
    let (drain_result, ()) = tokio::join!(
        call_product_drain_first_apply(&mut *drain, &canonical),
        async {
            wait_for_product_drain_first_apply_lock(
                &database.owner_pool,
                drain_pid,
                writer_pid,
            )
            .await;
            writer.commit().await.unwrap();
        }
    );
    drain.rollback().await.unwrap();
    assert_sqlstate(&drain_result.unwrap_err(), "40001");
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        baseline_epoch + 1
    );
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));

    let stale_retry = committed_product_drain_first_apply(&database.owner_pool, &canonical)
        .await
        .unwrap_err();
    assert_database_error(
        &stale_retry,
        "RX001",
        "runtime_product_drain_first_apply_deployment_mismatch",
    );
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        baseline_epoch + 1
    );
    assert_eq!(product_drain_row_counts(&database.owner_pool).await, (0, 0));
    assert_slot_writer_fence_gates_clear(&database.owner_pool).await;
}

async fn close_selector_global_writer_fence(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_writer_fence \
         SET fence_state = 'closed', fence_generation = 2, \
             cutover_lease_epoch_high_water = 1, \
             cutover_coordinator_id = '0123456789abcdeffedcba9876543210', \
             cutover_expires_at = pg_catalog.clock_timestamp() + INTERVAL '1 hour' \
         WHERE singleton",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE public.runtime_writer_fence ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn claim_selector_respects_global_fence(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let guild_id = GUILD.to_string();
    let baseline_epoch =
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let baseline_image =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;
    close_selector_global_writer_fence(&database.owner_pool).await;

    let error = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse("runtime-selector-closed-claim").unwrap(),
            lease_for: Duration::from_secs(300),
        })
        .await
        .unwrap_err();
    assert_eq!(error, RuntimeExecutionPersistenceErrorV1::RetryNotReady);
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        baseline_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        baseline_image
    );
}

async fn recovery_selector_respects_global_fence(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let (_, expires_at) = selector_stale_live_session(
        database,
        &adapter,
        "runtime-selector-closed-recovery-controller",
        "runtime-selector-closed-recovery-panel",
        "runtime-selector-closed-recovery-process",
    )
    .await;
    wait_for_database_time(&database.owner_pool, expires_at).await;
    let guild_id = GUILD.to_string();
    let baseline_epoch =
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await;
    let baseline_image =
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await;
    close_selector_global_writer_fence(&database.owner_pool).await;

    let error = RuntimeExecutionConvergencePort::recover_next_stale_live(&adapter)
        .await
        .unwrap_err();
    assert_eq!(error, RuntimeExecutionPersistenceErrorV1::RetryNotReady);
    assert_eq!(
        selector_slot_epoch(&database.owner_pool, &guild_id, RULESET).await,
        baseline_epoch
    );
    assert_eq!(
        selector_deployment_snapshot(&database.owner_pool, DEPLOYMENT).await,
        baseline_image
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_selectors_advance_epochs_atomically_and_skip_pending_slots() {
    let server = PostgresTestServer::start();

    let claim_database = isolated_database(server.connect_options()).await;
    claim_selector_epoch_scenario(&claim_database).await;
    cleanup(claim_database).await;

    let claim_skip_database = isolated_database(server.connect_options()).await;
    claim_selector_skips_pending_candidate(&claim_skip_database).await;
    cleanup(claim_skip_database).await;

    let recovery_database = isolated_database(server.connect_options()).await;
    recovery_selector_epoch_scenario(&recovery_database).await;
    cleanup(recovery_database).await;

    let recovery_pending_database = isolated_database(server.connect_options()).await;
    recovery_selector_pending_scenario(&recovery_pending_database).await;
    cleanup(recovery_pending_database).await;

    let recovery_skip_database = isolated_database(server.connect_options()).await;
    recovery_selector_skips_pending_candidate(&recovery_skip_database).await;
    cleanup(recovery_skip_database).await;

    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_selectors_release_rejections_race_atomically_and_respect_global_fence() {
    let server = PostgresTestServer::start();

    let claim_release_database = isolated_database(server.connect_options()).await;
    claim_selector_releases_rejected_candidate_locks(&claim_release_database).await;
    cleanup(claim_release_database).await;

    let recovery_release_database = isolated_database(server.connect_options()).await;
    recovery_selector_releases_rejected_candidate_locks(&recovery_release_database).await;
    cleanup(recovery_release_database).await;

    let writer_first_database = isolated_database(server.connect_options()).await;
    claim_selector_writer_wins_drain_race(&writer_first_database).await;
    cleanup(writer_first_database).await;

    let drain_first_database = isolated_database(server.connect_options()).await;
    claim_selector_drain_wins_writer_race(&drain_first_database).await;
    cleanup(drain_first_database).await;

    let recovery_writer_first_database = isolated_database(server.connect_options()).await;
    recovery_selector_writer_wins_drain_race(&recovery_writer_first_database).await;
    cleanup(recovery_writer_first_database).await;

    let closed_claim_database = isolated_database(server.connect_options()).await;
    claim_selector_respects_global_fence(&closed_claim_database).await;
    cleanup(closed_claim_database).await;

    let closed_recovery_database = isolated_database(server.connect_options()).await;
    recovery_selector_respects_global_fence(&closed_recovery_database).await;
    cleanup(closed_recovery_database).await;

    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_selector_epoch_upgrade_preserves_capabilities_and_rerun_is_atomic() {
    let server = PostgresTestServer::start();
    let (database_name, administrator, pool) =
        pre_slot_writer_fence_database(&server, "st_re_es_up").await;
    let pool =
        apply_execution_selector_epoch_prerequisites(&server, &database_name, pool).await;
    let role = format!("st_re_selector_maint_{:x}", unique_suffix());
    create_execution_selector_role(&pool, &role, false).await;

    let capabilities_before = assert_execution_selector_capabilities(&pool, &role).await;
    let catalog_before = execution_selector_catalog_image(&pool).await;
    assert_eq!(
        execution_slot_writer_private_exposure(&pool, &role).await,
        (false, false, false)
    );
    let mut upgrade = pool.begin().await.unwrap();
    sqlx::raw_sql(EXECUTION_SELECTOR_SLOT_WRITER_EPOCH_MIGRATION)
        .execute(&mut *upgrade)
        .await
        .unwrap();
    upgrade.commit().await.unwrap();

    assert_eq!(
        assert_execution_selector_capabilities(&pool, &role).await,
        capabilities_before
    );
    assert_eq!(
        execution_slot_writer_private_exposure(&pool, &role).await,
        (false, false, false)
    );
    assert_eq!(
        slot_writer_fence_manifests(&pool).await,
        (true, true, true, true)
    );
    let catalog_after = execution_selector_catalog_image(&pool).await;
    assert_eq!(
        execution_selector_catalog_contract(&catalog_after),
        execution_selector_catalog_contract(&catalog_before)
    );
    assert_ne!(catalog_after[0].4, catalog_before[0].4);
    assert_ne!(catalog_after[1].4, catalog_before[1].4);

    let manifests_after = slot_writer_fence_manifests(&pool).await;
    let private_after = execution_slot_writer_private_exposure(&pool, &role).await;
    let mut rerun = pool.begin().await.unwrap();
    let error = sqlx::raw_sql(EXECUTION_SELECTOR_SLOT_WRITER_EPOCH_MIGRATION)
        .execute(&mut *rerun)
        .await
        .unwrap_err();
    rerun.rollback().await.unwrap();
    assert_database_error(
        &error,
        "RE001",
        "runtime_execution_selector_slot_writer_epoch_preflight_drift",
    );
    assert_eq!(
        execution_selector_catalog_image(&pool).await,
        catalog_after
    );
    assert_eq!(slot_writer_fence_manifests(&pool).await, manifests_after);
    assert_eq!(
        execution_slot_writer_private_exposure(&pool, &role).await,
        private_after
    );
    assert_eq!(
        assert_execution_selector_capabilities(&pool, &role).await,
        capabilities_before
    );

    drop_execution_slot_writer_role(&pool, &role).await;
    drop_slot_writer_fence_test_database(&database_name, administrator, pool).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_selector_epoch_upgrade_rejects_login_executor_atomically() {
    let server = PostgresTestServer::start();
    let (database_name, administrator, pool) =
        pre_slot_writer_fence_database(&server, "st_re_es_login").await;
    let pool =
        apply_execution_selector_epoch_prerequisites(&server, &database_name, pool).await;
    let role = format!("st_re_selector_login_{:x}", unique_suffix());
    create_execution_selector_role(&pool, &role, true).await;

    let capabilities_before = assert_execution_selector_capabilities(&pool, &role).await;
    let catalog_before = execution_selector_catalog_image(&pool).await;
    let manifests_before = slot_writer_fence_manifests(&pool).await;
    let private_before = execution_slot_writer_private_exposure(&pool, &role).await;
    let mut rejected = pool.begin().await.unwrap();
    let quiescence = sqlx::query_as::<_, (bool, i64, i64, i64)>(
        "SELECT role.rolcanlogin, \
            (SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_auth_members AS membership \
             WHERE membership.roleid = role.oid), \
            (SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_stat_activity AS activity \
             WHERE activity.datid = ( \
                    SELECT database_row.oid \
                    FROM pg_catalog.pg_database AS database_row \
                    WHERE database_row.datname = pg_catalog.current_database() \
                ) \
                AND activity.pid <> pg_catalog.pg_backend_pid() \
                AND activity.backend_type = 'client backend'), \
            (SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_prepared_xacts AS prepared \
             WHERE prepared.database = pg_catalog.current_database()) \
         FROM pg_catalog.pg_roles AS role \
         WHERE role.oid = pg_catalog.to_regrole($1)",
    )
    .bind(&role)
    .fetch_one(&mut *rejected)
    .await
    .unwrap();
    assert_eq!(quiescence, (true, 0, 0, 0));
    let error = sqlx::raw_sql(EXECUTION_SELECTOR_SLOT_WRITER_EPOCH_MIGRATION)
        .execute(&mut *rejected)
        .await
        .unwrap_err();
    rejected.rollback().await.unwrap();
    assert_database_error(
        &error,
        "RE001",
        "runtime_execution_selector_slot_writer_epoch_executor_not_quiesced",
    );
    assert_eq!(
        assert_execution_selector_capabilities(&pool, &role).await,
        capabilities_before
    );
    assert_eq!(
        execution_selector_catalog_image(&pool).await,
        catalog_before
    );
    assert_eq!(slot_writer_fence_manifests(&pool).await, manifests_before);
    assert_eq!(
        execution_slot_writer_private_exposure(&pool, &role).await,
        private_before
    );

    drop_execution_slot_writer_role(&pool, &role).await;
    drop_slot_writer_fence_test_database(&database_name, administrator, pool).await;
    drop(server);
}
