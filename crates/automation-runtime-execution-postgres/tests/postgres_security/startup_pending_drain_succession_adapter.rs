type PendingDrainSuccessionAdapterAuthorization = (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    automation_runtime_worker::RuntimeAuthorizedPendingDrainSelectionV3,
);

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_succession_ports_apply_replay_and_validate_projection() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let recovery_id = "fedcba98765432100123456789abcdef";
    let fixture =
        expired_succession_fixture(&database, "1234567890abcdef1234567890abcdef", recovery_id)
            .await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = pending_drain_succession_owner_receipt(
        &fixture.owner,
        database_now(&database.owner_pool).await,
    );
    let (mut lifecycle, mut permit, selection_authorization) =
        authorize_pending_drain_adapter_call_v3(&adapter, &database.owner_pool, owner).await;

    let selection_receipt =
        automation_runtime_worker::RuntimePendingDrainSelectionPortV3::select_pending_drain_v3(
            &adapter,
            &selection_authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV3::ExpiredPreviousOwner(
        selected,
    ) = selection_authorization
        .accept_selection(selection_receipt)
        .unwrap()
    else {
        panic!("expired previous owner must authorize direct succession")
    };
    assert_eq!(
        selected.candidate().intent_id().as_str(),
        fixture.selected_drain_intent_id
    );
    assert_eq!(
        selected.candidate().source_intent_revision().get(),
        u64::try_from(fixture.selected_source_intent_revision).unwrap()
    );
    assert_eq!(
        selected
            .candidate()
            .predecessor_claim()
            .claim_revision()
            .get(),
        u64::try_from(fixture.predecessor_claim_revision).unwrap()
    );
    let candidate = selected.candidate().clone();
    let seal = pending_drain_succession_adapter_seal(selected.request(), &candidate);
    let succession = (*selected).bind_registry_seal(seal).unwrap();

    let applied = automation_runtime_worker::
        RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3::
        execute_pending_drain_succession_acknowledgement(
            &adapter,
            &succession,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    drop(applied);
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, recovery_id).await,
        1
    );

    let replayed = automation_runtime_worker::
        RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3::
        execute_pending_drain_succession_acknowledgement(
            &adapter,
            &succession,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, recovery_id).await,
        1
    );
    let durable = succession.complete(replayed).unwrap();
    let unseal = pending_drain_succession_adapter_unseal(durable.seal_witness(), &candidate);
    let completed = durable.complete_registry_rollover(unseal).unwrap();
    let accepted = lifecycle
        .complete_startup_recovery_execution(&mut permit, completed)
        .unwrap();
    let Some(automation_runtime_worker::RuntimePendingDrainExecutionProofV2::Succession(proof)) =
        accepted.pending_drain_proof()
    else {
        panic!("direct succession must preserve its durable proof")
    };
    assert_eq!(proof.candidate(), &candidate);
    assert_eq!(
        proof.acknowledged_intent_revision().get(),
        candidate.source_intent_revision().get() + 1
    );
    assert_ne!(proof.terminal_digest().as_bytes(), &[0; 32]);
    assert_ne!(proof.acknowledged_state_digest().as_bytes(), &[0; 32]);

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_v3_ports_pre_dispatch_cutoffs_are_timeout() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let recovery_id = "fedcba98765432100123456789abcdef";
    let fixture =
        expired_succession_fixture(&database, "abcdef1234567890abcdef1234567890", recovery_id)
            .await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = pending_drain_succession_owner_receipt(
        &fixture.owner,
        database_now(&database.owner_pool).await,
    );
    let (_, _, selection_authorization) =
        authorize_pending_drain_adapter_call_v3(&adapter, &database.owner_pool, owner).await;
    assert!(matches!(
        automation_runtime_worker::RuntimePendingDrainSelectionPortV3::select_pending_drain_v3(
            &adapter,
            &selection_authorization,
            Instant::now(),
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::Timeout)
    ));
    let selection_receipt =
        automation_runtime_worker::RuntimePendingDrainSelectionPortV3::select_pending_drain_v3(
            &adapter,
            &selection_authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV3::ExpiredPreviousOwner(
        selected,
    ) = selection_authorization
        .accept_selection(selection_receipt)
        .unwrap()
    else {
        panic!("expired previous owner must remain selectable")
    };
    let candidate = selected.candidate().clone();
    let seal = pending_drain_succession_adapter_seal(selected.request(), &candidate);
    let succession = (*selected).bind_registry_seal(seal).unwrap();
    assert!(matches!(
        automation_runtime_worker::
            RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3::
            execute_pending_drain_succession_acknowledgement(
                &adapter,
                &succession,
                Instant::now(),
            )
            .await,
        Err(RuntimeExecutionPersistenceErrorV1::Timeout)
    ));
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, recovery_id).await,
        0
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_succession_cutoff_detaches_dispatched_connection() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let recovery_id = "13579bdf2468ace013579bdf2468ace0";
    let fixture =
        expired_succession_fixture(&database, "02468ace13579bdf02468ace13579bdf", recovery_id)
            .await;
    let single_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            database
                .foreign_database_options
                .clone()
                .database(&database.name),
        )
        .await
        .unwrap();
    let adapter = verified_execution_adapter_for_pool(
        &database,
        single_pool.clone(),
        automation_runtime_execution_postgres::RuntimeExecutionDatabaseTimeoutsV1::new(
            Duration::from_secs(5),
            Duration::from_secs(4),
        )
        .unwrap(),
    )
    .await;
    let owner = pending_drain_succession_owner_receipt(
        &fixture.owner,
        database_now(&database.owner_pool).await,
    );
    let (_, _, selection_authorization) =
        authorize_pending_drain_adapter_call_v3(&adapter, &database.owner_pool, owner).await;
    let selection_receipt =
        automation_runtime_worker::RuntimePendingDrainSelectionPortV3::select_pending_drain_v3(
            &adapter,
            &selection_authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV3::ExpiredPreviousOwner(
        selected,
    ) = selection_authorization
        .accept_selection(selection_receipt)
        .unwrap()
    else {
        panic!("expired previous owner must remain selectable")
    };
    let candidate = selected.candidate().clone();
    let seal = pending_drain_succession_adapter_seal(selected.request(), &candidate);
    let succession = (*selected).bind_registry_seal(seal).unwrap();
    let mut blocker = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
            pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)\
         )",
    )
    .execute(&mut *blocker)
    .await
    .unwrap();
    let execution_adapter = adapter.clone();
    let execution = tokio::spawn(async move {
        automation_runtime_worker::
            RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3::
            execute_pending_drain_succession_acknowledgement(
                &execution_adapter,
                &succession,
                Instant::now() + Duration::from_millis(250),
            )
            .await
    });
    let blocked_backend =
        wait_for_blocked_pending_drain_succession_adapter(&database.owner_pool).await;
    assert!(matches!(
        execution.await.unwrap(),
        Err(RuntimeExecutionPersistenceErrorV1::Indeterminate)
    ));
    blocker.rollback().await.unwrap();
    wait_for_startup_recovery_execution_backend_exit(&database.owner_pool, blocked_backend).await;
    let replacement = sqlx::query_scalar::<_, i32>("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&single_pool)
        .await
        .unwrap();
    assert_ne!(replacement, blocked_backend);
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, recovery_id).await,
        0
    );

    single_pool.close().await;
    cleanup(database).await;
    drop(server);
}

