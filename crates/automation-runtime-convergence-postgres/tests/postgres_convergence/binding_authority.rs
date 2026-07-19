#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn runtime_authority_tracks_binding_identity_across_policy_rotation() {
    run_migrated_runtime_database_test(
        "binding_authority",
        runtime_authority_tracks_binding_identity_across_policy_rotation_scenario,
    )
    .await;
}

async fn runtime_authority_tracks_binding_identity_across_policy_rotation_scenario(
    pool: PgPool,
    _: PgConnectOptions,
) {
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let created = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
        outcome => panic!("atomically seeded deployment must replay exactly: {outcome:?}"),
    };

    let unchanged_bindings = json!({});
    rotate_authority(
        &pool,
        AuthorityRotation {
            revision: 2,
            binding_revision: 1,
            resource_bindings: &unchanged_bindings,
            binding_fingerprint: BINDING_FINGERPRINT,
            policy_revision: 2,
            required_approvals: 2,
            activation_ttl_seconds: 7200,
        },
    )
    .await;

    let controller = ControllerId::parse("runtime-policy-controller").unwrap();
    let claim = adapter
        .claim_next(ClaimNextDeploymentV1 {
            controller_id: controller,
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap()
        .expect("policy-only authority rotation must leave the deployment claimable");
    assert_eq!(
        claim.snapshot.identity.deployment_id,
        created.identity.deployment_id
    );
    let (live, serving) = converge_claimed(
        &adapter,
        claim,
        ProcessInstanceId::parse("runtime-policy-process").unwrap(),
    )
    .await;
    assert!(live.snapshot.live.is_some());
    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Live);
    assert_eq!(status.reason_code, "live");

    let heartbeat = adapter
        .heartbeat_serving(HeartbeatServingLeaseV1 {
            identity: serving.identity,
            lease_for: Duration::from_secs(45),
        })
        .await
        .unwrap();
    let disconnected = adapter
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: heartbeat.identity,
        })
        .await
        .unwrap();
    assert!(!disconnected.connected);
    let spoofed_bindings = json!({
        "channel_bindings": {"community_hub": "9200401"},
        "role_bindings": {}
    });
    rotate_authority(
        &pool,
        AuthorityRotation {
            revision: 3,
            binding_revision: 1,
            resource_bindings: &spoofed_bindings,
            binding_fingerprint: BINDING_FINGERPRINT,
            policy_revision: 3,
            required_approvals: 2,
            activation_ttl_seconds: 7200,
        },
    )
    .await;
    assert_eq!(
        adapter.status(&scope()).await.unwrap().availability,
        DeploymentAvailabilityV1::Superseded
    );
    assert!(adapter.recover_next_stale_live().await.unwrap().is_none());
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: disconnected.identity.clone(),
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::BindingAuthorityMismatch
    ));
    rotate_authority(
        &pool,
        AuthorityRotation {
            revision: 4,
            binding_revision: 1,
            resource_bindings: &unchanged_bindings,
            binding_fingerprint: BINDING_FINGERPRINT,
            policy_revision: 4,
            required_approvals: 2,
            activation_ttl_seconds: 7200,
        },
    )
    .await;
    let recovered = adapter
        .recover_next_stale_live()
        .await
        .unwrap()
        .expect("policy-only authority rotation must leave stale Live recovery eligible");
    let recovered_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: recovered.snapshot.revision,
            controller_id: ControllerId::parse("runtime-policy-controller-recovered").unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let (_, recovered_serving) = converge_recovered(
        &adapter,
        recovered_claim,
        ProcessInstanceId::parse("runtime-policy-process-recovered").unwrap(),
        "policy-build-recovered",
        "7",
        Duration::from_secs(45),
    )
    .await;
    assert_eq!(
        adapter.status(&scope()).await.unwrap().availability,
        DeploymentAvailabilityV1::Live
    );

    rotate_authority(
        &pool,
        AuthorityRotation {
            revision: 5,
            binding_revision: 2,
            resource_bindings: &unchanged_bindings,
            binding_fingerprint: ROTATED_BINDING_FINGERPRINT,
            policy_revision: 5,
            required_approvals: 2,
            activation_ttl_seconds: 7200,
        },
    )
    .await;

    let status = adapter.status(&scope()).await.unwrap();
    assert_eq!(status.availability, DeploymentAvailabilityV1::Superseded);
    assert_eq!(status.reason_code, "binding_authority_changed");
    assert!(matches!(
        adapter
            .heartbeat_serving(HeartbeatServingLeaseV1 {
                identity: recovered_serving.identity,
                lease_for: Duration::from_secs(45),
            })
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::BindingAuthorityMismatch
    ));
}
