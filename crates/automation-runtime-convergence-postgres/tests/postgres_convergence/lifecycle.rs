#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn exact_live_status_and_fencing_survive_postgres() {
    let pool = test_pool().await;
    seed_product_target(&pool).await;
    assert_search_path_shadow_resistance(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let request = enqueue_request();
    let mut cross_tenant = request.clone();
    cross_tenant.identity.deployment_id = DeploymentId::parse("runtime-pg-cross-tenant").unwrap();
    cross_tenant.identity.tenant_id = TenantId::parse("runtime-pg-other-tenant").unwrap();
    assert!(matches!(
        adapter.enqueue(cross_tenant).await.unwrap_err(),
        RuntimeConvergenceStoreError::ScopeMismatch
    ));
    let mut cross_promotion = request.clone();
    cross_promotion.identity.deployment_id =
        DeploymentId::parse("runtime-pg-cross-promotion").unwrap();
    cross_promotion.identity.promotion_id = PromotionId::parse("9".repeat(64)).unwrap();
    assert!(matches!(
        adapter.enqueue(cross_promotion).await.unwrap_err(),
        RuntimeConvergenceStoreError::ScopeMismatch
    ));
    let mut wrong_activation = request.clone();
    wrong_activation.identity.deployment_id =
        DeploymentId::parse("runtime-pg-wrong-activation").unwrap();
    wrong_activation.identity.activation_request_id =
        ActivationRequestId::parse("runtime_pg_wrong_activation").unwrap();
    assert!(matches!(
        adapter.enqueue(wrong_activation).await.unwrap_err(),
        RuntimeConvergenceStoreError::ScopeMismatch
    ));
    let first_adapter = adapter.clone();
    let first_request = request.clone();
    let first_enqueue =
        tokio::spawn(async move { first_adapter.enqueue(first_request).await });
    let second_adapter = adapter.clone();
    let second_request = request.clone();
    let second_enqueue =
        tokio::spawn(async move { second_adapter.enqueue(second_request).await });
    let (first_enqueue, second_enqueue) = tokio::join!(first_enqueue, second_enqueue);
    let first_enqueue = first_enqueue.unwrap();
    let second_enqueue = second_enqueue.unwrap();
    let (created, replayed) = match (first_enqueue.unwrap(), second_enqueue.unwrap()) {
        (
            EnqueueDeploymentOutcomeV1::ExactReplay(created),
            EnqueueDeploymentOutcomeV1::ExactReplay(replayed),
        ) => (created, replayed),
        outcome => panic!("atomically seeded deployment must replay exactly: {outcome:?}"),
    };
    assert_eq!(created, replayed);
    let initial = created;
    assert_adapter_search_path_resistance().await;
    assert!(matches!(
        adapter.enqueue(request).await.unwrap(),
        EnqueueDeploymentOutcomeV1::ExactReplay(_)
    ));
    let controller = ControllerId::parse("runtime-pg-controller").unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM runtime_deployments WHERE deployment_id = $1 FOR UPDATE")
        .bind(DEPLOYMENT)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let timeout_adapter = PostgresRuntimeConvergence::with_config(
        pool.clone(),
        PostgresRuntimeConvergenceConfigV1 {
            statement_timeout: Duration::from_millis(200),
            lock_timeout: Duration::from_millis(50),
            ..PostgresRuntimeConvergenceConfigV1::default()
        },
    )
    .unwrap();
    let timeout_error = timeout_adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: controller.clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        &timeout_error,
        RuntimeConvergenceStoreError::DatabaseTimeout
    ));
    assert!(timeout_error.is_retryable());
    let claiming_adapter = adapter.clone();
    let claim_request = ClaimDeploymentV1 {
        scope: scope(),
        expected_revision: initial.revision,
        controller_id: controller.clone(),
        lease_for: Duration::from_secs(90),
    };
    let claiming = tokio::spawn(async move { claiming_adapter.claim(claim_request).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    let released_at = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT clock_timestamp()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    blocker.commit().await.unwrap();
    let claim = claiming.await.unwrap().unwrap();
    assert!(claim.acquired_at >= released_at);
    assert_eq!(claim.expires_at - claim.acquired_at, TimeDelta::seconds(90));
    let replayed_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: controller.clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    assert_eq!(replayed_claim.fencing_token, claim.fencing_token);
    assert_eq!(replayed_claim.snapshot, claim.snapshot);
    let process = ProcessInstanceId::parse("runtime-pg-process").unwrap();
    let mut revision = claim.snapshot.revision;
    let preflight = PreflightAttestationV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::FIRST,
        observed_runtime: None,
        checked_at: claim.acquired_at,
    };
    let preflight_expected = revision;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptPreflight(preflight.clone()),
    )
    .await;
    let replayed_preflight = adapter
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope(),
            expected_revision: preflight_expected,
            controller_id: controller.clone(),
            fencing_token: claim.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: DeploymentMutationV1::AcceptPreflight(preflight),
        })
        .await
        .unwrap();
    assert_eq!(replayed_preflight.snapshot.revision, revision);
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: None,
            target_runtime_generation: RuntimeGeneration::FIRST,
            drained_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::BeginActivation,
    )
    .await;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptActivation(ActivationAttestationV1 {
            activation_request_id: ActivationRequestId::parse(ACTIVATION).unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            kind: ActivationOutcomeKindV1::AlreadyActive,
            activated_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::BeginPanelReconciliation,
    )
    .await;
    revision = mutate(
        &adapter,
        revision,
        &controller,
        claim.fencing_token,
        DeploymentMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse("runtime-pg-panel-certificate").unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process.clone(),
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
    let stale_ready = adapter
        .certify_live(SubmitLiveAttestationV1 {
            scope: scope(),
            expected_revision: revision,
            controller_id: controller.clone(),
            fencing_token: claim.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            gateway_ready: GatewayReadyAttestationV1 {
                target: target(),
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id: process.clone(),
                kind: GatewayReadyKindV1::DiscordReady,
                ready_at: claim.acquired_at - TimeDelta::minutes(5),
            },
            metadata: LiveMetadataV1 {
                runtime_build_revision: RuntimeBuildRevisionV1::parse("test-build-1").unwrap(),
                panel_report_digest: PanelReportDigestV1::parse("d".repeat(64)).unwrap(),
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            },
            serving_lease_for: Duration::from_secs(45),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        stale_ready,
        RuntimeConvergenceStoreError::InvalidInput("gateway Ready evidence is stale")
    ));
    let live_request = SubmitLiveAttestationV1 {
        scope: scope(),
        expected_revision: revision,
        controller_id: controller,
        fencing_token: claim.fencing_token,
        runtime_generation: RuntimeGeneration::FIRST,
        gateway_ready: GatewayReadyAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process,
            kind: GatewayReadyKindV1::DiscordReady,
            ready_at: claim.acquired_at,
        },
        metadata: LiveMetadataV1 {
            runtime_build_revision: RuntimeBuildRevisionV1::parse("test-build-1").unwrap(),
            panel_report_digest: PanelReportDigestV1::parse("d".repeat(64)).unwrap(),
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        },
        serving_lease_for: Duration::from_secs(45),
    };
    let drift_pool = pool.clone();
    let drift_adapter = adapter.clone();
    let drift_request = live_request.clone();
    let (live_transition, serving) = tokio::spawn(async move {
        certify_live_with_concurrent_active_drift(&drift_pool, &drift_adapter, drift_request).await
    })
    .await
    .unwrap();
    assert!(live_transition.snapshot.live.is_some());
    let (replayed_live, replayed_serving) = adapter.certify_live(live_request).await.unwrap();
    assert!(matches!(
        replayed_live.outcome,
        automation_runtime_convergence::TransitionOutcomeV1::Replayed { .. }
    ));
    assert_eq!(replayed_serving, serving);
    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Live);
    let first_adapter = adapter.clone();
    let second_adapter = adapter.clone();
    let first_identity = serving.identity.clone();
    let second_identity = serving.identity;
    let first = tokio::spawn(async move {
        first_adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: first_identity,
                lease_for: Duration::from_secs(45),
            })
            .await
    });
    let second = tokio::spawn(async move {
        second_adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: second_identity,
                lease_for: Duration::from_secs(45),
            })
            .await
    });
    let (first, second) = tokio::join!(first, second);
    let first = first.unwrap();
    let second = second.unwrap();
    let heartbeat = match (first, second) {
        (Ok(receipt), Err(RuntimeConvergenceStoreError::RevisionConflict))
        | (Err(RuntimeConvergenceStoreError::RevisionConflict), Ok(receipt)) => receipt,
        outcome => panic!("exactly one fenced heartbeat must win: {outcome:?}"),
    };
    let disconnected = adapter
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: heartbeat.identity.clone(),
        })
        .await
        .unwrap();
    assert!(!disconnected.connected);
    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(
        status.availability,
        DeploymentAvailabilityV1::RuntimePending
    );
    assert_eq!(status.reason_code, "gateway_not_serving");
    let disconnected_recovery = RecoverStaleLiveV1 {
        identity: disconnected.identity.clone(),
        expected_deployment_revision: live_transition.snapshot.revision,
    };
    let recovered = adapter
        .recover_stale_live(disconnected_recovery.clone())
        .await
        .unwrap();
    assert!(recovered.snapshot.last_live_recovery.is_some());
    assert!(recovered.snapshot.live.is_none());
    let replayed_recovery = adapter
        .recover_stale_live(disconnected_recovery)
        .await
        .unwrap();
    assert!(matches!(
        replayed_recovery.outcome,
        automation_runtime_convergence::TransitionOutcomeV1::Replayed { .. }
    ));
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: disconnected.identity,
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::ServingLeaseConflict
    ));
    let second_controller = ControllerId::parse("runtime-pg-controller-2").unwrap();
    let second_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: recovered.snapshot.revision,
            controller_id: second_controller,
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    assert!(second_claim.fencing_token > claim.fencing_token);
    let short_lease_adapter = PostgresRuntimeConvergence::with_config(
        pool.clone(),
        PostgresRuntimeConvergenceConfigV1 {
            statement_timeout: Duration::from_millis(100),
            lock_timeout: Duration::from_millis(50),
            ..PostgresRuntimeConvergenceConfigV1::default()
        },
    )
    .unwrap();
    let (second_live, second_serving) = converge_recovered(
        &short_lease_adapter,
        second_claim,
        ProcessInstanceId::parse("runtime-pg-process-2").unwrap(),
        "test-build-2",
        "e",
        Duration::from_millis(500),
    )
    .await;
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: heartbeat.identity,
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::ServingLeaseConflict
    ));
    let mut status_blocker = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE runtime_serving_leases IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *status_blocker)
        .await
        .unwrap();
    let status_adapter = adapter.clone();
    let delayed_status = tokio::spawn(async move { status_adapter.status(&scope()).await });
    tokio::time::sleep(Duration::from_millis(650)).await;
    status_blocker.commit().await.unwrap();
    let status = delayed_status.await.unwrap().unwrap();
    assert_eq!(
        status.availability,
        DeploymentAvailabilityV1::RuntimePending
    );
    assert_eq!(status.reason_code, "serving_lease_expired");
    sqlx::query(
        "UPDATE automation_ruleset_activations SET active_version = 2 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&pool)
    .await
    .unwrap();
    let superseded_status = adapter.status(&scope()).await.unwrap();
    assert_eq!(
        superseded_status.availability,
        DeploymentAvailabilityV1::Superseded
    );
    assert_eq!(superseded_status.reason_code, "active_target_changed");
    assert!(adapter.recover_next_stale_live().await.unwrap().is_none());
    sqlx::query(
        "UPDATE automation_ruleset_activations SET active_version = 1 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&pool)
    .await
    .unwrap();
    let expired_recovery = adapter
        .recover_next_stale_live()
        .await
        .unwrap()
        .expect("expired Live deployment is recoverable");
    assert!(expired_recovery.snapshot.live.is_none());
    let third_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: expired_recovery.snapshot.revision,
            controller_id: ControllerId::parse("runtime-pg-controller-3").unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    assert!(third_claim.fencing_token > second_claim_fencing(&second_live));
    let (third_live, third_serving) = converge_recovered(
        &adapter,
        third_claim,
        ProcessInstanceId::parse("runtime-pg-process-3").unwrap(),
        "test-build-3",
        "6",
        Duration::from_secs(45),
    )
    .await;
    assert!(third_serving.identity.lease_epoch > second_serving.identity.lease_epoch);
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: second_serving.identity,
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::ServingLeaseConflict
    ));
    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Live);
    assert_eq!(
        status.live.unwrap().process_instance_id,
        ProcessInstanceId::parse("runtime-pg-process-3").unwrap()
    );
    let suspension_pool = pool.clone();
    let suspension_adapter = adapter.clone();
    let suspension_serving = third_serving.clone();
    tokio::spawn(async move {
        assert_concurrent_tenant_suspension(
            &suspension_pool,
            &suspension_adapter,
            &suspension_serving,
        )
        .await;
    })
    .await
    .unwrap();
    let recovery_pool = pool.clone();
    let recovery_adapter = adapter.clone();
    tokio::spawn(async move {
        assert_recovery_and_newer_certification_do_not_deadlock(
            &recovery_pool,
            &recovery_adapter,
            &third_live,
            &third_serving,
        )
        .await;
    })
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn live_status_fails_closed_after_ruleset_artifact_corruption() {
    let database = isolated_runtime_migration_database().await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let pool = &database.pool;
        seed_product_target(pool).await;
        let adapter = PostgresRuntimeConvergence::new(pool.clone());
        let initial = adapter.status(&scope()).await.unwrap();
        let claim = adapter
            .claim(ClaimDeploymentV1 {
                scope: scope(),
                expected_revision: initial.snapshot.revision,
                controller_id: ControllerId::parse("runtime-integrity-controller").unwrap(),
                lease_for: Duration::from_secs(90),
            })
            .await
            .unwrap();
        converge_claimed(
            &adapter,
            claim,
            ProcessInstanceId::parse("runtime-integrity-process").unwrap(),
        )
        .await;
        assert_eq!(
            adapter.status(&scope()).await.unwrap().availability,
            DeploymentAvailabilityV1::Live
        );
        corrupt_seeded_ruleset_artifact(pool).await;
        assert!(matches!(
            adapter.enqueue(enqueue_request()).await.unwrap_err(),
            RuntimeConvergenceStoreError::InvalidPersistedState("RuleSet artifact integrity")
        ));
        assert!(matches!(
            adapter.status(&scope()).await.unwrap_err(),
            RuntimeConvergenceStoreError::InvalidPersistedState("RuleSet artifact integrity")
        ));
    }
    drop_runtime_migration_database(database).await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn worker_candidate_router_excludes_self_consistent_future_schema() {
    let database = isolated_runtime_migration_database().await;
    MIGRATOR.run(&database.pool).await.unwrap();
    {
        let pool = &database.pool;
        seed_product_target(pool).await;
        replace_seeded_target_with_future_schema(pool).await;
        let adapter = PostgresRuntimeConvergence::new(pool.clone());
        assert!(adapter
            .claim_next(ClaimNextDeploymentV1 {
                controller_id: ControllerId::parse("future-schema-controller").unwrap(),
                lease_for: Duration::from_secs(90),
            })
            .await
            .unwrap()
            .is_none());
    }
    drop_runtime_migration_database(database).await;
}
