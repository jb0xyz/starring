#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_mutations_are_proven_and_closed() {
    let server = PostgresTestServer::start();

    let canonicality_database = isolated_database(server.connect_options()).await;
    mutation_canonicality_and_expiry_scenario(&canonicality_database).await;
    cleanup(canonicality_database).await;

    let future_evidence_database = isolated_database(server.connect_options()).await;
    future_activation_failure_scenario(&future_evidence_database).await;
    cleanup(future_evidence_database).await;

    let recovery_database = isolated_database(server.connect_options()).await;
    retry_recovery_and_blocked_failure_scenario(&recovery_database).await;
    cleanup(recovery_database).await;

    let authority_database = isolated_database(server.connect_options()).await;
    replay_rechecks_current_authority_scenario(&authority_database).await;
    cleanup(authority_database).await;

    let claim_expiry_database = isolated_database(server.connect_options()).await;
    claim_revalidates_expiry_after_row_lock_scenario(&claim_expiry_database).await;
    cleanup(claim_expiry_database).await;

    drop(server);
}

#[tokio::test]
#[ignore = "requires PostgreSQL test authority"]
async fn execution_mutation_matrix_is_exact_and_marker_guarded() {
    let server = PostgresTestServer::start();
    let mut covered = BTreeSet::new();

    let happy_database = isolated_database(server.connect_options()).await;
    mutation_matrix_happy_flow(&happy_database, &mut covered).await;
    assert_marker_database_invariants(&happy_database).await;
    cleanup(happy_database).await;

    let retry_database = isolated_database(server.connect_options()).await;
    mutation_matrix_retry_flow(&retry_database, &mut covered).await;
    cleanup(retry_database).await;

    let cancel_database = isolated_database(server.connect_options()).await;
    mutation_matrix_cancel_flow(&cancel_database, &mut covered).await;
    cleanup(cancel_database).await;

    assert_eq!(
        covered,
        BTreeSet::from([
            "accept_activation",
            "accept_drain",
            "accept_panel_certificate",
            "accept_preflight",
            "begin_activation",
            "begin_panel_reconciliation",
            "cancel",
            "record_blocked_failure",
            "record_retryable_failure",
            "request_drain",
            "resume_runtime_pending",
            "supersede",
        ])
    );
    drop(server);
}

