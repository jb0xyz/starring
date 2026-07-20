#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn claims_and_failures_persist_exact_convergence_attempts() {
    run_migrated_runtime_database_test(
        "convergence_attempts",
        claims_and_failures_persist_exact_convergence_attempts_scenario,
    )
    .await;
}

async fn claims_and_failures_persist_exact_convergence_attempts_scenario(
    pool: PgPool,
    _connect_options: PgConnectOptions,
) {
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::with_config(
        pool.clone(),
        PostgresRuntimeConvergenceConfigV1 {
            maximum_controller_lease: Duration::from_secs(10),
            maximum_serving_lease: Duration::from_secs(10),
            maximum_retry_delay: Duration::from_secs(10),
            statement_timeout: Duration::from_millis(20),
            lock_timeout: Duration::from_millis(10),
            ..PostgresRuntimeConvergenceConfigV1::default()
        },
    )
    .unwrap();
    let initial = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::Created(snapshot)
        | EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
    };
    assert_eq!(deployment_attempt(&pool).await, (0, None));
    assert_unclaimed_attempt_tamper_is_rejected(&pool).await;
    let controller_a = ControllerId::parse("runtime-attempt-controller-a").unwrap();
    let first_request = ClaimDeploymentV1 {
        scope: scope(),
        expected_revision: initial.revision,
        controller_id: controller_a.clone(),
        lease_for: Duration::from_millis(100),
    };
    let first = adapter
        .claim_execution(first_request.clone())
        .await
        .unwrap();
    assert_eq!(first.convergence_attempt, NonZeroU32::MIN);
    let replay = adapter.claim_execution(first_request).await.unwrap();
    assert_eq!(replay.convergence_attempt, first.convergence_attempt);
    assert_eq!(replay.fencing_token, first.fencing_token);
    let renewed = adapter
        .claim_execution(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: first.snapshot.revision,
            controller_id: controller_a,
            lease_for: Duration::from_millis(300),
        })
        .await
        .unwrap();
    assert_eq!(renewed.convergence_attempt, NonZeroU32::MIN);
    assert!(renewed.fencing_token > first.fencing_token);
    assert_eq!(deployment_attempt(&pool).await, (1, None));
    assert_direct_claim_tamper_is_rejected(
        &pool,
        "runtime-attempt-tampered-active",
        FencingToken::new(renewed.fencing_token.get() + 1).unwrap(),
        1,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_direct_claim_tamper_is_rejected(
        &pool,
        "runtime-attempt-tampered-expired",
        renewed.fencing_token,
        2,
    )
    .await;
    let controller_b = ControllerId::parse("runtime-attempt-controller-b").unwrap();
    let reclaimed = adapter
        .claim_execution(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: renewed.snapshot.revision,
            controller_id: controller_b.clone(),
            lease_for: Duration::from_secs(5),
        })
        .await
        .unwrap();
    assert_eq!(reclaimed.convergence_attempt.get(), 2);
    assert_eq!(deployment_attempt(&pool).await, (2, None));
    let revision = advance_claim_to_runtime_pending(
        &adapter,
        &reclaimed,
        &controller_b,
        reclaimed.fencing_token,
    )
    .await;
    let wrong_attempt = adapter
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope(),
            expected_revision: revision,
            controller_id: controller_b.clone(),
            fencing_token: reclaimed.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: DeploymentMutationV1::RecordRetryableFailure {
                failure_id: automation_runtime_convergence::RuntimeFailureId::parse(
                    "runtime-attempt-wrong",
                )
                .unwrap(),
                kind: automation_runtime_convergence::RuntimeFailureKindV1::GatewayReadyTimeout,
                code: "gateway_ready_timeout".to_string(),
                message: "ignored".to_string(),
                attempt: NonZeroU32::MIN,
                retry_after: Duration::from_millis(150),
            },
        })
        .await
        .unwrap_err();
    assert!(matches!(
        wrong_attempt,
        RuntimeConvergenceStoreError::ConvergenceAttemptConflict
    ));
    let failure_request = SubmitDeploymentMutationV1 {
            scope: scope(),
            expected_revision: revision,
            controller_id: controller_b,
            fencing_token: reclaimed.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: DeploymentMutationV1::RecordRetryableFailure {
                failure_id: automation_runtime_convergence::RuntimeFailureId::parse(
                    "runtime-attempt-retry",
                )
                .unwrap(),
                kind: automation_runtime_convergence::RuntimeFailureKindV1::GatewayReadyTimeout,
                code: "gateway_ready_timeout".to_string(),
                message: "ignored".to_string(),
                attempt: NonZeroU32::new(2).unwrap(),
                retry_after: Duration::from_millis(150),
            },
        };
    let failure = adapter
        .mutate(failure_request.clone())
        .await
        .unwrap();
    assert!(failure.snapshot.controller_lease.is_none());
    assert_eq!(deployment_attempt(&pool).await, (2, Some(2)));
    assert_same_attempt_failure_rewrite_is_rejected(&pool).await;
    let replayed_failure = adapter.mutate(failure_request.clone()).await.unwrap();
    assert!(matches!(
        replayed_failure.outcome,
        automation_runtime_convergence::TransitionOutcomeV1::Replayed { .. }
    ));
    let retry_operator = adapter
        .recover_blocked_for_operator(RecoverBlockedDeploymentV1 {
            scope: scope(),
            expected_revision: failure.snapshot.revision,
            expected_failure_id: automation_runtime_convergence::RuntimeFailureId::parse(
                "runtime-attempt-retry",
            )
            .unwrap(),
            expected_failure_attempt: NonZeroU32::new(2).unwrap(),
            controller_id: ControllerId::parse("runtime-attempt-operator").unwrap(),
            lease_for: Duration::from_secs(5),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        retry_operator,
        RuntimeConvergenceStoreError::OperatorActionRequired
    ));
    let controller_c = ControllerId::parse("runtime-attempt-controller-c").unwrap();
    let early = adapter
        .claim_execution(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: failure.snapshot.revision,
            controller_id: controller_c.clone(),
            lease_for: Duration::from_secs(5),
        })
        .await
        .unwrap_err();
    assert!(matches!(early, RuntimeConvergenceStoreError::RetryNotReady));
    tokio::time::sleep(Duration::from_millis(175)).await;
    let retry = adapter
        .claim_execution(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: failure.snapshot.revision,
            controller_id: controller_c.clone(),
            lease_for: Duration::from_secs(5),
        })
        .await
        .unwrap();
    assert_eq!(retry.convergence_attempt.get(), 3);
    let stale_failure = adapter
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope(),
            expected_revision: failure.snapshot.revision,
            controller_id: controller_c.clone(),
            fencing_token: retry.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: failure_request.mutation,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        stale_failure,
        RuntimeConvergenceStoreError::IdempotencyConflict
    ));
    let resumed = adapter
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope(),
            expected_revision: retry.snapshot.revision,
            controller_id: controller_c,
            fencing_token: retry.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: DeploymentMutationV1::ResumeRuntimePending,
        })
        .await
        .unwrap();
    assert!(matches!(
        resumed.snapshot.phase,
        automation_runtime_convergence::RuntimeDeploymentPhaseV1::RuntimePending {
            condition: automation_runtime_convergence::RuntimePendingConditionV1::Ready
        }
    ));
    assert_eq!(deployment_attempt(&pool).await, (3, Some(2)));
    let panels = adapter
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope(),
            expected_revision: resumed.snapshot.revision,
            controller_id: ControllerId::parse("runtime-attempt-controller-c").unwrap(),
            fencing_token: retry.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: DeploymentMutationV1::BeginPanelReconciliation,
        })
        .await
        .unwrap();
    let blocked = adapter
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope(),
            expected_revision: panels.snapshot.revision,
            controller_id: ControllerId::parse("runtime-attempt-controller-c").unwrap(),
            fencing_token: retry.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: DeploymentMutationV1::RecordBlockedFailure {
                failure_id: automation_runtime_convergence::RuntimeFailureId::parse(
                    "runtime-attempt-blocked",
                )
                .unwrap(),
                kind: automation_runtime_convergence::RuntimeFailureKindV1::InvariantViolation,
                code: "runtime_invariant_violation".to_string(),
                message: "ignored".to_string(),
            },
        })
        .await
        .unwrap();
    assert!(blocked.snapshot.controller_lease.is_none());
    assert_eq!(deployment_attempt(&pool).await, (3, Some(3)));
    let blocked_claim = adapter
        .claim_execution(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: blocked.snapshot.revision,
            controller_id: ControllerId::parse("runtime-attempt-operator").unwrap(),
            lease_for: Duration::from_secs(5),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        blocked_claim,
        RuntimeConvergenceStoreError::OperatorActionRequired
    ));
    let wrong_operator = adapter
        .recover_blocked_for_operator(RecoverBlockedDeploymentV1 {
            scope: scope(),
            expected_revision: blocked.snapshot.revision,
            expected_failure_id: automation_runtime_convergence::RuntimeFailureId::parse(
                "runtime-attempt-other-block",
            )
            .unwrap(),
            expected_failure_attempt: NonZeroU32::new(3).unwrap(),
            controller_id: ControllerId::parse("runtime-attempt-operator").unwrap(),
            lease_for: Duration::from_secs(5),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        wrong_operator,
        RuntimeConvergenceStoreError::ConvergenceAttemptConflict
    ));
    let operator_request = RecoverBlockedDeploymentV1 {
        scope: scope(),
        expected_revision: blocked.snapshot.revision,
        expected_failure_id: automation_runtime_convergence::RuntimeFailureId::parse(
            "runtime-attempt-blocked",
        )
        .unwrap(),
        expected_failure_attempt: NonZeroU32::new(3).unwrap(),
        controller_id: ControllerId::parse("runtime-attempt-operator").unwrap(),
        lease_for: Duration::from_secs(5),
    };
    let recovered = adapter
        .recover_blocked_for_operator(operator_request.clone())
        .await
        .unwrap();
    assert_eq!(recovered.convergence_attempt.get(), 4);
    assert!(matches!(
        recovered.snapshot.phase,
        automation_runtime_convergence::RuntimeDeploymentPhaseV1::RuntimePending {
            condition: automation_runtime_convergence::RuntimePendingConditionV1::Ready
        }
    ));
    assert_eq!(deployment_attempt(&pool).await, (4, Some(3)));
    let recovered_replay = adapter
        .recover_blocked_for_operator(operator_request)
        .await
        .unwrap();
    assert_eq!(recovered_replay.fencing_token, recovered.fencing_token);
    assert_eq!(
        recovered_replay.convergence_attempt,
        recovered.convergence_attempt
    );
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn live_attestation_binds_the_current_convergence_attempt() {
    run_migrated_runtime_database_test(
        "live_attempt_binding",
        live_attestation_binds_the_current_convergence_attempt_scenario,
    )
    .await;
}

