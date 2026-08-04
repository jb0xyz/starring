use automation_runtime_worker::{
    RuntimeAuthorizedStartupRecoveryExecutionV2, RuntimeStartupRecoveryClassV2,
    RuntimeStartupRecoveryContinuationV2, RuntimeStartupRecoveryExecutionPortV2,
    RuntimeStartupRecoveryExecutionReceiptOutcomeV2,
};

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_accepts_progressed_and_replayed_receipts() {
    let server = PostgresTestServer::start();

    let applied_database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&applied_database, 1_000).await;
    wait_for_stale_live(&applied_database).await;
    let applied_adapter = verified_execution_adapter(&applied_database).await;
    let applied_owner = acquire_startup_observation_port_owner(&applied_adapter).await;
    let (mut applied_lifecycle, mut applied_permit, applied_authorization) =
        authorize_stale_live_execution_port_call(
            &applied_adapter,
            applied_owner,
            Duration::from_secs(5),
        )
        .await;
    let applied_action = applied_authorization.request().action_identity().clone();
    let applied_completed = RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
        &applied_adapter,
        applied_authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    let applied = applied_lifecycle
        .complete_startup_recovery_execution(&mut applied_permit, applied_completed)
        .unwrap();
    assert_eq!(applied.class(), RuntimeStartupRecoveryClassV2::StaleLive);
    let RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
        action_identity,
        terminal_digest,
    } = applied.outcome()
    else {
        panic!("stale live adapter execution must progress")
    };
    assert_eq!(action_identity, &applied_action);
    assert_ne!(terminal_digest.as_bytes(), &[0; 32]);
    assert_eq!(
        startup_stale_live_journal_count(
            &applied_database.owner_pool,
            applied_action.correlation().recovery_id().as_str(),
        )
        .await,
        1
    );
    cleanup(applied_database).await;

    let replayed_database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&replayed_database, 1_000).await;
    wait_for_stale_live(&replayed_database).await;
    let replayed_adapter = verified_execution_adapter(&replayed_database).await;
    let replayed_owner = acquire_startup_observation_port_owner(&replayed_adapter).await;
    let (mut replayed_lifecycle, mut replayed_permit, replayed_authorization) =
        authorize_stale_live_execution_port_call(
            &replayed_adapter,
            replayed_owner,
            Duration::from_secs(5),
        )
        .await;
    let replayed_input =
        startup_stale_live_input_from_authorization(&replayed_authorization).unwrap();
    let direct_applied =
        execute_startup_stale_live(&replayed_database.executor_pool, &replayed_input, "5s")
            .await
            .unwrap();
    assert_eq!(direct_applied["journal_outcome_name"], "applied");
    assert_eq!(direct_applied["terminal_outcome_name"], "progressed");
    let expected_digest =
        decode_lowercase_hex_32(direct_applied["terminal_digest"].as_str().unwrap());
    let replayed_action = replayed_authorization.request().action_identity().clone();
    let replayed_completed = RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
        &replayed_adapter,
        replayed_authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    let replayed = replayed_lifecycle
        .complete_startup_recovery_execution(&mut replayed_permit, replayed_completed)
        .unwrap();
    let RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
        action_identity,
        terminal_digest,
    } = replayed.outcome()
    else {
        panic!("exact stale live replay must preserve progress proof")
    };
    assert_eq!(action_identity, &replayed_action);
    assert_eq!(terminal_digest.as_bytes(), &expected_digest);
    assert_eq!(
        startup_stale_live_journal_count(
            &replayed_database.owner_pool,
            replayed_action.correlation().recovery_id().as_str(),
        )
        .await,
        1
    );
    cleanup(replayed_database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_verifies_reserved_progress_and_replay() {
    let server = PostgresTestServer::start();

    let applied_database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&applied_database).await;
    expire_current_gateway_owner(&applied_database.owner_pool).await;
    let applied_adapter = verified_execution_adapter(&applied_database).await;
    let applied_owner = acquire_startup_observation_port_owner(&applied_adapter).await;
    let (mut applied_lifecycle, mut applied_permit, applied_authorization) =
        authorize_reserved_awaiting_execution_port_call(
            &applied_adapter,
            applied_owner,
            Duration::from_secs(5),
        )
        .await;
    let applied_action = applied_authorization.request().action_identity().clone();
    let applied_completed = RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
        &applied_adapter,
        applied_authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    let applied = applied_lifecycle
        .complete_startup_recovery_execution(&mut applied_permit, applied_completed)
        .unwrap();
    assert_eq!(
        applied.class(),
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification
    );
    let RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
        action_identity,
        terminal_digest,
    } = applied.outcome()
    else {
        panic!("reserved awaiting adapter execution must progress")
    };
    assert_eq!(action_identity, &applied_action);
    assert_ne!(terminal_digest.as_bytes(), &[0; 32]);
    cleanup(applied_database).await;

    let replayed_database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&replayed_database).await;
    expire_current_gateway_owner(&replayed_database.owner_pool).await;
    let replayed_adapter = verified_execution_adapter(&replayed_database).await;
    let replayed_owner = acquire_startup_observation_port_owner(&replayed_adapter).await;
    let (mut replayed_lifecycle, mut replayed_permit, replayed_authorization) =
        authorize_reserved_awaiting_execution_port_call(
            &replayed_adapter,
            replayed_owner,
            Duration::from_secs(5),
        )
        .await;
    let direct_applied = execute_reserved_awaiting_from_authorization(
        &replayed_database.executor_pool,
        &replayed_authorization,
    )
    .await
    .unwrap();
    assert_eq!(direct_applied["journal_outcome_name"], "applied");
    assert_eq!(direct_applied["terminal_outcome_name"], "progressed");
    let expected_digest =
        decode_lowercase_hex_32(direct_applied["terminal_digest"].as_str().unwrap());
    let replayed_action = replayed_authorization.request().action_identity().clone();
    let replayed_completed = RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
        &replayed_adapter,
        replayed_authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    let replayed = replayed_lifecycle
        .complete_startup_recovery_execution(&mut replayed_permit, replayed_completed)
        .unwrap();
    let RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
        action_identity,
        terminal_digest,
    } = replayed.outcome()
    else {
        panic!("reserved awaiting exact replay must preserve progress proof")
    };
    assert_eq!(action_identity, &replayed_action);
    assert_eq!(terminal_digest.as_bytes(), &expected_digest);

    cleanup(replayed_database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_verifies_unreserved_progress_and_replay() {
    let server = PostgresTestServer::start();

    let applied_database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&applied_database).await;
    remove_current_certification_reservation(&applied_database.owner_pool).await;
    expire_current_gateway_owner(&applied_database.owner_pool).await;
    let applied_adapter = verified_execution_adapter(&applied_database).await;
    let applied_owner = acquire_startup_observation_port_owner(&applied_adapter).await;
    let (mut applied_lifecycle, mut applied_permit, applied_authorization) =
        authorize_reserved_awaiting_execution_port_call(
            &applied_adapter,
            applied_owner,
            Duration::from_secs(5),
        )
        .await;
    let applied_action = applied_authorization.request().action_identity().clone();
    let applied_completed = RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
        &applied_adapter,
        applied_authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    let applied = applied_lifecycle
        .complete_startup_recovery_execution(&mut applied_permit, applied_completed)
        .unwrap();
    assert_eq!(
        applied.class(),
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification
    );
    let RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
        action_identity,
        terminal_digest,
    } = applied.outcome()
    else {
        panic!("unreserved awaiting adapter execution must progress")
    };
    assert_eq!(action_identity, &applied_action);
    assert_ne!(terminal_digest.as_bytes(), &[0; 32]);
    assert_eq!(
        startup_stale_live_journal_count(
            &applied_database.owner_pool,
            applied_action.correlation().recovery_id().as_str(),
        )
        .await,
        1
    );
    cleanup(applied_database).await;

    let replayed_database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&replayed_database).await;
    remove_current_certification_reservation(&replayed_database.owner_pool).await;
    expire_current_gateway_owner(&replayed_database.owner_pool).await;
    let replayed_adapter = verified_execution_adapter(&replayed_database).await;
    let replayed_owner = acquire_startup_observation_port_owner(&replayed_adapter).await;
    let (mut replayed_lifecycle, mut replayed_permit, replayed_authorization) =
        authorize_reserved_awaiting_execution_port_call(
            &replayed_adapter,
            replayed_owner,
            Duration::from_secs(5),
        )
        .await;
    let direct_applied = execute_reserved_awaiting_from_authorization(
        &replayed_database.executor_pool,
        &replayed_authorization,
    )
    .await
    .unwrap();
    assert_eq!(direct_applied["journal_outcome_name"], "applied");
    assert_eq!(direct_applied["terminal_outcome_name"], "progressed");
    let expected_digest =
        decode_lowercase_hex_32(direct_applied["terminal_digest"].as_str().unwrap());
    let replayed_action = replayed_authorization.request().action_identity().clone();
    let replayed_completed = RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
        &replayed_adapter,
        replayed_authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    let replayed = replayed_lifecycle
        .complete_startup_recovery_execution(&mut replayed_permit, replayed_completed)
        .unwrap();
    assert_eq!(
        replayed.class(),
        RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification
    );
    let RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
        action_identity,
        terminal_digest,
    } = replayed.outcome()
    else {
        panic!("unreserved awaiting exact replay must preserve progress proof")
    };
    assert_eq!(action_identity, &replayed_action);
    assert_eq!(terminal_digest.as_bytes(), &expected_digest);
    assert_eq!(
        startup_stale_live_journal_count(
            &replayed_database.owner_pool,
            replayed_action.correlation().recovery_id().as_str(),
        )
        .await,
        1
    );

    cleanup(replayed_database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_maps_unreserved_owner_loss_without_mutation() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    certification_reservation_scenario(&database).await;
    remove_current_certification_reservation(&database.owner_pool).await;
    expire_current_gateway_owner(&database.owner_pool).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, authorization) = authorize_reserved_awaiting_execution_port_call(
        &adapter,
        owner.clone(),
        Duration::from_secs(5),
    )
    .await;
    let recovery_id = authorization
        .request()
        .correlation()
        .recovery_id()
        .as_str()
        .to_owned();
    let before = reserved_awaiting_execution_state(&database.owner_pool).await;
    let released = RuntimeGatewayOwnerLeasePortV1::release_gateway_owner(
        &adapter,
        RuntimeReleaseGatewayOwnerLeaseV1 {
            lease_id: owner.lease_id,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        released,
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released { .. }
    ));

    assert!(matches!(
        RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
            &adapter,
            authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::OwnershipLost)
    ));
    assert_eq!(
        reserved_awaiting_execution_state(&database.owner_pool).await,
        before
    );
    assert_eq!(
        startup_stale_live_journal_count(&database.owner_pool, &recovery_id).await,
        0
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_verifies_suspended_local_progress() {
    assert_suspended_local_execution_port_progress(Some(5)).await;
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_verifies_suspended_local_progress_without_resume() {
    assert_suspended_local_execution_port_progress(None).await;
}

async fn assert_suspended_local_execution_port_progress(last_resume_sequence: Option<u64>) {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_claimable_deployment(&database.owner_pool).await;
    let mut claim = database.executor_pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *claim)
        .await
        .unwrap();
    sqlx::query(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_claim_next_v1(\
            'suspended-local-controller', 300000\
         )",
    )
    .fetch_one(&mut *claim)
    .await
    .unwrap();
    claim.commit().await.unwrap();
    seed_exact_local_suspension(&database.owner_pool).await;

    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (mut lifecycle, mut permit, authorization) = authorize_suspended_local_execution_port_call(
        &adapter,
        owner,
        Duration::from_secs(5),
        last_resume_sequence,
    )
    .await;
    assert_eq!(
        authorization
            .request()
            .paused_gateway()
            .last_resume_sequence()
            .map(|sequence| sequence.get()),
        last_resume_sequence
    );
    let before = suspended_local_state(&database.owner_pool).await;
    let action = authorization.request().action_identity().clone();
    let completed = RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
        &adapter,
        authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    let accepted = lifecycle
        .complete_startup_recovery_execution(&mut permit, completed)
        .unwrap();
    assert_eq!(
        accepted.class(),
        RuntimeStartupRecoveryClassV2::SuspendedLocalEffect
    );
    let RuntimeStartupRecoveryExecutionReceiptOutcomeV2::Progressed {
        action_identity,
        terminal_digest,
    } = accepted.outcome()
    else {
        panic!("suspended local adapter execution must progress")
    };
    assert_eq!(action_identity, &action);
    assert_ne!(terminal_digest.as_bytes(), &[0; 32]);
    let after = suspended_local_state(&database.owner_pool).await;
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, before.1);
    assert_eq!(after.2, before.2 + 1);
    assert_eq!(after.3, "route_absent");
    assert_eq!(after.4, "none");
    assert_eq!(after.5, 1);

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_accepts_no_candidate_receipt() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&database, 1_000).await;
    wait_for_stale_live(&database).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (mut lifecycle, mut permit, authorization) =
        authorize_stale_live_execution_port_call(&adapter, owner, Duration::from_secs(5)).await;
    let recovery_id = authorization
        .request()
        .correlation()
        .recovery_id()
        .as_str()
        .to_owned();
    assert!(
        RuntimeExecutionConvergencePort::recover_next_stale_live(&adapter)
            .await
            .unwrap()
            .is_some()
    );

    let completed = RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
        &adapter,
        authorization,
        Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    let accepted = lifecycle
        .complete_startup_recovery_execution(&mut permit, completed)
        .unwrap();
    assert!(matches!(
        accepted.outcome(),
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate
    ));
    assert_eq!(
        startup_stale_live_journal_count(&database.owner_pool, &recovery_id).await,
        1
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_maps_lost_owner() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&database, 1_000).await;
    wait_for_stale_live(&database).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, authorization) =
        authorize_stale_live_execution_port_call(&adapter, owner.clone(), Duration::from_secs(5))
            .await;
    let released = RuntimeGatewayOwnerLeasePortV1::release_gateway_owner(
        &adapter,
        RuntimeReleaseGatewayOwnerLeaseV1 {
            lease_id: owner.lease_id,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        released,
        RuntimeReleaseGatewayOwnerLeaseOutcomeV1::Released { .. }
    ));

    assert!(matches!(
        RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
            &adapter,
            authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::OwnershipLost)
    ));

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_pre_dispatch_cutoff_is_timeout() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&database, 1_000).await;
    wait_for_stale_live(&database).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, authorization) =
        authorize_stale_live_execution_port_call(&adapter, owner, Duration::from_secs(5)).await;
    let mut blocker = database.owner_pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE public.product_control_plane_identity IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let execution_adapter = adapter.clone();
    let execution = tokio::spawn(async move {
        RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
            &execution_adapter,
            authorization,
            Instant::now() + Duration::from_millis(500),
        )
        .await
    });
    wait_for_blocked_startup_recovery_binding(&database.owner_pool).await;
    assert!(matches!(
        execution.await.unwrap(),
        Err(RuntimeExecutionPersistenceErrorV1::Timeout)
    ));
    blocker.rollback().await.unwrap();

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_cutoff_detaches_dispatched_connection() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&database, 1_000).await;
    wait_for_stale_live(&database).await;
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
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, authorization) =
        authorize_stale_live_execution_port_call(&adapter, owner, Duration::from_secs(5)).await;
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
        RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
            &execution_adapter,
            authorization,
            Instant::now() + Duration::from_millis(250),
        )
        .await
    });
    let blocked_backend = wait_for_blocked_startup_recovery_execution(&database.owner_pool).await;
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

    single_pool.close().await;
    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_recovery_execution_port_elapsed_cutoff_wins_before_database_access() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_live_for_startup_observation(&database, 1_000).await;
    wait_for_stale_live(&database).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, authorization) =
        authorize_stale_live_execution_port_call(&adapter, owner, Duration::from_secs(5)).await;
    database.executor_pool.close().await;

    assert!(matches!(
        RuntimeStartupRecoveryExecutionPortV2::execute_startup_recovery(
            &adapter,
            authorization,
            Instant::now(),
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::Timeout)
    ));

    cleanup(database).await;
    drop(server);
}

