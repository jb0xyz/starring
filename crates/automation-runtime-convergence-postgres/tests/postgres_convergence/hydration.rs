#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn exact_target_hydration_requires_the_current_fenced_claim() {
    run_migrated_runtime_database_test(
        "exact_target_hydration",
        exact_target_hydration_requires_the_current_fenced_claim_scenario,
    )
    .await;
}

async fn exact_target_hydration_requires_the_current_fenced_claim_scenario(
    pool: PgPool,
    _connect_options: PgConnectOptions,
) {
    seed_product_target(&pool).await;
    let adapter = PostgresRuntimeConvergence::new(pool.clone());
    let initial = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
        outcome => panic!("seeded target must replay exactly: {outcome:?}"),
    };
    let controller = ControllerId::parse("runtime-hydration-controller").unwrap();
    let claim = adapter
        .claim_execution(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: controller,
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let reader = PostgresRuntimeExactTargetReader::new(pool.clone());
    let hydrated = reader.load_for_claim(&claim).await.unwrap();
    assert_eq!(hydrated.snapshot, claim.snapshot);
    assert_eq!(hydrated.artifact.guild_id, GUILD);
    assert_eq!(hydrated.artifact.ruleset_key.as_str(), RULESET);
    assert_eq!(hydrated.artifact.version, RuleSetVersionId::FIRST);
    assert_eq!(hydrated.artifact.content_hash.to_hex(), CONTENT_HASH);
    assert!(hydrated.bindings.role_bindings.is_empty());
    assert!(hydrated.bindings.channel_bindings.is_empty());
    assert_eq!(hydrated.installation_authority_revision, 1);
    assert_eq!(hydrated.current_authority_revision, 1);
    assert_eq!(reader.database_identity().await.unwrap().len(), 36);

    let mut stale = claim.clone();
    stale.fencing_token = FencingToken::new(claim.fencing_token.get() + 1).unwrap();
    assert!(matches!(
        reader.load_for_claim(&stale).await.unwrap_err(),
        RuntimeConvergenceStoreError::ExecutionClaimStale
    ));

    let renewed = adapter
        .claim_execution(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: claim.snapshot.revision,
            controller_id: claim.controller_id.clone(),
            lease_for: Duration::from_secs(100),
        })
        .await
        .unwrap();
    assert!(renewed.fencing_token > claim.fencing_token);
    assert_eq!(renewed.convergence_attempt, claim.convergence_attempt);
    assert!(matches!(
        reader.load_for_claim(&claim).await.unwrap_err(),
        RuntimeConvergenceStoreError::ExecutionClaimStale
    ));
    assert_eq!(
        reader
            .load_for_claim(&renewed)
            .await
            .unwrap()
            .artifact
            .version,
        RuleSetVersionId::FIRST
    );

    let current_execution = RuntimeExecutionReceiptV1 {
        snapshot: renewed.snapshot.clone(),
        controller_id: renewed.controller_id.clone(),
        fencing_token: renewed.fencing_token,
        convergence_attempt: renewed.convergence_attempt,
        acquired_at: renewed.acquired_at,
        expires_at: renewed.expires_at,
    };
    assert_eq!(
        reader
            .load_for_execution(&current_execution)
            .await
            .unwrap()
            .snapshot,
        renewed.snapshot
    );

    let mutated = adapter
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope(),
            expected_revision: current_execution.snapshot.revision,
            controller_id: current_execution.controller_id.clone(),
            fencing_token: current_execution.fencing_token,
            runtime_generation: current_execution.snapshot.runtime_generation,
            mutation: DeploymentMutationV1::AcceptPreflight(PreflightAttestationV1 {
                target: current_execution.snapshot.target.clone(),
                runtime_generation: current_execution.snapshot.runtime_generation,
                observed_runtime: None,
                checked_at: current_execution.acquired_at,
            }),
        })
        .await
        .unwrap();
    assert!(matches!(
        reader.load_for_claim(&renewed).await.unwrap_err(),
        RuntimeConvergenceStoreError::ExecutionClaimStale
    ));
    let post_mutation_execution = RuntimeExecutionReceiptV1 {
        snapshot: mutated.snapshot,
        ..current_execution
    };
    let post_mutation = reader
        .load_for_execution(&post_mutation_execution)
        .await
        .unwrap();
    assert_eq!(post_mutation.snapshot, post_mutation_execution.snapshot);
    assert_eq!(post_mutation.artifact.version, RuleSetVersionId::FIRST);
}
