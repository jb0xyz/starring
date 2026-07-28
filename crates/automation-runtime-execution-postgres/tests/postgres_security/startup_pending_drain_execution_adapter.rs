type PendingDrainAdapterAuthorization = (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    automation_runtime_worker::RuntimeAuthorizedPendingDrainSelectionV2,
);

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_ports_record_no_candidate_applied_and_replayed() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_pending_drain_execution_candidate(&database).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (mut lifecycle, mut permit, authorization) = authorize_pending_drain_adapter_call(
        &adapter,
        &database.owner_pool,
        owner,
    )
    .await;
    remove_pending_drain_candidate(&database.owner_pool).await;

    let selection_receipt =
        automation_runtime_worker::RuntimePendingDrainSelectionPortV2::select_pending_drain(
            &adapter,
            &authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV2::NoCandidate(
        selection,
    ) = authorization.accept_selection(selection_receipt).unwrap()
    else {
        panic!("pending drain selector must observe the consumed candidate as absent")
    };
    let recovery_id = selection
        .request()
        .correlation()
        .recovery_id()
        .as_str()
        .to_owned();

    let applied = automation_runtime_worker::RuntimePendingDrainNoCandidateRecorderPortV2::
        record_pending_drain_no_candidate(
            &adapter,
            &selection,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, &recovery_id).await,
        1
    );
    drop(applied);

    let replayed = automation_runtime_worker::RuntimePendingDrainNoCandidateRecorderPortV2::
        record_pending_drain_no_candidate(
            &adapter,
            &selection,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, &recovery_id).await,
        1
    );
    let completed = (*selection).complete(replayed).unwrap();
    let accepted = lifecycle
        .complete_startup_recovery_execution(&mut permit, completed)
        .unwrap();
    assert!(matches!(
        accepted.outcome(),
        RuntimeStartupRecoveryExecutionReceiptOutcomeV2::NoCandidate
    ));
    let Some(automation_runtime_worker::RuntimePendingDrainExecutionProofV2::NoCandidate(proof)) =
        accepted.pending_drain_proof()
    else {
        panic!("no-candidate replay must preserve the pending drain terminal proof")
    };
    assert_ne!(proof.terminal_digest().as_bytes(), &[0; 32]);

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_ports_claim_ack_applied_replay_and_later_floor() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    let canonical = seed_pending_drain_execution_candidate(&database).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, first_selection_authorization) = authorize_pending_drain_adapter_call(
        &adapter,
        &database.owner_pool,
        owner.clone(),
    )
    .await;
    let (mut lifecycle, mut permit, replay_selection_authorization) =
        authorize_pending_drain_adapter_call(&adapter, &database.owner_pool, owner).await;

    let first_selection_receipt =
        automation_runtime_worker::RuntimePendingDrainSelectionPortV2::select_pending_drain(
            &adapter,
            &first_selection_authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let replay_selection_receipt =
        automation_runtime_worker::RuntimePendingDrainSelectionPortV2::select_pending_drain(
            &adapter,
            &replay_selection_authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV2::Candidate(
        first_selected,
    ) = first_selection_authorization
        .accept_selection(first_selection_receipt)
        .unwrap()
    else {
        panic!("first selector call must return the seeded pending drain")
    };
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV2::Candidate(
        replay_selected,
    ) = replay_selection_authorization
        .accept_selection(replay_selection_receipt)
        .unwrap()
    else {
        panic!("second selector call must return the same seeded pending drain")
    };
    assert_eq!(
        first_selected.candidate().intent_id(),
        &canonical.drain_preimage().key.intent_id
    );
    assert_eq!(first_selected.candidate(), replay_selected.candidate());
    let recovery_id = first_selected
        .request()
        .correlation()
        .recovery_id()
        .as_str()
        .to_owned();
    let first_seal =
        pending_drain_adapter_seal(first_selected.request(), first_selected.candidate());
    let replay_seal =
        pending_drain_adapter_seal(replay_selected.request(), replay_selected.candidate());
    assert_eq!(first_seal, replay_seal);
    let first_claim = (*first_selected).bind_registry_seal(first_seal).unwrap();
    let replay_claim = (*replay_selected)
        .bind_registry_seal(replay_seal)
        .unwrap();

    let first_claim_receipt =
        automation_runtime_worker::RuntimePendingDrainClaimExecutionPortV2::
            execute_pending_drain_claim(
                &adapter,
                &first_claim,
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .unwrap();
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, &recovery_id).await,
        1
    );
    let first_acknowledgement = first_claim.complete(first_claim_receipt).unwrap();
    let first_acknowledgement_floor = first_acknowledgement.minimum_database_now();
    wait_for_database_time(&database.owner_pool, first_acknowledgement_floor).await;

    let replay_claim_receipt =
        automation_runtime_worker::RuntimePendingDrainClaimExecutionPortV2::
            execute_pending_drain_claim(
                &adapter,
                &replay_claim,
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .unwrap();
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, &recovery_id).await,
        1
    );
    let replay_acknowledgement = replay_claim.complete(replay_claim_receipt).unwrap();
    let replay_acknowledgement_floor = replay_acknowledgement.minimum_database_now();
    assert!(replay_acknowledgement_floor > first_acknowledgement_floor);

    let applied_acknowledgement =
        automation_runtime_worker::RuntimePendingDrainAcknowledgementExecutionPortV2::
            execute_pending_drain_acknowledgement(
                &adapter,
                &first_acknowledgement,
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .unwrap();
    drop(applied_acknowledgement);
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, &recovery_id).await,
        2
    );
    let acknowledgement_authority_revision = i64::try_from(
        replay_acknowledgement
            .action_identity()
            .correlation()
            .authority_revision()
            .get(),
    )
    .unwrap();
    let persisted_first_floor = pending_drain_adapter_action_minimum(
        &database.owner_pool,
        &recovery_id,
        acknowledgement_authority_revision,
    )
    .await;
    assert_eq!(persisted_first_floor, first_acknowledgement_floor);

    let replayed_acknowledgement =
        automation_runtime_worker::RuntimePendingDrainAcknowledgementExecutionPortV2::
            execute_pending_drain_acknowledgement(
                &adapter,
                &replay_acknowledgement,
                Instant::now() + Duration::from_secs(5),
            )
            .await
            .unwrap();
    assert_eq!(
        pending_drain_adapter_action_minimum(
            &database.owner_pool,
            &recovery_id,
            acknowledgement_authority_revision,
        )
        .await,
        persisted_first_floor
    );
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, &recovery_id).await,
        2
    );
    let durable = replay_acknowledgement
        .complete(replayed_acknowledgement)
        .unwrap();
    let candidate = durable.candidate().clone();
    let unseal = pending_drain_adapter_unseal(durable.seal_witness(), &candidate);
    let completed = durable.complete_registry_rollover(unseal).unwrap();
    let accepted = lifecycle
        .complete_startup_recovery_execution(&mut permit, completed)
        .unwrap();
    let Some(automation_runtime_worker::RuntimePendingDrainExecutionProofV2::Compound(proof)) =
        accepted.pending_drain_proof()
    else {
        panic!("claim and acknowledgement replay must preserve the compound proof")
    };
    assert_eq!(proof.candidate(), &candidate);
    assert_eq!(proof.claimed_intent_revision().get(), 2);
    assert_eq!(proof.acknowledged_intent_revision().get(), 3);

    cleanup(database).await;
    drop(server);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_ports_pre_dispatch_cutoffs_are_timeout() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_pending_drain_execution_candidate(&database).await;
    let adapter = verified_execution_adapter(&database).await;
    let owner = acquire_startup_observation_port_owner(&adapter).await;
    let (_, _, authorization) =
        authorize_pending_drain_adapter_call(&adapter, &database.owner_pool, owner).await;

    assert!(matches!(
        automation_runtime_worker::RuntimePendingDrainSelectionPortV2::select_pending_drain(
            &adapter,
            &authorization,
            Instant::now(),
        )
        .await,
        Err(RuntimeExecutionPersistenceErrorV1::Timeout)
    ));
    let selection_receipt =
        automation_runtime_worker::RuntimePendingDrainSelectionPortV2::select_pending_drain(
            &adapter,
            &authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV2::Candidate(selected) =
        authorization.accept_selection(selection_receipt).unwrap()
    else {
        panic!("seeded pending drain must remain selectable")
    };
    let recovery_id = selected
        .request()
        .correlation()
        .recovery_id()
        .as_str()
        .to_owned();
    let seal = pending_drain_adapter_seal(selected.request(), selected.candidate());
    let claim = (*selected).bind_registry_seal(seal).unwrap();
    assert!(matches!(
        automation_runtime_worker::RuntimePendingDrainClaimExecutionPortV2::
            execute_pending_drain_claim(&adapter, &claim, Instant::now())
            .await,
        Err(RuntimeExecutionPersistenceErrorV1::Timeout)
    ));
    assert_eq!(
        pending_drain_adapter_journal_count(&database.owner_pool, &recovery_id).await,
        0
    );

    cleanup(database).await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable PostgreSQL 16"]
async fn startup_pending_drain_claim_cutoff_detaches_dispatched_connection() {
    let server = PostgresTestServer::start();
    let database = isolated_database(server.connect_options()).await;
    seed_pending_drain_execution_candidate(&database).await;
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
        authorize_pending_drain_adapter_call(&adapter, &database.owner_pool, owner).await;
    let selection_receipt =
        automation_runtime_worker::RuntimePendingDrainSelectionPortV2::select_pending_drain(
            &adapter,
            &authorization,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let automation_runtime_worker::RuntimeAcceptedPendingDrainSelectionV2::Candidate(selected) =
        authorization.accept_selection(selection_receipt).unwrap()
    else {
        panic!("seeded pending drain must remain selectable")
    };
    let recovery_id = selected
        .request()
        .correlation()
        .recovery_id()
        .as_str()
        .to_owned();
    let seal = pending_drain_adapter_seal(selected.request(), selected.candidate());
    let claim = (*selected).bind_registry_seal(seal).unwrap();
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
        automation_runtime_worker::RuntimePendingDrainClaimExecutionPortV2::
            execute_pending_drain_claim(
                &execution_adapter,
                &claim,
                Instant::now() + Duration::from_millis(250),
            )
            .await
    });
    let blocked_backend = wait_for_blocked_pending_drain_adapter(&database.owner_pool).await;
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
        pending_drain_adapter_journal_count(&database.owner_pool, &recovery_id).await,
        0
    );

    single_pool.close().await;
    cleanup(database).await;
    drop(server);
}

