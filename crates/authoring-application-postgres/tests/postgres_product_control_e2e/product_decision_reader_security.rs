const PRODUCT_DECISION_READER_RELATIONS: [&str; 12] = [
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
    "runtime_deployments",
];

#[derive(Clone)]
struct ProductDecisionReaderInput {
    promotion_id: String,
    tenant_id: String,
    installation_id: String,
    guild_id: String,
    principal_id: String,
    acting_user_id: String,
    session_digest: Vec<u8>,
}

async fn product_decision_reader_row_count(
    pool: &PgPool,
    input: &ProductDecisionReaderInput,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.starring_product_decision_read_v1( \
          $1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&input.promotion_id)
    .bind(&input.tenant_id)
    .bind(&input.installation_id)
    .bind(&input.guild_id)
    .bind(&input.principal_id)
    .bind(&input.acting_user_id)
    .bind(input.session_digest.as_slice())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_decision_reader_is_function_scoped_non_enumerating_and_fail_closed() {
    let mut database = isolated_product_control_database("decision_reader").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    let fixture = seed_fixture(&database.pool).await;
    let role_suffix = suffix();
    let owner_role = format!("starring_reader_owner_{role_suffix}");
    let reader_role = format!("starring_reader_api_{role_suffix}");
    let denied_role = format!("starring_reader_denied_{role_suffix}");
    let reader_password = database_role_password();
    let denied_password = database_role_password();
    sqlx::query(&format!(
        "CREATE ROLE {owner_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    create_decision_login_role(&database.pool, &reader_role, &reader_password, 4).await;
    create_decision_login_role(&database.pool, &denied_role, &denied_password, 4).await;
    for relation in PRODUCT_DECISION_READER_RELATIONS {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    for function in [
        PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION,
        PRODUCT_DECISION_READ_FUNCTION,
    ] {
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
        "GRANT CONNECT ON DATABASE {} TO {reader_role}, {denied_role}",
        database.name
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {owner_role}, {reader_role}, {denied_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    for function in [
        PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION,
        PRODUCT_DECISION_READ_FUNCTION,
    ] {
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {function} TO {reader_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    let reader_pool =
        database_role_login_pool(&database.name, &reader_role, &reader_password).await;
    let denied_pool =
        database_role_login_pool(&database.name, &denied_role, &denied_password).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let decisions = PostgresProductDecisions::new(
            ProductDecisionDatabasePoolsV1::new(
                reader_pool.clone(),
                database.pool.clone(),
                database.pool.clone(),
            ),
            decision_security_keyring(),
        )
        .unwrap();
        decisions.verify_decision_reader_readiness().await.unwrap();

        let authentication = ClaimsAuthentication {
            claims: authoring_application::AuthenticationClaimsV1::from_authentication(
                fixture.approver_principal.clone(),
                authoring_application::AuthenticatedSessionFingerprintV1::from_sha256_digest(
                    fixture.session_digest,
                ),
            ),
        };
        let authority = authority_adapter(fixture.clone());
        let deployments = PendingDeployments;
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        let preview = application
            .get_approval_preview(
                &fixture.credential,
                &selector(&fixture),
                status_query(&fixture),
            )
            .await
            .unwrap();
        assert_eq!(preview.installation_id(), &fixture.installation_id);
        assert_eq!(preview.guild_id(), fixture.guild_id);
        assert_eq!(preview.payload(), &fixture.payload);
        assert_eq!(preview.payload_digest().as_str(), fixture.payload_digest);
        assert_eq!(preview.revision().get(), 1);
        assert_eq!(preview.phase(), &ProductDecisionPhaseV1::PendingApproval);
        assert_eq!(
            application
                .get_product_status(
                    &fixture.credential,
                    &selector(&fixture),
                    status_query(&fixture),
                )
                .await
                .unwrap(),
            ProductStatusV1::PendingApproval
        );

        sqlx::query(
            "UPDATE public.product_tenants \
             SET lifecycle_state = 'suspended', updated_at = pg_catalog.clock_timestamp() \
             WHERE tenant_id = $1",
        )
        .bind(fixture.tenant_id.as_str())
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            application
                .get_product_status(
                    &fixture.credential,
                    &selector(&fixture),
                    status_query(&fixture),
                )
                .await
                .unwrap_err(),
            ProductApplicationError::Control(ProductControlPortError::InvalidState)
        );
        sqlx::query(
            "UPDATE public.product_tenants \
             SET lifecycle_state = 'active', updated_at = pg_catalog.clock_timestamp() \
             WHERE tenant_id = $1",
        )
        .bind(fixture.tenant_id.as_str())
        .execute(&database.pool)
        .await
        .unwrap();

        let valid = ProductDecisionReaderInput {
            promotion_id: fixture.promotion_id.as_str().to_string(),
            tenant_id: fixture.tenant_id.as_str().to_string(),
            installation_id: fixture.installation_id.as_str().to_string(),
            guild_id: fixture.guild_id.to_string(),
            principal_id: fixture.approver_principal.as_str().to_string(),
            acting_user_id: fixture.approver_user.to_string(),
            session_digest: fixture.session_digest.to_vec(),
        };
        assert_eq!(
            product_decision_reader_row_count(&reader_pool, &valid).await,
            1
        );
        let wrong_inputs = [
            ProductDecisionReaderInput {
                promotion_id: "f".repeat(64),
                tenant_id: valid.tenant_id.clone(),
                installation_id: valid.installation_id.clone(),
                guild_id: valid.guild_id.clone(),
                principal_id: valid.principal_id.clone(),
                acting_user_id: valid.acting_user_id.clone(),
                session_digest: valid.session_digest.clone(),
            },
            ProductDecisionReaderInput {
                promotion_id: valid.promotion_id.clone(),
                tenant_id: "missing_tenant".to_string(),
                installation_id: valid.installation_id.clone(),
                guild_id: valid.guild_id.clone(),
                principal_id: valid.principal_id.clone(),
                acting_user_id: valid.acting_user_id.clone(),
                session_digest: valid.session_digest.clone(),
            },
            ProductDecisionReaderInput {
                promotion_id: valid.promotion_id.clone(),
                tenant_id: valid.tenant_id.clone(),
                installation_id: "missing_installation".to_string(),
                guild_id: valid.guild_id.clone(),
                principal_id: valid.principal_id.clone(),
                acting_user_id: valid.acting_user_id.clone(),
                session_digest: valid.session_digest.clone(),
            },
            ProductDecisionReaderInput {
                promotion_id: valid.promotion_id.clone(),
                tenant_id: valid.tenant_id.clone(),
                installation_id: valid.installation_id.clone(),
                guild_id: "18446744073709551615".to_string(),
                principal_id: valid.principal_id.clone(),
                acting_user_id: valid.acting_user_id.clone(),
                session_digest: valid.session_digest.clone(),
            },
            ProductDecisionReaderInput {
                promotion_id: valid.promotion_id.clone(),
                tenant_id: valid.tenant_id.clone(),
                installation_id: valid.installation_id.clone(),
                guild_id: valid.guild_id.clone(),
                principal_id: "missing_principal".to_string(),
                acting_user_id: valid.acting_user_id.clone(),
                session_digest: valid.session_digest.clone(),
            },
            ProductDecisionReaderInput {
                promotion_id: valid.promotion_id.clone(),
                tenant_id: valid.tenant_id.clone(),
                installation_id: valid.installation_id.clone(),
                guild_id: valid.guild_id.clone(),
                principal_id: valid.principal_id.clone(),
                acting_user_id: "18446744073709551615".to_string(),
                session_digest: valid.session_digest.clone(),
            },
            ProductDecisionReaderInput {
                promotion_id: valid.promotion_id.clone(),
                tenant_id: valid.tenant_id.clone(),
                installation_id: valid.installation_id.clone(),
                guild_id: valid.guild_id.clone(),
                principal_id: valid.principal_id.clone(),
                acting_user_id: valid.acting_user_id.clone(),
                session_digest: vec![241_u8; 32],
            },
        ];
        for input in wrong_inputs {
            assert_eq!(
                product_decision_reader_row_count(&reader_pool, &input).await,
                0
            );
        }
        let malformed_inputs = [
            ProductDecisionReaderInput {
                promotion_id: "F".repeat(64),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                promotion_id: "a".repeat(63),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                tenant_id: "bad tenant".to_string(),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                installation_id: "a".repeat(129),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                principal_id: String::new(),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                guild_id: "0".to_string(),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                guild_id: "01".to_string(),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                guild_id: "18446744073709551616".to_string(),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                acting_user_id: "0001".to_string(),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                acting_user_id: "100000000000000000000".to_string(),
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                session_digest: vec![17_u8; 31],
                ..valid.clone()
            },
            ProductDecisionReaderInput {
                session_digest: vec![17_u8; 33],
                ..valid.clone()
            },
        ];
        for input in malformed_inputs {
            assert_eq!(
                product_decision_reader_row_count(&reader_pool, &input).await,
                0
            );
        }
        let null_rows = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.starring_product_decision_read_v1( \
              NULL, 'probe_tenant', 'probe_installation', '1', \
              'probe_principal', '1', pg_catalog.decode(pg_catalog.repeat('00', 32), 'hex'))",
        )
        .fetch_one(&reader_pool)
        .await
        .unwrap();
        assert_eq!(null_rows, 0);

        let missing_promotion = PromotionId::parse(&"f".repeat(64)).unwrap();
        assert_eq!(
            application
                .get_product_status(
                    &fixture.credential,
                    &selector(&fixture),
                    ProductStatusQueryV1 {
                        promotion: PromotionSelectorV1::new(missing_promotion),
                    },
                )
                .await
                .unwrap_err(),
            ProductApplicationError::Control(ProductControlPortError::NotFound)
        );

        let bounded_decisions = PostgresProductDecisions::with_config(
            ProductDecisionDatabasePoolsV1::new(
                reader_pool.clone(),
                database.pool.clone(),
                database.pool.clone(),
            ),
            authoring_application_postgres::PostgresProductDecisionsConfig::new(
                decision_security_keyring(),
                Duration::from_millis(200),
                Duration::from_millis(50),
            )
            .unwrap(),
        );
        let bounded_authority = authority_adapter(fixture.clone());
        let bounded_application = ProductControlApplication::new(
            &authentication,
            &bounded_authority,
            &bounded_decisions,
            &deployments,
        );
        let mut blocker = database.pool.begin().await.unwrap();
        sqlx::query("LOCK TABLE public.activation_requests IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let blocked = tokio::time::timeout(
            Duration::from_secs(2),
            bounded_application.get_product_status(
                &fixture.credential,
                &selector(&fixture),
                status_query(&fixture),
            ),
        )
        .await
        .expect("bounded reader must not hang")
        .unwrap_err();
        assert_eq!(
            blocked,
            ProductApplicationError::Control(ProductControlPortError::Backend(
                "product database request timed out".to_string()
            ))
        );
        blocker.rollback().await.unwrap();

        sqlx::query(
            "UPDATE public.product_auth_sessions \
             SET revoked_at = pg_catalog.clock_timestamp(), \
              revocation_reason = 'authority_revalidation' \
             WHERE session_digest = $1",
        )
        .bind(fixture.session_digest.as_slice())
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            application
                .get_product_status(
                    &fixture.credential,
                    &selector(&fixture),
                    status_query(&fixture),
                )
                .await
                .unwrap_err(),
            ProductApplicationError::Control(ProductControlPortError::InvalidState)
        );

        for statement in [
            "SELECT * FROM public.activation_requests LIMIT 1",
            "UPDATE public.activation_requests SET state = state",
            "DELETE FROM public.activation_request_approvals",
            "TRUNCATE TABLE public.activation_request_approvals",
            "SELECT public.starring_product_approval_executor_database_identity_v1()",
        ] {
            assert_database_permission_denied(&reader_pool, statement).await;
        }
        assert_database_permission_denied(
            &reader_pool,
            &format!("CREATE TABLE public.reader_escape_{role_suffix}(value INTEGER)"),
        )
        .await;
        assert_database_permission_denied(
            &reader_pool,
            &format!("CREATE TEMPORARY TABLE reader_escape_{role_suffix}(value INTEGER)"),
        )
        .await;
        assert_database_permission_denied(
            &denied_pool,
            "SELECT * FROM public.starring_product_decision_read_v1( \
             pg_catalog.repeat('0', 64), 'probe_tenant', 'probe_installation', \
             '1', 'probe_principal', '1', \
             pg_catalog.decode(pg_catalog.repeat('00', 32), 'hex'))",
        )
        .await;

        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {PRODUCT_DECISION_READ_FUNCTION} FROM {reader_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_decision_reader_readiness().await,
            Err(ProductDecisionReadinessErrorV1::CapabilityMissing)
        );
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {PRODUCT_DECISION_READ_FUNCTION} TO {reader_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {PRODUCT_APPROVAL_TOPOLOGY_FUNCTION} TO {reader_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_decision_reader_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {PRODUCT_APPROVAL_TOPOLOGY_FUNCTION} FROM {reader_role}"
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
            decisions.verify_decision_reader_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE SELECT(state) ON TABLE public.activation_requests FROM {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query("ALTER TABLE public.authoring_sessions ENABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            decisions.verify_decision_reader_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query("ALTER TABLE public.authoring_sessions DISABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(&format!("GRANT CREATE ON SCHEMA public TO {denied_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            decisions.verify_decision_reader_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!("REVOKE CREATE ON SCHEMA public FROM {denied_role}"))
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {PRODUCT_DECISION_READ_FUNCTION} TO PUBLIC"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_decision_reader_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {PRODUCT_DECISION_READ_FUNCTION} FROM PUBLIC"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER FUNCTION {PRODUCT_DECISION_READ_FUNCTION} STABLE"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_decision_reader_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "ALTER FUNCTION {PRODUCT_DECISION_READ_FUNCTION} VOLATILE"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER TABLE public.authoring_sessions OWNER TO {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            decisions.verify_decision_reader_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "ALTER TABLE public.authoring_sessions OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        decisions.verify_decision_reader_readiness().await.unwrap();

        sqlx::raw_sql(
            "ALTER TABLE public.activation_requests DISABLE TRIGGER USER; \
             ALTER TABLE public.activation_requests \
              DROP CONSTRAINT activation_requests_product_context_valid, \
              DROP CONSTRAINT activation_requests_product_scope_valid",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE public.activation_requests \
             SET authority_kind = 'legacy_manual', \
              approval_context = pg_catalog.jsonb_set( \
               approval_context, '{authority}', \
               pg_catalog.to_jsonb('legacy_manual'::TEXT)) \
             WHERE id = $1",
        )
        .bind(&fixture.activation_id)
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            product_decision_reader_row_count(&reader_pool, &valid).await,
            0
        );
    })
    .catch_unwind()
    .await;

    reader_pool.close().await;
    denied_pool.close().await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    for role in [&denied_role, &reader_role, &owner_role] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_decision_reader_migration_seals_hostile_acl_and_restores_session_state() {
    let mut database = isolated_product_control_database("reader_migrate").await;
    apply_product_decision_migrations_through(&database.pool, 202_607_190_020).await;
    let hostile_role = format!("starring_reader_hostile_{}", suffix());
    sqlx::query(&format!(
        "CREATE ROLE {hostile_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION} \
         TO {hostile_role} WITH GRANT OPTION"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "ALTER DEFAULT PRIVILEGES GRANT EXECUTE ON FUNCTIONS \
         TO {hostile_role} WITH GRANT OPTION"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT SELECT(state) ON TABLE public.activation_requests TO {hostile_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();

    let outcome = std::panic::AssertUnwindSafe(async {
        let mut connection = database.pool.acquire().await.unwrap();
        sqlx::query("SET SESSION search_path = pg_catalog, public")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query("SET SESSION quote_all_identifiers = on")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::raw_sql(
            product_decision_migration(202_607_190_021)
                .sql
                .as_ref(),
        )
        .execute(&mut *connection)
        .await
        .unwrap();
        let settings = sqlx::query_as::<_, (String, String)>(
            "SELECT pg_catalog.current_setting('search_path'), \
             pg_catalog.current_setting('quote_all_identifiers')",
        )
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        assert_eq!(settings.0, "pg_catalog, public");
        assert_eq!(settings.1, "on");
        drop(connection);

        let metadata_valid = sqlx::query_scalar::<_, bool>(
            "SELECT function_row.prokind = 'f' \
              AND function_row.pronargs = 7 \
              AND pg_catalog.cardinality(function_row.proallargtypes) \
               - function_row.pronargs = 49 \
              AND function_row.provolatile = 'v' \
              AND function_row.proisstrict \
              AND function_row.proparallel = 'u' \
              AND function_row.prosecdef \
              AND function_row.proretset \
              AND function_row.prorows = 1 \
              AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[] \
              AND function_row.proowner = relation.relowner \
              AND NOT EXISTS ( \
               SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
               )) AS privilege WHERE privilege.grantee <> function_row.proowner \
              ) \
             FROM pg_catalog.pg_proc AS function_row \
             INNER JOIN pg_catalog.pg_class AS relation \
              ON relation.oid = pg_catalog.to_regclass('public.activation_requests') \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(PRODUCT_DECISION_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(metadata_valid);
        let topology_acl_sealed = sqlx::query_scalar::<_, bool>(
            "SELECT NOT EXISTS ( \
              SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
               function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
              )) AS privilege WHERE privilege.grantee <> function_row.proowner \
             ) FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(PRODUCT_DECISION_READER_TOPOLOGY_FUNCTION)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(topology_acl_sealed);
        assert!(
            sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_column_privilege($1, \
                 'public.activation_requests', 'state', 'SELECT')",
            )
            .bind(&hostile_role)
            .fetch_one(&database.pool)
            .await
            .unwrap()
        );
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

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_decision_reader_migration_rejects_owner_and_rls_drift_atomically() {
    let mut database = isolated_product_control_database("reader_atomic").await;
    apply_product_decision_migrations_through(&database.pool, 202_607_190_020).await;
    let split_owner = format!("starring_reader_split_{}", suffix());
    sqlx::query(&format!(
        "CREATE ROLE {split_owner} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let original_owner = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.quote_ident(pg_catalog.pg_get_userbyid(relation.relowner)) \
         FROM pg_catalog.pg_class AS relation \
         WHERE relation.oid = pg_catalog.to_regclass('public.authoring_sessions')",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();

    let outcome = std::panic::AssertUnwindSafe(async {
        let mut wrong_role = database.pool.begin().await.unwrap();
        sqlx::query(&format!("SET LOCAL ROLE {split_owner}"))
            .execute(&mut *wrong_role)
            .await
            .unwrap();
        let wrong_role_error = sqlx::raw_sql(
            product_decision_migration(202_607_190_021)
                .sql
                .as_ref(),
        )
        .execute(&mut *wrong_role)
        .await
        .expect_err("migration must execute as the common object owner");
        assert!(matches!(
            wrong_role_error,
            sqlx::Error::Database(database_error)
                if database_error.code().as_deref() == Some("55000")
        ));
        wrong_role.rollback().await.unwrap();
        assert!(
            sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regprocedure($1) IS NULL")
                .bind(PRODUCT_DECISION_READ_FUNCTION)
                .fetch_one(&database.pool)
                .await
                .unwrap()
        );

        sqlx::query(&format!(
            "ALTER TABLE public.authoring_sessions OWNER TO {split_owner}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_product_decision_migration_rejected(&database.pool, 202_607_190_021).await;
        assert!(
            sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regprocedure($1) IS NULL")
            .bind(PRODUCT_DECISION_READ_FUNCTION)
            .fetch_one(&database.pool)
            .await
            .unwrap()
        );
        sqlx::query(&format!(
            "ALTER TABLE public.authoring_sessions OWNER TO {original_owner}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query("ALTER TABLE public.runtime_deployments ENABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();
        assert_product_decision_migration_rejected(&database.pool, 202_607_190_021).await;
        assert!(
            sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.to_regprocedure($1) IS NULL",
            )
            .bind(PRODUCT_DECISION_READ_FUNCTION)
            .fetch_one(&database.pool)
            .await
            .unwrap()
        );
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
