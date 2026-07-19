const PRODUCT_APPLY_FUNCTIONS: [&str; 5] = [
    PRODUCT_APPLY_TOPOLOGY_FUNCTION,
    "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    "public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)",
    "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)",
    "public.starring_product_apply_keyring_coverage_v1(text[],text[])",
];
const PRODUCT_DECISION_BOUNDARY_RELATIONS: [&str; 20] = [
    "product_control_plane_identity",
    "activation_requests",
    "activation_request_approvals",
    "authoring_promotions",
    "product_tenants",
    "automation_installations",
    "automation_installation_authority_versions",
    "authoring_sessions",
    "authoring_session_generations",
    "product_principals",
    "product_auth_sessions",
    "product_action_receipts",
    "product_action_receipt_idempotency_aliases",
    "product_audit_events",
    "product_action_receipt_audit_evidence",
    "automation_ruleset_activations",
    "automation_ruleset_versions",
    "runtime_deployments",
    "runtime_serving_leases",
    "runtime_attestations",
];
const PRODUCT_APPLY_SUPPORT_FUNCTIONS: [&str; 10] = [
    "public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    "public.starring_product_apply_authority_projection_v1(text,text,text,text,bytea,text,text,text,text,bigint,text,timestamp with time zone,timestamp with time zone,text,boolean,text)",
    "public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)",
    "public.starring_runtime_lock_current_authority(text,text,text,text,bigint,text,text,bigint,text,bigint,text)",
    "public.starring_runtime_current_mutation_clock()",
    "public.assert_product_ruleset_slot_pointer()",
    "public.enforce_runtime_deployment_policy_shadow()",
    "public.guard_runtime_ruleset_artifact_transition()",
    "public.reject_runtime_deployment_delete()",
    "public.validate_runtime_deployment_projection()",
];

fn incomplete_apply_security_keyring() -> ProductDecisionDigestKeyringV1 {
    ProductDecisionDigestKeyringV1::new(
        ProductDecisionDigestKeyV1::from_bytes(
            "product-security-unknown",
            std::array::from_fn(|index| 193_u8.wrapping_add(index as u8)),
        )
        .unwrap(),
        [],
    )
    .unwrap()
}

