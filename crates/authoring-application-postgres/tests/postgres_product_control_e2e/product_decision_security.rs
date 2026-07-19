const PRODUCT_APPROVAL_FUNCTION: &str = "public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)";
const PRODUCT_APPROVAL_COVERAGE_FUNCTION: &str =
    "public.starring_product_approval_keyring_coverage_v1(text[],text[])";
const PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION: &str =
    "public.starring_product_decision_reader_database_identity_v1()";
const PRODUCT_APPROVAL_TOPOLOGY_FUNCTION: &str =
    "public.starring_product_approval_executor_database_identity_v1()";
const PRODUCT_APPLY_TOPOLOGY_FUNCTION: &str =
    "public.starring_product_apply_executor_database_identity_v1()";
const PRODUCT_APPLY_SHARED_FUNCTIONS: [&str; 3] = [
    "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    "public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)",
];
const PRODUCT_APPROVAL_IMMUTABLE_FUNCTION: &str =
    "public.reject_immutable_product_approval_row()";
const PRODUCT_LEGACY_IMMUTABLE_FUNCTION: &str = "public.reject_immutable_product_row()";
const PRODUCT_APPROVAL_SUPPORT_FUNCTIONS: [&str; 19] = [
    "public.assert_atomic_product_apply_runtime_request()",
    "public.assert_no_committed_product_activation_applying()",
    "public.assert_product_approval_receipt_alias()",
    "public.assert_product_approval_receipt_audit()",
    "public.capture_product_action_receipt_audit_evidence()",
    "public.enforce_activation_approval_payload_binding()",
    "public.enforce_activation_approval_scope()",
    "public.enforce_product_action_receipt_alias_capacity()",
    "public.enforce_product_action_receipt_alias_retention()",
    "public.enforce_product_action_receipt_retention()",
    "public.enforce_product_activation_executor()",
    "public.enforce_product_activation_journal_link()",
    "public.enforce_product_activation_scope()",
    "public.guard_legacy_activation_product_slot()",
    "public.guard_product_activation_applied_record()",
    "public.guard_product_ruleset_artifact_transition()",
    "public.reject_activation_approval_mutation()",
    PRODUCT_APPROVAL_IMMUTABLE_FUNCTION,
    "public.starring_runtime_desired_target_digest_v1(jsonb,bigint)",
];

fn decision_security_keyring() -> ProductDecisionDigestKeyringV1 {
    ProductDecisionDigestKeyringV1::new(
        ProductDecisionDigestKeyV1::from_bytes(
            "product-security-v1",
            std::array::from_fn(|index| 43_u8.wrapping_add(index as u8)),
        )
        .unwrap(),
        [],
    )
    .unwrap()
}

