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
    let (lease_epoch, owner_revision) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT lease_epoch, owner_revision \
         FROM public.starring_runtime_gateway_owner_acquire_v1($1,$2,$3,$4)",
    )
    .bind(CERTIFICATION_SHARD)
    .bind(process_identity.process_instance_id.as_str())
    .bind(CERTIFICATION_BUILD)
    .bind(300_000_i64)
    .fetch_one(&database.executor_pool)
    .await
    .unwrap();
    let gateway_owner_lease_id =
        automation_runtime_controller::RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: automation_runtime_controller::GatewayShardIdV1::parse(
                CERTIFICATION_SHARD,
            )
            .unwrap(),
            process_instance_id: process_identity.process_instance_id.clone(),
            lease_epoch: std::num::NonZeroU64::new(lease_epoch as u64).unwrap(),
            expected_build_revision: automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                CERTIFICATION_BUILD,
            )
            .unwrap(),
        };
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
                observed_owner_revision: std::num::NonZeroU64::new(owner_revision as u64).unwrap(),
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
        attested_owner_revision: std::num::NonZeroU64::new(owner_revision as u64).unwrap(),
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