async fn mutation_canonicality_and_expiry_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-execution-canonicality-controller",
        Duration::from_secs(3),
    )
    .await;
    let checked_at = database_now(&database.owner_pool).await;
    let attestation = PreflightAttestationV1 {
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        observed_runtime: session.snapshot().previous_runtime.clone(),
        checked_at,
    };
    let guard = session.execution_guard().unwrap();
    let unchanged = persisted_deployment_image(&database.owner_pool).await;

    let mut noncanonical_version = serde_json::to_value(&attestation).unwrap();
    noncanonical_version["target"]["version"] = json!(1.0);
    let error = raw_mutate(
        &database.executor_pool,
        &guard,
        "accept_preflight",
        noncanonical_version,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );

    let mut noncanonical_time = serde_json::to_value(&attestation).unwrap();
    noncanonical_time["checked_at"] = json!("2026-07-22T24:00:00Z");
    let error = raw_mutate(
        &database.executor_pool,
        &guard,
        "accept_preflight",
        noncanonical_time,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );

    let mut noncanonical_fraction = serde_json::to_value(&attestation).unwrap();
    noncanonical_fraction["checked_at"] = json!((checked_at + TimeDelta::seconds(1))
        .format("%Y-%m-%dT%H:%M:%S.000Z")
        .to_string());
    let error = raw_mutate(
        &database.executor_pool,
        &guard,
        "accept_preflight",
        noncanonical_fraction,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX002");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );

    let request = session
        .begin_mutation(RuntimeConvergenceMutationV1::AcceptPreflight(
            attestation.clone(),
        ))
        .unwrap();
    let applied = adapter.mutate(request.clone()).await.unwrap();
    assert!(matches!(
        applied.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    let replayed = adapter.mutate(request.clone()).await.unwrap();
    assert!(matches!(
        replayed.outcome,
        TransitionOutcomeV1::Replayed { .. }
    ));
    assert_eq!(replayed.action_id, applied.action_id);
    assert_eq!(replayed.snapshot, applied.snapshot);
    assert_eq!(replayed.convergence_attempt, applied.convergence_attempt);
    session.apply_mutation(applied).unwrap();

    let database_time = database_now(&database.owner_pool).await;
    let remaining = (session.expires_at() - database_time)
        .to_std()
        .unwrap_or_default();
    tokio::time::sleep(remaining + Duration::from_millis(50)).await;
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    let error = adapter.mutate(request.clone()).await.unwrap_err();
    assert_eq!(error, RuntimeExecutionPersistenceErrorV1::OwnershipLost);
    let error = raw_mutate(
        &database.executor_pool,
        &request.guard,
        "accept_preflight",
        serde_json::to_value(attestation).unwrap(),
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX001");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
}

async fn future_activation_failure_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-execution-future-evidence-controller",
        Duration::from_secs(60),
    )
    .await;
    advance_to_activation_applying(&database.owner_pool, &adapter, &mut session).await;
    let guard = session.execution_guard().unwrap();
    let activated_at = database_now(&database.owner_pool).await + TimeDelta::seconds(20);
    let activation = ActivationAttestationV1 {
        activation_request_id: session.snapshot().identity.activation_request_id.clone(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        kind: ActivationOutcomeKindV1::Activated,
        activated_at,
    };
    let outcome = raw_mutate(
        &database.executor_pool,
        &guard,
        "accept_activation",
        serde_json::to_value(activation).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(outcome, ["applied"]);
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    assert_eq!(
        unchanged.0["snapshot"]["phase"]["condition"]["condition"],
        "ready"
    );
    let mut failure_guard = guard;
    failure_guard.expected_revision = failure_guard.expected_revision.next().unwrap();
    let failure = json!({
        "failure_id": "future-activation-failure",
        "kind": "gateway_start",
        "code": "gateway_start_failed",
        "attempt": failure_guard.convergence_attempt.get(),
        "retry_after_milliseconds": 1000
    });
    let error = raw_mutate(
        &database.executor_pool,
        &failure_guard,
        "record_retryable_failure",
        failure,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX005");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
}

async fn retry_recovery_and_blocked_failure_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-execution-retry-controller",
        Duration::from_secs(60),
    )
    .await;
    advance_to_activation_applying(&database.owner_pool, &adapter, &mut session).await;
    let activation = ActivationAttestationV1 {
        activation_request_id: session.snapshot().identity.activation_request_id.clone(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        kind: ActivationOutcomeKindV1::Activated,
        activated_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptActivation(activation),
    )
    .await;
    let retry_attempt = session.convergence_attempt();
    let retry_request = session
        .begin_mutation(RuntimeConvergenceMutationV1::RecordRetryableFailure {
            failure_id: RuntimeFailureId::parse("runtime-retryable-failure").unwrap(),
            kind: RuntimeFailureKindV1::GatewayStart,
            code: "gateway_start_failed".to_string(),
            attempt: retry_attempt,
            retry_after: Duration::from_millis(1),
        })
        .unwrap();
    let retry_receipt = adapter.mutate(retry_request.clone()).await.unwrap();
    assert!(matches!(
        retry_receipt.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    session.apply_mutation(retry_receipt).unwrap();
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    let error = raw_mutate(
        &database.executor_pool,
        &retry_request.guard,
        "record_retryable_failure",
        json!({
            "failure_id": "runtime-retryable-failure",
            "kind": "gateway_start",
            "code": "gateway_start_failed",
            "attempt": retry_attempt.get().to_string(),
            "retry_after_milliseconds": "1"
        }),
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
    assert_eq!(session.state(), RuntimeConvergenceSessionStateV1::Released);
    let retry_not_before = match &session.snapshot().phase {
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition:
                RuntimePendingConditionV1::Retryable {
                    retry_not_before, ..
                },
        } => *retry_not_before,
        _ => panic!("retryable failure must persist a retry boundary"),
    };
    wait_for_database_time(&database.owner_pool, retry_not_before).await;

    let mut resumed = claimed_session(
        &adapter,
        "runtime-execution-resume-controller",
        Duration::from_secs(60),
    )
    .await;
    assert_eq!(resumed.convergence_attempt().get(), retry_attempt.get() + 1);
    let activation_payload = serde_json::to_value(
        resumed
            .snapshot()
            .activation
            .as_ref()
            .expect("retry recovery must retain activation evidence"),
    )
    .unwrap();
    let resume_request = resumed
        .begin_mutation(RuntimeConvergenceMutationV1::ResumeRuntimePending)
        .unwrap();
    let resume_receipt = adapter.mutate(resume_request.clone()).await.unwrap();
    assert!(matches!(
        resume_receipt.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    resumed.apply_mutation(resume_receipt).unwrap();
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    let error = raw_mutate(
        &database.executor_pool,
        &resume_request.guard,
        "accept_activation",
        activation_payload,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
    mutate_applied(
        &adapter,
        &mut resumed,
        RuntimeConvergenceMutationV1::BeginPanelReconciliation,
    )
    .await;
    let certificate = PanelCertificateV1 {
        certificate_id: PanelCertificateId::parse("runtime-panel-certificate").unwrap(),
        report_digest: PanelReportDigestV1::parse("5".repeat(64)).unwrap(),
        target: resumed.snapshot().target.clone(),
        runtime_generation: resumed.snapshot().runtime_generation,
        process_instance_id: ProcessInstanceId::parse("runtime-process-instance").unwrap(),
        declared_count: 1,
        installed_count: 1,
        unchanged_count: 0,
        skipped_transient_count: 0,
        skipped_unresolved_channel_count: 0,
        failed_count: 0,
        ambiguous_outcome_count: 0,
        stale_message_cleanup_pending_count: 0,
        orphan_message_cleanup_pending_count: 0,
        reposted_old_message_cleanup_pending_count: 0,
        reconciled_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        &adapter,
        &mut resumed,
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate),
    )
    .await;
    mutate_applied(
        &adapter,
        &mut resumed,
        RuntimeConvergenceMutationV1::RecordBlockedFailure {
            failure_id: RuntimeFailureId::parse("runtime-blocked-failure").unwrap(),
            kind: RuntimeFailureKindV1::InvariantViolation,
            code: "invalid_runtime_state".to_string(),
        },
    )
    .await;
    assert_eq!(resumed.state(), RuntimeConvergenceSessionStateV1::Released);
    assert!(resumed.snapshot().controller_lease.is_none());
    assert!(matches!(
        resumed.snapshot().phase,
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { .. }
        }
    ));
    let persisted = persisted_deployment_image(&database.owner_pool).await;
    assert_eq!(
        persisted.0["snapshot"]["phase"]["condition"]["condition"],
        "blocked"
    );
    assert!(persisted.0["controller_id"].is_null());
}

async fn replay_rechecks_current_authority_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let controller_id = ControllerId::parse("runtime-execution-authority-controller").unwrap();
    let claim_request = RuntimeClaimNextExecutionV1 {
        controller_id: controller_id.clone(),
        lease_for: Duration::from_secs(60),
    };
    let claimed = adapter
        .claim_next_execution(claim_request)
        .await
        .unwrap()
        .expect("seeded execution must be claimable");
    let mut session = RuntimeConvergenceSessionV1::from_claim(claimed).unwrap();
    let renewal_request = session.begin_renewal(Duration::from_secs(90)).unwrap();
    let renewed = adapter
        .renew_execution(renewal_request.clone())
        .await
        .unwrap();
    session.apply_renewal(renewed.clone()).unwrap();
    let replayed_claim = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id,
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap()
        .expect("active execution must replay before authority drift");
    assert_eq!(replayed_claim, renewed.execution);
    assert_eq!(
        adapter
            .renew_execution(renewal_request.clone())
            .await
            .unwrap(),
        renewed
    );
    rotate_current_authority(&database.owner_pool).await;
    let unchanged = persisted_deployment_image(&database.owner_pool).await;
    let claim_error = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id: session.controller_id().clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap_err();
    assert_eq!(
        claim_error,
        RuntimeExecutionPersistenceErrorV1::AuthorityChanged
    );
    let renew_error = adapter.renew_execution(renewal_request).await.unwrap_err();
    assert_eq!(
        renew_error,
        RuntimeExecutionPersistenceErrorV1::AuthorityChanged
    );
    assert_eq!(
        persisted_deployment_image(&database.owner_pool).await,
        unchanged
    );
}

async fn claim_revalidates_expiry_after_row_lock_scenario(database: &IsolatedDatabase) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let controller_id = ControllerId::parse("runtime-execution-lock-wait-controller").unwrap();
    let initial = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id: controller_id.clone(),
            lease_for: Duration::from_secs(1),
        })
        .await
        .unwrap()
        .expect("seeded execution must be claimable");

    let mut blocker = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "SELECT deployment_id FROM public.runtime_deployments \
         WHERE deployment_id = $1 FOR UPDATE",
    )
    .bind(DEPLOYMENT)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();

    let executor_pool = database.executor_pool.clone();
    let controller = controller_id.as_str().to_string();
    let claim = tokio::spawn(async move {
        let mut transaction = executor_pool.begin().await.unwrap();
        sqlx::query(
            "SELECT pg_catalog.set_config('statement_timeout', '5000ms', TRUE), \
                pg_catalog.set_config('lock_timeout', '4000ms', TRUE)",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        let result = sqlx::query_as::<_, (String, Json<Value>, DateTime<Utc>)>(
            "SELECT outcome_name, snapshot, expires_at \
             FROM public.starring_runtime_execution_claim_next_v1($1, 30000)",
        )
        .bind(controller)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        result
    });

    wait_for_blocked_claim(&database.owner_pool).await;
    wait_for_database_time(&database.owner_pool, initial.expires_at).await;
    blocker.commit().await.unwrap();

    let (outcome, snapshot, expires_at) = claim.await.unwrap();
    assert_eq!(outcome, "applied");
    assert_eq!(snapshot.0["revision"], 3);
    assert_eq!(snapshot.0["controller_lease"]["fencing_token"], 2);
    assert!(expires_at > database_now(&database.owner_pool).await);
}

async fn wait_for_blocked_claim(pool: &PgPool) {
    for _ in 0..200 {
        let blocked = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.count(*) = 1 \
             FROM pg_catalog.pg_stat_activity AS activity \
             WHERE activity.datname = current_database() \
                AND activity.pid <> pg_catalog.pg_backend_pid() \
                AND activity.state = 'active' \
                AND activity.wait_event_type = 'Lock' \
                AND activity.query LIKE \
                    '%starring_runtime_execution_claim_next_v1($1, 30000)%'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if blocked {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("execution claim did not reach the deployment row lock");
}

async fn mutation_matrix_happy_flow(
    database: &IsolatedDatabase,
    covered: &mut BTreeSet<&'static str>,
) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-execution-matrix-happy-controller",
        Duration::from_secs(60),
    )
    .await;
    let preflight = PreflightAttestationV1 {
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        observed_runtime: session.snapshot().previous_runtime.clone(),
        checked_at: database_now(&database.owner_pool).await,
    };
    prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptPreflight(preflight),
        covered,
    )
    .await;
    prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::RequestDrain,
        covered,
    )
    .await;
    let drain = DrainAttestationV1 {
        previous_runtime: session.snapshot().previous_runtime.clone(),
        target_runtime_generation: session.snapshot().runtime_generation,
        drained_at: database_now(&database.owner_pool).await,
    };
    prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptDrain(drain),
        covered,
    )
    .await;
    prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::BeginActivation,
        covered,
    )
    .await;
    let activation = ActivationAttestationV1 {
        activation_request_id: session.snapshot().identity.activation_request_id.clone(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        kind: ActivationOutcomeKindV1::Activated,
        activated_at: database_now(&database.owner_pool).await,
    };
    prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptActivation(activation),
        covered,
    )
    .await;
    prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::BeginPanelReconciliation,
        covered,
    )
    .await;
    let certificate = PanelCertificateV1 {
        certificate_id: PanelCertificateId::parse("runtime-matrix-panel-certificate").unwrap(),
        report_digest: PanelReportDigestV1::parse("8".repeat(64)).unwrap(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        process_instance_id: ProcessInstanceId::parse("runtime-matrix-process").unwrap(),
        declared_count: 1,
        installed_count: 1,
        unchanged_count: 0,
        skipped_transient_count: 0,
        skipped_unresolved_channel_count: 0,
        failed_count: 0,
        ambiguous_outcome_count: 0,
        stale_message_cleanup_pending_count: 0,
        orphan_message_cleanup_pending_count: 0,
        reposted_old_message_cleanup_pending_count: 0,
        reconciled_at: database_now(&database.owner_pool).await,
    };
    prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(certificate),
        covered,
    )
    .await;
    prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::RecordBlockedFailure {
            failure_id: RuntimeFailureId::parse("runtime-matrix-blocked-failure").unwrap(),
            kind: RuntimeFailureKindV1::InvariantViolation,
            code: "invalid_runtime_state".to_string(),
        },
        covered,
    )
    .await;
    assert_eq!(session.state(), RuntimeConvergenceSessionStateV1::Released);
}

