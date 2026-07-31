#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn runtime_certification_v2_preserves_ready_semantics_and_exact_replay() {
    let server = PostgresTestServer::start();

    for (kind, expected_v1_kind) in [
        (
            automation_runtime_controller::RuntimeGatewayReadyKindV2::Ready,
            GatewayReadyKindV1::DiscordReady,
        ),
        (
            automation_runtime_controller::RuntimeGatewayReadyKindV2::Resumed,
            GatewayReadyKindV1::DiscordResumed,
        ),
    ] {
        let database = isolated_database(server.connect_options()).await;
        runtime_certification_ready_semantics_scenario(&database, kind, expected_v1_kind).await;
        cleanup(database).await;
    }

    drop(server);
}

async fn runtime_certification_ready_semantics_scenario(
    database: &IsolatedDatabase,
    kind: automation_runtime_controller::RuntimeGatewayReadyKindV2,
    expected_v1_kind: GatewayReadyKindV1,
) {
    let adapter = verified_execution_adapter(database).await;
    let mut session =
        gateway_ready_session(database, "runtime-certification-ready-semantics").await;
    let execution = session.current_execution_receipt().unwrap();
    let awaiting_snapshot = execution.snapshot.clone();
    let panel = awaiting_snapshot.panel_certificate.as_ref().unwrap();
    let process_identity = automation_runtime_convergence::RuntimeProcessIdentityV1 {
        target: awaiting_snapshot.target.clone(),
        runtime_generation: awaiting_snapshot.runtime_generation,
        process_instance_id: panel.process_instance_id.clone(),
    };
    let owner_request = RuntimeAcquireGatewayOwnerLeaseV1 {
        gateway_shard_id: GatewayShardIdV1::parse(CERTIFICATION_SHARD).unwrap(),
        process_instance_id: process_identity.process_instance_id.clone(),
        expected_build_revision: RuntimeBuildRevisionV1::parse(CERTIFICATION_BUILD).unwrap(),
        lease_for: RuntimeGatewayOwnerLeaseDurationV1::new(Duration::from_secs(300)).unwrap(),
    };
    let RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(owner_receipt) =
        RuntimeGatewayOwnerLeasePortV1::acquire_gateway_owner(&adapter, owner_request)
            .await
            .unwrap()
    else {
        panic!("certification owner acquisition must win")
    };
    let owner_revision = owner_receipt.owner_revision.get();
    let gateway_owner_lease_id = owner_receipt.lease_id.clone();
    let reservation = session
        .begin_certification_reservation_v2(
            automation_runtime_controller::RuntimeCertificationReservationInputV2 {
                operation_id:
                    automation_runtime_controller::RuntimeCertificationOperationIdV2::parse(
                        "102132435465768798a9bacbdcedfe0f",
                    )
                    .unwrap(),
                binding_pin: automation_runtime_controller::RuntimeBindingPinV1 {
                    tenant_id: awaiting_snapshot.identity.tenant_id.clone(),
                    installation_id: awaiting_snapshot.identity.installation_id.clone(),
                    installation_authority_revision: std::num::NonZeroU64::MIN,
                    binding_revision: awaiting_snapshot.target.binding_revision,
                    binding_fingerprint: awaiting_snapshot.target.binding_fingerprint.clone(),
                },
                gateway_owner_lease_id: gateway_owner_lease_id.clone(),
                observed_owner_revision: owner_receipt.owner_revision,
                runtime_build_revision:
                    automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                        CERTIFICATION_BUILD,
                    )
                    .unwrap(),
                panel: automation_runtime_controller::RuntimePanelEvidenceV2 {
                    certificate_id: panel.certificate_id.clone(),
                    report_digest: panel.report_digest.clone(),
                    process_identity: process_identity.clone(),
                    controller_fencing_token: execution.fencing_token,
                },
                serving_lease_for: Duration::from_millis(
                    CERTIFICATION_LEASE_MILLISECONDS as u64,
                ),
            },
        )
        .unwrap();
    let reservation_outcome =
        automation_runtime_worker::RuntimeCertificationReservationPortV2::reserve_certification_intent(
            &adapter,
            reservation,
        )
        .await
        .unwrap();
    let reservation_authority = session
        .apply_certification_reservation_v2(reservation_outcome)
        .unwrap();
    let reserved = automation_runtime_worker::RuntimeReservedCertificationV2::
        from_reservation_authority(reservation_authority);
    let prepared = reserved.prepare(&adapter).await.unwrap();
    let barrier_id = automation_runtime_controller::RuntimeBarrierIdV1::parse(
        "ffeeddccbbaa99887766554433221100",
    )
    .unwrap();
    let coordinator_generation = std::num::NonZeroU64::new(8).unwrap();
    let connection_epoch = std::num::NonZeroU64::new(9).unwrap();
    let admission_revision = std::num::NonZeroU64::new(10).unwrap();
    let connected_sequence = automation_runtime_controller::RuntimeGatewayAdmissionSequenceV2::new(
        std::num::NonZeroU64::new(11).unwrap(),
    );
    let pause_sequence = automation_runtime_controller::RuntimeGatewayAdmissionSequenceV2::new(
        std::num::NonZeroU64::new(12).unwrap(),
    );
    let resume_sequence = automation_runtime_controller::RuntimeGatewayAdmissionSequenceV2::new(
        std::num::NonZeroU64::new(13).unwrap(),
    );
    let paused_gateway = automation_runtime_worker::RuntimePausedGatewayObservationV2::new(
        automation_runtime_worker::RuntimeGatewayCoordinatorGenerationV2::new(
            coordinator_generation,
        ),
        process_identity.process_instance_id.clone(),
        connection_epoch,
        kind,
        admission_revision,
        automation_runtime_worker::RuntimePausedGatewaySequenceV2::new(
            pause_sequence,
            connected_sequence,
            None,
        )
        .unwrap(),
    );
    let route_admission = automation_runtime_controller::RuntimeRouteAdmissionAttestationV2 {
        barrier_id: barrier_id.clone(),
        pause: automation_runtime_controller::RuntimeBarrierPauseWitnessV2 {
            coordinator_generation,
            connection_epoch,
            paused_admission_revision: admission_revision,
            pause_sequence,
        },
        gateway: automation_runtime_controller::RuntimeGatewayReadyAttestationV2 {
            process_instance_id: process_identity.process_instance_id.clone(),
            connection_epoch,
            kind,
            admission_revision,
            connected_event_sequence: connected_sequence,
            resume_sequence,
        },
        gateway_owner_lease_id,
        attested_owner_revision: owner_receipt.owner_revision,
        route: automation_runtime_controller::RuntimeServingRouteAttestationV2 {
            identity: process_identity,
            controller_fencing_token: execution.fencing_token,
            route_incarnation: std::num::NonZeroU64::new(14).unwrap(),
            activation_sequence: std::num::NonZeroU64::new(15).unwrap(),
        },
    };
    let job = prepared
        .complete_barrier_b_v2(barrier_id, paused_gateway, route_admission)
        .unwrap()
        .authorize_finalization()
        .into_owned_job();
    let lookup = job.lookup();
    let committed = match job.run().await {
        automation_runtime_worker::RuntimeCertificationFinalizationOutcomeV2::Committed(
            committed,
        ) => committed,
        other => panic!("expected committed certification, got {other:?}"),
    };
    let (canonical, receipt) = committed.into_parts();

    assert!(matches!(
        receipt.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    assert!(matches!(
        receipt.snapshot.phase,
        RuntimeDeploymentPhaseV1::Live
    ));
    let gateway_ready = receipt.snapshot.gateway_ready.as_ref().unwrap();
    let live_gateway_ready = &receipt.snapshot.live.as_ref().unwrap().gateway_ready;
    assert_eq!(gateway_ready, live_gateway_ready);
    assert_eq!(gateway_ready.kind, expected_v1_kind);
    assert_eq!(receipt.route_admission.gateway.kind, kind);
    let serving = session
        .apply_certification_v2(canonical.clone(), receipt.clone())
        .unwrap();
    assert!(serving.connected);
    assert!(serving.serving);

    let replay = raw_runtime_certification_v2_replay(database, &canonical).await;
    assert_eq!(replay.0, "replayed");
    assert_eq!(
        serde_json::from_value::<RuntimeDeploymentSnapshotV1>(replay.1.0.clone()).unwrap(),
        awaiting_snapshot
    );
    assert_eq!(
        serde_json::from_value::<RuntimeDeploymentSnapshotV1>(replay.2.0.clone()).unwrap(),
        receipt.snapshot
    );
    let request_value: Value =
        serde_json::from_slice(canonical.certification_request_bytes()).unwrap();
    assert_eq!(replay.3.0, request_value["route_admission"]);
    assert_eq!(replay.4, canonical.request_digest().as_str());
    assert_eq!(replay.5, canonical.live_attestation_digest().as_str());

    let projection = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT gateway_ready_kind, \
            v2_route_admission #>> '{gateway,kind}', \
            v2_certified_snapshot #>> '{gateway_ready,kind}', \
            v2_certified_snapshot #>> '{live,gateway_ready,kind}' \
         FROM public.runtime_attestations \
         WHERE v2_operation_id = $1",
    )
    .bind(canonical.request().intent.operation_id.as_str())
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let expected_v1_kind_text = match expected_v1_kind {
        GatewayReadyKindV1::DiscordReady => "discord_ready",
        GatewayReadyKindV1::DiscordResumed => "discord_resumed",
    };
    let expected_v2_kind_text = match kind {
        automation_runtime_controller::RuntimeGatewayReadyKindV2::Ready => "ready",
        automation_runtime_controller::RuntimeGatewayReadyKindV2::Resumed => "resumed",
    };
    assert_eq!(projection.0, expected_v1_kind_text);
    assert_eq!(projection.1, expected_v2_kind_text);
    assert_eq!(projection.2, expected_v1_kind_text);
    assert_eq!(projection.3, expected_v1_kind_text);

    let first_observation =
        automation_runtime_worker::RuntimeLiveCertificationPortV2::observe_live_v2(
            &adapter,
            lookup.clone(),
        )
        .await
        .unwrap();
    let second_observation =
        automation_runtime_worker::RuntimeLiveCertificationPortV2::observe_live_v2(
            &adapter, lookup,
        )
        .await
        .unwrap();
    assert_eq!(first_observation, second_observation);
    match first_observation {
        automation_runtime_controller::RuntimeCertificationObservationV2::Committed(observed) => {
            assert!(matches!(
                observed.outcome,
                TransitionOutcomeV1::Replayed { .. }
            ));
            assert_eq!(observed.snapshot, receipt.snapshot);
            assert_eq!(observed.request_digest, receipt.request_digest);
            assert_eq!(observed.attestation_digest, receipt.attestation_digest);
        }
        other => panic!("expected committed replay observation, got {other:?}"),
    }

    if matches!(
        kind,
        automation_runtime_controller::RuntimeGatewayReadyKindV2::Ready
    ) {
        runtime_serving_owner_successor_scenario(
            database,
            &adapter,
            owner_receipt,
            &receipt,
            owner_revision,
        )
        .await;
    }
}