async fn authorize_pending_drain_adapter_call(
    adapter: &PostgresRuntimeExecutionV1,
    pool: &PgPool,
    owner: RuntimeGatewayOwnerLeaseReceiptV1,
) -> PendingDrainAdapterAuthorization {
    let (lifecycle, permit, authorization) =
        authorize_pending_drain_adapter_execution(adapter, pool, owner).await;
    (
        lifecycle,
        permit,
        authorization.into_pending_drain_selection().unwrap(),
    )
}

async fn authorize_pending_drain_adapter_execution(
    adapter: &PostgresRuntimeExecutionV1,
    pool: &PgPool,
    owner: RuntimeGatewayOwnerLeaseReceiptV1,
) -> (
    RuntimeGatewayClosedLifecycleV2,
    RuntimeClosedDrainRecoveryPermitV2,
    automation_runtime_worker::RuntimeAuthorizedStartupRecoveryExecutionV2,
) {
    let (mut lifecycle, mut permit, observation_authorization) =
        authorize_startup_observation_port_call(adapter, owner);
    let request = observation_authorization.request().clone();
    let observed_database_now = database_now(pool).await;
    let completed = observation_authorization.complete(
        automation_runtime_controller::RuntimeStartupRecoveryObservationReceiptV2 {
            correlation: request.correlation,
            owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1 {
                lease_id: request.gateway_owner_lease_id,
                owner_revision: request.expected_owner_revision,
                database_now: observed_database_now,
                expires_at: request.expected_owner_expires_at,
            },
            state: automation_runtime_controller::RuntimeStartupRecoveryStateV2 {
                serving: automation_runtime_controller::RuntimeStartupServingStateV2::Empty,
                recoverable_awaiting_certification_count: 0,
                suspended_local_effect_count: 0,
                pending_runtime_drain_intent_count: 1,
                acknowledged_product_handoff_count: 0,
            },
        },
    );
    let RuntimeAcceptedStartupRecoveryOutcomeV2::Continue(continuation) = lifecycle
        .complete_startup_recovery_observation(&mut permit, completed)
        .unwrap()
    else {
        panic!("pending drain observation must continue to execution")
    };
    assert_eq!(
        continuation,
        RuntimeStartupRecoveryContinuationV2::Recover(
            RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent
        )
    );
    let authorization = lifecycle
        .begin_startup_recovery_execution(&mut permit, continuation)
        .unwrap();
    (lifecycle, permit, authorization)
}