async fn mutation_matrix_retry_flow(
    database: &IsolatedDatabase,
    covered: &mut BTreeSet<&'static str>,
) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-execution-matrix-retry-controller",
        Duration::from_secs(60),
    )
    .await;
    advance_to_activation_applying(&database.owner_pool, &adapter, &mut session).await;
    let activation = ActivationAttestationV1 {
        activation_request_id: session.snapshot().identity.activation_request_id.clone(),
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        kind: ActivationOutcomeKindV1::Activated,
        activated_at: database_now(&database.owner_pool).await,
    };
    mutate_applied(
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::AcceptActivation(activation),
    )
    .await;
    let retry_attempt = session.convergence_attempt();
    let retry = prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::RecordRetryableFailure {
            failure_id: RuntimeFailureId::parse("runtime-matrix-retryable-failure").unwrap(),
            kind: RuntimeFailureKindV1::GatewayStart,
            code: "gateway_start_failed".to_string(),
            attempt: retry_attempt,
            retry_after: Duration::from_millis(1),
        },
        covered,
    )
    .await;
    let retry_not_before = match &session.snapshot().phase {
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition:
                RuntimePendingConditionV1::Retryable {
                    retry_not_before, ..
                },
        } => *retry_not_before,
        _ => panic!("matrix retry must persist its retry boundary"),
    };
    wait_for_database_time(&database.owner_pool, retry_not_before).await;
    let mut resumed = claimed_session(
        &adapter,
        "runtime-execution-matrix-resume-controller",
        Duration::from_secs(60),
    )
    .await;
    let resume = prove_mutation_case(
        database,
        &adapter,
        &mut resumed,
        RuntimeConvergenceMutationV1::ResumeRuntimePending,
        covered,
    )
    .await;
    assert_eq!(
        resume.snapshot.revision.get(),
        retry.snapshot.revision.get() + 2
    );
    let successor = SupersedingDeploymentV1 {
        identity: serde_json::from_value(json!({
            "deployment_id": "runtime-execution-matrix-successor",
            "tenant_id": TENANT,
            "installation_id": INSTALLATION,
            "promotion_id": "d".repeat(64),
            "activation_request_id": "runtime-execution-matrix-successor-activation"
        }))
        .unwrap(),
        target: resumed.snapshot().target.clone(),
        runtime_generation: RuntimeGeneration::new(2).unwrap(),
    };
    prove_mutation_case(
        database,
        &adapter,
        &mut resumed,
        RuntimeConvergenceMutationV1::Supersede {
            by: successor,
            reason: "new deployment selected".to_string(),
        },
        covered,
    )
    .await;
    assert_eq!(resumed.state(), RuntimeConvergenceSessionStateV1::Released);
}