async fn create_decision_login_role(
    pool: &PgPool,
    role: &str,
    password: &str,
    connection_limit: usize,
) {
    let password_literal = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
        .bind(password)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {role} LOGIN PASSWORD {password_literal} \
         NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
         NOBYPASSRLS CONNECTION LIMIT {connection_limit}"
    ))
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_approval_executor_is_exactly_scoped_and_topology_bound() {
    let mut database = isolated_product_control_database("decision_acl").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    let fixture = seed_fixture(&database.pool).await;
    let role_suffix = suffix();
    let owner_role = format!("starring_decision_owner_{role_suffix}");
    let reader_role = format!("starring_decision_reader_{role_suffix}");
    let approval_role = format!("starring_decision_approval_{role_suffix}");
    let apply_role = format!("starring_decision_apply_{role_suffix}");
    let denied_role = format!("starring_decision_denied_{role_suffix}");
    let reader_password = database_role_password();
    let approval_password = database_role_password();
    let apply_password = database_role_password();
    let denied_password = database_role_password();
    for role in [
        &owner_role,
        &reader_role,
        &approval_role,
        &apply_role,
        &denied_role,
    ] {
        assert!(
            role.len() <= 63
                && role
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        );
    }
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
    for relation in [
        "product_control_plane_identity",
        "activation_requests",
        "authoring_promotions",
        "product_tenants",
        "automation_installations",
        "automation_installation_authority_versions",
        "product_principals",
        "product_auth_sessions",
        "product_action_receipts",
        "product_action_receipt_idempotency_aliases",
        "product_audit_events",
        "product_action_receipt_audit_evidence",
        "activation_request_approvals",
        "automation_ruleset_activations",
        "automation_ruleset_versions",
        "runtime_deployments",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    for function in [
        PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION,
        PRODUCT_APPROVAL_TOPOLOGY_FUNCTION,
        PRODUCT_APPLY_TOPOLOGY_FUNCTION,
        PRODUCT_APPROVAL_FUNCTION,
        PRODUCT_APPROVAL_COVERAGE_FUNCTION,
    ]
    .into_iter()
    .chain(PRODUCT_APPROVAL_SUPPORT_FUNCTIONS)
    .chain(PRODUCT_APPLY_SHARED_FUNCTIONS)
    {
        sqlx::query(&format!(
            "ALTER FUNCTION {function} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
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
    for (role, functions) in [
        (
            &reader_role,
            [PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION].as_slice(),
        ),
        (
            &approval_role,
            [
                PRODUCT_APPROVAL_TOPOLOGY_FUNCTION,
                PRODUCT_APPROVAL_FUNCTION,
                PRODUCT_APPROVAL_COVERAGE_FUNCTION,
            ]
            .as_slice(),
        ),
        (
            &apply_role,
            [PRODUCT_APPLY_TOPOLOGY_FUNCTION].as_slice(),
        ),
    ] {
        for function in functions {
            sqlx::query(&format!(
                "GRANT EXECUTE ON FUNCTION {function} TO {role}"
            ))
            .execute(&database.pool)
            .await
            .unwrap();
        }
    }
    let reader_pool =
        database_role_login_pool(&database.name, &reader_role, &reader_password).await;
    let approval_pool =
        database_role_login_pool(&database.name, &approval_role, &approval_password).await;
    let apply_pool = database_role_login_pool(&database.name, &apply_role, &apply_password).await;
    let denied_pool =
        database_role_login_pool(&database.name, &denied_role, &denied_password).await;

    let mut mixed_database = isolated_product_control_database("decision_mix").await;
    MIGRATOR.run(&mixed_database.pool).await.unwrap();
    let mixed_suffix = suffix();
    let mixed_owner_role = format!("starring_decision_mix_owner_{mixed_suffix}");
    let mixed_apply_role = format!("starring_decision_mix_apply_{mixed_suffix}");
    let mixed_apply_password = database_role_password();
    sqlx::query(&format!(
        "CREATE ROLE {mixed_owner_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    create_decision_login_role(
        &mixed_database.pool,
        &mixed_apply_role,
        &mixed_apply_password,
        4,
    )
    .await;
    sqlx::query(&format!(
        "ALTER TABLE public.product_control_plane_identity OWNER TO {mixed_owner_role}"
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "ALTER FUNCTION {PRODUCT_APPLY_TOPOLOGY_FUNCTION} OWNER TO {mixed_owner_role}"
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "REVOKE ALL ON DATABASE {} FROM PUBLIC",
        mixed_database.name
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    sqlx::query("REVOKE ALL ON SCHEMA public FROM PUBLIC")
        .execute(&mixed_database.pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {} TO {mixed_apply_role}",
        mixed_database.name
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {mixed_owner_role}, {mixed_apply_role}"
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {PRODUCT_APPLY_TOPOLOGY_FUNCTION} \
         TO {mixed_apply_role}"
    ))
    .execute(&mixed_database.pool)
    .await
    .unwrap();
    let mixed_apply_pool = database_role_login_pool(
        &mixed_database.name,
        &mixed_apply_role,
        &mixed_apply_password,
    )
    .await;

    let outcome = std::panic::AssertUnwindSafe(async {
        let decisions = PostgresProductDecisions::new(
            ProductDecisionDatabasePoolsV1::new(
                reader_pool.clone(),
                approval_pool.clone(),
                apply_pool.clone(),
            ),
            decision_security_keyring(),
        )
        .unwrap();
        decisions.verify_approval_executor_readiness().await.unwrap();
        decisions.verify_approval_boundary_readiness().await.unwrap();

        let authentication = PostgresAuthentication::new(database.pool.clone());
        let authority = authority_adapter(fixture.clone());
        let deployments = PendingDeployments;
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let request_id = ProductRequestIdV1::parse(&format!("decision.approve.{role_suffix}"))
            .unwrap();
        let idempotency_key = format!("decision-security-{role_suffix}");
        let first = application
            .approve(
                &fixture.credential,
                &fixture.csrf,
                &request_id,
                &selector(&fixture),
                approval_command(&fixture, &idempotency_key),
            )
            .await
            .unwrap();
        assert!(!first.exact_replay());
        let replay = application
            .approve(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse(&format!("decision.replay.{role_suffix}")).unwrap(),
                &selector(&fixture),
                approval_command(&fixture, &idempotency_key),
            )
            .await
            .unwrap();
        assert!(replay.exact_replay());
        assert_eq!(replay.projection(), first.projection());
        decisions.verify_approval_executor_readiness().await.unwrap();

        for statement in [
            "SELECT * FROM public.activation_requests LIMIT 1",
            "INSERT INTO public.activation_request_approvals DEFAULT VALUES",
            "UPDATE public.activation_requests SET state = state",
            "DELETE FROM public.activation_request_approvals",
            "TRUNCATE TABLE public.activation_request_approvals",
            "SELECT public.starring_product_decision_reader_database_identity_v1()",
            "SELECT public.starring_product_apply_executor_database_identity_v1()",
            "SELECT * FROM public.starring_product_session_security_revoke_v1( \
             pg_catalog.decode(pg_catalog.repeat('00', 32), 'hex'))",
        ] {
            assert_database_permission_denied(&approval_pool, statement).await;
        }
        assert_database_permission_denied(
            &approval_pool,
            &format!("CREATE TABLE public.decision_escape_{role_suffix}(value INTEGER)"),
        )
        .await;
        assert_database_permission_denied(
            &approval_pool,
            &format!("CREATE TEMPORARY TABLE decision_escape_{role_suffix}(value INTEGER)"),
        )
        .await;
        assert_database_permission_denied(
            &denied_pool,
            "SELECT public.starring_product_approval_executor_database_identity_v1()",
        )
        .await;
        let denied_approval = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege( \
             current_user, pg_catalog.to_regprocedure($1), 'EXECUTE')",
        )
        .bind(PRODUCT_APPROVAL_FUNCTION)
        .fetch_one(&denied_pool)
        .await
        .unwrap();
        assert!(!denied_approval);

        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {PRODUCT_APPROVAL_FUNCTION} FROM {approval_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::CapabilityMissing)
        );
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {PRODUCT_APPROVAL_FUNCTION} TO {approval_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {PRODUCT_APPLY_TOPOLOGY_FUNCTION} TO {approval_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {PRODUCT_APPLY_TOPOLOGY_FUNCTION} FROM {approval_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {PRODUCT_APPROVAL_FUNCTION} \
             TO {approval_role} WITH GRANT OPTION"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE GRANT OPTION FOR EXECUTE ON FUNCTION {PRODUCT_APPROVAL_FUNCTION} \
             FROM {approval_role} CASCADE"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SELECT(state) ON TABLE public.activation_requests TO {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE SELECT(state) ON TABLE public.activation_requests FROM {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query("ALTER TABLE public.activation_requests ENABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query("ALTER TABLE public.activation_requests DISABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(
            "ALTER TABLE public.activation_requests \
             DISABLE TRIGGER activation_requests_enforce_product_scope",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(
            "ALTER TABLE public.activation_requests \
             ENABLE TRIGGER activation_requests_enforce_product_scope",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::raw_sql(
            "DROP TRIGGER activation_requests_assert_no_product_applying \
             ON public.activation_requests; \
             CREATE CONSTRAINT TRIGGER activation_requests_assert_no_product_applying \
             AFTER INSERT OR UPDATE ON public.activation_requests \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (FALSE) \
             EXECUTE FUNCTION public.assert_no_committed_product_activation_applying()",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::raw_sql(
            "DROP TRIGGER activation_requests_assert_no_product_applying \
             ON public.activation_requests; \
             CREATE CONSTRAINT TRIGGER activation_requests_assert_no_product_applying \
             AFTER INSERT OR UPDATE ON public.activation_requests \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW \
             WHEN (NEW.authority_kind = 'product_authoring' AND NEW.state = 'applying') \
             EXECUTE FUNCTION public.assert_no_committed_product_activation_applying()",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        decisions.verify_approval_executor_readiness().await.unwrap();

        let rogue_relation = format!("decision_rogue_{role_suffix}");
        sqlx::raw_sql(&format!(
            "CREATE TABLE public.{rogue_relation}(value INTEGER); \
             CREATE TRIGGER decision_rogue_capture AFTER INSERT \
             ON public.{rogue_relation} FOR EACH ROW \
             EXECUTE FUNCTION public.capture_product_action_receipt_audit_evidence()"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!("DROP TABLE public.{rogue_relation}"))
            .execute(&database.pool)
            .await
            .unwrap();
        decisions.verify_approval_executor_readiness().await.unwrap();

        sqlx::query(&format!(
            "GRANT CREATE ON SCHEMA public TO {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "REVOKE CREATE ON SCHEMA public FROM {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::query(&format!("ALTER SCHEMA public OWNER TO {denied_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            decisions.verify_approval_executor_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!("ALTER SCHEMA public OWNER TO {owner_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        decisions.verify_approval_boundary_readiness().await.unwrap();

        let mixed = PostgresProductDecisions::new(
            ProductDecisionDatabasePoolsV1::new(
                reader_pool.clone(),
                approval_pool.clone(),
                mixed_apply_pool.clone(),
            ),
            decision_security_keyring(),
        )
        .unwrap();
        assert_eq!(
            mixed.verify_approval_boundary_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
    })
    .catch_unwind()
    .await;

    reader_pool.close().await;
    approval_pool.close().await;
    apply_pool.close().await;
    denied_pool.close().await;
    mixed_apply_pool.close().await;
    database.pool.close().await;
    mixed_database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        mixed_database.name
    ))
    .execute(&mut mixed_database.administrator)
    .await
    .unwrap();
    for role in [
        &denied_role,
        &apply_role,
        &approval_role,
        &reader_role,
        &owner_role,
    ] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
    }
    for role in [&mixed_apply_role, &mixed_owner_role] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut mixed_database.administrator)
            .await
            .unwrap();
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

async fn apply_product_decision_migrations_through(pool: &PgPool, version: i64) {
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= version)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(pool)
            .await
            .unwrap();
    }
}

fn product_decision_migration(version: i64) -> &'static sqlx::migrate::Migration {
    MIGRATOR
        .iter()
        .find(|migration| migration.version == version)
        .unwrap()
}

async fn assert_product_decision_migration_rejected(pool: &PgPool, version: i64) {
    let mut transaction = pool.begin().await.unwrap();
    let error = sqlx::raw_sql(product_decision_migration(version).sql.as_ref())
        .execute(&mut *transaction)
        .await
        .expect_err("invalid product decision contract must reject migration");
    assert!(matches!(
        error,
        sqlx::Error::Database(database_error)
            if database_error.code().as_deref() == Some("55000")
    ));
    transaction.rollback().await.unwrap();
}

async fn approval_function_catalog_state(pool: &PgPool) -> (String, String) {
    sqlx::query_as::<_, (String, String)>(
        "SELECT pg_catalog.string_agg( \
          pg_catalog.pg_get_functiondef(function_row.oid), E'\\n' \
          ORDER BY function_row.oid), \
         pg_catalog.string_agg( \
          COALESCE(function_row.proacl::TEXT, '<null>'), E'\\n' \
          ORDER BY function_row.oid) \
         FROM pg_catalog.pg_proc AS function_row \
         WHERE function_row.oid IN ( \
          pg_catalog.to_regprocedure($1), pg_catalog.to_regprocedure($2))",
    )
    .bind(PRODUCT_APPROVAL_FUNCTION)
    .bind(PRODUCT_APPROVAL_COVERAGE_FUNCTION)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_approval_scope_migration_rejects_owner_drift_atomically() {
    let mut database = isolated_product_control_database("decision_owner").await;
    apply_product_decision_migrations_through(&database.pool, 202_607_190_018).await;
    let split_owner = format!("starring_decision_split_{}", suffix());
    sqlx::query(&format!(
        "CREATE ROLE {split_owner} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "ALTER TABLE public.activation_requests OWNER TO {split_owner}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let before = approval_function_catalog_state(&database.pool).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut transaction = database.pool.begin().await.unwrap();
        let error = sqlx::raw_sql(
            product_decision_migration(202_607_190_019)
                .sql
                .as_ref(),
        )
        .execute(&mut *transaction)
        .await
        .expect_err("split approval relation ownership must reject migration");
        assert!(matches!(
            error,
            sqlx::Error::Database(database_error)
                if database_error.code().as_deref() == Some("55000")
        ));
        transaction.rollback().await.unwrap();
        let topology_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM (VALUES \
              (pg_catalog.to_regprocedure( \
               'public.starring_product_decision_reader_database_identity_v1()')), \
              (pg_catalog.to_regprocedure( \
               'public.starring_product_approval_executor_database_identity_v1()')), \
              (pg_catalog.to_regprocedure( \
               'public.starring_product_apply_executor_database_identity_v1()')) \
             ) AS expected(function_oid) WHERE function_oid IS NOT NULL",
        )
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(topology_count, 0);
        assert_eq!(approval_function_catalog_state(&database.pool).await, before);
    })
    .catch_unwind()
    .await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {split_owner}"))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_approval_scope_migration_restores_deparser_configuration() {
    let mut database = isolated_product_control_database("decision_deparser").await;
    apply_product_decision_migrations_through(&database.pool, 202_607_190_018).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut connection = database.pool.acquire().await.unwrap();
        sqlx::query("SET SESSION quote_all_identifiers = on")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::raw_sql(
            product_decision_migration(202_607_190_019)
                .sql
                .as_ref(),
        )
        .execute(&mut *connection)
        .await
        .unwrap();
        let quote_all_identifiers =
            sqlx::query_scalar::<_, String>(
                "SELECT pg_catalog.current_setting('quote_all_identifiers')",
            )
                .fetch_one(&mut *connection)
                .await
                .unwrap();
        assert_eq!(quote_all_identifiers, "on");
    })
    .catch_unwind()
    .await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_approval_trigger_migration_rejects_manifest_drift_atomically() {
    let mut database = isolated_product_control_database("decision_trigger").await;
    apply_product_decision_migrations_through(&database.pool, 202_607_190_019).await;
    let function_identity = "public.assert_product_approval_receipt_alias()";
    let before = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT pg_catalog.pg_get_functiondef(function_row.oid), \
          function_row.proacl::TEXT \
         FROM pg_catalog.pg_proc AS function_row \
         WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
    )
    .bind(function_identity)
    .fetch_one(&database.pool)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.activation_requests \
         DISABLE TRIGGER activation_requests_enforce_product_scope",
    )
    .execute(&database.pool)
    .await
    .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut transaction = database.pool.begin().await.unwrap();
        let error = sqlx::raw_sql(
            product_decision_migration(202_607_190_020)
                .sql
                .as_ref(),
        )
        .execute(&mut *transaction)
        .await
        .expect_err("disabled approval trigger must reject migration");
        assert!(matches!(
            error,
            sqlx::Error::Database(database_error)
                if database_error.code().as_deref() == Some("55000")
        ));
        transaction.rollback().await.unwrap();
        let after = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT pg_catalog.pg_get_functiondef(function_row.oid), \
              function_row.proacl::TEXT \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(function_identity)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(after, before);
    })
    .catch_unwind()
    .await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_approval_trigger_migration_rejects_semantic_and_global_drift() {
    let mut database = isolated_product_control_database("decision_trigger_semantic").await;
    apply_product_decision_migrations_through(&database.pool, 202_607_190_019).await;
    let before = approval_function_catalog_state(&database.pool).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        sqlx::raw_sql(
            "DROP TRIGGER product_action_receipts_assert_approval_alias \
             ON public.product_action_receipts; \
             CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_alias \
             AFTER INSERT ON public.product_action_receipts NOT DEFERRABLE \
             FOR EACH ROW EXECUTE FUNCTION public.assert_product_approval_receipt_alias()",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_product_decision_migration_rejected(&database.pool, 202_607_190_020).await;
        assert_eq!(approval_function_catalog_state(&database.pool).await, before);

        sqlx::raw_sql(
            "DROP TRIGGER product_action_receipts_assert_approval_alias \
             ON public.product_action_receipts; \
             CREATE CONSTRAINT TRIGGER product_action_receipts_assert_approval_alias \
             AFTER INSERT ON public.product_action_receipts \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW \
             EXECUTE FUNCTION public.assert_product_approval_receipt_alias(); \
             CREATE TABLE public.product_approval_rogue_trigger(value INTEGER); \
             CREATE TRIGGER product_approval_rogue_trigger AFTER INSERT \
             ON public.product_approval_rogue_trigger FOR EACH ROW \
             EXECUTE FUNCTION public.capture_product_action_receipt_audit_evidence()",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_product_decision_migration_rejected(&database.pool, 202_607_190_020).await;
        assert_eq!(approval_function_catalog_state(&database.pool).await, before);
    })
    .catch_unwind()
    .await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_approval_trigger_migration_rejects_split_apply_owner_atomically() {
    let mut database = isolated_product_control_database("decision_apply_owner").await;
    apply_product_decision_migrations_through(&database.pool, 202_607_190_019).await;
    let split_owner = format!("starring_decision_apply_split_{}", suffix());
    sqlx::query(&format!(
        "CREATE ROLE {split_owner} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "ALTER FUNCTION {} OWNER TO {split_owner}",
        PRODUCT_APPLY_SHARED_FUNCTIONS[1]
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let before = approval_function_catalog_state(&database.pool).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        assert_product_decision_migration_rejected(&database.pool, 202_607_190_020).await;
        assert_eq!(approval_function_catalog_state(&database.pool).await, before);
    })
    .catch_unwind()
    .await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {split_owner}"))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_approval_migrations_strip_hostile_grants_and_preserve_relation_acl() {
    let mut database = isolated_product_control_database("decision_grants").await;
    apply_product_decision_migrations_through(&database.pool, 202_607_190_018).await;
    let hostile_role = format!("starring_decision_hostile_{}", suffix());
    sqlx::query(&format!(
        "CREATE ROLE {hostile_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let migrator_role = sqlx::query_scalar::<_, String>("SELECT current_user::TEXT")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    let quoted_migrator = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_ident($1)")
        .bind(&migrator_role)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "ALTER DEFAULT PRIVILEGES FOR ROLE {quoted_migrator} IN SCHEMA public \
         GRANT EXECUTE ON FUNCTIONS TO {hostile_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    for function in [
        PRODUCT_APPROVAL_FUNCTION,
        PRODUCT_APPROVAL_COVERAGE_FUNCTION,
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
    let outcome = std::panic::AssertUnwindSafe(async {
        sqlx::raw_sql(
            product_decision_migration(202_607_190_019)
                .sql
                .as_ref(),
        )
        .execute(&database.pool)
        .await
        .unwrap();
        for function in PRODUCT_APPROVAL_SUPPORT_FUNCTIONS
            .into_iter()
            .filter(|function| *function != PRODUCT_APPROVAL_IMMUTABLE_FUNCTION)
            .chain([PRODUCT_LEGACY_IMMUTABLE_FUNCTION])
        {
            sqlx::query(&format!(
                "GRANT EXECUTE ON FUNCTION {function} TO {hostile_role} WITH GRANT OPTION"
            ))
            .execute(&database.pool)
            .await
            .unwrap();
        }
        sqlx::raw_sql(
            product_decision_migration(202_607_190_020)
                .sql
                .as_ref(),
        )
        .execute(&database.pool)
        .await
        .unwrap();

        let protected_functions = [
            PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION,
            PRODUCT_APPROVAL_TOPOLOGY_FUNCTION,
            PRODUCT_APPLY_TOPOLOGY_FUNCTION,
            PRODUCT_APPROVAL_FUNCTION,
            PRODUCT_APPROVAL_COVERAGE_FUNCTION,
        ]
        .into_iter()
        .chain(PRODUCT_APPROVAL_SUPPORT_FUNCTIONS)
        .collect::<Vec<_>>();
        for function in protected_functions {
            let contract = sqlx::query_as::<_, (bool, i64)>(
                "SELECT pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE'), \
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
            assert_eq!(contract, (false, 0));
        }
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
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {hostile_role}"))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