async fn live_attestation_binds_the_current_convergence_attempt_scenario(
    pool: PgPool,
    _connect_options: PgConnectOptions,
) {
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let initial = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::Created(snapshot)
        | EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
    };
    let claim = adapter
        .claim_execution(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: ControllerId::parse("runtime-live-attempt-controller").unwrap(),
            lease_for: Duration::from_secs(30),
        })
        .await
        .unwrap();
    let expected_attempt = claim.convergence_attempt;
    converge_claimed(
        &adapter,
        automation_runtime_convergence_postgres::ClaimReceiptV1::from(claim),
        ProcessInstanceId::parse("runtime-live-attempt-process").unwrap(),
    )
    .await;
    let attestation_attempt = sqlx::query_scalar::<_, i64>(
        "SELECT convergence_attempt_no FROM public.runtime_attestations \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attestation_attempt, i64::from(expected_attempt.get()));
    let status = adapter.status(&scope()).await.unwrap();
    assert!(matches!(status.availability, DeploymentAvailabilityV1::Live));
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn concurrent_claims_start_only_one_convergence_attempt() {
    run_migrated_runtime_database_test(
        "concurrent_attempt_claim",
        concurrent_claims_start_only_one_convergence_attempt_scenario,
    )
    .await;
}

async fn concurrent_claims_start_only_one_convergence_attempt_scenario(
    pool: PgPool,
    _connect_options: PgConnectOptions,
) {
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let initial = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::Created(snapshot)
        | EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
    };
    let first_adapter = adapter.clone();
    let first = tokio::spawn(async move {
        first_adapter
            .claim_execution(ClaimDeploymentV1 {
                scope: scope(),
                expected_revision: initial.revision,
                controller_id: ControllerId::parse("concurrent-attempt-a").unwrap(),
                lease_for: Duration::from_secs(5),
            })
            .await
    });
    let second_adapter = adapter.clone();
    let second = tokio::spawn(async move {
        second_adapter
            .claim_execution(ClaimDeploymentV1 {
                scope: scope(),
                expected_revision: initial.revision,
                controller_id: ControllerId::parse("concurrent-attempt-b").unwrap(),
                lease_for: Duration::from_secs(5),
            })
            .await
    });
    let first = first.await.unwrap();
    let second = second.await.unwrap();
    let successes = [first.as_ref(), second.as_ref()]
        .into_iter()
        .filter(|result| result.is_ok())
        .count();
    assert_eq!(successes, 1);
    let failure = [first, second]
        .into_iter()
        .find_map(Result::err)
        .unwrap();
    assert!(matches!(
        failure,
        RuntimeConvergenceStoreError::RevisionConflict
    ));
    assert_eq!(deployment_attempt(&pool).await, (1, None));
}