async fn authorize_stale_live_execution_port_call(
    adapter: &PostgresRuntimeExecutionV1,
    owner: RuntimeGatewayOwnerLeaseReceiptV1,
    cutoff_after: Duration,
) -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    RuntimeAuthorizedStartupRecoveryExecutionV2,
) {
    let (mut lifecycle, mut permit, observation_authorization) =
        authorize_startup_observation_port_call(adapter, owner);
    let completed = RuntimeStartupRecoveryObservationPortV2::observe_startup_recovery(
        adapter,
        observation_authorization,
        Instant::now() + cutoff_after,
    )
    .await
    .unwrap();
    let RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("stale live observation must request recovery")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(RuntimeStartupRecoveryClassV2::StaleLive)
    );
    let authorization = lifecycle
        .begin_startup_recovery_execution(&mut permit, continuation)
        .unwrap();
    (lifecycle, permit, authorization)
}

async fn authorize_reserved_awaiting_execution_port_call(
    adapter: &PostgresRuntimeExecutionV1,
    owner: RuntimeGatewayOwnerLeaseReceiptV1,
    cutoff_after: Duration,
) -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    RuntimeAuthorizedStartupRecoveryExecutionV2,
) {
    let (mut lifecycle, mut permit, observation_authorization) =
        authorize_startup_observation_port_call(adapter, owner);
    let completed = RuntimeStartupRecoveryObservationPortV2::observe_startup_recovery(
        adapter,
        observation_authorization,
        Instant::now() + cutoff_after,
    )
    .await
    .unwrap();
    let RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("reserved awaiting observation must request recovery")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(
            RuntimeStartupRecoveryClassV2::ReservedAwaitingCertification
        )
    );
    let authorization = lifecycle
        .begin_startup_recovery_execution(&mut permit, continuation)
        .unwrap();
    (lifecycle, permit, authorization)
}

