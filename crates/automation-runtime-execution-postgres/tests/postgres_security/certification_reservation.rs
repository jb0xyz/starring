#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn certification_reservation_is_canonical_replay_safe_and_dormant() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;
    cleanup(database).await;
    drop(server);
}

async fn certification_reservation_scenario(database: &IsolatedDatabase) {
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.starring_runtime_execution_database_readiness_v1()",
        )
        .fetch_one(&database.executor_pool)
        .await
        .unwrap(),
        1
    );

    let mut session =
        gateway_ready_session(database, "runtime-certification-reservation-controller").await;
    let gateway_ready = gateway_ready_attestation(database, &session).await;
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
            gateway_ready,
            automation_runtime_controller::RuntimeLiveMetadataV1 {
                runtime_build_revision:
                    automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                        CERTIFICATION_BUILD,
                    )
                    .unwrap(),
                panel_report_digest:
                    automation_runtime_controller::PanelReportDigestV1::parse(
                        CERTIFICATION_REPORT,
                    )
                    .unwrap(),
                gateway_shard_id:
                    automation_runtime_controller::GatewayShardIdV1::parse(
                        CERTIFICATION_SHARD,
                    )
                    .unwrap(),
            },
            Duration::from_millis(CERTIFICATION_LEASE_MILLISECONDS as u64),
    )
    .unwrap();
    let target = execution.snapshot.target.clone();
    let target_guild_id = target.guild_id.0.to_string();
    let process_identity = automation_runtime_convergence::RuntimeProcessIdentityV1 {
        target: target.clone(),
        runtime_generation: execution.snapshot.runtime_generation,
        process_instance_id: process_instance_id.clone(),
    };
    let panel = execution.snapshot.panel_certificate.as_ref().unwrap();
    let intent = automation_runtime_controller::RuntimeCertificationIntentV2 {
        action_id: request.action_id,
        operation_id:
            automation_runtime_controller::RuntimeCertificationOperationIdV2::parse(
                "00112233445566778899aabbccddeeff",
            )
            .unwrap(),
        guard: request.guard,
        target: target.clone(),
        binding_pin: automation_runtime_controller::RuntimeBindingPinV1 {
            tenant_id: execution.snapshot.identity.tenant_id.clone(),
            installation_id: execution.snapshot.identity.installation_id.clone(),
            installation_authority_revision:
                std::num::NonZeroU64::new(1).unwrap(),
            binding_revision: target.binding_revision,
            binding_fingerprint: target.binding_fingerprint.clone(),
        },
        process_identity: process_identity.clone(),
        gateway_owner_lease_id:
            automation_runtime_controller::RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id:
                    automation_runtime_controller::GatewayShardIdV1::parse(
                        CERTIFICATION_SHARD,
                    )
                    .unwrap(),
                process_instance_id: process_instance_id.clone(),
                lease_epoch: std::num::NonZeroU64::new(lease_epoch as u64).unwrap(),
                expected_build_revision:
                    automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                        CERTIFICATION_BUILD,
                    )
                    .unwrap(),
            },
        observed_owner_revision:
            std::num::NonZeroU64::new(owner_revision as u64).unwrap(),
        runtime_build_revision:
            automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                CERTIFICATION_BUILD,
            )
            .unwrap(),
        panel: automation_runtime_controller::RuntimePanelEvidenceV2 {
            certificate_id: panel.certificate_id.clone(),
            report_digest: panel.report_digest.clone(),
            process_identity,
            controller_fencing_token: execution.fencing_token,
        },
        serving_lease_for: Duration::from_millis(
            CERTIFICATION_LEASE_MILLISECONDS as u64,
        ),
    };
    let canonical =
        automation_runtime_controller::RuntimeCanonicalCertificationIntentV2::new(
            intent.clone(),
        )
        .unwrap();
    let reservation =
        automation_runtime_controller::RuntimeReservedCertificationIntentV2::new(
            &execution,
            canonical,
        )
        .unwrap();

    let before_absent = database_now(&database.owner_pool).await;
    let absent = raw_certification_reservation_observe(
        &database.owner_pool,
        reservation.operation_scope(),
    )
    .await;
    let after_absent = database_now(&database.owner_pool).await;
    assert_eq!(absent.0, "absent");
    assert!(absent.1.is_none());
    assert!(absent.2.is_none());
    assert!(absent.3 >= before_absent);
    assert!(absent.3 <= after_absent);

    let initial_slot_epoch = certification_slot_writer_epoch(
        &database.owner_pool,
        target_guild_id.as_str(),
        target.ruleset_key.as_str(),
    )
    .await;
    let isolation_error = raw_certification_reserve_result(
        &database.owner_pool,
        &reservation,
        None,
        false,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&isolation_error, "RX004");
    assert_eq!(
        certification_slot_writer_epoch(
            &database.owner_pool,
            target_guild_id.as_str(),
            target.ruleset_key.as_str(),
        )
        .await,
        initial_slot_epoch
    );

    let first =
        raw_certification_reserve(&database.owner_pool, &reservation, None).await;
    let reserved_slot_epoch = certification_slot_writer_epoch(
        &database.owner_pool,
        target_guild_id.as_str(),
        target.ruleset_key.as_str(),
    )
    .await;
    assert_eq!(reserved_slot_epoch, initial_slot_epoch + 1);
    assert_eq!(first.0, "reserved");
    assert_eq!(
        first.1.as_deref(),
        Some(reservation.operation_id().as_str())
    );
    assert_eq!(
        first.2.as_deref(),
        Some(reservation.certification_intent_bytes())
    );
    assert_eq!(
        first.3.as_deref(),
        Some(reservation.intent_fingerprint().as_str())
    );

    let replay =
        raw_certification_reserve(&database.owner_pool, &reservation, None).await;
    assert_eq!(replay, first);
    assert_eq!(
        certification_slot_writer_epoch(
            &database.owner_pool,
            target_guild_id.as_str(),
            target.ruleset_key.as_str(),
        )
        .await,
        reserved_slot_epoch
    );

    let observed = raw_certification_reservation_observe(
        &database.owner_pool,
        reservation.operation_scope(),
    )
    .await;
    assert_eq!(observed.0, "reserved");
    assert_eq!(
        observed.1.as_deref(),
        Some(reservation.operation_id().as_str())
    );
    assert_eq!(
        observed.2.as_deref(),
        Some(reservation.certification_intent_bytes())
    );

    let mut competing_intent = intent;
    competing_intent.operation_id =
        automation_runtime_controller::RuntimeCertificationOperationIdV2::parse(
            "ffeeddccbbaa99887766554433221100",
        )
        .unwrap();
    let competing =
        automation_runtime_controller::RuntimeReservedCertificationIntentV2::new(
            &execution,
            automation_runtime_controller::RuntimeCanonicalCertificationIntentV2::new(
                competing_intent,
            )
            .unwrap(),
        )
        .unwrap();
    let diverged =
        raw_certification_reserve(&database.owner_pool, &competing, None).await;
    assert_eq!(diverged.0, "diverged");
    assert!(diverged.1.is_none());
    assert!(diverged.2.is_none());
    assert!(diverged.3.is_none());

    let mut hostile_bytes = reservation.certification_intent_bytes().to_vec();
    hostile_bytes.push(b' ');
    let hostile_fingerprint = sqlx::query_scalar::<_, String>(
        "SELECT starring_runtime_private_v2.\
         starring_runtime_certification_intent_fingerprint_v2($1)",
    )
    .bind(&hostile_bytes)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let error = raw_certification_reserve_result(
        &database.owner_pool,
        &reservation,
        Some((&hostile_bytes, hostile_fingerprint.as_str())),
        true,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");

    let executor_observe_error = sqlx::query(
        "SELECT * FROM public.\
         starring_runtime_certification_reservation_observe_v2($1,$2,$3,$4,$5)",
    )
    .bind(reservation.operation_scope().scope().tenant_id.as_str())
    .bind(
        reservation
            .operation_scope()
            .scope()
            .installation_id
            .as_str(),
    )
    .bind(
        reservation
            .operation_scope()
            .scope()
            .deployment_id
            .as_str(),
    )
    .bind(reservation.operation_scope().deployment_revision().get() as i64)
    .bind(i64::from(
        reservation.operation_scope().convergence_attempt().get(),
    ))
    .fetch_optional(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&executor_observe_error, "42501");

    let executor_table_error = sqlx::query(
        "SELECT operation_id FROM public.runtime_certification_operations_v2",
    )
    .fetch_optional(&database.executor_pool)
    .await
    .unwrap_err();
    assert_sqlstate(&executor_table_error, "42501");

    for statement in [
        "INSERT INTO public.runtime_certification_operations_v2 \
         SELECT * FROM public.runtime_certification_operations_v2",
        "UPDATE public.runtime_certification_operations_v2 \
         SET intent_fingerprint = intent_fingerprint",
        "DELETE FROM public.runtime_certification_operations_v2",
        "TRUNCATE TABLE public.runtime_certification_operations_v2",
    ] {
        let mut transaction = database.owner_pool.begin().await.unwrap();
        let error = sqlx::query(statement)
            .execute(&mut *transaction)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "23514");
        transaction.rollback().await.unwrap();
    }

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
}

