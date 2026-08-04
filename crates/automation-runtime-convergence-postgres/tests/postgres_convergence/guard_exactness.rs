#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn controller_guards_are_exact_and_renewal_replay_is_bounded() {
    run_migrated_runtime_database_test(
        "guard_exactness",
        controller_guards_are_exact_and_renewal_replay_is_bounded_scenario,
    )
    .await;
}

async fn controller_guards_are_exact_and_renewal_replay_is_bounded_scenario(
    pool: PgPool,
    _: PgConnectOptions,
) {
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    adapter.enqueue(enqueue_request()).await.unwrap();
    let execution = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::claim_next_execution(
        &adapter,
        automation_runtime_controller::RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse("guard-exact-controller").unwrap(),
            lease_for: Duration::from_secs(90),
        },
    )
    .await
    .unwrap()
    .unwrap();
    let mut renewal_session =
        RuntimeConvergenceSessionV1::from_claim(execution.clone()).unwrap();
    let renewal = renewal_session
        .begin_renewal(Duration::from_secs(90))
        .unwrap();
    let baseline = execution_projection(&pool).await;

    let mut wrong_attempt = renewal.clone();
    wrong_attempt.guard.convergence_attempt = NonZeroU32::new(
        renewal
            .guard
            .convergence_attempt
            .get()
            .checked_add(1)
            .unwrap(),
    )
    .unwrap();
    let error = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::renew_execution(
        &adapter,
        wrong_attempt,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        RuntimeConvergenceStoreError::ConvergenceAttemptConflict
    ));
    assert_eq!(execution_projection(&pool).await, baseline);

    let mut wrong_fence = renewal.clone();
    wrong_fence.guard.fencing_token = FencingToken::new(
        renewal
            .guard
            .fencing_token
            .get()
            .checked_add(1)
            .unwrap(),
    )
    .unwrap();
    let error = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::renew_execution(
        &adapter,
        wrong_fence,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        RuntimeConvergenceStoreError::Domain(
            automation_runtime_convergence::RuntimeDeploymentError::FencingTokenConflict { .. }
        )
    ));
    assert_eq!(execution_projection(&pool).await, baseline);

    let mut wrong_generation = renewal.clone();
    wrong_generation.guard.runtime_generation = RuntimeGeneration::new(2).unwrap();
    let error = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::renew_execution(
        &adapter,
        wrong_generation,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        RuntimeConvergenceStoreError::Domain(
            automation_runtime_convergence::RuntimeDeploymentError::RuntimeGenerationConflict {
                ..
            }
        )
    ));
    assert_eq!(execution_projection(&pool).await, baseline);

    let first_adapter = adapter.clone();
    let first_request = renewal.clone();
    let first = tokio::spawn(async move {
        <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::renew_execution(
            &first_adapter,
            first_request,
        )
        .await
    });
    let second_adapter = adapter.clone();
    let second_request = renewal.clone();
    let second = tokio::spawn(async move {
        <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::renew_execution(
            &second_adapter,
            second_request,
        )
        .await
    });
    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    assert_eq!(first, second);
    assert_eq!(first.execution.convergence_attempt, execution.convergence_attempt);
    assert_eq!(
        first.execution.snapshot.revision,
        execution.snapshot.revision.next().unwrap()
    );
    assert_eq!(
        first.execution.fencing_token,
        execution.fencing_token.next().unwrap()
    );
    let renewed_projection = execution_projection(&pool).await;
    let mut altered_replay = renewal;
    altered_replay.lease_for = Duration::from_secs(91);
    let error = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::renew_execution(
        &adapter,
        altered_replay,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        RuntimeConvergenceStoreError::ExecutionClaimStale
    ));
    assert_eq!(execution_projection(&pool).await, renewed_projection);

    let mut mutation_session =
        RuntimeConvergenceSessionV1::from_claim(first.execution.clone()).unwrap();
    let mutation = mutation_session
        .begin_mutation(
            automation_runtime_controller::RuntimeConvergenceMutationV1::AcceptPreflight(
                PreflightAttestationV1 {
                    target: target(),
                    runtime_generation: RuntimeGeneration::FIRST,
                    observed_runtime: None,
                    checked_at: first.execution.acquired_at,
                },
            ),
        )
        .unwrap();
    let mutation_baseline = execution_projection(&pool).await;
    let mut stale_attempt = mutation.clone();
    stale_attempt.guard.convergence_attempt = NonZeroU32::new(
        mutation
            .guard
            .convergence_attempt
            .get()
            .checked_add(1)
            .unwrap(),
    )
    .unwrap();
    let error = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::mutate(
        &adapter,
        stale_attempt,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        RuntimeConvergenceStoreError::ConvergenceAttemptConflict
    ));
    assert_eq!(execution_projection(&pool).await, mutation_baseline);

    let mut stale_fence = mutation.clone();
    stale_fence.guard.fencing_token = execution.fencing_token;
    let error = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::mutate(
        &adapter,
        stale_fence,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        RuntimeConvergenceStoreError::Domain(
            automation_runtime_convergence::RuntimeDeploymentError::FencingTokenConflict { .. }
        )
    ));
    assert_eq!(execution_projection(&pool).await, mutation_baseline);

    let mut stale_generation = mutation.clone();
    stale_generation.guard.runtime_generation = RuntimeGeneration::new(2).unwrap();
    let error = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::mutate(
        &adapter,
        stale_generation,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        RuntimeConvergenceStoreError::Domain(
            automation_runtime_convergence::RuntimeDeploymentError::RuntimeGenerationConflict {
                ..
            }
        )
    ));
    assert_eq!(execution_projection(&pool).await, mutation_baseline);

    let receipt = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::mutate(
        &adapter,
        mutation.clone(),
    )
    .await
    .unwrap();
    assert_eq!(receipt.convergence_attempt, execution.convergence_attempt);
    let replay = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::mutate(
        &adapter,
        mutation,
    )
    .await
    .unwrap();
    assert_eq!(replay.convergence_attempt, execution.convergence_attempt);
    assert!(matches!(
        replay.outcome,
        automation_runtime_convergence::TransitionOutcomeV1::Replayed { .. }
    ));
    let terminal_request = SubmitDeploymentMutationV1 {
        scope: scope(),
        expected_revision: receipt.snapshot.revision,
        controller_id: first.execution.controller_id.clone(),
        fencing_token: first.execution.fencing_token,
        convergence_attempt: first.execution.convergence_attempt,
        runtime_generation: first.execution.snapshot.runtime_generation,
        mutation: DeploymentMutationV1::Cancel {
            reason: "guard exact cancellation".to_string(),
        },
    };
    let terminal = adapter.mutate(terminal_request.clone()).await.unwrap();
    let terminal_replay = adapter.mutate(terminal_request.clone()).await.unwrap();
    assert!(matches!(
        terminal_replay.outcome,
        automation_runtime_convergence::TransitionOutcomeV1::Replayed { .. }
    ));
    assert_eq!(terminal_replay.snapshot, terminal.snapshot);
    let terminal_projection = execution_projection(&pool).await;
    let mut altered_terminal_controller = terminal_request;
    altered_terminal_controller.controller_id =
        ControllerId::parse("guard-exact-controller-altered").unwrap();
    let error = adapter
        .mutate(altered_terminal_controller)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        RuntimeConvergenceStoreError::Domain(
            automation_runtime_convergence::RuntimeDeploymentError::LeaseRequired
        )
    ));
    assert_eq!(execution_projection(&pool).await, terminal_projection);
    let mut constraint_probe = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER")
    .execute(&mut *constraint_probe)
    .await
    .unwrap();
    let error = sqlx::query(
        "UPDATE public.runtime_deployments SET last_controller_id = NULL \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .execute(&mut *constraint_probe)
    .await
    .unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some("23514")
    );
    constraint_probe.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn renewal_and_mutation_from_one_guard_cannot_both_commit() {
    run_migrated_runtime_database_test(
        "guard_race",
        renewal_and_mutation_from_one_guard_cannot_both_commit_scenario,
    )
    .await;
}