async fn runtime_serving_owner_successor_scenario(
    database: &IsolatedDatabase,
    adapter: &PostgresRuntimeExecutionV1,
    owner_receipt: automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1,
    certification: &automation_runtime_controller::RuntimeCertificationReceiptV2,
    attested_owner_revision: u64,
) {
    let equal_request = certification_ingress_acknowledgement_request(
        None,
        owner_receipt.clone(),
        certification.route_admission.gateway.clone(),
    );
    let equal_acknowledgement =
        publish_ingress_acknowledgement(&database.executor_pool, &equal_request)
            .await
            .unwrap();
    assert_eq!(equal_acknowledgement.outcome_name, "applied");
    let equal_error = raw_runtime_serving_heartbeat_v2(
        &database.owner_pool,
        &certification.serving.identity,
        i64::try_from(certification.serving.identity.revision.get()).unwrap(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        equal_error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some("RS001"),
        "{equal_error:?}"
    );

    let first_successor = renew_certification_owner(adapter, &owner_receipt).await;
    assert_eq!(
        first_successor.owner_revision.get(),
        attested_owner_revision + 1
    );
    let attested_acknowledgement_error = raw_runtime_serving_heartbeat_v2(
        &database.owner_pool,
        &certification.serving.identity,
        i64::try_from(certification.serving.identity.revision.get()).unwrap(),
    )
    .await
    .unwrap_err();
    assert_sqlstate(&attested_acknowledgement_error, "RS001");
    let first_request = certification_ingress_acknowledgement_request(
        acknowledgement_revision(&equal_acknowledgement),
        first_successor.clone(),
        certification.route_admission.gateway.clone(),
    );
    let first_acknowledgement =
        publish_ingress_acknowledgement(&database.executor_pool, &first_request)
            .await
            .unwrap();
    assert_eq!(first_acknowledgement.outcome_name, "applied");
    let first_serving_revision = raw_runtime_serving_heartbeat_v2(
        &database.owner_pool,
        &certification.serving.identity,
        i64::try_from(certification.serving.identity.revision.get()).unwrap(),
    )
    .await
    .unwrap();

    let second_successor = renew_certification_owner(adapter, &first_successor).await;
    let rollover_serving_revision = raw_runtime_serving_heartbeat_v2(
        &database.owner_pool,
        &certification.serving.identity,
        first_serving_revision,
    )
    .await
    .unwrap();
    let third_successor = renew_certification_owner(adapter, &second_successor).await;
    let stale_acknowledgement_error = raw_runtime_serving_heartbeat_v2(
        &database.owner_pool,
        &certification.serving.identity,
        rollover_serving_revision,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&stale_acknowledgement_error, "RS001");
    let third_request = certification_ingress_acknowledgement_request(
        acknowledgement_revision(&first_acknowledgement),
        third_successor,
        certification.route_admission.gateway.clone(),
    );
    let third_acknowledgement =
        publish_ingress_acknowledgement(&database.executor_pool, &third_request)
            .await
            .unwrap();
    assert_eq!(third_acknowledgement.outcome_name, "applied");
    let third_serving_revision = assert_heartbeat_writer_fence_precedes_owner_lock(
        database,
        certification.serving.identity.clone(),
        rollover_serving_revision,
    )
    .await;

    let left_pool = database.owner_pool.clone();
    let right_pool = database.owner_pool.clone();
    let left_identity = certification.serving.identity.clone();
    let right_identity = certification.serving.identity.clone();
    let (left, right) = tokio::join!(
        raw_runtime_serving_heartbeat_v2(
            &left_pool,
            &left_identity,
            third_serving_revision,
        ),
        raw_runtime_serving_heartbeat_v2(
            &right_pool,
            &right_identity,
            third_serving_revision,
        )
    );
    let expected_replay_revision = third_serving_revision + 1;
    assert_eq!(left.unwrap(), expected_replay_revision);
    assert_eq!(right.unwrap(), expected_replay_revision);

    let disconnected = raw_runtime_serving_disconnect_v2(
        &database.owner_pool,
        &certification.serving.identity,
        expected_replay_revision,
    )
    .await
    .unwrap();
    assert_eq!(
        disconnected,
        (
            i64::from(certification.serving.identity.process_identity.target.version.get()),
            certification
                .serving
                .identity
                .process_identity
                .target
                .content_hash
                .to_string(),
            i64::try_from(
                certification
                    .serving
                    .identity
                    .process_identity
                    .target
                    .binding_revision
                    .get(),
            )
            .unwrap(),
            certification
                .serving
                .identity
                .process_identity
                .target
                .binding_fingerprint
                .as_str()
                .to_owned(),
            expected_replay_revision + 1,
            false,
            false,
        )
    );
    let disconnect_replay = raw_runtime_serving_disconnect_v2(
        &database.owner_pool,
        &certification.serving.identity,
        expected_replay_revision,
    )
    .await
    .unwrap();
    assert_eq!(disconnect_replay, disconnected);
}

fn certification_ingress_acknowledgement_request(
    source_acknowledgement_revision: Option<NonZeroU64>,
    owner_receipt: automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1,
    gateway_ready: RuntimeGatewayReadyAttestationV2,
) -> RuntimePublishIngressOpenAcknowledgementV2 {
    RuntimePublishIngressOpenAcknowledgementV2::new(
        RuntimePublishIngressOpenAcknowledgementInputV2 {
            source_acknowledgement_revision,
            fence_generation: RuntimeWriterFenceGenerationV1::new(NonZeroU64::MIN),
            maintenance_gate_generation: NonZeroU64::MIN,
            gateway_ready,
            owner_receipt,
            lease_for: RuntimeIngressOpenAcknowledgementLeaseDurationV2::from_duration(
                Duration::from_secs(10),
            )
            .unwrap(),
        },
    )
    .unwrap()
}

fn acknowledgement_revision(
    acknowledgement: &IngressAcknowledgementSqlRowV2,
) -> Option<NonZeroU64> {
    acknowledgement
        .acknowledgement_revision
        .and_then(|revision| u64::try_from(revision).ok())
        .and_then(NonZeroU64::new)
}

async fn renew_certification_owner(
    adapter: &PostgresRuntimeExecutionV1,
    current: &automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1,
) -> automation_runtime_controller::RuntimeGatewayOwnerLeaseReceiptV1 {
    let request = RuntimeRenewGatewayOwnerLeaseV1 {
        lease_id: current.lease_id.clone(),
        expected_owner_revision: current.owner_revision,
        lease_for: RuntimeGatewayOwnerLeaseDurationV1::new(Duration::from_secs(300)).unwrap(),
    };
    let RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(receipt) =
        RuntimeGatewayOwnerLeasePortV1::renew_gateway_owner(adapter, request)
            .await
            .unwrap()
    else {
        panic!("certification owner renewal must win")
    };
    receipt
}

async fn raw_runtime_serving_heartbeat_v2(
    pool: &PgPool,
    identity: &automation_runtime_controller::RuntimeServingIdentityV2,
    expected_revision: i64,
) -> Result<i64, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', '5000ms', TRUE), \
            pg_catalog.set_config('lock_timeout', '1000ms', TRUE), \
            pg_catalog.set_config(\
                'idle_in_transaction_session_timeout', '10000ms', TRUE\
            )",
    )
    .execute(&mut *transaction)
    .await?;
    let result = sqlx::query_scalar(
        "SELECT serving_revision \
         FROM public.starring_runtime_serving_heartbeat_v2(\
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(identity.operation_id.as_str())
    .bind(identity.scope.tenant_id.as_str())
    .bind(identity.scope.installation_id.as_str())
    .bind(identity.scope.deployment_id.as_str())
    .bind(identity.attestation_digest.as_str())
    .bind(identity.process_identity.process_instance_id.as_str())
    .bind(i64::try_from(identity.process_identity.runtime_generation.get()).unwrap())
    .bind(i64::try_from(identity.lease_epoch.get()).unwrap())
    .bind(expected_revision)
    .bind(CERTIFICATION_LEASE_MILLISECONDS)
    .fetch_one(&mut *transaction)
    .await;
    match result {
        Ok(revision) => {
            transaction.commit().await?;
            Ok(revision)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn raw_runtime_serving_disconnect_v2(
    pool: &PgPool,
    identity: &automation_runtime_controller::RuntimeServingIdentityV2,
    expected_revision: i64,
) -> Result<(i64, String, i64, String, i64, bool, bool), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', '5000ms', TRUE), \
            pg_catalog.set_config('lock_timeout', '1000ms', TRUE), \
            pg_catalog.set_config(\
                'idle_in_transaction_session_timeout', '10000ms', TRUE\
            )",
    )
    .execute(&mut *transaction)
    .await?;
    let result = sqlx::query_as(
        "SELECT target_version, target_content_hash, binding_revision, \
            binding_fingerprint, serving_revision, connected, serving \
         FROM public.starring_runtime_serving_disconnect_if_current_v2(\
            $1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(identity.operation_id.as_str())
    .bind(identity.scope.tenant_id.as_str())
    .bind(identity.scope.installation_id.as_str())
    .bind(identity.scope.deployment_id.as_str())
    .bind(identity.attestation_digest.as_str())
    .bind(identity.process_identity.process_instance_id.as_str())
    .bind(i64::try_from(identity.process_identity.runtime_generation.get()).unwrap())
    .bind(i64::try_from(identity.lease_epoch.get()).unwrap())
    .bind(expected_revision)
    .fetch_one(&mut *transaction)
    .await;
    match result {
        Ok(disconnected) => {
            transaction.commit().await?;
            Ok(disconnected)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn assert_heartbeat_writer_fence_precedes_owner_lock(
    database: &IsolatedDatabase,
    identity: automation_runtime_controller::RuntimeServingIdentityV2,
    expected_revision: i64,
) -> i64 {
    let mut writer_blocker = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
        )",
    )
    .execute(&mut *writer_blocker)
    .await
    .unwrap();

    let heartbeat_pool = database.owner_pool.clone();
    let heartbeat = tokio::spawn(async move {
        raw_runtime_serving_heartbeat_v2(&heartbeat_pool, &identity, expected_revision).await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut owner_probe = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '250ms'")
        .execute(&mut *owner_probe)
        .await
        .unwrap();
    sqlx::query(
        "SELECT owner_revision \
         FROM public.runtime_gateway_owners \
         WHERE gateway_shard_id = $1 \
         FOR UPDATE",
    )
    .bind(CERTIFICATION_SHARD)
    .fetch_one(&mut *owner_probe)
    .await
    .unwrap();
    owner_probe.rollback().await.unwrap();
    writer_blocker.rollback().await.unwrap();

    tokio::time::timeout(Duration::from_secs(2), heartbeat)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}

async fn raw_runtime_certification_v2_replay(
    database: &IsolatedDatabase,
    canonical: &automation_runtime_controller::RuntimeCanonicalLiveAttestationV2,
) -> (
    String,
    Json<Value>,
    Json<Value>,
    Json<Value>,
    String,
    String,
) {
    let request = canonical.request();
    let guard = &request.intent.guard;
    let mut transaction = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ WRITE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let replay = sqlx::query_as::<
        _,
        (
            String,
            Json<Value>,
            Json<Value>,
            Json<Value>,
            String,
            String,
        ),
    >(
        "SELECT outcome_name, previous_snapshot, snapshot, route_admission, \
            request_digest, attestation_digest \
         FROM public.starring_runtime_certification_commit_v2(\
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
    )
    .bind(request.intent.operation_id.as_str())
    .bind(request.intent_fingerprint.as_str())
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(guard.expected_revision.get() as i64)
    .bind(guard.controller_id.as_str())
    .bind(guard.fencing_token.get() as i64)
    .bind(guard.runtime_generation.get() as i64)
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(canonical.certification_request_bytes())
    .bind(canonical.request_digest().as_str())
    .bind(canonical.live_attestation_record_bytes())
    .bind(canonical.live_attestation_digest().as_str())
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    replay
}