async fn raw_certification_reserve(
    pool: &PgPool,
    reservation: &automation_runtime_controller::RuntimeReservedCertificationIntentV2,
    proposed: Option<(&[u8], &str)>,
) -> (
    String,
    Option<String>,
    Option<Vec<u8>>,
    Option<String>,
) {
    raw_certification_reserve_result(pool, reservation, proposed, true)
        .await
        .unwrap()
}

async fn raw_certification_reserve_result(
    pool: &PgPool,
    reservation: &automation_runtime_controller::RuntimeReservedCertificationIntentV2,
    proposed: Option<(&[u8], &str)>,
    serializable: bool,
) -> Result<
    (
        String,
        Option<String>,
        Option<Vec<u8>>,
        Option<String>,
    ),
    sqlx::Error,
> {
    let intent = reservation.canonical_intent().intent();
    let guard = &intent.guard;
    let scope = &guard.scope;
    let target = &intent.target;
    let proposed_bytes =
        proposed.map_or(reservation.certification_intent_bytes(), |value| value.0);
    let proposed_fingerprint =
        proposed.map_or(reservation.intent_fingerprint().as_str(), |value| value.1);
    let mut transaction = pool.begin().await?;
    if serializable {
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
            .execute(&mut *transaction)
            .await?;
    }
    let result = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<Vec<u8>>,
            Option<String>,
        ),
    >(
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
    .bind(proposed_bytes)
    .bind(proposed_fingerprint)
    .fetch_one(&mut *transaction)
    .await;
    match result {
        Ok(row) => {
            transaction.commit().await?;
            Ok(row)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn certification_slot_writer_epoch(
    pool: &PgPool,
    guild_id: &str,
    ruleset_key: &str,
) -> i64 {
    sqlx::query_scalar(
        "SELECT writer_epoch \
         FROM public.runtime_slot_writer_fences_v2 \
         WHERE slot_guild_id = $1 AND slot_ruleset_key = $2",
    )
    .bind(guild_id)
    .bind(ruleset_key)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn raw_certification_reservation_observe(
    pool: &PgPool,
    scope:
        &automation_runtime_controller::RuntimeCertificationOperationScopeV2,
) -> (
    String,
    Option<String>,
    Option<Vec<u8>>,
    DateTime<Utc>,
) {
    let deployment_scope = scope.scope();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED READ WRITE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let row = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<Vec<u8>>,
            DateTime<Utc>,
        ),
    >(
        "SELECT outcome_name, operation_id, certification_intent_bytes, observed_at \
         FROM public.starring_runtime_certification_reservation_observe_v2(\
         $1,$2,$3,$4,$5)",
    )
    .bind(deployment_scope.tenant_id.as_str())
    .bind(deployment_scope.installation_id.as_str())
    .bind(deployment_scope.deployment_id.as_str())
    .bind(scope.deployment_revision().get() as i64)
    .bind(i64::from(scope.convergence_attempt().get()))
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    row
}
