#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn previous_serving_observation_is_fenced_private_and_closed() {
    run_migrated_runtime_database_test(
        "previous_serving",
        previous_serving_observation_scenario,
    )
    .await;
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn previous_serving_observation_proves_exact_absence() {
    run_migrated_runtime_database_test(
        "previous_absent",
        previous_serving_absence_scenario,
    )
    .await;
}

async fn previous_serving_absence_scenario(pool: PgPool, _: PgConnectOptions) {
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool);
    let initial = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
        outcome => panic!("seeded deployment must replay: {outcome:?}"),
    };
    let controller = ControllerId::parse("runtime-observation-absent").unwrap();
    let claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: controller.clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let revision = mutate(
        &adapter,
        claim.snapshot.revision,
        &controller,
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
        &controller,
        claim.fencing_token,
        claim.convergence_attempt,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    let snapshot = adapter.status(&scope()).await.unwrap().snapshot;
    assert_eq!(snapshot.revision, revision);
    let mut session = RuntimeConvergenceSessionV1::from_claim(RuntimeExecutionReceiptV1 {
        snapshot,
        controller_id: controller,
        fencing_token: claim.fencing_token,
        convergence_attempt: claim.convergence_attempt,
        acquired_at: claim.acquired_at,
        expires_at: claim.expires_at,
    })
    .unwrap();
    let request = session.begin_previous_serving_observation().unwrap();
    let receipt = RuntimePreviousServingObservationPort::observe_previous_serving(
        &adapter,
        request,
    )
    .await
    .unwrap();
    assert!(matches!(receipt.state, RuntimePreviousServingStateV1::Absent));
    session
        .apply_previous_serving_observation(receipt)
        .unwrap();
}

async fn previous_serving_observation_scenario(
    pool: PgPool,
    connect_options: PgConnectOptions,
) {
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::with_config(
        pool.clone(),
        PostgresRuntimeConvergenceConfigV1 {
            statement_timeout: Duration::from_millis(200),
            lock_timeout: Duration::from_millis(100),
            ..PostgresRuntimeConvergenceConfigV1::default()
        },
    )
    .unwrap();
    let first = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
        outcome => panic!("seeded first deployment must replay: {outcome:?}"),
    };
    let first_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: first.revision,
            controller_id: ControllerId::parse("runtime-observation-first").unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let (_, first_serving) = converge_claimed_with_lease(
        &adapter,
        first_claim,
        ProcessInstanceId::parse("runtime-observation-process").unwrap(),
        Duration::from_secs(5),
    )
    .await;
    let previous_runtime = RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::FIRST,
        process_instance_id: first_serving.identity.process_instance_id.clone(),
    };
    let next_request = EnqueueDeploymentV1 {
        identity: RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse(NEXT_DEPLOYMENT).unwrap(),
            tenant_id: TenantId::parse(TENANT).unwrap(),
            installation_id: InstallationId::parse(INSTALLATION).unwrap(),
            promotion_id: PromotionId::parse(NEXT_PROMOTION).unwrap(),
            activation_request_id: ActivationRequestId::parse(NEXT_ACTIVATION).unwrap(),
        },
        target: target(),
        runtime_generation: RuntimeGeneration::new(2).unwrap(),
        previous_runtime: Some(previous_runtime.clone()),
        installation_authority_revision: 1,
    };
    seed_next_product_journal(&pool, &next_request).await;
    let next = match adapter.enqueue(next_request).await.unwrap() {
        EnqueueDeploymentOutcomeV1::Created(snapshot)
        | EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
    };
    let next_controller = ControllerId::parse("runtime-observation-next").unwrap();
    let next_claim = adapter
        .claim(ClaimDeploymentV1 {
            scope: next_scope(),
            expected_revision: next.revision,
            controller_id: next_controller.clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let revision = mutate_scoped(
        &adapter,
        next_scope(),
        RuntimeGeneration::new(2).unwrap(),
        next_claim.snapshot.revision,
        &next_controller,
        next_claim.fencing_token,
        next_claim.convergence_attempt,
        DeploymentMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target(),
            runtime_generation: RuntimeGeneration::new(2).unwrap(),
            observed_runtime: Some(previous_runtime.clone()),
            checked_at: next_claim.acquired_at,
        }),
    )
    .await;
    let revision = mutate_scoped(
        &adapter,
        next_scope(),
        RuntimeGeneration::new(2).unwrap(),
        revision,
        &next_controller,
        next_claim.fencing_token,
        next_claim.convergence_attempt,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    let snapshot = adapter.status(&next_scope()).await.unwrap().snapshot;
    assert_eq!(snapshot.revision, revision);
    let mut session = RuntimeConvergenceSessionV1::from_claim(RuntimeExecutionReceiptV1 {
        snapshot,
        controller_id: next_controller,
        fencing_token: next_claim.fencing_token,
        convergence_attempt: next_claim.convergence_attempt,
        acquired_at: next_claim.acquired_at,
        expires_at: next_claim.expires_at,
    })
    .unwrap();
    let request = session.begin_previous_serving_observation().unwrap();
    let serving = RuntimePreviousServingObservationPort::observe_previous_serving(
        &adapter,
        request.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        serving.state,
        RuntimePreviousServingStateV1::Serving { .. }
    ));

    let mut tampered = request.clone();
    tampered.expected_previous_runtime = Some(RuntimeProcessIdentityV1 {
        process_instance_id: ProcessInstanceId::parse("runtime-observation-impostor").unwrap(),
        ..previous_runtime.clone()
    });
    assert!(matches!(
        RuntimePreviousServingObservationPort::observe_previous_serving(&adapter, tampered)
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::ExecutionClaimStale
    ));

    assert_observation_capability_privileges(&pool, &connect_options, &request).await;

    let wait = (first_serving.expires_at - Utc::now())
        .to_std()
        .unwrap_or_default()
        .saturating_add(Duration::from_millis(50));
    tokio::time::sleep(wait).await;
    let expired = RuntimePreviousServingObservationPort::observe_previous_serving(
        &adapter,
        request.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        expired.state,
        RuntimePreviousServingStateV1::Expired { .. }
    ));

    let disconnected = adapter
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: first_serving.identity,
        })
        .await
        .unwrap();
    assert!(!disconnected.connected);
    let disconnected = RuntimePreviousServingObservationPort::observe_previous_serving(
        &adapter,
        request,
    )
    .await
    .unwrap();
    assert!(matches!(
        disconnected.state,
        RuntimePreviousServingStateV1::Disconnected { .. }
    ));
}

