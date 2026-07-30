const EXACT_TARGET_IDENTITY_FUNCTION: &str =
    "public.starring_runtime_exact_target_reader_database_identity_v1()";
const EXACT_TARGET_READINESS_FUNCTION: &str =
    "public.starring_runtime_exact_target_database_readiness_v2()";
const EXACT_TARGET_READ_FUNCTION: &str = "public.starring_runtime_exact_target_read_v2(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)";
const EXACT_TARGET_READINESS_RESULT: &str = "TABLE(database_identity text, database_name text, executor_role text, checked_at timestamp with time zone)";
const EXACT_TARGET_READ_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_deployment_id text, expected_promotion_id text, expected_activation_request_id text, expected_deployment_revision bigint, expected_controller_id text, expected_controller_fencing_token bigint, expected_convergence_attempt_no bigint, expected_runtime_generation bigint, expected_guild_id text, expected_ruleset_key text, expected_target_version bigint, expected_target_content_hash text, expected_binding_revision bigint, expected_binding_fingerprint text";
const EXACT_TARGET_READ_RESULT: &str = "TABLE(deployment_revision bigint, convergence_attempt_no bigint, desired_target_digest text, installation_authority_revision bigint, installation_authority_payload_digest text, installation_authority_policy_revision bigint, installation_authority_required_approvals integer, installation_authority_activation_ttl_seconds bigint, current_authority_revision bigint, current_authority_payload_digest text, current_authority_policy_revision bigint, current_authority_required_approvals integer, current_authority_activation_ttl_seconds bigint, guild_id text, ruleset_key text, target_version bigint, schema_version bigint, definition jsonb, content_hash text, canonical_content_hash text, created_by text, binding_revision bigint, binding_fingerprint text, resource_bindings jsonb, database_observed_at timestamp with time zone)";

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn exact_target_hydration_requires_a_current_fence_and_restricted_capability() {
    let mut database = isolated_runtime_database("exact_target_acl").await;
    if let Err(error) = MIGRATOR.run(&database.pool).await {
        drop_runtime_database(database).await;
        panic!("runtime test database migration failed: {error}");
    }
    let suffix = &database.name[database.name.len().saturating_sub(18)..];
    let role = format!("srt_exact_reader_{suffix}");
    assert!(
        role.len() <= 63
            && role
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    let password = format!("{role}_exact_target_test_password");
    let owner_pool = database.pool.clone();
    let database_name = database.name.clone();
    let restricted_options = database
        .connect_options
        .clone()
        .username(&role)
        .password(&password);
    let role_for_test = role.clone();
    let outcome = tokio::spawn(async move {
        install_exact_target_restricted_role(
            &owner_pool,
            &database_name,
            &role_for_test,
            &password,
        )
        .await;
        exact_target_restricted_role_scenario(
            owner_pool,
            restricted_options,
            database_name,
            role_for_test,
        )
        .await;
    })
    .await;

    remove_exact_target_restricted_role(&mut database, &role).await;
    drop_runtime_database(database).await;
    outcome.expect("restricted exact-target role proof must complete");
}

async fn install_exact_target_restricted_role(
    pool: &PgPool,
    database_name: &str,
    role: &str,
    password: &str,
) {
    let password_literal = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
        .bind(password)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {role} LOGIN PASSWORD {password_literal} NOSUPERUSER NOCREATEDB \
         NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("CREATE SEQUENCE public.runtime_exact_target_acl_probe")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "REVOKE ALL PRIVILEGES ON DATABASE {database_name} FROM PUBLIC"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {database_name} TO {role}"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {role}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {EXACT_TARGET_READINESS_FUNCTION}, \
         {EXACT_TARGET_IDENTITY_FUNCTION}, \
         {EXACT_TARGET_READ_FUNCTION} TO {role}"
    ))
    .execute(pool)
    .await
    .unwrap();
}