async fn renewal_and_mutation_from_one_guard_cannot_both_commit_scenario(
    pool: PgPool,
    _: PgConnectOptions,
) {
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    adapter.enqueue(enqueue_request()).await.unwrap();
    let execution = <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::claim_next_execution(
        &adapter,
        automation_runtime_controller::RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse("guard-race-controller").unwrap(),
            lease_for: Duration::from_secs(90),
        },
    )
    .await
    .unwrap()
    .unwrap();
    let mut renewal_session =
        RuntimeConvergenceSessionV1::from_claim(execution.clone()).unwrap();
    let renewal = renewal_session
        .begin_renewal(Duration::from_secs(90))
        .unwrap();
    let mut mutation_session =
        RuntimeConvergenceSessionV1::from_claim(execution.clone()).unwrap();
    let mutation = mutation_session
        .begin_mutation(
            automation_runtime_controller::RuntimeConvergenceMutationV1::AcceptPreflight(
                PreflightAttestationV1 {
                    target: target(),
                    runtime_generation: RuntimeGeneration::FIRST,
                    observed_runtime: None,
                    checked_at: execution.acquired_at,
                },
            ),
        )
        .unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT deployment_id FROM public.runtime_deployments \
         WHERE deployment_id = $1 FOR UPDATE",
    )
    .bind(DEPLOYMENT)
    .execute(&mut *blocker)
    .await
    .unwrap();
    let renewal_adapter = adapter.clone();
    let renewal_task = tokio::spawn(async move {
        <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::renew_execution(
            &renewal_adapter,
            renewal,
        )
        .await
        .map(|receipt| receipt.execution.snapshot.phase)
    });
    let mutation_adapter = adapter.clone();
    let mutation_task = tokio::spawn(async move {
        <PostgresRuntimeConvergence as automation_runtime_controller::RuntimeExecutionConvergencePort>::mutate(
            &mutation_adapter,
            mutation,
        )
        .await
        .map(|receipt| receipt.snapshot.phase)
    });
    let blocked = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let waiters = sqlx::query_scalar::<_, i64>(
                "SELECT pg_catalog.count(*) FROM pg_catalog.pg_stat_activity \
                 WHERE datname = pg_catalog.current_database() \
                   AND pid <> pg_catalog.pg_backend_pid() \
                   AND wait_event_type = 'Lock' \
                   AND query LIKE '%public.runtime_deployments%FOR UPDATE%'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiters == 2 {
                break waiters;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(blocked, 2);
    blocker.commit().await.unwrap();
    let renewal = renewal_task.await.unwrap();
    let mutation = mutation_task.await.unwrap();
    assert_eq!(usize::from(renewal.is_ok()) + usize::from(mutation.is_ok()), 1);
    let failure = renewal.err().or_else(|| mutation.err()).unwrap();
    assert!(matches!(
        &failure,
        RuntimeConvergenceStoreError::ExecutionClaimStale
            | RuntimeConvergenceStoreError::RevisionConflict
            | RuntimeConvergenceStoreError::Domain(
                automation_runtime_convergence::RuntimeDeploymentError::RevisionConflict { .. }
                    | automation_runtime_convergence::RuntimeDeploymentError::FencingTokenConflict {
                        ..
                    }
            )
    ), "unexpected race failure: {failure:?}");
    let projection = execution_projection(&pool).await;
    assert_eq!(
        u64::try_from(projection.0).unwrap(),
        execution.snapshot.revision.next().unwrap().get()
    );
    assert_eq!(projection.2, i64::from(execution.convergence_attempt.get()));
}

async fn execution_projection(pool: &PgPool) -> (i64, Option<i64>, i64, String) {
    sqlx::query_as(
        "SELECT revision, controller_fencing_token, convergence_attempt_no, phase \
         FROM public.runtime_deployments WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}