async fn authorize_pending_drain_adapter_call_v3(
    adapter: &PostgresRuntimeExecutionV1,
    pool: &PgPool,
    owner: RuntimeGatewayOwnerLeaseReceiptV1,
) -> PendingDrainSuccessionAdapterAuthorization {
    let (lifecycle, permit, authorization) =
        authorize_pending_drain_adapter_execution(adapter, pool, owner).await;
    (
        lifecycle,
        permit,
        authorization.into_pending_drain_selection_v3().unwrap(),
    )
}

fn pending_drain_succession_owner_receipt(
    owner: &StartupObservationOwnerTuple,
    database_now: DateTime<Utc>,
) -> RuntimeGatewayOwnerLeaseReceiptV1 {
    RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: automation_runtime_controller::RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: automation_runtime_controller::GatewayShardIdV1::parse(
                owner.0.clone(),
            )
            .unwrap(),
            process_instance_id: ProcessInstanceId::parse(owner.1.clone()).unwrap(),
            lease_epoch: NonZeroU64::new(u64::try_from(owner.2).unwrap()).unwrap(),
            expected_build_revision: automation_runtime_controller::RuntimeBuildRevisionV1::parse(
                owner.3.clone(),
            )
            .unwrap(),
        },
        owner_revision: NonZeroU64::new(u64::try_from(owner.4).unwrap()).unwrap(),
        database_now,
        expires_at: owner.5,
    }
}