async fn remove_exact_target_restricted_role(database: &mut RuntimeTestDatabase, role: &str) {
    let exists = sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regrole($1) IS NOT NULL")
        .bind(role)
        .fetch_one(&mut database.administrator)
        .await
        .unwrap();
    if !exists {
        return;
    }
    sqlx::query(
        "SELECT pg_catalog.pg_terminate_backend(activity.pid) \
         FROM pg_catalog.pg_stat_activity AS activity \
         WHERE activity.usename = $1 AND activity.pid <> pg_catalog.pg_backend_pid()",
    )
    .bind(role)
    .execute(&mut database.administrator)
    .await
    .unwrap();
    let grant_role = format!("{role}_grant");
    let grant_exists =
        sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regrole($1) IS NOT NULL")
            .bind(&grant_role)
            .fetch_one(&mut database.administrator)
            .await
            .unwrap();
    if grant_exists {
        sqlx::query(&format!("REVOKE {grant_role} FROM {role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
        sqlx::query(&format!("DROP OWNED BY {grant_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        sqlx::query(&format!("DROP ROLE {grant_role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
    }
    sqlx::query(&format!("DROP OWNED BY {role}"))
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {role}"))
        .execute(&mut database.administrator)
        .await
        .unwrap();
}

async fn exact_target_restricted_role_scenario(
    owner_pool: PgPool,
    restricted_options: PgConnectOptions,
    database_name: String,
    role: String,
) {
    seed_product_target(&owner_pool).await;
    let adapter = PostgresRuntimeConvergence::new(owner_pool.clone());
    let initial = match adapter.enqueue(enqueue_request()).await.unwrap() {
        EnqueueDeploymentOutcomeV1::ExactReplay(snapshot) => snapshot,
        outcome => panic!("seeded target must replay exactly: {outcome:?}"),
    };
    let claim = adapter
        .claim_execution(ClaimDeploymentV1 {
            scope: scope(),
            expected_revision: initial.revision,
            controller_id: ControllerId::parse("runtime-restricted-hydration-controller").unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    assert_exact_target_function_contract(&owner_pool, &role).await;
    assert_exact_target_role_boundary(&owner_pool, &role).await;

    let expected_database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT FROM public.product_control_plane_identity \
         WHERE singleton",
    )
    .fetch_one(&owner_pool)
    .await
    .unwrap();
    let owner_role = sqlx::query_scalar::<_, String>("SELECT session_user::TEXT")
        .fetch_one(&owner_pool)
        .await
        .unwrap();
    let owner_expectation = RuntimeExactTargetDatabaseExpectationV1::new(
        expected_database_identity.clone(),
        database_name.clone(),
        owner_role,
    )
    .unwrap();
    let owner_attempt = PostgresRuntimeExactTargetReader::connect_verified_default(
        owner_pool.clone(),
        owner_expectation,
    )
    .await;
    assert!(matches!(
        owner_attempt,
        Err(RuntimeConvergenceStoreError::DatabaseAuthorityMismatch)
    ));
    let restricted_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(restricted_options.clone())
        .await
        .unwrap();
    let expectation = RuntimeExactTargetDatabaseExpectationV1::new(
        expected_database_identity.clone(),
        database_name.clone(),
        role.clone(),
    )
    .unwrap();
    let reader = PostgresRuntimeExactTargetReader::connect_verified_default(
        restricted_pool.clone(),
        expectation,
    )
    .await
    .unwrap();
    assert_eq!(
        reader.initial_readiness().database_identity,
        expected_database_identity
    );
    assert_eq!(reader.initial_readiness().database_name, database_name);
    assert_eq!(reader.initial_readiness().executor_role, role);
    let refreshed = reader.verify_database_v1().await.unwrap();
    assert_eq!(
        refreshed.database_identity,
        reader.initial_readiness().database_identity
    );
    assert_eq!(
        refreshed.database_name,
        reader.initial_readiness().database_name
    );
    assert_eq!(
        refreshed.executor_role,
        reader.initial_readiness().executor_role
    );
    assert!(refreshed.checked_at >= reader.initial_readiness().checked_at);
    let wrong_expectation = RuntimeExactTargetDatabaseExpectationV1::new(
        expected_database_identity.clone(),
        "starring_wrong_database",
        reader.initial_readiness().executor_role.clone(),
    )
    .unwrap();
    let wrong_attempt = PostgresRuntimeExactTargetReader::connect_verified_default(
        restricted_pool.clone(),
        wrong_expectation,
    )
    .await;
    assert!(matches!(
        wrong_attempt,
        Err(RuntimeConvergenceStoreError::DatabaseAuthorityMismatch)
    ));
    let hydrated = reader.load_for_claim(&claim).await.unwrap();
    assert_eq!(hydrated.snapshot, claim.snapshot);
    assert_eq!(hydrated.artifact.guild_id, GUILD);
    assert_eq!(hydrated.artifact.ruleset_key.as_str(), RULESET);
    assert_eq!(hydrated.artifact.version, RuleSetVersionId::FIRST);
    assert_eq!(hydrated.artifact.content_hash.to_hex(), CONTENT_HASH);
    assert_eq!(
        hydrated.desired_target_digest,
        runtime_desired_target_digest_v1(
            &claim.snapshot.identity,
            &claim.snapshot.target,
            claim.snapshot.runtime_generation.get(),
            1,
            claim.snapshot.previous_runtime.as_ref(),
        )
    );
    assert_eq!(hydrated.installation_authority_revision, 1);
    assert_eq!(
        hydrated.installation_authority_payload_digest.as_str(),
        authority_payload_digest_for_test(1, 1, BINDING_FINGERPRINT, 1, 1, 3600)
    );
    assert_eq!(hydrated.current_authority_revision, 1);
    assert_eq!(
        hydrated.current_authority_payload_digest.as_str(),
        authority_payload_digest_for_test(1, 1, BINDING_FINGERPRINT, 1, 1, 3600)
    );
    assert!(hydrated.bindings.role_bindings.is_empty());
    assert!(hydrated.bindings.channel_bindings.is_empty());
    assert!(hydrated.database_observed_at >= claim.acquired_at);
    assert!(hydrated.database_observed_at < claim.expires_at);

    let unchanged_bindings = json!({});
    rotate_authority(
        &owner_pool,
        AuthorityRotation {
            revision: 2,
            binding_revision: 1,
            resource_bindings: &unchanged_bindings,
            binding_fingerprint: BINDING_FINGERPRINT,
            policy_revision: 2,
            required_approvals: 1,
            activation_ttl_seconds: 7200,
        },
    )
    .await;
    let policy_rotated = reader.load_for_claim(&claim).await.unwrap();
    assert_eq!(policy_rotated.installation_authority_revision, 1);
    assert_eq!(policy_rotated.current_authority_revision, 2);
    assert_ne!(
        policy_rotated.installation_authority_payload_digest,
        policy_rotated.current_authority_payload_digest
    );
    assert_eq!(
        policy_rotated.current_authority_payload_digest.as_str(),
        authority_payload_digest_for_test(2, 1, BINDING_FINGERPRINT, 2, 1, 7200)
    );
    let desired_target_digest = policy_rotated.desired_target_digest.as_str().to_owned();
    replace_runtime_desired_target_digest_for_test(&owner_pool, &"0".repeat(64)).await;
    assert!(matches!(
        reader.load_for_claim(&claim).await.unwrap_err(),
        RuntimeConvergenceStoreError::InvalidPersistedState("runtime exact target projection")
    ));
    replace_runtime_desired_target_digest_for_test(&owner_pool, &desired_target_digest).await;
    let historical_authority_payload_digest = policy_rotated
        .installation_authority_payload_digest
        .as_str()
        .to_owned();
    replace_authority_payload_digest_for_test(&owner_pool, 1, &"0".repeat(64)).await;
    assert!(matches!(
        reader.load_for_claim(&claim).await.unwrap_err(),
        RuntimeConvergenceStoreError::InvalidPersistedState("runtime exact target projection")
    ));
    replace_authority_payload_digest_for_test(&owner_pool, 1, &historical_authority_payload_digest)
        .await;
    let current_authority_payload_digest = policy_rotated
        .current_authority_payload_digest
        .as_str()
        .to_owned();
    replace_authority_payload_digest_for_test(&owner_pool, 2, &"0".repeat(64)).await;
    assert!(matches!(
        reader.load_for_claim(&claim).await.unwrap_err(),
        RuntimeConvergenceStoreError::InvalidPersistedState("runtime exact target projection")
    ));
    replace_authority_payload_digest_for_test(&owner_pool, 2, &current_authority_payload_digest)
        .await;
    let restored = reader.load_for_claim(&claim).await.unwrap();
    assert_eq!(
        restored.desired_target_digest.as_str(),
        desired_target_digest
    );
    assert_eq!(
        restored.installation_authority_payload_digest.as_str(),
        historical_authority_payload_digest
    );
    assert_eq!(
        restored.current_authority_payload_digest.as_str(),
        current_authority_payload_digest
    );

    sqlx::query(
        "UPDATE public.product_control_plane_identity \
         SET database_identity = '11111111-2222-3333-8444-555555555555'::UUID \
         WHERE singleton",
    )
    .execute(&owner_pool)
    .await
    .unwrap();
    assert!(matches!(
        reader.load_for_claim(&claim).await.unwrap_err(),
        RuntimeConvergenceStoreError::DatabaseAuthorityMismatch
    ));
    sqlx::query(
        "UPDATE public.product_control_plane_identity SET database_identity = $1::UUID \
         WHERE singleton",
    )
    .bind(&expected_database_identity)
    .execute(&owner_pool)
    .await
    .unwrap();
    assert_eq!(
        reader.load_for_claim(&claim).await.unwrap().snapshot,
        claim.snapshot
    );

    let mut stale = claim.clone();
    stale.fencing_token = FencingToken::new(claim.fencing_token.get() + 1).unwrap();
    assert!(matches!(
        reader.load_for_claim(&stale).await.unwrap_err(),
        RuntimeConvergenceStoreError::ExecutionClaimStale
    ));

    let renewed = adapter
        .renew_execution(RenewDeploymentV1 {
            scope: scope(),
            expected_revision: claim.snapshot.revision,
            controller_id: claim.controller_id.clone(),
            fencing_token: claim.fencing_token,
            convergence_attempt: claim.convergence_attempt,
            runtime_generation: claim.snapshot.runtime_generation,
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
            convergence_attempt: current_execution.convergence_attempt,
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
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT public.starring_runtime_exact_target_reader_database_identity_v1()",
        )
        .fetch_one(&restricted_pool)
        .await
        .unwrap(),
        expected_database_identity
    );

    for statement in [
        "SELECT deployment_id FROM public.runtime_deployments LIMIT 1",
        "INSERT INTO public.runtime_deployments DEFAULT VALUES",
        "UPDATE public.runtime_deployments SET phase = phase WHERE FALSE",
        "DELETE FROM public.runtime_deployments WHERE FALSE",
        "SELECT pg_catalog.nextval('public.runtime_exact_target_acl_probe')",
        "SELECT public.starring_runtime_mutation_clock()",
        "SELECT * FROM public.starring_runtime_observe_previous_serving_v1(\
         NULL::TEXT,NULL::TEXT,NULL::TEXT,NULL::BIGINT,NULL::TEXT,NULL::BIGINT,\
         NULL::BIGINT,NULL::BIGINT,NULL::TEXT,NULL::TEXT,NULL::BIGINT,NULL::TEXT,\
         NULL::BIGINT,NULL::TEXT,NULL::JSONB)",
        "SELECT public.starring_product_apply_executor_database_identity_v1()",
        "SELECT * FROM public.starring_purge_product_action_receipts_v1(1)",
    ] {
        assert_runtime_permission_denied(&restricted_pool, statement).await;
    }

    sqlx::query(&format!(
        "GRANT SELECT ON TABLE public.runtime_deployments TO {role}"
    ))
    .execute(&owner_pool)
    .await
    .unwrap();
    assert_exact_target_readiness_authority_mismatch(&reader).await;
    sqlx::query(&format!(
        "REVOKE SELECT ON TABLE public.runtime_deployments FROM {role}"
    ))
    .execute(&owner_pool)
    .await
    .unwrap();
    reader.verify_database_v1().await.unwrap();

    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION public.starring_runtime_mutation_clock() TO {role}"
    ))
    .execute(&owner_pool)
    .await
    .unwrap();
    assert_exact_target_readiness_authority_mismatch(&reader).await;
    sqlx::query(&format!(
        "REVOKE EXECUTE ON FUNCTION public.starring_runtime_mutation_clock() FROM {role}"
    ))
    .execute(&owner_pool)
    .await
    .unwrap();
    reader.verify_database_v1().await.unwrap();

    sqlx::query("ALTER TABLE public.automation_ruleset_versions ENABLE ROW LEVEL SECURITY")
        .execute(&owner_pool)
        .await
        .unwrap();
    assert_exact_target_readiness_authority_mismatch(&reader).await;
    sqlx::query("ALTER TABLE public.automation_ruleset_versions DISABLE ROW LEVEL SECURITY")
        .execute(&owner_pool)
        .await
        .unwrap();
    reader.verify_database_v1().await.unwrap();

    sqlx::query(
        "ALTER INDEX public.product_control_plane_identity_database_identity_key \
         RENAME TO product_control_plane_identity_database_identity_drift",
    )
    .execute(&owner_pool)
    .await
    .unwrap();
    assert_exact_target_readiness_authority_mismatch(&reader).await;
    sqlx::query(
        "ALTER INDEX public.product_control_plane_identity_database_identity_drift \
         RENAME TO product_control_plane_identity_database_identity_key",
    )
    .execute(&owner_pool)
    .await
    .unwrap();
    reader.verify_database_v1().await.unwrap();

    sqlx::query(&format!("ALTER ROLE {role} SET statement_timeout = '9s'"))
        .execute(&owner_pool)
        .await
        .unwrap();
    assert_exact_target_readiness_authority_mismatch(&reader).await;
    sqlx::query(&format!("ALTER ROLE {role} RESET ALL"))
        .execute(&owner_pool)
        .await
        .unwrap();
    reader.verify_database_v1().await.unwrap();

    let original_readiness_definition = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(\
         'public.starring_runtime_exact_target_database_readiness_v2()'))",
    )
    .fetch_one(&owner_pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE OR REPLACE FUNCTION public.starring_runtime_exact_target_database_readiness_v2() \
         RETURNS TABLE(database_identity TEXT, database_name TEXT, executor_role TEXT, \
          checked_at TIMESTAMPTZ) \
         LANGUAGE plpgsql VOLATILE STRICT PARALLEL UNSAFE SECURITY DEFINER \
         SET search_path = pg_catalog ROWS 1 \
         AS $function$ \
         BEGIN \
          SELECT identity.database_identity::TEXT \
          INTO database_identity \
          FROM public.product_control_plane_identity AS identity \
          WHERE identity.singleton; \
          database_name := pg_catalog.current_database()::TEXT; \
          executor_role := session_user::TEXT; \
          checked_at := pg_catalog.clock_timestamp(); \
          RETURN NEXT; \
         END; \
         $function$",
    )
    .execute(&owner_pool)
    .await
    .unwrap();
    assert_exact_target_readiness_authority_mismatch(&reader).await;
    sqlx::query(&original_readiness_definition)
        .execute(&owner_pool)
        .await
        .unwrap();
    reader.verify_database_v1().await.unwrap();

    let grant_role = format!("{role}_grant");
    sqlx::query(&format!(
        "CREATE ROLE {grant_role} NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB \
         NOCREATEROLE NOREPLICATION NOBYPASSRLS"
    ))
    .execute(&owner_pool)
    .await
    .unwrap();
    sqlx::query(&format!("GRANT {grant_role} TO {role}"))
        .execute(&owner_pool)
        .await
        .unwrap();
    assert_exact_target_readiness_authority_mismatch(&reader).await;
    sqlx::query(&format!("REVOKE {grant_role} FROM {role}"))
        .execute(&owner_pool)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {grant_role}"))
        .execute(&owner_pool)
        .await
        .unwrap();
    reader.verify_database_v1().await.unwrap();

    let bounded_timeouts = RuntimeExactTargetDatabaseTimeoutsV1::new(
        Duration::from_millis(400),
        Duration::from_millis(300),
    )
    .unwrap();
    let single_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(restricted_options)
        .await
        .unwrap();
    let single_expectation = RuntimeExactTargetDatabaseExpectationV1::new(
        expected_database_identity.clone(),
        database_name,
        role.clone(),
    )
    .unwrap();
    let single_reader = PostgresRuntimeExactTargetReader::connect_verified(
        single_pool.clone(),
        single_expectation,
        bounded_timeouts,
    )
    .await
    .unwrap();
    let held_connection = single_pool.acquire().await.unwrap();
    assert!(matches!(
        single_reader.verify_database_v1().await.unwrap_err(),
        RuntimeConvergenceStoreError::DatabaseTimeout
    ));
    drop(held_connection);
    single_reader.verify_database_v1().await.unwrap();

    let mut blocker = owner_pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE public.product_control_plane_identity IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let externally_cancelled = tokio::time::timeout(
        Duration::from_millis(100),
        single_reader.load_for_execution(&post_mutation_execution),
    )
    .await;
    assert!(externally_cancelled.is_err());
    let replacement = tokio::time::timeout(Duration::from_secs(1), single_pool.acquire())
        .await
        .expect("cancelled hydration must release the pool permit")
        .unwrap();
    drop(replacement);
    blocker.rollback().await.unwrap();
    assert_eq!(
        single_reader
            .load_for_execution(&post_mutation_execution)
            .await
            .unwrap()
            .snapshot,
        post_mutation_execution.snapshot
    );
    single_pool.close().await;

    sqlx::query(&format!(
        "REVOKE EXECUTE ON FUNCTION {EXACT_TARGET_READ_FUNCTION} FROM {role}"
    ))
    .execute(&owner_pool)
    .await
    .unwrap();
    assert!(matches!(
        reader
            .load_for_execution(&post_mutation_execution)
            .await
            .unwrap_err(),
        RuntimeConvergenceStoreError::DatabaseFailure
    ));
    assert!(matches!(
        reader.verify_database_v1().await.unwrap_err(),
        RuntimeConvergenceStoreError::DatabaseAuthorityMismatch
    ));
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {EXACT_TARGET_READ_FUNCTION} TO {role}"
    ))
    .execute(&owner_pool)
    .await
    .unwrap();
    reader.verify_database_v1().await.unwrap();
    sqlx::query(&format!(
        "REVOKE EXECUTE ON FUNCTION {EXACT_TARGET_READINESS_FUNCTION} FROM {role}"
    ))
    .execute(&owner_pool)
    .await
    .unwrap();
    assert!(matches!(
        reader.verify_database_v1().await.unwrap_err(),
        RuntimeConvergenceStoreError::DatabaseFailure
    ));
    restricted_pool.close().await;
}