async fn mutation_matrix_cancel_flow(
    database: &IsolatedDatabase,
    covered: &mut BTreeSet<&'static str>,
) {
    seed_claimable_deployment(&database.owner_pool).await;
    let adapter = verified_execution_adapter(database).await;
    let mut session = claimed_session(
        &adapter,
        "runtime-execution-matrix-cancel-controller",
        Duration::from_secs(60),
    )
    .await;
    prove_mutation_case(
        database,
        &adapter,
        &mut session,
        RuntimeConvergenceMutationV1::Cancel {
            reason: "operator requested cancellation".to_string(),
        },
        covered,
    )
    .await;
    assert_eq!(session.state(), RuntimeConvergenceSessionStateV1::Released);
}

async fn prove_mutation_case(
    database: &IsolatedDatabase,
    adapter: &PostgresRuntimeExecutionV1,
    session: &mut RuntimeConvergenceSessionV1,
    mutation: RuntimeConvergenceMutationV1,
    covered: &mut BTreeSet<&'static str>,
) -> RuntimeMutationReceiptV1 {
    let kind = mutation_contract_name(&mutation);
    assert!(covered.insert(kind));
    let request = session.begin_mutation(mutation).unwrap();
    let applied = adapter.mutate(request.clone()).await.unwrap();
    assert!(matches!(
        applied.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    let persisted = persisted_mutation_image(&database.owner_pool).await;
    assert_eq!(
        persisted.1,
        i64::try_from(applied.snapshot.revision.get()).unwrap()
    );
    assert_eq!(persisted.2, kind);
    let marker_payload = (persisted.3).0.clone();

    let replayed = adapter.mutate(request.clone()).await.unwrap();
    assert!(matches!(
        replayed.outcome,
        TransitionOutcomeV1::Replayed { .. }
    ));
    assert_eq!(replayed.action_id, applied.action_id);
    assert_eq!(replayed.snapshot, applied.snapshot);
    assert_eq!(replayed.convergence_attempt, applied.convergence_attempt);
    assert_eq!(
        persisted_mutation_image(&database.owner_pool).await,
        persisted
    );

    let mut wrong_payload = marker_payload.clone();
    wrong_payload
        .as_object_mut()
        .unwrap()
        .insert("wrong_provenance".to_string(), json!(true));
    let error = raw_mutate(&database.executor_pool, &request.guard, kind, wrong_payload)
        .await
        .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(
        persisted_mutation_image(&database.owner_pool).await,
        persisted
    );

    let wrong_kind = if kind == "cancel" {
        "supersede"
    } else {
        "cancel"
    };
    let error = raw_mutate(
        &database.executor_pool,
        &request.guard,
        wrong_kind,
        marker_payload,
    )
    .await
    .unwrap_err();
    assert_sqlstate(&error, "RX004");
    assert_eq!(
        persisted_mutation_image(&database.owner_pool).await,
        persisted
    );

    session.apply_mutation(applied.clone()).unwrap();
    applied
}

fn mutation_contract_name(mutation: &RuntimeConvergenceMutationV1) -> &'static str {
    match mutation {
        RuntimeConvergenceMutationV1::AcceptPreflight(_) => "accept_preflight",
        RuntimeConvergenceMutationV1::RequestDrain => "request_drain",
        RuntimeConvergenceMutationV1::AcceptDrain(_) => "accept_drain",
        RuntimeConvergenceMutationV1::BeginActivation => "begin_activation",
        RuntimeConvergenceMutationV1::AcceptActivation(_) => "accept_activation",
        RuntimeConvergenceMutationV1::RecordRetryableFailure { .. } => "record_retryable_failure",
        RuntimeConvergenceMutationV1::RecordBlockedFailure { .. } => "record_blocked_failure",
        RuntimeConvergenceMutationV1::ResumeRuntimePending => "resume_runtime_pending",
        RuntimeConvergenceMutationV1::BeginPanelReconciliation => "begin_panel_reconciliation",
        RuntimeConvergenceMutationV1::AcceptPanelCertificate(_) => "accept_panel_certificate",
        RuntimeConvergenceMutationV1::Supersede { .. } => "supersede",
        RuntimeConvergenceMutationV1::Cancel { .. } => "cancel",
    }
}

async fn persisted_mutation_image(pool: &PgPool) -> (Json<Value>, i64, String, Json<Value>) {
    sqlx::query_as(
        "SELECT pg_catalog.to_jsonb(deployment), marker.mutation_revision, \
            marker.mutation_kind, marker.mutation_payload \
         FROM public.runtime_deployments AS deployment \
         INNER JOIN public.runtime_execution_mutation_markers AS marker \
            ON marker.deployment_id = deployment.deployment_id \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_marker_database_invariants(database: &IsolatedDatabase) {
    let persisted = persisted_mutation_image(&database.owner_pool).await;
    for statement in [
        "UPDATE public.runtime_execution_mutation_markers \
         SET mutation_revision = mutation_revision",
        "UPDATE public.runtime_execution_mutation_markers \
         SET mutation_revision = mutation_revision - 1",
        "UPDATE public.runtime_execution_mutation_markers \
         SET deployment_id = 'runtime-marker-other'",
        "UPDATE public.runtime_execution_mutation_markers \
         SET mutation_revision = mutation_revision + 1",
        "DELETE FROM public.runtime_execution_mutation_markers",
    ] {
        let error = sqlx::query(statement)
            .execute(&database.owner_pool)
            .await
            .unwrap_err();
        assert_sqlstate(&error, "23514");
        assert_eq!(
            persisted_mutation_image(&database.owner_pool).await,
            persisted
        );
    }

    let mut transaction = database.owner_pool.begin().await.unwrap();
    sqlx::query(
        "DROP TRIGGER runtime_execution_mutation_markers_validate_transition \
         ON public.runtime_execution_mutation_markers",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let manifests = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT public.starring_runtime_exact_target_schema_manifest_v2(), \
            public.starring_runtime_serving_schema_manifest_v1(), \
            public.starring_runtime_execution_schema_manifest_v1()",
    )
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(manifests, (true, true, false));
    transaction.rollback().await.unwrap();
    let manifests = sqlx::query_as::<_, (bool, bool, bool)>(
        "SELECT public.starring_runtime_exact_target_schema_manifest_v2(), \
            public.starring_runtime_serving_schema_manifest_v1(), \
            public.starring_runtime_execution_schema_manifest_v1()",
    )
    .fetch_one(&database.owner_pool)
    .await
    .unwrap();
    assert_eq!(manifests, (true, true, true));
}

async fn rotate_current_authority(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions (installation_id, \
         revision, tenant_id, binding_revision, resource_bindings, binding_fingerprint, \
         policy_revision, required_approvals, activation_ttl_seconds, \
         authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         SELECT installation_id, 2, tenant_id, 2, resource_bindings, \
         $2, policy_revision, required_approvals, activation_ttl_seconds, \
         $3, created_by_principal_id, $4 \
         FROM public.automation_installation_authority_versions \
         WHERE installation_id = $1 AND revision = 1",
    )
    .bind(INSTALLATION)
    .bind("b".repeat(64))
    .bind("6".repeat(64))
    .bind("7".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_installations \
         SET current_authority_revision = 2, \
             updated_at = GREATEST(pg_catalog.clock_timestamp(), \
                 updated_at + INTERVAL '1 microsecond') \
         WHERE tenant_id = $1 AND installation_id = $2 \
             AND current_authority_revision = 1",
    )
    .bind(TENANT)
    .bind(INSTALLATION)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    transaction.commit().await.unwrap();
}

async fn verified_execution_adapter(database: &IsolatedDatabase) -> PostgresRuntimeExecutionV1 {
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
    PostgresRuntimeExecutionV1::connect_verified_default(
        database.executor_pool.clone(),
        expectation,
    )
    .await
    .unwrap()
}

async fn claimed_session(
    adapter: &PostgresRuntimeExecutionV1,
    controller_id: &str,
    lease_for: Duration,
) -> RuntimeConvergenceSessionV1 {
    let receipt = adapter
        .claim_next_execution(RuntimeClaimNextExecutionV1 {
            controller_id: ControllerId::parse(controller_id).unwrap(),
            lease_for,
        })
        .await
        .unwrap()
        .expect("seeded execution must be claimable");
    RuntimeConvergenceSessionV1::from_claim(receipt).unwrap()
}

async fn advance_to_activation_applying(
    owner_pool: &PgPool,
    adapter: &PostgresRuntimeExecutionV1,
    session: &mut RuntimeConvergenceSessionV1,
) {
    let preflight = PreflightAttestationV1 {
        target: session.snapshot().target.clone(),
        runtime_generation: session.snapshot().runtime_generation,
        observed_runtime: session.snapshot().previous_runtime.clone(),
        checked_at: database_now(owner_pool).await,
    };
    mutate_applied(
        adapter,
        session,
        RuntimeConvergenceMutationV1::AcceptPreflight(preflight),
    )
    .await;
    mutate_applied(adapter, session, RuntimeConvergenceMutationV1::RequestDrain).await;
    let drain = DrainAttestationV1 {
        previous_runtime: session.snapshot().previous_runtime.clone(),
        target_runtime_generation: session.snapshot().runtime_generation,
        drained_at: database_now(owner_pool).await,
    };
    mutate_applied(
        adapter,
        session,
        RuntimeConvergenceMutationV1::AcceptDrain(drain),
    )
    .await;
    mutate_applied(
        adapter,
        session,
        RuntimeConvergenceMutationV1::BeginActivation,
    )
    .await;
}

async fn mutate_applied(
    adapter: &PostgresRuntimeExecutionV1,
    session: &mut RuntimeConvergenceSessionV1,
    mutation: RuntimeConvergenceMutationV1,
) -> RuntimeMutationReceiptV1 {
    let request = session.begin_mutation(mutation).unwrap();
    let receipt = adapter.mutate(request).await.unwrap();
    assert!(matches!(
        receipt.outcome,
        TransitionOutcomeV1::Applied { .. }
    ));
    session.apply_mutation(receipt.clone()).unwrap();
    receipt
}

async fn raw_mutate(
    executor_pool: &PgPool,
    guard: &RuntimeExecutionGuardV1,
    kind: &str,
    payload: Value,
) -> Result<Vec<String>, sqlx::Error> {
    let mut transaction = executor_pool.begin().await?;
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await?;
    let result = sqlx::query_scalar(
        "SELECT outcome_name \
         FROM public.starring_runtime_execution_mutate_v1( \
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10 \
         )",
    )
    .bind(guard.scope.tenant_id.as_str())
    .bind(guard.scope.installation_id.as_str())
    .bind(guard.scope.deployment_id.as_str())
    .bind(i64::try_from(guard.expected_revision.get()).unwrap())
    .bind(guard.controller_id.as_str())
    .bind(i64::try_from(guard.fencing_token.get()).unwrap())
    .bind(i64::from(guard.convergence_attempt.get()))
    .bind(i64::try_from(guard.runtime_generation.get()).unwrap())
    .bind(kind)
    .bind(Json(payload))
    .fetch_all(&mut *transaction)
    .await;
    match result {
        Ok(outcomes) => {
            transaction.commit().await?;
            Ok(outcomes)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn persisted_deployment_image(pool: &PgPool) -> Json<Value> {
    sqlx::query_scalar(
        "SELECT pg_catalog.to_jsonb(deployment) \
         FROM public.runtime_deployments AS deployment \
         WHERE deployment.deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn wait_for_database_time(pool: &PgPool, boundary: DateTime<Utc>) {
    let now = database_now(pool).await;
    let remaining = (boundary - now).to_std().unwrap_or_default();
    tokio::time::sleep(remaining + Duration::from_millis(10)).await;
}
