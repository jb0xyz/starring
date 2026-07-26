struct DrainAcceptanceFixture {
    adapter: PostgresRuntimeConvergence,
    prior_claim: automation_runtime_convergence_postgres::ClaimReceiptV1,
    prior_live: automation_runtime_convergence_postgres::MutationReceiptV1,
    prior_serving: automation_runtime_convergence_postgres::ServingLeaseReceiptV1,
    next_claim: automation_runtime_convergence_postgres::ClaimReceiptV1,
    next_revision: automation_runtime_convergence::DeploymentRevision,
    previous_runtime: RuntimeProcessIdentityV1,
}

struct NoPreviousDrainAcceptanceFixture {
    adapter: PostgresRuntimeConvergence,
    claim: automation_runtime_convergence_postgres::ClaimReceiptV1,
    revision: automation_runtime_convergence::DeploymentRevision,
    requested_at: chrono::DateTime<chrono::Utc>,
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn accept_drain_rechecks_exact_previous_serving_state() {
    run_migrated_runtime_database_test("accept_drain_exact", |pool, _| async move {
        let fixture = prepare_drain_acceptance(&pool).await;
        assert_accept_drain_rejected(
            &fixture,
            RuntimeConvergenceStoreError::ServingLeaseConflict.code(),
        )
        .await;

        sqlx::query("CREATE TABLE runtime_test_serving_backup AS TABLE runtime_serving_leases")
            .execute(&pool)
            .await
            .unwrap();
        let requested_at = fixture
            .adapter
            .status(&next_scope())
            .await
            .unwrap()
            .snapshot
            .requested_at;

        replace_serving_with_backup(&pool, false).await;
        assert_accept_drain_rejected(
            &fixture,
            RuntimeConvergenceStoreError::ServingLeaseConflict.code(),
        )
        .await;
        replace_serving_with_backup(&pool, true).await;

        for assignment in [
            "tenant_id = 'tenant-other'",
            "installation_id = 'installation-other'",
            "deployment_id = 'runtime-pg-deployment-next'",
            "process_instance_id = 'wrong-process'",
            "runtime_generation = 2",
            "target_version = 2",
            "target_content_hash = '1111111111111111111111111111111111111111111111111111111111111111'",
            "binding_revision = 2",
            "binding_fingerprint = '2222222222222222222222222222222222222222222222222222222222222222'",
            "acquired_at = closure.at",
        ] {
            close_and_corrupt_serving(&pool, assignment).await;
            assert_accept_drain_rejected(
                &fixture,
                RuntimeConvergenceStoreError::ServingLeaseConflict.code(),
            )
            .await;
            replace_serving_with_backup(&pool, true).await;
        }

        corrupt_serving(
            &pool,
            "UPDATE runtime_serving_leases SET connected = FALSE, serving = FALSE",
        )
        .await;
        assert_accept_drain_rejected(
            &fixture,
            RuntimeConvergenceStoreError::InvalidPersistedState("").code(),
        )
        .await;
        replace_serving_with_backup(&pool, true).await;

        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE runtime_serving_leases SET connected = FALSE, serving = FALSE, \
             last_heartbeat_at = $1 - INTERVAL '1 microsecond', \
             expires_at = $1 - INTERVAL '1 microsecond'",
        )
        .bind(requested_at)
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert_accept_drain_rejected(
            &fixture,
            RuntimeConvergenceStoreError::ServingLeaseConflict.code(),
        )
        .await;
        replace_serving_with_backup(&pool, true).await;

        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE runtime_serving_leases SET connected = TRUE, serving = TRUE, \
             last_heartbeat_at = $1, expires_at = $1",
        )
        .bind(requested_at)
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert_accept_drain_rejected(
            &fixture,
            RuntimeConvergenceStoreError::InvalidPersistedState("").code(),
        )
        .await;
        replace_serving_with_backup(&pool, true).await;

        let disconnected = fixture
            .adapter
            .mark_serving_disconnected(MarkServingDisconnectedV1 {
                identity: fixture.prior_serving.identity.clone(),
            })
            .await
            .unwrap();
        assert_accept_drain_rejected(
            &fixture,
            RuntimeConvergenceStoreError::Domain(
                automation_runtime_convergence::RuntimeDeploymentError::AttestationTimeRegression,
            )
            .code(),
        )
        .await;
        let request = accept_drain_request_at(&fixture, disconnected.last_heartbeat_at);
        let applied = fixture.adapter.mutate(request.clone()).await.unwrap();
        assert!(matches!(
            applied.outcome,
            automation_runtime_convergence::TransitionOutcomeV1::Applied { .. }
        ));
        assert!(matches!(
            applied.snapshot.phase,
            automation_runtime_convergence::RuntimeDeploymentPhaseV1::Drained
        ));
        let replayed = fixture.adapter.mutate(request).await.unwrap();
        assert!(matches!(
            replayed.outcome,
            automation_runtime_convergence::TransitionOutcomeV1::Replayed { .. }
        ));
        assert_eq!(replayed.snapshot.revision, applied.snapshot.revision);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn accept_drain_serializes_with_heartbeat_and_certification() {
    run_migrated_runtime_database_test("accept_drain_race", |pool, _| async move {
        let fixture = prepare_drain_acceptance(&pool).await;

        let mut blocker = pool.begin().await.unwrap();
        lock_test_serving_slot(&mut blocker).await;
        let heartbeat_adapter = fixture.adapter.clone();
        let heartbeat_identity = fixture.prior_serving.identity.clone();
        let heartbeat = tokio::spawn(async move {
            heartbeat_adapter
                .heartbeat_serving(HeartbeatServingLeaseV1 {
                    identity: heartbeat_identity,
                    lease_for: Duration::from_secs(45),
                })
                .await
        });
        wait_for_ungranted_locks(&pool, 1).await;
        let accepting_adapter = fixture.adapter.clone();
        let acceptance = accept_drain_request(&fixture);
        let accepting = tokio::spawn(async move { accepting_adapter.mutate(acceptance).await });
        wait_for_ungranted_locks(&pool, 2).await;
        blocker.commit().await.unwrap();
        let heartbeat = heartbeat.await.unwrap().unwrap();
        assert!(matches!(
            accepting.await.unwrap().unwrap_err(),
            RuntimeConvergenceStoreError::ServingLeaseConflict
        ));
        assert_drain_requested(&fixture).await;

        let mut blocker = pool.begin().await.unwrap();
        lock_test_serving_slot(&mut blocker).await;
        let certifying_adapter = fixture.adapter.clone();
        let certification = replay_certification_request(&fixture);
        let certifying =
            tokio::spawn(async move { certifying_adapter.certify_live(certification).await });
        wait_for_ungranted_locks(&pool, 1).await;
        let accepting_adapter = fixture.adapter.clone();
        let acceptance = accept_drain_request(&fixture);
        let accepting = tokio::spawn(async move { accepting_adapter.mutate(acceptance).await });
        wait_for_ungranted_locks(&pool, 2).await;
        blocker.commit().await.unwrap();
        certifying.await.unwrap().unwrap();
        assert!(matches!(
            accepting.await.unwrap().unwrap_err(),
            RuntimeConvergenceStoreError::ServingLeaseConflict
        ));
        assert_drain_requested(&fixture).await;

        let disconnected = fixture
            .adapter
            .mark_serving_disconnected(MarkServingDisconnectedV1 {
                identity: heartbeat.identity,
            })
            .await
            .unwrap();

        let mut blocker = pool.begin().await.unwrap();
        lock_test_serving_slot(&mut blocker).await;
        let accepting_adapter = fixture.adapter.clone();
        let acceptance = accept_drain_request_at(&fixture, disconnected.last_heartbeat_at);
        let accepting = tokio::spawn(async move { accepting_adapter.mutate(acceptance).await });
        wait_for_ungranted_locks(&pool, 1).await;
        let heartbeat_adapter = fixture.adapter.clone();
        let heartbeat_identity = disconnected.identity;
        let heartbeat = tokio::spawn(async move {
            heartbeat_adapter
                .heartbeat_serving(HeartbeatServingLeaseV1 {
                    identity: heartbeat_identity,
                    lease_for: Duration::from_secs(45),
                })
                .await
        });
        wait_for_ungranted_locks(&pool, 2).await;
        let certifying_adapter = fixture.adapter.clone();
        let certification = replay_certification_request(&fixture);
        let certifying =
            tokio::spawn(async move { certifying_adapter.certify_live(certification).await });
        wait_for_ungranted_locks(&pool, 3).await;
        blocker.commit().await.unwrap();
        let (accepted, heartbeat, certification) =
            tokio::time::timeout(Duration::from_secs(4), async {
                tokio::join!(accepting, heartbeat, certifying)
            })
            .await
            .unwrap();
        let accepted = accepted.unwrap().unwrap();
        assert!(matches!(
            accepted.snapshot.phase,
            automation_runtime_convergence::RuntimeDeploymentPhaseV1::Drained
        ));
        assert!(matches!(
            heartbeat.unwrap().unwrap_err(),
            RuntimeConvergenceStoreError::ServingLeaseConflict
        ));
        assert!(matches!(
            certification.unwrap().unwrap_err(),
            RuntimeConvergenceStoreError::ServingLeaseConflict
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn accept_drain_accepts_an_exact_lease_expired_after_the_request() {
    run_migrated_runtime_database_test("accept_drain_expired", |pool, _| async move {
        let fixture = prepare_drain_acceptance(&pool).await;
        let closure_at = fixture.next_claim.acquired_at + chrono::TimeDelta::microseconds(1);
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE runtime_serving_leases SET connected = TRUE, serving = TRUE, \
             last_heartbeat_at = $1, expires_at = $2",
        )
        .bind(fixture.next_claim.acquired_at)
        .bind(closure_at)
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        let applied = fixture
            .adapter
            .mutate(accept_drain_request_at(&fixture, closure_at))
            .await
            .unwrap();
        assert!(matches!(
            applied.outcome,
            automation_runtime_convergence::TransitionOutcomeV1::Applied { .. }
        ));
        assert!(matches!(
            applied.snapshot.phase,
            automation_runtime_convergence::RuntimeDeploymentPhaseV1::Drained
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn accept_drain_without_a_previous_runtime_rejects_an_active_lease() {
    run_migrated_runtime_database_test("accept_drain_none_active", |pool, _| async move {
        let fixture = prepare_no_previous_drain_acceptance(&pool).await;
        let heartbeat_at = database_now(&pool).await;
        insert_retained_serving(
            &pool,
            fixture.requested_at - chrono::TimeDelta::seconds(1),
            heartbeat_at,
            heartbeat_at + chrono::TimeDelta::seconds(45),
            true,
        )
        .await;
        let error = fixture
            .adapter
            .mutate(no_previous_accept_drain_request(
                &fixture,
                fixture.claim.acquired_at,
            ))
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            RuntimeConvergenceStoreError::ServingLeaseConflict.code()
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn accept_drain_without_a_previous_runtime_accepts_an_old_closed_lease() {
    run_migrated_runtime_database_test("accept_drain_none_old_closed", |pool, _| async move {
        let fixture = prepare_no_previous_drain_acceptance(&pool).await;
        let closure_at = fixture.requested_at - chrono::TimeDelta::microseconds(1);
        insert_retained_serving(
            &pool,
            fixture.requested_at - chrono::TimeDelta::seconds(1),
            closure_at,
            closure_at,
            false,
        )
        .await;
        let applied = fixture
            .adapter
            .mutate(no_previous_accept_drain_request(
                &fixture,
                fixture.claim.acquired_at,
            ))
            .await
            .unwrap();
        assert!(matches!(
            applied.snapshot.phase,
            automation_runtime_convergence::RuntimeDeploymentPhaseV1::Drained
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn accept_drain_without_a_previous_runtime_requires_causal_closure_evidence() {
    run_migrated_runtime_database_test("accept_drain_none_causal", |pool, _| async move {
        let fixture = prepare_no_previous_drain_acceptance(&pool).await;
        let closure_at = database_now(&pool).await;
        insert_retained_serving(
            &pool,
            fixture.requested_at,
            closure_at,
            closure_at,
            false,
        )
        .await;
        let stale = fixture
            .adapter
            .mutate(no_previous_accept_drain_request(
                &fixture,
                fixture.claim.acquired_at,
            ))
            .await
            .unwrap_err();
        assert_eq!(
            stale.code(),
            RuntimeConvergenceStoreError::Domain(
                automation_runtime_convergence::RuntimeDeploymentError::AttestationTimeRegression,
            )
            .code()
        );
        let applied = fixture
            .adapter
            .mutate(no_previous_accept_drain_request(&fixture, closure_at))
            .await
            .unwrap();
        assert!(matches!(
            applied.snapshot.phase,
            automation_runtime_convergence::RuntimeDeploymentPhaseV1::Drained
        ));
    })
    .await;
}

async fn prepare_drain_acceptance(pool: &PgPool) -> DrainAcceptanceFixture {
    seed_product_target(pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let initial = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
        outcome => panic!("seeded deployment must replay: {outcome:?}"),
    };
    let prior_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: ControllerId::parse("runtime-drain-prior").unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let (prior_live, prior_serving) = converge_claimed_with_lease(
        &adapter,
        prior_claim.clone(),
        ProcessInstanceId::parse("runtime-drain-prior-process").unwrap(),
        Duration::from_secs(45),
    )
    .await;
    let previous_runtime = RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::FIRST,
        process_instance_id: prior_serving.identity.process_instance_id.clone(),
    };
    let next_generation = RuntimeGeneration::new(2).unwrap();
    let next_request = EnqueueDeploymentV1 {
        identity: RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse(NEXT_DEPLOYMENT).unwrap(),
            tenant_id: TenantId::parse(TENANT).unwrap(),
            installation_id: InstallationId::parse(INSTALLATION).unwrap(),
            promotion_id: PromotionId::parse(NEXT_PROMOTION).unwrap(),
            activation_request_id: ActivationRequestId::parse(NEXT_ACTIVATION).unwrap(),
        },
        target: target(),
        runtime_generation: next_generation,
        previous_runtime: Some(previous_runtime.clone()),
        installation_authority_revision: 1,
    };
    seed_next_product_journal(pool, &next_request).await;
    let next = match adapter.enqueue(next_request).await.unwrap() {
        EnqueueDeploymentOutcomeV1::Created(snapshot)
        | EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
    };
    let next_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: next_scope(),
            expected_revision: next.revision,
            controller_id: ControllerId::parse("runtime-drain-next").unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let next_revision = mutate_scoped(
        &adapter,
        next_scope(),
        next_generation,
        next_claim.snapshot.revision,
        &next_claim.controller_id,
        next_claim.fencing_token,
        next_claim.convergence_attempt,
        DeploymentMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target(),
            runtime_generation: next_generation,
            observed_runtime: Some(previous_runtime.clone()),
            checked_at: next_claim.acquired_at,
        }),
    )
    .await;
    let next_revision = mutate_scoped(
        &adapter,
        next_scope(),
        next_generation,
        next_revision,
        &next_claim.controller_id,
        next_claim.fencing_token,
        next_claim.convergence_attempt,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    DrainAcceptanceFixture {
        adapter,
        prior_claim,
        prior_live,
        prior_serving,
        next_claim,
        next_revision,
        previous_runtime,
    }
}

async fn prepare_no_previous_drain_acceptance(
    pool: &PgPool,
) -> NoPreviousDrainAcceptanceFixture {
    seed_product_target(pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let initial = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::Created(snapshot)
        | EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
    };
    let claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: ControllerId::parse("runtime-drain-none").unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let revision = mutate(
        &adapter,
        claim.snapshot.revision,
        &claim.controller_id,
        claim.fencing_token,
        claim.convergence_attempt,
        DeploymentMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            observed_runtime: None,
            checked_at: claim.acquired_at,
        }),
    )
    .await;
    let revision = mutate(
        &adapter,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        claim.convergence_attempt,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    NoPreviousDrainAcceptanceFixture {
        adapter,
        claim,
        revision,
        requested_at: initial.requested_at,
    }
}

fn no_previous_accept_drain_request(
    fixture: &NoPreviousDrainAcceptanceFixture,
    drained_at: chrono::DateTime<chrono::Utc>,
) -> SubmitDeploymentMutationV1 {
    SubmitDeploymentMutationV1 {
        scope: scope(),
        expected_revision: fixture.revision,
        controller_id: fixture.claim.controller_id.clone(),
        fencing_token: fixture.claim.fencing_token,
        convergence_attempt: fixture.claim.convergence_attempt,
        runtime_generation: RuntimeGeneration::FIRST,
        mutation: DeploymentMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: None,
            target_runtime_generation: RuntimeGeneration::FIRST,
            drained_at,
        }),
    }
}

async fn insert_retained_serving(
    pool: &PgPool,
    acquired_at: chrono::DateTime<chrono::Utc>,
    last_heartbeat_at: chrono::DateTime<chrono::Utc>,
    expires_at: chrono::DateTime<chrono::Utc>,
    connected: bool,
) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO runtime_serving_leases (guild_id, ruleset_key, tenant_id, installation_id, \
         deployment_id, attestation_id, process_instance_id, runtime_generation, target_version, \
         target_content_hash, binding_revision, binding_fingerprint, lease_epoch, revision, \
         connected, serving, acquired_at, last_heartbeat_at, expires_at) \
         VALUES ($1, $2, $3, $4, 'retained-runtime-deployment', $5, \
         'retained-runtime-process', 1, 1, $6, 1, $7, 1, 1, $8, $8, $9, $10, $11)",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .bind(TENANT)
    .bind(INSTALLATION)
    .bind("3".repeat(64))
    .bind(CONTENT_HASH)
    .bind(BINDING_FINGERPRINT)
    .bind(connected)
    .bind(acquired_at)
    .bind(last_heartbeat_at)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn accept_drain_request(fixture: &DrainAcceptanceFixture) -> SubmitDeploymentMutationV1 {
    accept_drain_request_at(fixture, fixture.next_claim.acquired_at)
}

fn accept_drain_request_at(
    fixture: &DrainAcceptanceFixture,
    drained_at: chrono::DateTime<chrono::Utc>,
) -> SubmitDeploymentMutationV1 {
    SubmitDeploymentMutationV1 {
        scope: next_scope(),
        expected_revision: fixture.next_revision,
        controller_id: fixture.next_claim.controller_id.clone(),
        fencing_token: fixture.next_claim.fencing_token,
        convergence_attempt: fixture.next_claim.convergence_attempt,
        runtime_generation: RuntimeGeneration::new(2).unwrap(),
        mutation: DeploymentMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: Some(fixture.previous_runtime.clone()),
            target_runtime_generation: RuntimeGeneration::new(2).unwrap(),
            drained_at,
        }),
    }
}

fn replay_certification_request(fixture: &DrainAcceptanceFixture) -> SubmitLiveAttestationV1 {
    let live = fixture.prior_live.snapshot.live.clone().unwrap();
    SubmitLiveAttestationV1 {
        scope: scope(),
        expected_revision: automation_runtime_convergence::DeploymentRevision::new(
            fixture.prior_live.snapshot.revision.get() - 1,
        )
        .unwrap(),
        controller_id: fixture.prior_claim.controller_id.clone(),
        fencing_token: fixture.prior_claim.fencing_token,
        convergence_attempt: fixture.prior_claim.convergence_attempt,
        runtime_generation: RuntimeGeneration::FIRST,
        gateway_ready: live.gateway_ready,
        metadata: LiveMetadataV1 {
            runtime_build_revision: RuntimeBuildRevisionV1::parse("policy-build").unwrap(),
            panel_report_digest: PanelReportDigestV1::parse("7".repeat(64)).unwrap(),
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        },
        serving_lease_for: Duration::from_secs(45),
    }
}

async fn assert_accept_drain_rejected(fixture: &DrainAcceptanceFixture, expected_code: &str) {
    let error = fixture
        .adapter
        .mutate(accept_drain_request(fixture))
        .await
        .unwrap_err();
    assert_eq!(error.code(), expected_code);
    assert_drain_requested(fixture).await;
}

async fn assert_drain_requested(fixture: &DrainAcceptanceFixture) {
    let snapshot = fixture
        .adapter
        .status(&next_scope())
        .await
        .unwrap()
        .snapshot;
    assert_eq!(snapshot.revision, fixture.next_revision);
    assert!(matches!(
        snapshot.phase,
        automation_runtime_convergence::RuntimeDeploymentPhaseV1::DrainRequested
    ));
}

async fn corrupt_serving(pool: &PgPool, statement: &str) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(statement)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn close_and_corrupt_serving(pool: &PgPool, assignment: &str) {
    let statement = format!(
        "WITH closure AS (SELECT pg_catalog.clock_timestamp() AS at) \
         UPDATE runtime_serving_leases SET connected = FALSE, serving = FALSE, \
         last_heartbeat_at = closure.at, expires_at = closure.at, {assignment} FROM closure"
    );
    corrupt_serving(pool, &statement).await;
}

async fn replace_serving_with_backup(pool: &PgPool, restore: bool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("DELETE FROM runtime_serving_leases")
        .execute(&mut *transaction)
        .await
        .unwrap();
    if restore {
        sqlx::query("INSERT INTO runtime_serving_leases SELECT * FROM runtime_test_serving_backup")
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn lock_test_serving_slot(transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>) {
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(\
         pg_catalog.concat('starring-runtime-serving-slot-v1:', $1::TEXT, ':', $2::TEXT), 0))",
    )
    .bind(GUILD.to_string())
    .bind(RULESET)
    .execute(&mut **transaction)
    .await
    .unwrap();
}
