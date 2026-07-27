type LegacyFenceReservationOutcome = (String, Option<String>, Option<Vec<u8>>, Option<String>);

struct LegacyCertificationFenceFixture {
    guard: RuntimeExecutionGuardV1,
    renewal: automation_runtime_controller::RuntimeRenewExecutionV1,
    gateway_ready: GatewayReadyAttestationV1,
    reservation: automation_runtime_controller::RuntimeReservedCertificationIntentV2,
    commit_input: CertificationInput,
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires PostgreSQL test authority"]
async fn legacy_certification_reservation_fence_is_permanent_and_atomic() {
    let server = PostgresTestServer::start();

    let fence_database = isolated_database(server.connect_options()).await;
    legacy_certification_reservation_fence_scenario(&fence_database).await;
    cleanup(fence_database).await;

    let recover_database = isolated_database(server.connect_options()).await;
    historical_reserved_live_recovery_is_fenced(&recover_database).await;
    cleanup(recover_database).await;

    let race_database = isolated_database(server.connect_options()).await;
    certification_reservation_wins_legacy_renew_race(&race_database).await;
    cleanup(race_database).await;

    drop(server);
}

async fn legacy_certification_reservation_fence_scenario(database: &IsolatedDatabase) {
    let controller = "runtime-legacy-certification-fence-controller";
    let fixture = legacy_certification_fence_fixture(
        database,
        controller,
        "11112222333344445555666677778888",
    )
    .await;
    let first = raw_certification_reserve(&database.owner_pool, &fixture.reservation, None).await;
    assert_eq!(first.0, "reserved");
    let baseline = execution_slot_writer_image(database).await;

    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT phase \
             FROM public.runtime_deployments \
             WHERE deployment_id = $1",
        )
        .bind(DEPLOYMENT)
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        "awaiting_gateway_ready"
    );

    let replay = raw_certification_reserve(&database.owner_pool, &fixture.reservation, None).await;
    assert_eq!(replay, first);
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let executor_table_error = sqlx::query(
        "SELECT operation_id \
         FROM public.runtime_certification_operations_v2",
    )
    .fetch_optional(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&executor_table_error, "42501");
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let mut claim = database.executor_pool.begin().await.unwrap();
    assert_eq!(
        raw_selector_claim(&mut claim, controller).await.unwrap(),
        None
    );
    claim.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let mut recover = database.executor_pool.begin().await.unwrap();
    assert_eq!(raw_selector_recover(&mut recover).await.unwrap(), None);
    recover.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let mut renew = database.executor_pool.begin().await.unwrap();
    let renew_error = raw_epoch_renew(&mut renew, &fixture.renewal)
        .await
        .unwrap_err();
    assert_sqlstate(&renew_error, "RX001");
    renew.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let mut mutate = database.executor_pool.begin().await.unwrap();
    let mutate_error = raw_epoch_mutate(&mut mutate, &fixture.guard, "cancel", &json!({}))
        .await
        .unwrap_err();
    assert_sqlstate(&mutate_error, "RX001");
    mutate.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let mut prepare = database.executor_pool.begin().await.unwrap();
    let prepare_error = raw_certify_prepare(
        &mut prepare,
        &fixture.guard,
        serde_json::to_value(&fixture.gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&prepare_error, "RX001");
    prepare.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    let mut commit = database.executor_pool.begin().await.unwrap();
    let commit_error = raw_certify_commit(
        &mut commit,
        &fixture.commit_input,
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&commit_error, "RX001");
    commit.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);

    assert_eq!(
        raw_certification_reserve(&database.owner_pool, &fixture.reservation, None,).await,
        first
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.runtime_certification_operations_v2",
        )
        .fetch_one(&database.owner_pool)
        .await
        .unwrap(),
        1
    );
    assert_eq!(execution_slot_writer_image(database).await, baseline);
}