async fn assert_exact_target_readiness_authority_mismatch(
    reader: &PostgresRuntimeExactTargetReader,
) {
    assert!(matches!(
        reader.verify_database_v1().await.unwrap_err(),
        RuntimeConvergenceStoreError::DatabaseAuthorityMismatch
    ));
}

async fn replace_runtime_desired_target_digest_for_test(pool: &PgPool, digest: &str) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.runtime_deployments SET desired_target_digest = $2 \
         WHERE deployment_id = $1",
    )
    .bind(DEPLOYMENT)
    .bind(digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn replace_authority_payload_digest_for_test(pool: &PgPool, revision: i64, digest: &str) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.automation_installation_authority_versions \
         SET authority_payload_digest = $3 \
         WHERE installation_id = $1 AND revision = $2",
    )
    .bind(INSTALLATION)
    .bind(revision)
    .bind(digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SET LOCAL session_replication_role = origin")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn assert_exact_target_role_boundary(pool: &PgPool, role: &str) {
    let role_contract =
        sqlx::query_as::<_, (bool, bool, bool, bool, bool, bool, bool, i32, Vec<String>)>(
            "SELECT rolcanlogin, rolinherit, rolsuper, rolcreatedb, rolcreaterole, \
         rolreplication, rolbypassrls, rolconnlimit, \
         COALESCE(rolconfig, ARRAY[]::TEXT[]) \
         FROM pg_catalog.pg_roles WHERE rolname = $1",
        )
        .bind(role)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        role_contract,
        (
            true,
            false,
            false,
            false,
            false,
            false,
            false,
            4,
            Vec::new()
        )
    );
    let role_setting_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_db_role_setting \
         WHERE setrole = pg_catalog.to_regrole($1)",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(role_setting_count, 0);
    let scope = sqlx::query_as::<_, (bool, bool, bool, bool, bool)>(
        "SELECT pg_catalog.has_database_privilege($1, pg_catalog.current_database(), 'CONNECT'), \
         pg_catalog.has_database_privilege($1, pg_catalog.current_database(), 'CREATE'), \
         pg_catalog.has_database_privilege($1, pg_catalog.current_database(), 'TEMPORARY'), \
         pg_catalog.has_schema_privilege($1, 'public', 'USAGE'), \
         pg_catalog.has_schema_privilege($1, 'public', 'CREATE')",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(scope, (true, false, false, true, false));
    let unexpected_schema_privilege_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_namespace AS namespace \
         WHERE namespace.nspname <> 'public' \
          AND namespace.nspname <> 'information_schema' \
          AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
          AND (pg_catalog.has_schema_privilege($1, namespace.oid, 'USAGE') \
           OR pg_catalog.has_schema_privilege($1, namespace.oid, 'CREATE'))",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(unexpected_schema_privilege_count, 0);
    let membership_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_auth_members \
         WHERE roleid = pg_catalog.to_regrole($1) OR member = pg_catalog.to_regrole($1)",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(membership_count, 0);
    let relation_privilege_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname <> 'information_schema' \
          AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
          AND relation.relkind IN ('r', 'p', 'v', 'm', 'f') \
          AND (pg_catalog.has_table_privilege($1, relation.oid, \
            'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') \
           OR pg_catalog.has_any_column_privilege($1, relation.oid, \
            'SELECT,INSERT,UPDATE,REFERENCES'))",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(relation_privilege_count, 0);
    let sequence_privilege_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_sequences AS sequence \
         WHERE sequence.schemaname <> 'information_schema' \
          AND pg_catalog.left(sequence.schemaname::TEXT, 3) <> 'pg_' \
          AND pg_catalog.has_sequence_privilege($1, \
           pg_catalog.format('%I.%I', sequence.schemaname, sequence.sequencename), \
           'USAGE,SELECT,UPDATE')",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(sequence_privilege_count, 0);
    let executable_names = sqlx::query_scalar::<_, Vec<String>>(
        "SELECT COALESCE(pg_catalog.array_agg(function_row.proname::TEXT \
          ORDER BY function_row.proname), ARRAY[]::TEXT[]) \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = function_row.pronamespace \
         WHERE namespace.nspname <> 'information_schema' \
          AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
          AND pg_catalog.has_function_privilege($1, function_row.oid, 'EXECUTE')",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        executable_names,
        vec![
            "starring_runtime_exact_target_database_readiness_v2".to_string(),
            "starring_runtime_exact_target_read_v2".to_string(),
            "starring_runtime_exact_target_reader_database_identity_v1".to_string(),
        ]
    );
    let parameter_privilege_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_parameter_acl AS parameter_acl \
         CROSS JOIN LATERAL pg_catalog.aclexplode(parameter_acl.paracl) AS privilege \
         WHERE privilege.grantee IN (0, pg_catalog.to_regrole($1)) \
          AND privilege.privilege_type IN ('SET', 'ALTER SYSTEM')",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(parameter_privilege_count, 0);
    let large_object_privilege_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_largeobject_metadata AS large_object \
         WHERE large_object.lomowner = pg_catalog.to_regrole($1) \
          OR EXISTS ( \
           SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
            large_object.lomacl, \
            pg_catalog.acldefault('L', large_object.lomowner) \
           )) AS privilege \
           WHERE privilege.grantee IN (0, pg_catalog.to_regrole($1)) \
            AND (privilege.privilege_type IN ('SELECT', 'UPDATE') \
             OR privilege.is_grantable) \
          )",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(large_object_privilege_count, 0);
    for denied in [
        "public.starring_runtime_exact_target_database_readiness_v1()",
        "public.starring_runtime_exact_target_read_v1(text,text,text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text)",
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v2()",
        "public.starring_runtime_mutation_clock()",
        "public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)",
        "public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)",
        "public.starring_runtime_panel_reconciliation_snapshot_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,bigint,bigint,text,bigint)",
        "public.starring_product_apply_executor_database_identity_v1()",
        "public.starring_purge_product_action_receipts_v1(integer)",
    ] {
        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
        )
        .bind(role)
        .bind(denied)
        .fetch_one(pool)
        .await
        .unwrap());
    }
}

