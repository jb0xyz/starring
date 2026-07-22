use automation_runtime_controller::{
    RuntimeExecutionConvergencePort, RuntimePreviousServingObservationPort,
};

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_adapter_proves_certification_observation_and_recovery() {
    let server = PostgresTestServer::start();

    let lifecycle_database = isolated_database(server.connect_options()).await;
    execution_adapter_certification_and_recovery_scenario(&lifecycle_database).await;
    cleanup(lifecycle_database).await;

    let observation_database = isolated_database(server.connect_options()).await;
    execution_adapter_previous_serving_observation_scenario(&observation_database).await;
    cleanup(observation_database).await;

    let advanced_database = isolated_database(server.connect_options()).await;
    execution_adapter_rejects_advanced_certification_replay_scenario(&advanced_database).await;
    cleanup(advanced_database).await;

    drop(server);
}

async fn execution_adapter_certification_and_recovery_scenario(database: &IsolatedDatabase) {
    let adapter = verified_execution_adapter(database).await;
    let mut session = gateway_ready_session(database, "runtime-adapter-lifecycle-controller").await;
    let gateway_ready = gateway_ready_attestation(database, &session).await;
    let request = session
        .begin_certification(
            gateway_ready,
            adapter_certification_metadata(),
            Duration::from_secs(1),
        )
        .unwrap();
    let applied = RuntimeExecutionConvergencePort::certify_live(&adapter, request.clone())
        .await
        .unwrap();
    assert!(matches!(
        applied.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    let replayed = RuntimeExecutionConvergencePort::certify_live(&adapter, request)
        .await
        .unwrap();
    assert!(matches!(
        replayed.outcome,
        TransitionOutcomeV1::Replayed { .. }
    ));
    assert_eq!(replayed.snapshot, applied.snapshot);
    assert_eq!(replayed.serving, applied.serving);
    let serving = session.apply_certification(applied.clone()).unwrap();
    assert_eq!(serving.ownership(), &applied.serving);
    wait_for_database_time(&database.owner_pool, applied.serving.expires_at).await;
    let recovered = RuntimeExecutionConvergencePort::recover_next_stale_live(&adapter)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        recovered.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    assert!(matches!(
        recovered.snapshot.phase,
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Ready
        }
    ));
    assert!(recovered.snapshot.last_live_recovery.is_some());
    assert!(RuntimeExecutionConvergencePort::recover_next_stale_live(&adapter)
        .await
        .unwrap()
        .is_none());
}

async fn execution_adapter_previous_serving_observation_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-adapter-observation-controller",
        Duration::from_secs(60),
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
    let request = session.begin_previous_serving_observation().unwrap();
    let receipt = RuntimePreviousServingObservationPort::observe_previous_serving(
        &adapter,
        request.clone(),
    )
        .await
        .unwrap();
    assert!(matches!(
        receipt.state,
        automation_runtime_controller::RuntimePreviousServingStateV1::Absent
    ));
    assert_eq!(receipt.action_id, request.action_id);
    assert_eq!(receipt.guard, request.guard);
    session.apply_previous_serving_observation(receipt).unwrap();
}

async fn execution_adapter_rejects_advanced_certification_replay_scenario(
    database: &IsolatedDatabase,
) {
    let adapter = verified_execution_adapter(database).await;
    let mut session = gateway_ready_session(database, "runtime-adapter-advanced-controller").await;
    let gateway_ready = gateway_ready_attestation(database, &session).await;
    let request = session
        .begin_certification(
            gateway_ready,
            adapter_certification_metadata(),
            Duration::from_secs(200),
        )
        .unwrap();
    let applied = RuntimeExecutionConvergencePort::certify_live(&adapter, request.clone())
        .await
        .unwrap();
    let identity = &applied.serving.identity;
    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "SELECT * FROM public.starring_runtime_serving_heartbeat_v1(\
         $1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(identity.scope.tenant_id.as_str())
    .bind(identity.scope.installation_id.as_str())
    .bind(identity.scope.deployment_id.as_str())
    .bind(identity.attestation_id.as_str())
    .bind(identity.process_instance_id.as_str())
    .bind(i64::try_from(identity.runtime_generation.get()).unwrap())
    .bind(i64::try_from(identity.lease_epoch.get()).unwrap())
    .bind(i64::try_from(identity.expected_revision.get()).unwrap())
    .bind(300_000_i64)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let persisted = persisted_live_execution_image(&database.owner_pool).await;
    assert_eq!(
        RuntimeExecutionConvergencePort::certify_live(&adapter, request).await,
        Err(RuntimeExecutionPersistenceErrorV1::OwnershipLost)
    );
    assert_eq!(
        persisted_live_execution_image(&database.owner_pool).await,
        persisted
    );
}

fn adapter_certification_metadata() -> automation_runtime_controller::RuntimeLiveMetadataV1 {
    automation_runtime_controller::RuntimeLiveMetadataV1 {
        runtime_build_revision: RuntimeBuildRevisionV1::parse(CERTIFICATION_BUILD).unwrap(),
        panel_report_digest: PanelReportDigestV1::parse(CERTIFICATION_REPORT).unwrap(),
        gateway_shard_id: GatewayShardIdV1::parse(CERTIFICATION_SHARD).unwrap(),
    }
}

async fn persisted_live_execution_image(pool: &PgPool) -> (Json<Value>, Json<Value>, Json<Value>) {
    sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(deployment), pg_catalog.to_jsonb(attestation), \
            pg_catalog.to_jsonb(lease) \
         FROM public.runtime_deployments AS deployment \
         INNER JOIN public.runtime_attestations AS attestation \
            ON attestation.attestation_id = deployment.live_attestation_id \
         INNER JOIN public.runtime_serving_leases AS lease \
            ON lease.deployment_id = deployment.deployment_id \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}