async fn historical_reserved_live_recovery_is_fenced(database: &IsolatedDatabase) {
    let fixture = legacy_certification_fence_fixture(
        database,
        "runtime-legacy-certification-recover-controller",
        "aaaabbbbccccddddeeeeffff00001111",
    )
    .await;
    let mut certification = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut certification,
        &fixture.guard,
        serde_json::to_value(&fixture.gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap();
    let input = certification_input(&fixture.guard, fixture.gateway_ready.clone(), &prepared);
    assert_eq!(
        raw_certify_commit(&mut certification, &input, CERTIFICATION_LEASE_MILLISECONDS,)
            .await
            .unwrap(),
        "applied"
    );
    certification.commit().await.unwrap();

    insert_historical_legacy_fence_reservation(&database.owner_pool, &fixture.reservation).await;
    disconnect_product_drain_serving_lease(database).await;
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT deployment.phase = 'live' \
                    AND NOT lease.connected \
                    AND NOT lease.serving \
             FROM public.runtime_deployments AS deployment \
             JOIN public.runtime_serving_leases AS lease \
               ON lease.tenant_id = deployment.tenant_id \
              AND lease.installation_id = deployment.installation_id \
              AND lease.deployment_id = deployment.deployment_id \
             WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap());
    let baseline = execution_slot_writer_image(database).await;

    let mut recover = database.executor_pool.begin().await.unwrap();
    assert_eq!(raw_selector_recover(&mut recover).await.unwrap(), None);
    recover.rollback().await.unwrap();
    assert_eq!(execution_slot_writer_image(database).await, baseline);
}

async fn certification_reservation_wins_legacy_renew_race(database: &IsolatedDatabase) {
    let fixture = legacy_certification_fence_fixture(
        database,
        "runtime-legacy-certification-race-controller",
        "88887777666655554444333322221111",
    )
    .await;
    let before = execution_slot_writer_image(database).await;

    let mut reservation_transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *reservation_transaction)
        .await
        .unwrap();
    let reservation_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *reservation_transaction)
        .await
        .unwrap();
    let reserved =
        raw_legacy_fence_reserve_in_transaction(&mut reservation_transaction, &fixture.reservation)
            .await
            .unwrap();
    assert_eq!(reserved.0, "reserved");

    let mut writer = database.executor_pool.begin().await.unwrap();
    let writer_pid = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *writer)
        .await
        .unwrap();
    let (writer_result, ()) = tokio::join!(raw_epoch_renew(&mut writer, &fixture.renewal), async {
        wait_for_advisory_lock_blocked_by(&database.owner_pool, writer_pid, reservation_pid).await;
        reservation_transaction.commit().await.unwrap();
    });
    let writer_error = writer_result.unwrap_err();
    assert_sqlstate(&writer_error, "RX001");
    writer.rollback().await.unwrap();

    let mut expected = before;
    expected.0 .2 += 1;
    assert_eq!(execution_slot_writer_image(database).await, expected);
    assert_eq!(
        raw_certification_reserve(&database.owner_pool, &fixture.reservation, None,).await,
        reserved
    );
    assert_eq!(execution_slot_writer_image(database).await, expected);
}