async fn alter_product_decision_boundary_owner(pool: &PgPool, owner_role: &str) {
    for relation in PRODUCT_DECISION_BOUNDARY_RELATIONS {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(pool)
        .await
        .unwrap();
    }
    for function in [
        PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION,
        PRODUCT_DECISION_READ_FUNCTION,
        PRODUCT_APPROVAL_TOPOLOGY_FUNCTION,
        PRODUCT_APPROVAL_FUNCTION,
        PRODUCT_APPROVAL_COVERAGE_FUNCTION,
    ]
    .into_iter()
    .chain(PRODUCT_APPLY_FUNCTIONS)
    .chain(PRODUCT_APPROVAL_SUPPORT_FUNCTIONS)
    .chain(PRODUCT_APPLY_SUPPORT_FUNCTIONS)
    {
        sqlx::query(&format!(
            "ALTER FUNCTION {function} OWNER TO {owner_role}"
        ))
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn grant_product_decision_functions(pool: &PgPool, role: &str, functions: &[&str]) {
    for function in functions {
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {function} TO {role}"
        ))
        .execute(pool)
        .await
        .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_apply_executor_is_exactly_scoped_replay_safe_and_fail_closed() {
    let mut database = isolated_product_control_database("apply_security").await;
    let role_suffix = suffix();
    let owner_role = format!("starring_apply_owner_{role_suffix}");
    let reader_role = format!("starring_apply_reader_{role_suffix}");
    let approval_role = format!("starring_apply_approval_{role_suffix}");
    let apply_role = format!("starring_apply_executor_{role_suffix}");
    let denied_role = format!("starring_apply_denied_{role_suffix}");
    let roles = [
        owner_role.clone(),
        reader_role.clone(),
        approval_role.clone(),
        apply_role.clone(),
        denied_role.clone(),
    ];
    let reader_password = database_role_password();
    let approval_password = database_role_password();
    let apply_password = database_role_password();
    let denied_password = database_role_password();

    let outcome = std::panic::AssertUnwindSafe(async {
        MIGRATOR.run(&database.pool).await.unwrap();
        let fixture = seed_fixture(&database.pool).await;
        sqlx::query(&format!(
            "CREATE ROLE {owner_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        for (role, password) in [
            (&reader_role, &reader_password),
            (&approval_role, &approval_password),
            (&apply_role, &apply_password),
            (&denied_role, &denied_password),
        ] {
            create_decision_login_role(&database.pool, role, password, 4).await;
        }
        alter_product_decision_boundary_owner(&database.pool, &owner_role).await;
        sqlx::query(&format!("ALTER SCHEMA public OWNER TO {owner_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        sqlx::query(&format!(
            "REVOKE ALL ON DATABASE {} FROM PUBLIC",
            database.name
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::query("REVOKE ALL ON SCHEMA public FROM PUBLIC")
            .execute(&database.pool)
            .await
            .unwrap();
        sqlx::query(&format!(
            "GRANT CONNECT ON DATABASE {} TO {reader_role}, {approval_role}, \
             {apply_role}, {denied_role}",
            database.name
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT USAGE ON SCHEMA public TO {owner_role}, {reader_role}, \
             {approval_role}, {apply_role}, {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        grant_product_decision_functions(
            &database.pool,
            &reader_role,
            &[
                PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION,
                PRODUCT_DECISION_READ_FUNCTION,
            ],
        )
        .await;
        grant_product_decision_functions(
            &database.pool,
            &approval_role,
            &[
                PRODUCT_APPROVAL_TOPOLOGY_FUNCTION,
                PRODUCT_APPROVAL_FUNCTION,
                PRODUCT_APPROVAL_COVERAGE_FUNCTION,
            ],
        )
        .await;
        grant_product_decision_functions(&database.pool, &apply_role, &PRODUCT_APPLY_FUNCTIONS)
            .await;

        let reader_pool =
            database_role_login_pool(&database.name, &reader_role, &reader_password).await;
        let approval_pool =
            database_role_login_pool(&database.name, &approval_role, &approval_password).await;
        let apply_pool =
            database_role_login_pool(&database.name, &apply_role, &apply_password).await;
        let denied_pool =
            database_role_login_pool(&database.name, &denied_role, &denied_password).await;
        let decisions = PostgresProductDecisions::new(
            ProductDecisionDatabasePoolsV1::new(
                reader_pool.clone(),
                approval_pool.clone(),
                apply_pool.clone(),
            ),
            decision_security_keyring(),
        )
        .unwrap();
        decisions.verify_decision_reader_readiness().await.unwrap();
        decisions.verify_approval_executor_readiness().await.unwrap();
        decisions.verify_apply_executor_readiness().await.unwrap();
        decisions
            .verify_product_decision_boundary_readiness()
            .await
            .unwrap();

        let authentication = PostgresAuthentication::new(database.pool.clone());
        let authority = authority_adapter(fixture.clone());
        let deployments = PendingDeployments;
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let approval = application
            .approve(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse(&format!("apply.security.approve.{role_suffix}"))
                    .unwrap(),
                &selector(&fixture),
                approval_command(&fixture, &format!("apply-security-approve-{role_suffix}")),
            )
            .await
            .unwrap();
        assert!(!approval.exact_replay());
        assert_eq!(approval.projection().revision().get(), 2);
        assert_eq!(
            approval.projection().phase(),
            &ProductDecisionPhaseV1::Approved
        );

        let apply_key = format!("apply-security-{role_suffix}");
        let applied = application
            .apply(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse(&format!("apply.security.first.{role_suffix}"))
                    .unwrap(),
                &selector(&fixture),
                apply_command(&fixture, &apply_key),
            )
            .await
            .unwrap();
        assert_eq!(applied.status(), ProductStatusV1::RuntimePending);
        assert!(!applied.exact_replay());
        let replay = application
            .apply(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse(&format!("apply.security.replay.{role_suffix}"))
                    .unwrap(),
                &selector(&fixture),
                apply_command(&fixture, &apply_key),
            )
            .await
            .unwrap();
        assert_eq!(replay.status(), ProductStatusV1::RuntimePending);
        assert!(replay.exact_replay());
        assert_eq!(replay.exact_deployment(), applied.exact_deployment());

        let evidence = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64, i64)>(
             "SELECT activation.state, activation.product_revision, \
             (SELECT pg_catalog.count(*) FROM public.activation_request_approvals AS approval \
              WHERE approval.request_id = activation.id), \
             (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
              WHERE deployment.activation_request_id = activation.id), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
              WHERE receipt.target_resource_id = activation.promotion_id \
               AND receipt.endpoint_domain = 'product_apply_v1'), \
             (SELECT pg_catalog.count(*) \
              FROM public.product_action_receipt_idempotency_aliases AS alias \
              WHERE alias.receipt_id IN ( \
               SELECT receipt.receipt_id FROM public.product_action_receipts AS receipt \
               WHERE receipt.target_resource_id = activation.promotion_id \
                AND receipt.endpoint_domain = 'product_apply_v1')), \
             (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
              WHERE audit.target_resource_id = activation.promotion_id \
               AND audit.action = 'promotion.apply'), \
             (SELECT pg_catalog.count(*) \
              FROM public.product_action_receipt_audit_evidence AS evidence \
              WHERE evidence.target_resource_id = activation.promotion_id \
               AND evidence.endpoint_domain = 'product_apply_v1') \
             FROM public.activation_requests AS activation WHERE activation.id = $1",
        )
        .bind(&fixture.activation_id)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(evidence, ("applied".to_string(), 4, 1, 1, 1, 1, 1, 1));
        decisions.verify_apply_executor_readiness().await.unwrap();
        decisions
            .verify_product_decision_boundary_readiness()
            .await
            .unwrap();

        for statement in [
            "SELECT * FROM public.activation_requests LIMIT 1",
            "INSERT INTO public.activation_request_approvals DEFAULT VALUES",
            "UPDATE public.activation_requests SET state = state",
            "DELETE FROM public.runtime_deployments",
            "TRUNCATE TABLE public.product_action_receipts",
            "SELECT public.starring_product_decision_reader_database_identity_v1()",
            "SELECT public.starring_product_approval_executor_database_identity_v1()",
            "SELECT public.starring_runtime_current_mutation_clock()",
        ] {
            assert_database_permission_denied(&apply_pool, statement).await;
        }
        assert_database_permission_denied(
            &apply_pool,
            &format!("CREATE TABLE public.apply_escape_{role_suffix}(value INTEGER)"),
        )
        .await;
        assert_database_permission_denied(
            &apply_pool,
            &format!("CREATE TEMPORARY TABLE apply_escape_{role_suffix}(value INTEGER)"),
        )
        .await;
        assert_database_permission_denied(
            &reader_pool,
            "SELECT public.starring_product_apply_executor_database_identity_v1()",
        )
        .await;
        assert_database_permission_denied(
            &approval_pool,
            "SELECT public.starring_product_apply_executor_database_identity_v1()",
        )
        .await;
        assert_database_permission_denied(
            &denied_pool,
            "SELECT public.starring_product_apply_executor_database_identity_v1()",
        )
        .await;
        for function in PRODUCT_APPLY_FUNCTIONS {
            let allowed = sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_function_privilege( \
                 current_user, pg_catalog.to_regprocedure($1), 'EXECUTE')",
            )
            .bind(function)
            .fetch_one(&denied_pool)
            .await
            .unwrap();
            assert!(!allowed);
        }

        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {} FROM {apply_role}",
            PRODUCT_APPLY_FUNCTIONS[4]
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_apply_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::CapabilityMissing)
        );
        grant_product_decision_functions(
            &database.pool,
            &apply_role,
            &[PRODUCT_APPLY_FUNCTIONS[4]],
        )
        .await;

        grant_product_decision_functions(
            &database.pool,
            &apply_role,
            &[PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION],
        )
        .await;
        assert_eq!(
            decisions.verify_apply_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION} \
             FROM {apply_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {} TO PUBLIC",
            PRODUCT_APPLY_FUNCTIONS[2]
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_apply_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {} FROM PUBLIC",
            PRODUCT_APPLY_FUNCTIONS[2]
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER FUNCTION {} STABLE",
            PRODUCT_APPLY_FUNCTIONS[2]
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_apply_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "ALTER FUNCTION {} VOLATILE",
            PRODUCT_APPLY_FUNCTIONS[2]
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER FUNCTION {} LEAKPROOF",
            PRODUCT_APPLY_FUNCTIONS[2]
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_apply_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "ALTER FUNCTION {} NOT LEAKPROOF",
            PRODUCT_APPLY_FUNCTIONS[2]
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query("ALTER TABLE public.runtime_serving_leases ENABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            decisions.verify_apply_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query("ALTER TABLE public.runtime_serving_leases DISABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(
            "ALTER TABLE public.runtime_deployments \
             DISABLE TRIGGER runtime_deployments_policy_shadow_guard",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_apply_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(
            "ALTER TABLE public.runtime_deployments \
             ENABLE TRIGGER runtime_deployments_policy_shadow_guard",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        let incomplete = PostgresProductDecisions::new(
            ProductDecisionDatabasePoolsV1::new(
                reader_pool.clone(),
                approval_pool.clone(),
                apply_pool.clone(),
            ),
            incomplete_apply_security_keyring(),
        )
        .unwrap();
        assert_eq!(
            incomplete.verify_apply_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::IncompleteCoverage)
        );
        decisions.verify_apply_executor_readiness().await.unwrap();
        decisions
            .verify_product_decision_boundary_readiness()
            .await
            .unwrap();

        reader_pool.close().await;
        approval_pool.close().await;
        apply_pool.close().await;
        denied_pool.close().await;
    })
    .catch_unwind()
    .await;

    database.pool.close().await;
    let mut cleanup_failure = sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        database.name
    ))
        .execute(&mut database.administrator)
        .await
        .err();
    for role in roles.into_iter().rev() {
        if let Err(error) = sqlx::query(&format!("DROP ROLE IF EXISTS {role}"))
            .execute(&mut database.administrator)
            .await
        {
            cleanup_failure.get_or_insert(error);
        }
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
    if let Some(error) = cleanup_failure {
        panic!("product apply security cleanup failed: {error}");
    }
}

fn product_apply_existing_protected_functions() -> Vec<String> {
    [
        PRODUCT_APPLY_FUNCTIONS[0],
        PRODUCT_APPLY_FUNCTIONS[1],
        PRODUCT_APPLY_FUNCTIONS[3],
    ]
    .into_iter()
    .chain(PRODUCT_APPROVAL_SUPPORT_FUNCTIONS)
    .chain(PRODUCT_APPLY_SUPPORT_FUNCTIONS)
    .map(str::to_owned)
    .collect()
}

async fn product_apply_protected_function_catalog_state(pool: &PgPool) -> (i64, String) {
    let signatures = product_apply_existing_protected_functions();
    let state = sqlx::query_as::<_, (i64, String)>(
        "WITH expected(signature) AS ( \
          SELECT pg_catalog.unnest($1::TEXT[]) \
         ) \
         SELECT pg_catalog.count(function_row.oid), \
          COALESCE( \
           pg_catalog.jsonb_agg( \
            pg_catalog.jsonb_build_object( \
             'signature', expected.signature, \
             'oid', function_row.oid::TEXT, \
             'owner', pg_catalog.pg_get_userbyid(function_row.proowner), \
             'acl', COALESCE(function_row.proacl::TEXT, '<null>'), \
             'kind', function_row.prokind::TEXT, \
             'volatility', function_row.provolatile::TEXT, \
             'strict', function_row.proisstrict, \
             'parallel', function_row.proparallel::TEXT, \
             'security_definer', function_row.prosecdef, \
             'returns_set', function_row.proretset, \
             'rows', function_row.prorows, \
             'configuration', COALESCE(function_row.proconfig::TEXT, '<null>'), \
             'leakproof', function_row.proleakproof, \
             'argument_defaults', function_row.pronargdefaults, \
             'variadic', function_row.provariadic::TEXT, \
             'language', language_row.lanname, \
             'identity_arguments', \
              pg_catalog.pg_get_function_identity_arguments(function_row.oid), \
             'result', pg_catalog.pg_get_function_result(function_row.oid), \
             'definition', pg_catalog.pg_get_functiondef(function_row.oid) \
            ) ORDER BY expected.signature \
           )::TEXT, \
           '[]' \
          ) \
         FROM expected \
         LEFT JOIN pg_catalog.pg_proc AS function_row \
          ON function_row.oid = pg_catalog.to_regprocedure(expected.signature) \
         LEFT JOIN pg_catalog.pg_language AS language_row \
          ON language_row.oid = function_row.prolang",
    )
    .bind(&signatures)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(state.0, signatures.len() as i64);
    state
}

async fn product_apply_new_function_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
          ON namespace.oid = function_row.pronamespace \
         WHERE namespace.nspname = 'public' \
          AND function_row.proname IN ( \
           'starring_product_apply_target_artifact_v1', \
           'starring_product_apply_keyring_coverage_v1' \
          )",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_product_apply_scope_migration_rejected_atomically(pool: &PgPool) {
    let before = product_apply_protected_function_catalog_state(pool).await;
    assert_eq!(product_apply_new_function_count(pool).await, 0);
    assert_product_decision_migration_rejected(pool, 202_607_190_022).await;
    assert_eq!(product_apply_new_function_count(pool).await, 0);
    assert_eq!(product_apply_protected_function_catalog_state(pool).await, before);
}

async fn assert_product_apply_scope_migration_rejects_role_atomically(
    pool: &PgPool,
    role: &str,
) {
    let before = product_apply_protected_function_catalog_state(pool).await;
    assert_eq!(product_apply_new_function_count(pool).await, 0);
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(&format!("SET LOCAL ROLE {role}"))
        .execute(&mut *transaction)
        .await
        .unwrap();
    let error = sqlx::raw_sql(
        product_decision_migration(202_607_190_022)
            .sql
            .as_ref(),
    )
    .execute(&mut *transaction)
    .await
    .expect_err("wrong migration role must reject product apply scoping");
    assert!(matches!(
        error,
        sqlx::Error::Database(database_error)
            if database_error.code().as_deref() == Some("55000")
    ));
    transaction.rollback().await.unwrap();
    assert_eq!(product_apply_new_function_count(pool).await, 0);
    assert_eq!(product_apply_protected_function_catalog_state(pool).await, before);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_apply_scope_migration_rejects_drift_atomically_and_normalizes_function_acl() {
    let mut database = isolated_product_control_database("apply_migration_security").await;
    let role_suffix = suffix();
    let wrong_role = format!("starring_apply_wrong_{role_suffix}");
    let split_owner = format!("starring_apply_split_{role_suffix}");
    let hostile_role = format!("starring_apply_hostile_{role_suffix}");
    let roles = [wrong_role.clone(), split_owner.clone(), hostile_role.clone()];

    let outcome = std::panic::AssertUnwindSafe(async {
        apply_product_decision_migrations_through(&database.pool, 202_607_190_021).await;
        for role in &roles {
            sqlx::query(&format!(
                "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
                 NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
            ))
            .execute(&database.pool)
            .await
            .unwrap();
        }

        sqlx::query(&format!(
            "GRANT SELECT ON TABLE public.product_control_plane_identity TO {wrong_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_product_apply_scope_migration_rejects_role_atomically(
            &database.pool,
            &wrong_role,
        )
        .await;
        sqlx::query(&format!(
            "REVOKE SELECT ON TABLE public.product_control_plane_identity FROM {wrong_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER TABLE public.runtime_attestations OWNER TO {split_owner}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_product_apply_scope_migration_rejected_atomically(&database.pool).await;
        sqlx::query("ALTER TABLE public.runtime_attestations OWNER TO CURRENT_USER")
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query("ALTER TABLE public.runtime_serving_leases ENABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();
        assert_product_apply_scope_migration_rejected_atomically(&database.pool).await;
        sqlx::query("ALTER TABLE public.runtime_serving_leases DISABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(
            "ALTER TABLE public.runtime_deployments \
             DISABLE TRIGGER runtime_deployments_policy_shadow_guard",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_product_apply_scope_migration_rejected_atomically(&database.pool).await;
        sqlx::query(
            "ALTER TABLE public.runtime_deployments \
             ENABLE TRIGGER runtime_deployments_policy_shadow_guard",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::raw_sql(
            "DROP TRIGGER runtime_deployments_policy_shadow_guard \
             ON public.runtime_deployments; \
             CREATE TRIGGER runtime_deployments_policy_shadow_guard \
             BEFORE INSERT OR UPDATE ON public.runtime_deployments \
             FOR EACH ROW EXECUTE FUNCTION public.validate_runtime_deployment_projection()",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_product_apply_scope_migration_rejected_atomically(&database.pool).await;
        sqlx::raw_sql(
            "DROP TRIGGER runtime_deployments_policy_shadow_guard \
             ON public.runtime_deployments; \
             CREATE TRIGGER runtime_deployments_policy_shadow_guard \
             BEFORE INSERT OR UPDATE ON public.runtime_deployments \
             FOR EACH ROW EXECUTE FUNCTION public.enforce_runtime_deployment_policy_shadow()",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TRIGGER starring_apply_hostile_trigger \
             AFTER INSERT ON public.runtime_deployments \
             FOR EACH STATEMENT EXECUTE FUNCTION \
              public.validate_runtime_deployment_projection('hostile')",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_product_apply_scope_migration_rejected_atomically(&database.pool).await;
        sqlx::query(
            "DROP TRIGGER starring_apply_hostile_trigger ON public.runtime_deployments",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER FUNCTION {} SECURITY INVOKER",
            PRODUCT_APPLY_SUPPORT_FUNCTIONS[1]
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_product_apply_scope_migration_rejected_atomically(&database.pool).await;
        sqlx::query(&format!(
            "ALTER FUNCTION {} SECURITY DEFINER",
            PRODUCT_APPLY_SUPPORT_FUNCTIONS[1]
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        for function in [
            PRODUCT_APPLY_FUNCTIONS[1],
            PRODUCT_APPLY_SUPPORT_FUNCTIONS[1],
        ] {
            sqlx::query(&format!(
                "GRANT EXECUTE ON FUNCTION {function} TO {hostile_role} WITH GRANT OPTION"
            ))
            .execute(&database.pool)
            .await
            .unwrap();
        }
        sqlx::query(&format!(
            "GRANT SELECT(state) ON TABLE public.activation_requests TO {hostile_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::raw_sql(
            product_decision_migration(202_607_190_022)
                .sql
                .as_ref(),
        )
        .execute(&database.pool)
        .await
        .unwrap();

        assert_eq!(product_apply_new_function_count(&database.pool).await, 2);
        let migrator_role = sqlx::query_scalar::<_, String>("SELECT current_user::TEXT")
            .fetch_one(&database.pool)
            .await
            .unwrap();
        let protected_functions = product_apply_existing_protected_functions()
            .into_iter()
            .chain([
                PRODUCT_APPLY_FUNCTIONS[2].to_owned(),
                PRODUCT_APPLY_FUNCTIONS[4].to_owned(),
            ])
            .collect::<Vec<_>>();
        for function in &protected_functions {
            let contract = sqlx::query_as::<_, (String, bool, bool, i64)>(
                "SELECT pg_catalog.pg_get_userbyid(function_row.proowner), \
                  pg_catalog.has_function_privilege( \
                   pg_catalog.pg_get_userbyid(function_row.proowner), \
                   function_row.oid, \
                   'EXECUTE' \
                  ), \
                  pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE'), \
                  (SELECT pg_catalog.count(*) \
                   FROM pg_catalog.aclexplode(COALESCE( \
                    function_row.proacl, \
                    pg_catalog.acldefault('f', function_row.proowner) \
                   )) AS privilege \
                   WHERE privilege.grantee <> function_row.proowner) \
                 FROM pg_catalog.pg_proc AS function_row \
                 WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
            )
            .bind(function)
            .bind(&hostile_role)
            .fetch_one(&database.pool)
            .await
            .unwrap();
            assert_eq!(contract, (migrator_role.clone(), true, false, 0));
        }
        assert_eq!(protected_functions.len(), 34);

        let relation_grant_preserved = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_column_privilege( \
             $1, 'public.activation_requests', 'state', 'SELECT')",
        )
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(relation_grant_preserved);
    })
    .catch_unwind()
    .await;

    database.pool.close().await;
    let mut cleanup_failure = sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        database.name
    ))
    .execute(&mut database.administrator)
    .await
    .err();
    for role in roles.into_iter().rev() {
        if let Err(error) = sqlx::query(&format!("DROP ROLE IF EXISTS {role}"))
            .execute(&mut database.administrator)
            .await
        {
            cleanup_failure.get_or_insert(error);
        }
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
    if let Some(error) = cleanup_failure {
        panic!("product apply migration security cleanup failed: {error}");
    }
}