async fn advance_claim_to_runtime_pending(
    adapter: &PostgresRuntimeConvergence,
    claim: &automation_runtime_convergence_postgres::ClaimExecutionReceiptV1,
    controller_id: &ControllerId,
    fencing_token: automation_runtime_convergence::FencingToken,
) -> automation_runtime_convergence::DeploymentRevision {
    let mut revision = mutate(
        adapter,
        claim.snapshot.revision,
        controller_id,
        fencing_token,
        DeploymentMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            observed_runtime: None,
            checked_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        controller_id,
        fencing_token,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        controller_id,
        fencing_token,
        DeploymentMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: None,
            target_runtime_generation: RuntimeGeneration::FIRST,
            drained_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate(
        adapter,
        revision,
        controller_id,
        fencing_token,
        DeploymentMutationV1::BeginActivation,
    )
    .await;
    mutate(
        adapter,
        revision,
        controller_id,
        fencing_token,
        DeploymentMutationV1::AcceptActivation(ActivationAttestationV1 {
            activation_request_id: ActivationRequestId::parse(ACTIVATION).unwrap(),
            target: target(),
            runtime_generation: RuntimeGeneration::FIRST,
            kind: ActivationOutcomeKindV1::AlreadyActive,
            activated_at: claim.acquired_at,
        }),
    )
    .await
}

async fn deployment_attempt(pool: &PgPool) -> (i64, Option<i64>) {
    sqlx::query_as::<_, (i64, Option<i64>)>(
        "SELECT convergence_attempt_no, last_failure_attempt_no \
         FROM public.runtime_deployments WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_unclaimed_attempt_tamper_is_rejected(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', $1, TRUE)")
        .bind(now.to_rfc3339())
        .execute(&mut *transaction)
        .await
        .unwrap();
    let error = sqlx::query(
        "UPDATE public.runtime_deployments SET convergence_attempt_no = 1 \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .execute(&mut *transaction)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|error| error.code()).as_deref(),
        Some("23514")
    );
    transaction.rollback().await.unwrap();
    assert_eq!(deployment_attempt(pool).await, (0, None));
}

async fn assert_direct_claim_tamper_is_rejected(
    pool: &PgPool,
    controller_id: &str,
    fencing_token: FencingToken,
    convergence_attempt: i64,
) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', $1, TRUE)")
        .bind(now.to_rfc3339())
        .execute(&mut *transaction)
        .await
        .unwrap();
    let (snapshot, revision) = sqlx::query_as::<_, (Json<Value>, i64)>(
        "SELECT snapshot, revision FROM public.runtime_deployments WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    let expires_at = now + TimeDelta::seconds(5);
    let mut snapshot = snapshot.0;
    snapshot["revision"] = json!(revision + 1);
    snapshot["controller_lease"] = serde_json::to_value(ControllerLeaseV1 {
        controller_id: ControllerId::parse(controller_id).unwrap(),
        fencing_token,
        acquired_at: now,
        expires_at,
    })
    .unwrap();
    snapshot["last_fencing_token"] = json!(fencing_token.get());
    let error = sqlx::query(
        "UPDATE public.runtime_deployments SET snapshot = $2, revision = revision + 1, \
         controller_id = $3, controller_fencing_token = $4, controller_acquired_at = $5, \
         controller_lease_expires_at = $6, last_fencing_token = $4, \
         convergence_attempt_no = $7, updated_at = $5 WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .bind(Json(snapshot))
    .bind(controller_id)
    .bind(i64::try_from(fencing_token.get()).unwrap())
    .bind(now)
    .bind(expires_at)
    .bind(convergence_attempt)
    .execute(&mut *transaction)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|error| error.code()).as_deref(),
        Some("23514")
    );
    transaction.rollback().await.unwrap();
}

