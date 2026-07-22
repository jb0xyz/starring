const CERTIFICATION_BUILD: &str = "runtime:test";
const CERTIFICATION_SHARD: &str = "shard:0";
const CERTIFICATION_LEASE_MILLISECONDS: i64 = 200_000;
const CERTIFICATION_REPORT: &str =
    "9999999999999999999999999999999999999999999999999999999999999999";

#[derive(Clone)]
struct CertificationInput {
    guard: RuntimeExecutionGuardV1,
    gateway_ready: GatewayReadyAttestationV1,
    observed_snapshot: Value,
    mutation_clock: DateTime<Utc>,
    record: Value,
    record_bytes: String,
    attestation_id: String,
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_certification_is_canonical_utc_and_authority_fenced() {
    let server = PostgresTestServer::start();

    let authority_database = isolated_database(server.connect_options()).await;
    certification_replay_authority_and_canonicality_scenario(&authority_database).await;
    cleanup(authority_database).await;

    let recovery_database = isolated_database(server.connect_options()).await;
    certification_recovery_uses_utc_scenario(&recovery_database).await;
    cleanup(recovery_database).await;

    let observation_database = isolated_database(server.connect_options()).await;
    previous_serving_observation_rechecks_authority_scenario(&observation_database).await;
    cleanup(observation_database).await;

    drop(server);
}

async fn certification_replay_authority_and_canonicality_scenario(
    database: &IsolatedDatabase,
) {
    let session = gateway_ready_session(database, "runtime-certification-controller").await;
    let gateway_ready = gateway_ready_attestation(database, &session).await;
    let guard = session.execution_guard().unwrap();

    for noncanonical_ready_at in [
        "2026-07-22T24:00:00Z".to_string(),
        database_now(&database.owner_pool)
            .await
            .format("%Y-%m-%dT%H:%M:60Z")
            .to_string(),
        database_now(&database.owner_pool)
            .await
            .format("%Y-%m-%dT%H:%M:%S.000Z")
            .to_string(),
    ] {
        let mut gateway_value = serde_json::to_value(&gateway_ready).unwrap();
        gateway_value["ready_at"] = json!(noncanonical_ready_at);
        let mut transaction = database.executor_pool.begin().await.unwrap();
        let error = raw_certify_prepare(
            &mut transaction,
            &guard,
            gateway_value,
            300_000,
        )
        .await
        .unwrap_err();
        assert_sqlstate(&error, "RX002");
        transaction.rollback().await.unwrap();
    }

    let mut future_gateway = gateway_ready.clone();
    future_gateway.ready_at = database_now(&database.owner_pool).await + TimeDelta::seconds(5);
    let mut transaction = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_prepare(
        &mut transaction,
        &guard,
        serde_json::to_value(future_gateway).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    transaction.rollback().await.unwrap();

    let mut inexact_target = serde_json::to_value(&gateway_ready).unwrap();
    inexact_target["target"]["version"] = json!(1.0);
    let mut transaction = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_prepare(&mut transaction, &guard, inexact_target, 300_000)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RX002");
    transaction.rollback().await.unwrap();

    let mut oversized_gateway = serde_json::to_value(&gateway_ready).unwrap();
    oversized_gateway["target"]["padding"] = json!("x".repeat(5000));
    let mut transaction = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_prepare(&mut transaction, &guard, oversized_gateway, 300_000)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RX002");
    transaction.rollback().await.unwrap();

    let mut transaction = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut transaction,
        &guard,
        serde_json::to_value(&gateway_ready).unwrap(),
        300_000,
    )
    .await
    .unwrap();
    let valid = certification_input(&guard, gateway_ready.clone(), &prepared);
    let alias_bytes = valid.record_bytes.replacen(
        "\"installed_count\":1",
        "\"installed_count\":1.0",
        1,
    );
    assert_ne!(alias_bytes, valid.record_bytes);
    let alias_input = CertificationInput {
        record: serde_json::from_str(&alias_bytes).unwrap(),
        attestation_id: database_live_attestation_digest(&database.owner_pool, &alias_bytes).await,
        record_bytes: alias_bytes,
        ..valid
    };
    let error = raw_certify_commit(&mut transaction, &alias_input, 300_000)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RX004");
    transaction.rollback().await.unwrap();

    let mut transaction = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut transaction,
        &guard,
        serde_json::to_value(&gateway_ready).unwrap(),
        300_000,
    )
    .await
    .unwrap();
    assert_eq!(prepared.0, "apply");
    assert_eq!(transaction_timezone(&mut transaction).await, "UTC");
    let valid = certification_input(&guard, gateway_ready.clone(), &prepared);
    let mut noncanonical_record = valid.record.clone();
    noncanonical_record["live"]["activation"]["activated_at"] =
        json!(prepared.4.format("%Y-%m-%dT%H:%M:%S.000Z").to_string());
    let noncanonical_bytes = serde_json::to_string(&noncanonical_record).unwrap();
    let error = raw_certify_commit(
        &mut transaction,
        &CertificationInput {
            record: noncanonical_record,
            record_bytes: noncanonical_bytes,
            attestation_id: "a".repeat(64),
            ..valid.clone()
        },
        300_000,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    transaction.rollback().await.unwrap();

    let mut transaction = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut transaction,
        &guard,
        serde_json::to_value(&gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap();
    let applied_input = certification_input(&guard, gateway_ready, &prepared);
    let outcome = raw_certify_commit(
        &mut transaction,
        &applied_input,
        CERTIFICATION_LEASE_MILLISECONDS,
    )
        .await
        .unwrap();
    assert_eq!(outcome, "applied");
    assert_eq!(transaction_timezone(&mut transaction).await, "UTC");
    transaction.commit().await.unwrap();

    let mut oversized_gateway = serde_json::to_value(&applied_input.gateway_ready).unwrap();
    oversized_gateway["target"]["padding"] = json!("x".repeat(5000));
    let mut oversized_commit = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_commit_with_gateway(
        &mut oversized_commit,
        &applied_input,
        CERTIFICATION_LEASE_MILLISECONDS,
        oversized_gateway,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    oversized_commit.rollback().await.unwrap();

    let mut oversized_snapshot_input = applied_input.clone();
    oversized_snapshot_input.observed_snapshot = json!({"padding": "x".repeat(300_000)});
    let mut oversized_snapshot = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_commit(
        &mut oversized_snapshot,
        &oversized_snapshot_input,
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    oversized_snapshot.rollback().await.unwrap();

    let mut replay = database.executor_pool.begin().await.unwrap();
    let replay_prepared = raw_certify_prepare(
        &mut replay,
        &guard,
        serde_json::to_value(&applied_input.gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap();
    assert_eq!(replay_prepared.0, "replayed");
    let replay_input = CertificationInput {
        observed_snapshot: replay_prepared.1.0,
        mutation_clock: replay_prepared.3,
        ..applied_input.clone()
    };
    assert_eq!(
        raw_certify_commit(
            &mut replay,
            &replay_input,
            CERTIFICATION_LEASE_MILLISECONDS,
        )
            .await
            .unwrap(),
        "replayed"
    );
    replay.commit().await.unwrap();

    let mut corrupted = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "DROP TRIGGER runtime_serving_leases_validate_transition \
         ON public.runtime_serving_leases",
    )
    .execute(&mut *corrupted)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.runtime_serving_leases \
         DROP CONSTRAINT runtime_serving_leases_state_valid",
    )
    .execute(&mut *corrupted)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.runtime_serving_leases SET serving = FALSE \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .execute(&mut *corrupted)
    .await
    .unwrap();
    let error = raw_certify_prepare(
        &mut corrupted,
        &guard,
        serde_json::to_value(&applied_input.gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    corrupted.rollback().await.unwrap();

    let mut heartbeat_transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *heartbeat_transaction)
        .await
        .unwrap();
    let heartbeat = sqlx::query_scalar::<_, i64>(
        "SELECT revision \
         FROM public.starring_runtime_serving_heartbeat_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, $9 \
         )",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind(DEPLOYMENT)
    .bind(&applied_input.attestation_id)
    .bind(applied_input.gateway_ready.process_instance_id.as_str())
    .bind(i64::try_from(guard.runtime_generation.get()).unwrap())
    .bind(1_i64)
    .bind(1_i64)
    .bind(300_000_i64)
    .fetch_one(&mut *heartbeat_transaction)
    .await
    .unwrap();
    assert_eq!(heartbeat, 2);
    heartbeat_transaction.commit().await.unwrap();

    let mut advanced_prepare = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_prepare(
        &mut advanced_prepare,
        &guard,
        serde_json::to_value(&applied_input.gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX001");
    advanced_prepare.rollback().await.unwrap();

    let mut advanced_commit = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_commit(
        &mut advanced_commit,
        &applied_input,
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX001");
    advanced_commit.rollback().await.unwrap();

    rotate_current_authority(&database.owner_pool).await;
    let unchanged = persisted_deployment_image(&database.owner_pool).await;

    let mut prepare_replay = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_prepare(
        &mut prepare_replay,
        &guard,
        serde_json::to_value(&applied_input.gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX003");
    prepare_replay.rollback().await.unwrap();

    let mut commit_replay = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_commit(
        &mut commit_replay,
        &applied_input,
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX003");
    commit_replay.rollback().await.unwrap();
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
}

async fn certification_recovery_uses_utc_scenario(database: &IsolatedDatabase) {
    let session = gateway_ready_session(database, "runtime-recovery-controller").await;
    let gateway_ready = gateway_ready_attestation(database, &session).await;
    let guard = session.execution_guard().unwrap();
    let mut transaction = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut transaction,
        &guard,
        serde_json::to_value(&gateway_ready).unwrap(),
        1_000,
    )
    .await
    .unwrap();
    let input = certification_input(&guard, gateway_ready, &prepared);
    assert_eq!(
        raw_certify_commit(&mut transaction, &input, 1_000)
            .await
            .unwrap(),
        "applied"
    );
    transaction.commit().await.unwrap();

    let expiry = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT expires_at FROM public.runtime_serving_leases \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    wait_for_database_time(&database.owner_pool, expiry).await;

    let mut prepare_replay = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_prepare(
        &mut prepare_replay,
        &guard,
        serde_json::to_value(&input.gateway_ready).unwrap(),
        1_000,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX005");
    prepare_replay.rollback().await.unwrap();

    let mut commit_replay = database.executor_pool.begin().await.unwrap();
    let error = raw_certify_commit(&mut commit_replay, &input, 1_000)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RX005");
    commit_replay.rollback().await.unwrap();

    let mut recovery = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL TimeZone = 'Pacific/Chatham'")
        .execute(&mut *recovery)
        .await
        .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *recovery)
        .await
        .unwrap();
    let recovered = sqlx::query_as::<_, (String, Json<Value>, Json<Value>, DateTime<Utc>)>(
        "SELECT outcome_name, observed_snapshot, deployment_snapshot, recovered_at \
         FROM public.starring_runtime_execution_recover_stale_live_v1()",
    )
    .fetch_one(&mut *recovery)
    .await
    .unwrap();
    assert_eq!(recovered.0, "applied");
    assert_eq!(transaction_timezone(&mut recovery).await, "UTC");
    for timestamp in [
        recovered.2.0["last_live_recovery"]["evidence_at"]
            .as_str()
            .unwrap(),
        recovered.2.0["last_live_recovery"]["recovered_at"]
            .as_str()
            .unwrap(),
    ] {
        assert_eq!(
            DateTime::parse_from_rfc3339(timestamp)
                .unwrap()
                .offset()
                .local_minus_utc(),
            0
        );
    }
    recovery.commit().await.unwrap();
}

async fn previous_serving_observation_rechecks_authority_scenario(
    database: &IsolatedDatabase,
) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-observation-controller",
        Duration::from_secs(300),
    )
    .await;
    let preflight = PreflightAttestationV1 {
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        observed_runtime: None,
        checked_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptPreflight(preflight),
    )
    .await;
    mutate_applied(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::RequestDrain,
    )
    .await;
    let guard = session.execution_guard().unwrap();

    let mut observed = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL TimeZone = 'Pacific/Chatham'")
        .execute(&mut *observed)
        .await
        .unwrap();
    assert_eq!(
        raw_observe_previous_serving(&mut observed, &guard)
            .await
            .unwrap(),
        Some("absent".to_string())
    );
    assert_eq!(transaction_timezone(&mut observed).await, "UTC");
    observed.commit().await.unwrap();

    rotate_current_authority(&database.owner_pool).await;
    let mut drifted = database.executor_pool.begin().await.unwrap();
    assert_eq!(
        raw_observe_previous_serving(&mut drifted, &guard)
            .await
            .unwrap(),
        None
    );
    drifted.commit().await.unwrap();
}

async fn gateway_ready_session(
    database: &IsolatedDatabase,
    controller: &str,
) -> RuntimeConvergenceSessionV1 {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(&adapter, controller, Duration::from_secs(300)).await;
    advance_to_activation_applying(&database.owner_pool, &adapter, &mut session).await;
    let activation = ActivationAttestationV1 {
        activation_request_id: session.snapshot().identity.activation_request_id.clone(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        kind: ActivationOutcomeKindV1::Activated,
        activated_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptActivation(activation),
    )
    .await;
    mutate_applied(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::BeginPanelReconciliation,
    )
    .await;
    let certificate = PanelCertificateV1 {
        certificate_id: PanelCertificateId::parse("runtime-certification-panel").unwrap(),
        report_digest: PanelReportDigestV1::parse(CERTIFICATION_REPORT).unwrap(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        process_instance_id: ProcessInstanceId::parse("runtime-certification-process").unwrap(),
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
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate),
    )
    .await;
    session
}

async fn gateway_ready_attestation(
    database: &IsolatedDatabase,
    session: &RuntimeConvergenceSessionV1,
) -> GatewayReadyAttestationV1 {
    GatewayReadyAttestationV1 {
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        process_instance_id: session
            .snapshot()
            .panel_certificate
            .as_ref()
            .unwrap()
            .process_instance_id
            .clone(),
        kind: GatewayReadyKindV1::DiscordReady,
        ready_at: database_now(&database.owner_pool).await,
    }
}

fn certification_input(
    guard: &RuntimeExecutionGuardV1,
    gateway_ready: GatewayReadyAttestationV1,
    prepared: &(String, Json<Value>, i64, DateTime<Utc>, DateTime<Utc>),
) -> CertificationInput {
    let snapshot: RuntimeDeploymentSnapshotV1 =
        serde_json::from_value(prepared.1.0.clone()).unwrap();
    let record = RuntimeLiveAttestationRecordV1 {
        live: LiveAttestationV1 {
            target: snapshot.target,
            runtime_generation: snapshot.runtime_generation,
            process_instance_id: gateway_ready.process_instance_id.clone(),
            activation: snapshot.activation.unwrap(),
            panel_certificate: snapshot.panel_certificate.unwrap(),
            gateway_ready: gateway_ready.clone(),
            certified_at: prepared.4,
        },
        runtime_build_revision: RuntimeBuildRevisionV1::parse(CERTIFICATION_BUILD).unwrap(),
        panel_report_digest: PanelReportDigestV1::parse(CERTIFICATION_REPORT).unwrap(),
        gateway_shard_id: GatewayShardIdV1::parse(CERTIFICATION_SHARD).unwrap(),
        controller_fencing_token: guard.fencing_token,
        deployment_revision: guard.expected_revision.next().unwrap(),
    };
    let record_bytes = String::from_utf8(
        encode_runtime_live_attestation_record_v1(&record).unwrap(),
    )
    .unwrap();
    let attestation_id = runtime_live_attestation_digest_v1(&record)
        .unwrap()
        .as_str()
        .to_string();
    CertificationInput {
        guard: guard.clone(),
        gateway_ready,
        observed_snapshot: prepared.1.0.clone(),
        mutation_clock: prepared.3,
        record: serde_json::to_value(record).unwrap(),
        record_bytes,
        attestation_id,
    }
}

async fn raw_certify_prepare(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    guard: &RuntimeExecutionGuardV1,
    gateway_ready: Value,
    lease_milliseconds: i64,
) -> Result<(String, Json<Value>, i64, DateTime<Utc>, DateTime<Utc>), sqlx::Error> {
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut **transaction)
        .await?;
    sqlx::query_as(
        "SELECT preparation_name, observed_snapshot, convergence_attempt_no, \
            mutation_clock, certified_at \
         FROM public.starring_runtime_execution_certify_prepare_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13 \
         )",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(i64::try_from(guard.expected_revision.get()).unwrap())
    .bind(guard.controller_id.as_str())
    .bind(i64::try_from(guard.fencing_token.get()).unwrap())
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(i64::try_from(guard.runtime_generation.get()).unwrap())
    .bind(Json(gateway_ready))
    .bind(CERTIFICATION_BUILD)
    .bind(CERTIFICATION_REPORT)
    .bind(CERTIFICATION_SHARD)
    .bind(lease_milliseconds)
    .fetch_one(&mut **transaction)
    .await
}

async fn raw_certify_commit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: &CertificationInput,
    lease_milliseconds: i64,
) -> Result<String, sqlx::Error> {
    raw_certify_commit_with_gateway(
        transaction,
        input,
        lease_milliseconds,
        serde_json::to_value(&input.gateway_ready).unwrap(),
    )
    .await
}

async fn raw_certify_commit_with_gateway(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: &CertificationInput,
    lease_milliseconds: i64,
    gateway_ready: Value,
) -> Result<String, sqlx::Error> {
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut **transaction)
        .await?;
    sqlx::query_scalar(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_certify_commit_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
            $14, $15, $16, $17, $18 \
         )",
    )
    .bind(input.guard.scope.tenant_id.as_str())
    .bind(input.guard.scope.installation_id.as_str())
    .bind(input.guard.scope.deployment_id.as_str())
    .bind(i64::try_from(input.guard.expected_revision.get()).unwrap())
    .bind(input.guard.controller_id.as_str())
    .bind(i64::try_from(input.guard.fencing_token.get()).unwrap())
    .bind(i64::from(input.guard.convergence_attempt.get()))
    .bind(i64::try_from(input.guard.runtime_generation.get()).unwrap())
    .bind(Json(gateway_ready))
    .bind(CERTIFICATION_BUILD)
    .bind(CERTIFICATION_REPORT)
    .bind(CERTIFICATION_SHARD)
    .bind(lease_milliseconds)
    .bind(input.mutation_clock)
    .bind(Json(&input.observed_snapshot))
    .bind(&input.attestation_id)
    .bind(Json(&input.record))
    .bind(&input.record_bytes)
    .fetch_one(&mut **transaction)
    .await
}

async fn transaction_timezone(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> String {
    sqlx::query_scalar("SELECT pg_catalog.current_setting('TimeZone')")
        .fetch_one(&mut **transaction)
        .await
        .unwrap()
}

async fn raw_observe_previous_serving(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    guard: &RuntimeExecutionGuardV1,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT state_name \
         FROM public.starring_runtime_observe_previous_serving_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15 \
         )",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(i64::try_from(guard.expected_revision.get()).unwrap())
    .bind(guard.controller_id.as_str())
    .bind(i64::try_from(guard.fencing_token.get()).unwrap())
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(i64::try_from(guard.runtime_generation.get()).unwrap())
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(1_i64)
    .bind(CONTENT_HASH)
    .bind(1_i64)
    .bind(BINDING_FINGERPRINT)
    .bind(Option::<Json<Value>>::None)
    .fetch_optional(&mut **transaction)
    .await
}

async fn database_live_attestation_digest(pool: &PgPool, record_bytes: &str) -> String {
    sqlx::query_scalar(
        "WITH framed AS ( \
            SELECT pg_catalog.convert_to( \
                    'starring.runtime.live_attestation.v1', 'UTF8' \
                ) || pg_catalog.decode('00', 'hex') AS domain_bytes, \
                pg_catalog.convert_to($1, 'UTF8') AS record_bytes \
         ) \
         SELECT pg_catalog.encode(pg_catalog.sha256( \
            pg_catalog.int8send(pg_catalog.octet_length(domain_bytes)::BIGINT) \
            || domain_bytes \
            || pg_catalog.int8send(pg_catalog.octet_length(record_bytes)::BIGINT) \
            || record_bytes \
         ), 'hex') FROM framed",
    )
    .bind(record_bytes)
    .fetch_one(pool)
    .await
    .unwrap()
}