async fn insert_historical_legacy_fence_reservation(
    pool: &PgPool,
    reservation: &automation_runtime_controller::RuntimeReservedCertificationIntentV2,
) {
    let intent = reservation.canonical_intent().intent();
    let guard = &intent.guard;
    let mut transaction = pool.begin().await.unwrap();
    let settings = [
        (
            "starring.runtime_certification_reservation_action_v2",
            "insert".to_string(),
        ),
        (
            "starring.runtime_certification_reservation_operation_id_v2",
            intent.operation_id.as_str().to_string(),
        ),
        (
            "starring.runtime_certification_reservation_tenant_id_v2",
            guard.scope.tenant_id.as_str().to_string(),
        ),
        (
            "starring.runtime_certification_reservation_installation_id_v2",
            guard.scope.installation_id.as_str().to_string(),
        ),
        (
            "starring.runtime_certification_reservation_deployment_id_v2",
            guard.scope.deployment_id.as_str().to_string(),
        ),
        (
            "starring.runtime_certification_reservation_revision_v2",
            guard.expected_revision.get().to_string(),
        ),
        (
            "starring.runtime_certification_reservation_attempt_v2",
            guard.convergence_attempt.get().to_string(),
        ),
        (
            "starring.runtime_certification_reservation_fingerprint_v2",
            reservation.intent_fingerprint().as_str().to_string(),
        ),
    ];
    for (name, value) in settings {
        sqlx::query("SELECT pg_catalog.set_config($1, $2, TRUE)")
            .bind(name)
            .bind(value)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO public.runtime_certification_operations_v2 (\
            operation_id, tenant_id, installation_id, deployment_id, \
            deployment_revision, convergence_attempt_no, \
            certification_intent_bytes, intent_fingerprint\
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(intent.operation_id.as_str())
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(guard.expected_revision.get() as i64)
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(reservation.certification_intent_bytes())
    .bind(reservation.intent_fingerprint().as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn legacy_certification_fence_fixture(
    database: &IsolatedDatabase,
    controller: &str,
    operation_id: &str,
) -> LegacyCertificationFenceFixture {
    let mut session = gateway_ready_session(database, controller).await;
    let gateway_ready = gateway_ready_attestation(database, &session).await;
    let guard = session.execution_guard().unwrap().clone();
    let mut renewal_session = session.clone();
    let renewal = renewal_session
        .begin_renewal(Duration::from_secs(400))
        .unwrap();

    let mut prepare = database.executor_pool.begin().await.unwrap();
    let prepared = raw_certify_prepare(
        &mut prepare,
        &guard,
        serde_json::to_value(&gateway_ready).unwrap(),
        CERTIFICATION_LEASE_MILLISECONDS,
    )
    .await
    .unwrap();
    let commit_input = certification_input(&guard, gateway_ready.clone(), &prepared);
    prepare.rollback().await.unwrap();

    let execution = session.current_execution_receipt().unwrap();
    let process_instance_id = session
        .snapshot()
        .panel_certificate
        .as_ref()
        .unwrap()
        .process_instance_id
        .clone();
    let (lease_epoch, owner_revision) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT lease_epoch, owner_revision \
         FROM public.starring_runtime_gateway_owner_acquire_v1($1,$2,$3,$4)",
    )
    .bind(CERTIFICATION_SHARD)
    .bind(process_instance_id.as_str())
    .bind(CERTIFICATION_BUILD)
    .bind(300_000_i64)
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    let request = session
        .begin_certification(
            gateway_ready.clone(),
            automation_runtime_controller::RuntimeLiveMetadataV1 {
                runtime_build_revision:
                    automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                        CERTIFICATION_BUILD,
                    )
                    .unwrap(),
                panel_report_digest: automation_runtime_controller::PanelReportDigestV1::parse(
                    CERTIFICATION_REPORT,
                )
                .unwrap(),
                gateway_shard_id: automation_runtime_controller::GatewayShardIdV1::parse(
                    CERTIFICATION_SHARD,
                )
                .unwrap(),
            },
            Duration::from_millis(CERTIFICATION_LEASE_MILLISECONDS as u64),
        )
        .unwrap();
    let target = execution.snapshot.target.clone();
    let process_identity = automation_runtime_convergence::RuntimeProcessIdentityV1 {
        target: target.clone(),
        runtime_generation: execution.snapshot.runtime_generation,
        process_instance_id: process_instance_id.clone(),
    };
    let panel = execution.snapshot.panel_certificate.as_ref().unwrap();
    let intent = automation_runtime_controller::RuntimeCertificationIntentV2 {
        action_id: request.action_id,
        operation_id: automation_runtime_controller::RuntimeCertificationOperationIdV2::parse(
            operation_id,
        )
        .unwrap(),
        guard: request.guard,
        target: target.clone(),
        binding_pin: automation_runtime_controller::RuntimeBindingPinV1 {
            tenant_id: execution.snapshot.identity.tenant_id.clone(),
            installation_id: execution.snapshot.identity.installation_id.clone(),
            installation_authority_revision: std::num::NonZeroU64::new(1).unwrap(),
            binding_revision: target.binding_revision,
            binding_fingerprint: target.binding_fingerprint.clone(),
        },
        process_identity: process_identity.clone(),
        gateway_owner_lease_id: automation_runtime_controller::RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: automation_runtime_controller::GatewayShardIdV1::parse(
                CERTIFICATION_SHARD,
            )
            .unwrap(),
            process_instance_id: process_instance_id.clone(),
            lease_epoch: std::num::NonZeroU64::new(lease_epoch as u64).unwrap(),
            expected_build_revision: automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                CERTIFICATION_BUILD,
            )
            .unwrap(),
        },
        observed_owner_revision: std::num::NonZeroU64::new(owner_revision as u64).unwrap(),
        runtime_build_revision: automation_runtime_controller::RuntimeBuildRevisionV1::parse(
            CERTIFICATION_BUILD,
        )
        .unwrap(),
        panel: automation_runtime_controller::RuntimePanelEvidenceV2 {
            certificate_id: panel.certificate_id.clone(),
            report_digest: panel.report_digest.clone(),
            process_identity,
            controller_fencing_token: execution.fencing_token,
        },
        serving_lease_for: Duration::from_millis(CERTIFICATION_LEASE_MILLISECONDS as u64),
    };
    let canonical =
        automation_runtime_controller::RuntimeCanonicalCertificationIntentV2::new(intent).unwrap();
    let reservation = automation_runtime_controller::RuntimeReservedCertificationIntentV2::new(
        &execution, canonical,
    )
    .unwrap();

    LegacyCertificationFenceFixture {
        guard,
        renewal,
        gateway_ready,
        reservation,
        commit_input,
    }
}