async fn assert_same_attempt_failure_rewrite_is_rejected(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', $1, TRUE)")
        .bind(now.to_rfc3339())
        .execute(&mut *transaction)
        .await
        .unwrap();
    let error = sqlx::query(
        "UPDATE public.runtime_deployments SET \
         snapshot = pg_catalog.jsonb_set(pg_catalog.jsonb_set(pg_catalog.jsonb_set( \
             snapshot, '{revision}', pg_catalog.to_jsonb(revision + 1)), \
             '{last_runtime_failure,failure,failure_id}', \
             pg_catalog.to_jsonb('runtime-attempt-rewritten'::TEXT)), \
             '{phase,condition,failure,failure_id}', \
             pg_catalog.to_jsonb('runtime-attempt-rewritten'::TEXT)), \
         revision = revision + 1, updated_at = $2 WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|error| error.code()).as_deref(),
        Some("23514")
    );
    transaction.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn attempt_migration_rejects_ambiguous_legacy_execution_history() {
    let database = isolated_runtime_database("attempt_migration_legacy").await;
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version < 202607200003)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();
    }
    seed_product_target(&database.pool).await;
    let snapshot = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT snapshot FROM public.runtime_deployments WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&database.pool)
    .await
    .unwrap();
    let mut deployment = automation_runtime_convergence::RuntimeDeployment::restore(
        serde_json::from_value(snapshot.0).unwrap(),
    )
    .unwrap();
    let mut transaction = database.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    let expires_at = now + TimeDelta::seconds(60);
    deployment
        .acquire_lease(automation_runtime_convergence::LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: ControllerId::parse("legacy-attempt-controller").unwrap(),
            fencing_token: automation_runtime_convergence::FencingToken::new(1).unwrap(),
            now,
            expires_at,
        })
        .unwrap();
    let claimed = deployment.snapshot();
    sqlx::query("SELECT pg_catalog.set_config('starring.runtime_mutation_clock', $1, TRUE)")
        .bind(now.to_rfc3339())
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments SET snapshot = $2, revision = $3, \
         controller_id = $4, controller_fencing_token = 1, controller_acquired_at = $5, \
         controller_lease_expires_at = $6, last_fencing_token = 1, updated_at = $5 \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .bind(Json(serde_json::to_value(&claimed).unwrap()))
    .bind(i64::try_from(claimed.revision.get()).unwrap())
    .bind("legacy-attempt-controller")
    .bind(now)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let attempt_migration = MIGRATOR
        .iter()
        .find(|migration| migration.version == 202607200003)
        .unwrap();
    let mut migration_transaction = database.pool.begin().await.unwrap();
    let error = sqlx::raw_sql(attempt_migration.sql.as_ref())
        .execute(&mut *migration_transaction)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|error| error.code()).as_deref(),
        Some("55000")
    );
    migration_transaction.rollback().await.unwrap();
    let columns = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_attribute AS attribute \
         WHERE attribute.attrelid = pg_catalog.to_regclass('public.runtime_deployments') \
           AND attribute.attname IN ('convergence_attempt_no', 'last_failure_attempt_no') \
           AND attribute.attnum > 0 AND NOT attribute.attisdropped",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(columns, 0);
    drop_runtime_database(database).await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn attempt_migration_rejects_trigger_drift_atomically() {
    let database = isolated_runtime_database("attempt_migration_trigger").await;
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version < 202607200003)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "ALTER TABLE public.runtime_deployments \
         DISABLE TRIGGER runtime_deployments_validate_projection",
    )
    .execute(&database.pool)
    .await
    .unwrap();
    let attempt_migration = MIGRATOR
        .iter()
        .find(|migration| migration.version == 202607200003)
        .unwrap();
    let mut transaction = database.pool.begin().await.unwrap();
    let error = sqlx::raw_sql(attempt_migration.sql.as_ref())
        .execute(&mut *transaction)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|error| error.code()).as_deref(),
        Some("55000")
    );
    transaction.rollback().await.unwrap();
    let residue = sqlx::query_as::<_, (i64, bool)>(
        "SELECT pg_catalog.count(*), \
         pg_catalog.to_regprocedure( \
         'public.validate_runtime_convergence_attempt_projection()') IS NOT NULL \
         FROM pg_catalog.pg_attribute AS attribute \
         WHERE attribute.attrelid = pg_catalog.to_regclass('public.runtime_deployments') \
           AND attribute.attname IN ('convergence_attempt_no', 'last_failure_attempt_no') \
           AND attribute.attnum > 0 AND NOT attribute.attisdropped",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(residue, (0, false));
    drop_runtime_database(database).await;
}
