#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn gateway_owner_is_durable_fenced_and_exactly_recoverable() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    gateway_owner_lifecycle_scenario(&database).await;
    cleanup(database).await;
    drop(server);
}

async fn gateway_owner_lifecycle_scenario(database: &IsolatedDatabase) {
    let adapter = verified_execution_adapter(database).await;
    let first_request = gateway_owner_acquire_request("gateway-owner-process-1", "runtime:test");
    let first = RuntimeGatewayOwnerLeasePortV1::acquire_gateway_owner(
        &adapter,
        first_request.clone(),
    )
    .await
    .unwrap();
    let RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(first_receipt) = first else {
        panic!("first owner acquisition must win")
    };
    assert_eq!(first_receipt.owner_revision.get(), 1);
    assert_eq!(first_receipt.lease_id.lease_epoch.get(), 1);

    let observed = RuntimeGatewayOwnerLeasePortV1::observe_gateway_owner(
        &adapter,
        RuntimeObserveGatewayOwnerLeaseV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        },
    )
    .await
    .unwrap();
    let RuntimeGatewayOwnerAcquireRecoveryV1::Adopt(observed_authority) =
        classify_unknown_gateway_owner_acquire_v1(&first_request, observed)
    else {
        panic!("unknown acquisition must adopt its exact current lease")
    };
    let observed_receipt = observed_authority.receipt();
    assert_eq!(observed_receipt.lease_id, first_receipt.lease_id);
    assert_eq!(observed_receipt.owner_revision, first_receipt.owner_revision);
    assert_eq!(observed_receipt.expires_at, first_receipt.expires_at);

    let replay = RuntimeGatewayOwnerLeasePortV1::acquire_gateway_owner(
        &adapter,
        first_request.clone(),
    )
    .await
    .unwrap();
    let RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(replay_receipt) = replay else {
        panic!("exact acquisition replay must remain acquired")
    };
    assert_eq!(replay_receipt.lease_id, first_receipt.lease_id);
    assert_eq!(replay_receipt.owner_revision, first_receipt.owner_revision);
    assert_eq!(replay_receipt.expires_at, first_receipt.expires_at);
    assert!(replay_receipt.database_now >= first_receipt.database_now);

    let foreign_request = gateway_owner_acquire_request("gateway-owner-process-2", "runtime:test");
    let contended = RuntimeGatewayOwnerLeasePortV1::acquire_gateway_owner(
        &adapter,
        foreign_request.clone(),
    )
    .await
    .unwrap();
    let RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Contended(contended_receipt) = contended else {
        panic!("foreign acquisition must contend")
    };
    assert_eq!(contended_receipt.lease_id, first_receipt.lease_id);

    let renew_request = RuntimeRenewGatewayOwnerLeaseV1 {
        lease_id: first_receipt.lease_id.clone(),
        expected_owner_revision: first_receipt.owner_revision,
        lease_for: gateway_owner_lease_duration(),
    };
    let renewed = RuntimeGatewayOwnerLeasePortV1::renew_gateway_owner(
        &adapter,
        renew_request.clone(),
    )
    .await
    .unwrap();
    let RuntimeRenewGatewayOwnerLeaseOutcomeV1::Renewed(renewed_receipt) = renewed else {
        panic!("exact renewal must win")
    };
    assert_eq!(renewed_receipt.lease_id, first_receipt.lease_id);
    assert_eq!(renewed_receipt.owner_revision.get(), 2);

    let observed = RuntimeGatewayOwnerLeasePortV1::observe_gateway_owner(
        &adapter,
        RuntimeObserveGatewayOwnerLeaseV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        },
    )
    .await
    .unwrap();
    let RuntimeGatewayOwnerRenewRecoveryV1::AdoptSuccessor(observed_renewed) =
        classify_unknown_gateway_owner_renew_v1(&renew_request, observed)
    else {
        panic!("unknown renewal must adopt only its exact successor")
    };
    assert_eq!(observed_renewed.lease_id, renewed_receipt.lease_id);
    assert_eq!(
        observed_renewed.owner_revision,
        renewed_receipt.owner_revision
    );
    assert_eq!(observed_renewed.expires_at, renewed_receipt.expires_at);

    let release_request = RuntimeReleaseGatewayOwnerLeaseV1 {
        lease_id: first_receipt.lease_id.clone(),
    };
    assert!(matches!(
        RuntimeGatewayOwnerLeasePortV1::release_gateway_owner(
            &adapter,
            release_request.clone()
        )
        .await
        .unwrap(),
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released { .. }
    ));
    let observed = RuntimeGatewayOwnerLeasePortV1::observe_gateway_owner(
        &adapter,
        RuntimeObserveGatewayOwnerLeaseV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        },
    )
    .await
    .unwrap();
    let RuntimeGatewayOwnerReleaseRecoveryV1::CompleteWithoutOwnership(
        RuntimeGatewayOwnerLeaseObservationV1::Unowned {
            gateway_shard_id, ..
        },
    ) = classify_unknown_gateway_owner_release_v1(&release_request, observed)
    else {
        panic!("released lease must exact-observe as unowned")
    };
    assert_eq!(gateway_shard_id.as_str(), "shard:0");

    let second = RuntimeGatewayOwnerLeasePortV1::acquire_gateway_owner(
        &adapter,
        foreign_request,
    )
    .await
    .unwrap();
    let RuntimeAcquireGatewayOwnerLeaseOutcomeV1::Acquired(second_receipt) = second else {
        panic!("released tombstone must permit a successor")
    };
    assert_eq!(second_receipt.lease_id.lease_epoch.get(), 2);
    let stale_release = RuntimeGatewayOwnerLeasePortV1::release_gateway_owner(
        &adapter,
        release_request,
    )
    .await
    .unwrap();
    let RuntimeReleaseGatewayOwnerLeaseOutcomeV1::NotHeld(
        RuntimeGatewayOwnerLeaseObservationV1::Owned(current),
    ) = stale_release
    else {
        panic!("stale epoch release must preserve the successor")
    };
    assert_eq!(current.lease_id, second_receipt.lease_id);
}

fn gateway_owner_acquire_request(
    process: &str,
    build: &str,
) -> RuntimeAcquireGatewayOwnerLeaseV1 {
    RuntimeAcquireGatewayOwnerLeaseV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: ProcessInstanceId::parse(process).unwrap(),
        expected_build_revision: RuntimeBuildRevisionV1::parse(build).unwrap(),
        lease_for: gateway_owner_lease_duration(),
    }
}

fn gateway_owner_lease_duration() -> RuntimeGatewayOwnerLeaseDurationV1 {
    RuntimeGatewayOwnerLeaseDurationV1::new(Duration::from_secs(30)).unwrap()
}