fn pending_drain_succession_adapter_seal(
    request: &automation_runtime_worker::RuntimeStartupRecoveryExecutionRequestV2,
    candidate: &automation_runtime_worker::RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
) -> automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessV2 {
    automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessV2::new(
        automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessInputV2 {
            process_instance_id: request.registry_process_instance_id().clone(),
            slot: candidate.slot().clone(),
            pre_slot_observation: None,
            seal_key: candidate.intent_id().canonical_bytes(),
            seal_generation: NonZeroU64::MIN,
            post_slot_admission_generation: NonZeroU64::MIN,
            post_slot_observation_sequence: NonZeroU64::MIN,
            pre_registry_observation_sequence: request.registry_observation_sequence(),
            pre_registry_retained_slot_count: request.registry_retained_slot_count(),
            pre_registry_retained_empty_tombstone_count: request
                .registry_retained_empty_tombstone_count(),
            post_registry_observation: RuntimeRegistryRecoveryObservationInputV2 {
                observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                    NonZeroU64::new(request.registry_observation_sequence().get() + 1).unwrap(),
                ),
                retained_slot_count: request.registry_retained_slot_count() + 1,
                retained_empty_tombstone_count: request.registry_retained_empty_tombstone_count(),
                staged_route_count: 0,
                serving_route_count: 0,
                draining_route_count: 0,
                sealed_slot_count: 1,
                active_interaction_count: 0,
                failed_closed_slot_count: 0,
                registry_failed_closed: false,
            },
        },
    )
    .unwrap()
}

fn pending_drain_succession_adapter_unseal(
    seal: &automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessV2,
    candidate: &automation_runtime_worker::RuntimePendingDrainPreviousOwnerClaimedCandidateV3,
) -> automation_runtime_worker::RuntimePendingDrainRegistryUnsealWitnessV2 {
    let post = seal.post_registry_observation();
    let registry = accept_runtime_registry_recovery_empty_observation_v2(
        seal.process_instance_id().clone(),
        RuntimeRegistryRecoveryObservationInputV2 {
            observation_sequence: RuntimeRegistryGlobalObservationSequenceV2::new(
                NonZeroU64::new(post.observation_sequence.get() + 1).unwrap(),
            ),
            retained_slot_count: post.retained_slot_count,
            retained_empty_tombstone_count: post.retained_slot_count,
            staged_route_count: 0,
            serving_route_count: 0,
            draining_route_count: 0,
            sealed_slot_count: 0,
            active_interaction_count: 0,
            failed_closed_slot_count: 0,
            registry_failed_closed: false,
        },
    )
    .unwrap();
    automation_runtime_worker::RuntimePendingDrainRegistryUnsealWitnessV2::new(
        seal.process_instance_id().clone(),
        candidate.slot().clone(),
        NonZeroU64::new(seal.post_slot_admission_generation().get() + 1).unwrap(),
        NonZeroU64::new(seal.post_slot_observation_sequence().get() + 1).unwrap(),
        registry,
    )
    .unwrap()
}

async fn wait_for_blocked_pending_drain_succession_adapter(pool: &PgPool) -> i32 {
    for _ in 0..200 {
        let backend = sqlx::query_scalar::<_, i32>(
            "SELECT activity.pid \
             FROM pg_catalog.pg_stat_activity AS activity \
             WHERE activity.datname = pg_catalog.current_database() \
                AND activity.pid <> pg_catalog.pg_backend_pid() \
                AND activity.state = 'active' \
                AND activity.wait_event_type = 'Lock' \
                AND activity.query LIKE \
                    '%starring_runtime_startup_recovery_pending_drain_succession_v3%' \
             ORDER BY activity.pid \
             LIMIT 1",
        )
        .fetch_optional(pool)
        .await
        .unwrap();
        if let Some(backend) = backend {
            return backend;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("pending drain succession adapter did not reach the writer fence lock")
}