async fn assert_exact_target_function_contract(pool: &PgPool, role: &str) {
    let relation_owner = sqlx::query_scalar::<_, String>(
        "SELECT owner.rolname FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_roles AS owner ON owner.oid = relation.relowner \
         WHERE relation.oid = pg_catalog.to_regclass('public.runtime_deployments')",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    for (identity, language_name, arguments, result, returns_set, rows) in [
        (
            EXACT_TARGET_READINESS_FUNCTION,
            "plpgsql",
            "",
            EXACT_TARGET_READINESS_RESULT,
            true,
            1.0_f32,
        ),
        (
            EXACT_TARGET_IDENTITY_FUNCTION,
            "sql",
            "",
            "text",
            false,
            0.0_f32,
        ),
        (
            EXACT_TARGET_READ_FUNCTION,
            "sql",
            EXACT_TARGET_READ_ARGUMENTS,
            EXACT_TARGET_READ_RESULT,
            true,
            1.0_f32,
        ),
    ] {
        let contract = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                bool,
                String,
                bool,
                bool,
                f32,
                Vec<String>,
                bool,
                i64,
                i64,
                String,
                String,
            ),
        >(
            "SELECT owner.rolname, language.lanname, function_row.provolatile::TEXT, \
             function_row.proisstrict, function_row.proparallel::TEXT, \
             function_row.prosecdef, function_row.proretset, function_row.prorows, \
             COALESCE(function_row.proconfig, ARRAY[]::TEXT[]), function_row.proleakproof, \
             function_row.pronargdefaults::BIGINT, function_row.provariadic::BIGINT, \
             pg_catalog.pg_get_function_identity_arguments(function_row.oid), \
             pg_catalog.pg_get_function_result(function_row.oid) \
             FROM pg_catalog.pg_proc AS function_row \
             INNER JOIN pg_catalog.pg_roles AS owner ON owner.oid = function_row.proowner \
             INNER JOIN pg_catalog.pg_language AS language ON language.oid = function_row.prolang \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(identity)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(contract.0, relation_owner);
        assert_eq!(contract.1, language_name);
        assert_eq!(contract.2, "v");
        assert!(contract.3);
        assert_eq!(contract.4, "u");
        assert!(contract.5);
        assert_eq!(contract.6, returns_set);
        assert_eq!(contract.7, rows);
        assert_eq!(contract.8, vec!["search_path=pg_catalog".to_string()]);
        assert!(!contract.9);
        assert_eq!(contract.10, 0);
        assert_eq!(contract.11, 0);
        assert_eq!(contract.12, arguments);
        assert_eq!(contract.13, result);
    }
    let public_execute_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
          function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
         )) AS privilege \
         WHERE function_row.oid IN (pg_catalog.to_regprocedure($1), pg_catalog.to_regprocedure($2), \
          pg_catalog.to_regprocedure($3)) \
          AND privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE'",
    )
    .bind(EXACT_TARGET_READINESS_FUNCTION)
    .bind(EXACT_TARGET_IDENTITY_FUNCTION)
    .bind(EXACT_TARGET_READ_FUNCTION)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(public_execute_count, 0);
    let explicit_acl = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT pg_catalog.count(*), \
         pg_catalog.count(*) FILTER (WHERE privilege.grantee = pg_catalog.to_regrole($1)), \
         pg_catalog.count(*) FILTER (WHERE privilege.is_grantable) \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
          function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
         )) AS privilege \
         WHERE function_row.oid IN (pg_catalog.to_regprocedure($2), pg_catalog.to_regprocedure($3), \
          pg_catalog.to_regprocedure($4)) \
          AND privilege.grantee <> function_row.proowner \
          AND privilege.privilege_type = 'EXECUTE'",
    )
    .bind(role)
    .bind(EXACT_TARGET_READINESS_FUNCTION)
    .bind(EXACT_TARGET_IDENTITY_FUNCTION)
    .bind(EXACT_TARGET_READ_FUNCTION)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(explicit_acl, (3, 3, 0));
    for identity in [
        EXACT_TARGET_READINESS_FUNCTION,
        EXACT_TARGET_IDENTITY_FUNCTION,
        EXACT_TARGET_READ_FUNCTION,
    ] {
        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE WITH GRANT OPTION')",
        )
        .bind(role)
        .bind(identity)
        .fetch_one(pool)
        .await
        .unwrap());
    }
}

async fn assert_runtime_permission_denied(pool: &PgPool, statement: &str) {
    let error = sqlx::query(statement)
        .execute(pool)
        .await
        .expect_err("restricted runtime capability must be denied");
    assert!(matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("42501")
    ));
}