async fn raw_legacy_fence_reserve_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    reservation: &automation_runtime_controller::RuntimeReservedCertificationIntentV2,
) -> Result<LegacyFenceReservationOutcome, sqlx::Error> {
    let intent = reservation.canonical_intent().intent();
    let guard = &intent.guard;
    let scope = &guard.scope;
    let target = &intent.target;
    sqlx::query_as(
        "SELECT outcome_name, operation_id, certification_intent_bytes, \
         intent_fingerprint \
         FROM public.starring_runtime_certification_reserve_intent_v2(\
         $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,\
         $19,$20,$21,$22,$23,$24,$25,$26,$27)",
    )
    .bind(intent.action_id.get() as i64)
    .bind(intent.operation_id.as_str())
    .bind(scope.tenant_id.as_str())
    .bind(scope.installation_id.as_str())
    .bind(scope.deployment_id.as_str())
    .bind(guard.expected_revision.get() as i64)
    .bind(guard.controller_id.as_str())
    .bind(guard.fencing_token.get() as i64)
    .bind(guard.runtime_generation.get() as i64)
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(target.guild_id.0.to_string())
    .bind(target.ruleset_key.as_str())
    .bind(i64::from(target.version.get()))
    .bind(target.content_hash.to_hex())
    .bind(target.binding_revision.get() as i64)
    .bind(target.binding_fingerprint.as_str())
    .bind(intent.binding_pin.installation_authority_revision.get() as i64)
    .bind(intent.process_identity.process_instance_id.as_str())
    .bind(intent.gateway_owner_lease_id.gateway_shard_id.as_str())
    .bind(intent.gateway_owner_lease_id.lease_epoch.get() as i64)
    .bind(intent.observed_owner_revision.get() as i64)
    .bind(intent.runtime_build_revision.as_str())
    .bind(intent.panel.certificate_id.as_str())
    .bind(intent.panel.report_digest.as_str())
    .bind(intent.serving_lease_for.as_millis() as i64)
    .bind(reservation.certification_intent_bytes())
    .bind(reservation.intent_fingerprint().as_str())
    .fetch_one(&mut **transaction)
    .await
}