async fn assert_observation_capability_privileges(
    pool: &PgPool,
    connect_options: &PgConnectOptions,
    request: &automation_runtime_controller::RuntimeObservePreviousServingV1,
) {
    let database_name = connect_options.get_database().unwrap();
    let suffix = &database_name[database_name.len().saturating_sub(18)..];
    let role = format!("srt_observer_{suffix}");
    assert!(
        role.len() <= 63
            && role
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    sqlx::query(&format!("CREATE ROLE {role} NOLOGIN"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {role}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION public.starring_runtime_observe_previous_serving_v1(\
         text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb) \
         TO {role}"
    ))
    .execute(pool)
    .await
    .unwrap();
    for relation in ["runtime_deployments", "runtime_serving_leases"] {
        let can_select = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_table_privilege($1, $2, 'SELECT')",
        )
        .bind(&role)
        .bind(format!("public.{relation}"))
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(!can_select);
    }
    let public_execute = sqlx::query_scalar::<_, bool>(
        "SELECT pg_catalog.has_function_privilege(\
         'public', \
         'public.starring_runtime_observe_previous_serving_v1(\
          text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)', \
         'EXECUTE')",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(!public_execute);

    let previous = request
        .expected_previous_runtime
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .unwrap()
        .map(Json);
    let mut connection = PgConnection::connect_with(connect_options).await.unwrap();
    let mut transaction = connection.begin().await.unwrap();
    sqlx::query(&format!("SET LOCAL ROLE {role}"))
        .execute(&mut *transaction)
        .await
        .unwrap();
    let state = sqlx::query_scalar::<_, String>(
        "SELECT state_name FROM public.starring_runtime_observe_previous_serving_v1(\
         $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(request.guard.scope.tenant_id.as_str())
    .bind(request.guard.scope.installation_id.as_str())
    .bind(request.guard.scope.deployment_id.as_str())
    .bind(i64::try_from(request.guard.expected_revision.get()).unwrap())
    .bind(request.guard.controller_id.as_str())
    .bind(i64::try_from(request.guard.fencing_token.get()).unwrap())
    .bind(i64::from(request.guard.convergence_attempt.get()))
    .bind(i64::try_from(request.guard.runtime_generation.get()).unwrap())
    .bind(request.expected_target.guild_id.to_string())
    .bind(request.expected_target.ruleset_key.as_str())
    .bind(i64::from(request.expected_target.version.get()))
    .bind(request.expected_target.content_hash.to_hex())
    .bind(i64::try_from(request.expected_target.binding_revision.get()).unwrap())
    .bind(request.expected_target.binding_fingerprint.as_str())
    .bind(previous)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(state, "serving");
    transaction.commit().await.unwrap();
    drop(connection);
    sqlx::query(&format!("DROP OWNED BY {role}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {role}"))
        .execute(pool)
        .await
        .unwrap();
}