async fn authorize_suspended_local_execution_port_call(
    adapter: &PostgresRuntimeExecutionV1,
    owner: RuntimeGatewayOwnerLeaseReceiptV1,
    cutoff_after: Duration,
    last_resume_sequence: Option<u64>,
) -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    RuntimeAuthorizedStartupRecoveryExecutionV2,
) {
    let (mut lifecycle, mut permit, observation_authorization) =
        authorize_startup_observation_port_call_with_last_resume(
            adapter,
            owner,
            last_resume_sequence,
        );
    let completed = RuntimeStartupRecoveryObservationPortV2::observe_startup_recovery(
        adapter,
        observation_authorization,
        Instant::now() + cutoff_after,
    )
    .await
    .unwrap();
    let RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("suspended local observation must request recovery")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(
            RuntimeStartupRecoveryClassV2::SuspendedLocalEffect
        )
    );
    let authorization = lifecycle
        .begin_startup_recovery_execution(&mut permit, continuation)
        .unwrap();
    (lifecycle, permit, authorization)
}

async fn expire_current_gateway_owner(pool: &PgPool) {
    let expired_at = database_now(pool).await - TimeDelta::seconds(1);
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_gateway_owners DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query(
            "UPDATE public.runtime_gateway_owners \
             SET expires_at = $1 \
             WHERE gateway_shard_id = 'shard:0'",
        )
        .bind(expired_at)
        .execute(&mut *transaction)
        .await
        .unwrap()
        .rows_affected(),
        1
    );
    sqlx::query("ALTER TABLE public.runtime_gateway_owners ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn execute_reserved_awaiting_from_authorization(
    pool: &PgPool,
    authorization: &RuntimeAuthorizedStartupRecoveryExecutionV2,
) -> Result<Value, sqlx::Error> {
    let request = authorization.request();
    let correlation = request.correlation();
    let owner = request.gateway_owner_lease_id();
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', '5s', TRUE)")
        .execute(&mut *transaction)
        .await?;
    let result = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT pg_catalog.to_jsonb(result) \
         FROM public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(\
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12\
         ) AS result",
    )
    .bind(correlation.recovery_id().as_str())
    .bind(i64::try_from(correlation.originating_emergency_generation().get()).unwrap())
    .bind(i64::try_from(correlation.coordinator_generation().get()).unwrap())
    .bind(i64::try_from(correlation.authority_revision().get()).unwrap())
    .bind(i64::try_from(correlation.selection_authority_revision().get()).unwrap())
    .bind(owner.gateway_shard_id.as_str())
    .bind(owner.process_instance_id.as_str())
    .bind(i64::try_from(owner.lease_epoch.get()).unwrap())
    .bind(owner.expected_build_revision.as_str())
    .bind(i64::try_from(request.expected_owner_revision().get()).unwrap())
    .bind(request.expected_owner_expires_at())
    .bind(request.minimum_database_now())
    .fetch_one(&mut *transaction)
    .await;
    match result {
        Ok(Json(value)) => {
            transaction.commit().await?;
            Ok(value)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

fn startup_stale_live_input_from_authorization(
    authorization: &RuntimeAuthorizedStartupRecoveryExecutionV2,
) -> Result<StartupStaleLiveExecutionInput, std::num::TryFromIntError> {
    let request = authorization.request();
    let correlation = request.correlation();
    let owner = request.gateway_owner_lease_id();
    Ok(StartupStaleLiveExecutionInput {
        recovery_id: correlation.recovery_id().as_str().to_owned(),
        originating_emergency_generation: i64::try_from(
            correlation.originating_emergency_generation().get(),
        )?,
        coordinator_generation: i64::try_from(correlation.coordinator_generation().get())?,
        action_authority_revision: i64::try_from(correlation.authority_revision().get())?,
        selection_authority_revision: i64::try_from(
            correlation.selection_authority_revision().get(),
        )?,
        owner: (
            owner.gateway_shard_id.as_str().to_owned(),
            owner.process_instance_id.as_str().to_owned(),
            i64::try_from(owner.lease_epoch.get())?,
            owner.expected_build_revision.as_str().to_owned(),
            i64::try_from(request.expected_owner_revision().get())?,
            request.expected_owner_expires_at(),
        ),
        minimum_database_now: request.minimum_database_now(),
    })
}

async fn wait_for_stale_live(database: &IsolatedDatabase) {
    let expiry = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT expires_at FROM public.runtime_serving_leases \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    wait_for_database_time(&database.owner_pool, expiry).await;
}

async fn verified_execution_adapter_for_pool(
    database: &IsolatedDatabase,
    pool: PgPool,
    timeouts: automation_runtime_execution_postgres::RuntimeExecutionDatabaseTimeoutsV1,
) -> PostgresRuntimeExecutionV1 {
    let database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity WHERE singleton",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    let expectation = RuntimeExecutionDatabaseExpectationV1::new(
        database_identity,
        &database.name,
        &database.role,
    )
    .unwrap();
    PostgresRuntimeExecutionV1::connect_verified(pool, expectation, timeouts)
        .await
        .unwrap()
}

async fn wait_for_blocked_startup_recovery_execution(pool: &PgPool) -> i32 {
    for _ in 0..200 {
        let backend = sqlx::query_scalar::<_, i32>(
            "SELECT activity.pid \
             FROM pg_catalog.pg_stat_activity AS activity \
             WHERE activity.datname = pg_catalog.current_database() \
                AND activity.pid <> pg_catalog.pg_backend_pid() \
                AND activity.state = 'active' \
                AND activity.wait_event_type = 'Lock' \
                AND activity.query LIKE \
                    '%starring_runtime_startup_recovery_execute_stale_live_v2%' \
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
    panic!("startup recovery execution did not reach the writer fence lock")
}

async fn wait_for_blocked_startup_recovery_binding(pool: &PgPool) {
    for _ in 0..200 {
        let blocked = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
                SELECT 1 \
                FROM pg_catalog.pg_stat_activity AS activity \
                WHERE activity.datname = pg_catalog.current_database() \
                    AND activity.pid <> pg_catalog.pg_backend_pid() \
                    AND activity.state = 'active' \
                    AND activity.wait_event_type = 'Lock' \
                    AND activity.query LIKE \
                        '%starring_runtime_execution_database_identity_v1%'\
             )",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if blocked {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("startup recovery execution did not reach the binding read")
}

async fn wait_for_startup_recovery_execution_backend_exit(pool: &PgPool, backend: i32) {
    for _ in 0..200 {
        let absent = sqlx::query_scalar::<_, bool>(
            "SELECT NOT EXISTS (\
                SELECT 1 \
                FROM pg_catalog.pg_stat_activity AS activity \
                WHERE activity.pid = $1\
             )",
        )
        .bind(backend)
        .fetch_one(pool)
        .await
        .unwrap();
        if absent {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out startup recovery execution backend did not exit")
}

fn decode_lowercase_hex_32(value: &str) -> [u8; 32] {
    assert_eq!(value.len(), 64);
    assert!(value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    let mut decoded = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    decoded
}