fn pending_drain_adapter_seal(
    request: &automation_runtime_worker::RuntimeStartupRecoveryExecutionRequestV2,
    candidate: &automation_runtime_worker::RuntimePendingDrainCandidateV2,
) -> automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessV2 {
    automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessV2::new(
        automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessInputV2 {
            process_instance_id: request.registry_process_instance_id().clone(),
            slot: candidate.slot().clone(),
            pre_slot_observation: None,
            seal_key: candidate.intent_id().canonical_bytes(),
            seal_generation: NonZeroU64::new(1).unwrap(),
            post_slot_admission_generation: NonZeroU64::new(1).unwrap(),
            post_slot_observation_sequence: NonZeroU64::new(1).unwrap(),
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

fn pending_drain_adapter_unseal(
    seal: &automation_runtime_worker::RuntimePendingDrainRegistrySealWitnessV2,
    candidate: &automation_runtime_worker::RuntimePendingDrainCandidateV2,
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

async fn remove_pending_drain_candidate(pool: &PgPool) {
    set_product_drain_row_triggers(pool, false).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_slot_writer_fences_v2 DISABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.runtime_slot_writer_fences_v2")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.runtime_drain_intents_v2")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.runtime_product_operations_v2")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.runtime_slot_writer_fences_v2 ENABLE TRIGGER USER")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    set_product_drain_row_triggers(pool, true).await;
}

async fn pending_drain_adapter_journal_count(pool: &PgPool, recovery_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT pg_catalog.count(*) \
         FROM public.runtime_startup_recovery_actions_v2 \
         WHERE recovery_id = $1",
    )
    .bind(recovery_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn pending_drain_adapter_action_minimum(
    pool: &PgPool,
    recovery_id: &str,
    action_authority_revision: i64,
) -> DateTime<Utc> {
    sqlx::query_scalar(
        "SELECT minimum_database_now \
         FROM public.runtime_startup_recovery_actions_v2 \
         WHERE recovery_id = $1 AND action_authority_revision = $2",
    )
    .bind(recovery_id)
    .bind(action_authority_revision)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn wait_for_blocked_pending_drain_adapter(pool: &PgPool) -> i32 {
    for _ in 0..200 {
        let backend = sqlx::query_scalar::<_, i32>(
            "SELECT activity.pid \
             FROM pg_catalog.pg_stat_activity AS activity \
             WHERE activity.datname = pg_catalog.current_database() \
                AND activity.pid <> pg_catalog.pg_backend_pid() \
                AND activity.state = 'active' \
                AND activity.wait_event_type = 'Lock' \
                AND activity.query LIKE \
                    '%starring_runtime_startup_recovery_execute_pending_drain_v2%' \
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
    panic!("pending drain adapter did not reach the writer fence lock")
}
