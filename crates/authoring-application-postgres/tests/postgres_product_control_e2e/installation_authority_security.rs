fn installation_authority_role_password() -> String {
    let mut material = [0_u8; 32];
    getrandom::fill(&mut material).unwrap();
    lower_hex(&material)
}

async fn installation_authority_login_pool(
    database_name: &str,
    role: &str,
    password: &str,
) -> PgPool {
    assert!(
        !role.is_empty()
            && role.len() <= 63
            && role
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    assert!(password.len() == 64 && password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    PgPoolOptions::new()
        .max_connections(4)
        .connect_with(
            database_url()
                .parse::<PgConnectOptions>()
                .unwrap()
                .database(database_name)
                .username(role)
                .password(password),
        )
        .await
        .unwrap()
}

async fn assert_database_permission_denied(pool: &PgPool, statement: &str) {
    let error = sqlx::query(statement)
        .execute(pool)
        .await
        .expect_err("database capability must be denied");
    assert!(matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("42501")
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn installation_authority_read_is_exactly_scoped_for_a_non_owner_role() {
    let mut database = isolated_product_control_database("authority_acl").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    let fixture = seed_fixture(&database.pool).await;
    let role_suffix = suffix();
    let owner_role = format!("starring_authority_owner_{role_suffix}");
    let api_role = format!("starring_authority_api_{role_suffix}");
    let denied_role = format!("starring_authority_denied_{role_suffix}");
    let api_password = installation_authority_role_password();
    let denied_password = installation_authority_role_password();
    let roles = [&owner_role, &api_role, &denied_role];
    for role in roles {
        assert!(
            role.len() <= 63
                && role
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_')
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
        (&api_role, &api_password),
        (&denied_role, &denied_password),
    ] {
        let password_literal = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.quote_literal($1)",
        )
        .bind(password)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "CREATE ROLE {role} LOGIN PASSWORD {password_literal} \
             NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
             NOBYPASSRLS CONNECTION LIMIT 4"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    for relation in [
        "product_principals",
        "product_auth_sessions",
        "product_tenants",
        "automation_installations",
        "automation_installation_authority_versions",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "ALTER FUNCTION public.starring_product_installation_authority_read_v1( \
         TEXT, TEXT, BYTEA) OWNER TO {owner_role}"
    ))
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
        "GRANT CONNECT ON DATABASE {} TO {api_role}, {denied_role}",
        database.name
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {owner_role}, {api_role}, {denied_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION \
         public.starring_product_installation_authority_read_v1( \
          TEXT, TEXT, BYTEA) TO {api_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let api_pool =
        installation_authority_login_pool(&database.name, &api_role, &api_password).await;
    let denied_pool =
        installation_authority_login_pool(&database.name, &denied_role, &denied_password).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let role_identity = sqlx::query_as::<_, (String, String)>(
            "SELECT current_user::TEXT, session_user::TEXT",
        )
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(role_identity.0, api_role);
        assert_eq!(role_identity.0, role_identity.1);

        let source = PostgresInstallationAuthoritySource::new(api_pool.clone());
        source.verify_readiness().await.unwrap();

        let client_calls = Arc::new(AtomicUsize::new(0));
        let decision_calls = Arc::new(AtomicUsize::new(0));
        let authentication = ClaimsAuthentication {
            claims: authoring_application::AuthenticationClaimsV1::from_authentication(
                fixture.approver_principal.clone(),
                authoring_application::AuthenticatedSessionFingerprintV1::from_sha256_digest(
                    fixture.session_digest,
                ),
            ),
        };
        let authority = postgres_authority_adapter(
            api_pool.clone(),
            fixture.clone(),
            client_calls.clone(),
        );
        let decisions = CapturingPreviewDecisions {
            fixture: fixture.clone(),
            calls: decision_calls.clone(),
        };
        let deployments = PendingDeployments;
        let application =
            ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
        application
            .get_approval_preview(
                &fixture.credential,
                &selector(&fixture),
                status_query(&fixture),
            )
            .await
            .unwrap();
        assert_eq!(client_calls.load(Ordering::SeqCst), 1);
        assert_eq!(decision_calls.load(Ordering::SeqCst), 1);

        for (installation_id, principal_id, session_digest) in [
            (
                format!("missing-installation-{}", suffix()),
                fixture.approver_principal.as_str().to_string(),
                fixture.session_digest,
            ),
            (
                fixture.installation_id.as_str().to_string(),
                format!("missing-principal-{}", suffix()),
                fixture.session_digest,
            ),
            (
                fixture.installation_id.as_str().to_string(),
                fixture.approver_principal.as_str().to_string(),
                [255_u8; 32],
            ),
        ] {
            let rows = sqlx::query_scalar::<_, i64>(
                "SELECT pg_catalog.count(*) \
                 FROM public.starring_product_installation_authority_read_v1($1, $2, $3)",
            )
            .bind(installation_id)
            .bind(principal_id)
            .bind(session_digest.as_slice())
            .fetch_one(&api_pool)
            .await
            .unwrap();
            assert_eq!(rows, 0);
        }

        let direct_privilege_count = sqlx::query_scalar::<_, i64>(
            "WITH relations(name) AS (VALUES \
              ('public.product_principals'), \
              ('public.product_auth_sessions'), \
              ('public.product_tenants'), \
              ('public.automation_installations'), \
              ('public.automation_installation_authority_versions') \
             ), privileges(name) AS (VALUES \
              ('SELECT'), ('INSERT'), ('UPDATE'), ('DELETE'), \
              ('TRUNCATE'), ('REFERENCES'), ('TRIGGER') \
             ) \
             SELECT pg_catalog.count(*) \
             FROM relations CROSS JOIN privileges \
             WHERE pg_catalog.has_table_privilege( \
              current_user, pg_catalog.to_regclass(relations.name), privileges.name)",
        )
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(direct_privilege_count, 0);
        let column_privilege_count = sqlx::query_scalar::<_, i64>(
            "WITH relations(name) AS (VALUES \
              ('public.product_principals'), \
              ('public.product_auth_sessions'), \
              ('public.product_tenants'), \
              ('public.automation_installations'), \
              ('public.automation_installation_authority_versions') \
             ), privileges(name) AS (VALUES \
              ('SELECT'), ('INSERT'), ('UPDATE'), ('REFERENCES') \
             ) \
             SELECT pg_catalog.count(*) \
             FROM relations CROSS JOIN privileges \
             WHERE pg_catalog.has_any_column_privilege( \
              current_user, pg_catalog.to_regclass(relations.name), privileges.name)",
        )
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(column_privilege_count, 0);
        for statement in [
            "SELECT principal_id FROM public.product_principals LIMIT 1",
            "INSERT INTO public.product_tenants (tenant_id, lifecycle_state, display_name) \
             VALUES ('forbidden', 'active', 'forbidden')",
            "UPDATE public.automation_installations \
             SET lifecycle_state = lifecycle_state WHERE FALSE",
            "DELETE FROM public.automation_installation_authority_versions WHERE FALSE",
            "CREATE TABLE public.forbidden_authority_table (value INTEGER)",
            "CREATE TEMPORARY TABLE forbidden_authority_temp (value INTEGER)",
            "SELECT * FROM public.starring_product_approval_keyring_coverage_v1( \
             ARRAY[]::TEXT[], ARRAY[]::TEXT[])",
            "SELECT * FROM public.starring_purge_product_identity_v1(1)",
            "SELECT public.starring_runtime_mutation_clock()",
        ] {
            assert_database_permission_denied(&api_pool, statement).await;
        }
        assert_database_permission_denied(
            &denied_pool,
            "SELECT * FROM public.starring_product_installation_authority_read_v1( \
             'missing', 'missing', decode(repeat('00', 32), 'hex'))",
        )
        .await;

        sqlx::query(&format!(
            "GRANT SELECT (discord_user_id) \
             ON public.product_principals TO {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        let column_grant = sqlx::query_as::<_, (bool, bool)>(
            "SELECT pg_catalog.has_table_privilege( \
              current_user, 'public.product_principals', 'SELECT'), \
             pg_catalog.has_any_column_privilege( \
              current_user, 'public.product_principals', 'SELECT')",
        )
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(column_grant, (false, true));
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE SELECT (discord_user_id) \
             ON public.product_principals FROM {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION \
             public.starring_product_installation_authority_read_v1( \
              TEXT, TEXT, BYTEA) TO {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION \
             public.starring_product_installation_authority_read_v1( \
              TEXT, TEXT, BYTEA) FROM {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!("ALTER ROLE {denied_role} CREATEROLE"))
            .execute(&database.pool)
            .await
            .unwrap();
        sqlx::query(&format!("GRANT {denied_role} TO {api_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!("REVOKE {denied_role} FROM {api_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        sqlx::query(&format!("ALTER ROLE {denied_role} NOCREATEROLE"))
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(&format!("GRANT {api_role} TO {denied_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!("REVOKE {api_role} FROM {denied_role}"))
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(&format!("GRANT {owner_role} TO {denied_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!("REVOKE {owner_role} FROM {denied_role}"))
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(&format!("GRANT {denied_role} TO {owner_role}"))
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!("REVOKE {denied_role} FROM {owner_role}"))
            .execute(&database.pool)
            .await
            .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION \
             public.starring_product_installation_authority_read_v1( \
              TEXT, TEXT, BYTEA) TO {api_role} WITH GRANT OPTION"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE GRANT OPTION FOR EXECUTE ON FUNCTION \
             public.starring_product_installation_authority_read_v1( \
              TEXT, TEXT, BYTEA) FROM {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION \
             public.starring_product_installation_authority_read_v1( \
              TEXT, TEXT, BYTEA) FROM {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::CapabilityMissing)
        );
        let revoked_client_calls = Arc::new(AtomicUsize::new(0));
        let revoked_authentication = ClaimsAuthentication {
            claims: authoring_application::AuthenticationClaimsV1::from_authentication(
                fixture.approver_principal.clone(),
                authoring_application::AuthenticatedSessionFingerprintV1::from_sha256_digest(
                    fixture.session_digest,
                ),
            ),
        };
        let revoked_authority = postgres_authority_adapter(
            api_pool.clone(),
            fixture.clone(),
            revoked_client_calls.clone(),
        );
        let revoked_decisions = NeverDecisions;
        let revoked_application = ProductControlApplication::new(
            &revoked_authentication,
            &revoked_authority,
            &revoked_decisions,
            &deployments,
        );
        assert_eq!(
            revoked_application
                .get_approval_preview(
                    &fixture.credential,
                    &selector(&fixture),
                    status_query(&fixture),
                )
                .await,
            Err(ProductApplicationError::FreshAuthority(
                authoring_application::FreshGuildAuthorityError::Backend(
                    "installation_authority_unavailable".to_string()
                )
            ))
        );
        assert_eq!(revoked_client_calls.load(Ordering::SeqCst), 0);
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION \
             public.starring_product_installation_authority_read_v1( \
              TEXT, TEXT, BYTEA) TO {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(
            "ALTER FUNCTION public.starring_product_installation_authority_read_v1( \
             TEXT, TEXT, BYTEA) SECURITY INVOKER",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(
            "ALTER FUNCTION public.starring_product_installation_authority_read_v1( \
             TEXT, TEXT, BYTEA) SECURITY DEFINER",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(
            "GRANT EXECUTE ON FUNCTION \
             public.starring_product_installation_authority_read_v1( \
              TEXT, TEXT, BYTEA) TO PUBLIC",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            source.verify_readiness().await,
            Err(InstallationAuthorityReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(
            "REVOKE EXECUTE ON FUNCTION \
             public.starring_product_installation_authority_read_v1( \
              TEXT, TEXT, BYTEA) FROM PUBLIC",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        source.verify_readiness().await.unwrap();
    })
    .catch_unwind()
    .await;
    api_pool.close().await;
    denied_pool.close().await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    for role in [&denied_role, &api_role, &owner_role] {
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
async fn installation_authority_read_migration_rejects_split_ownership() {
    let mut database = isolated_product_control_database("authority_owner").await;
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 202_607_190_013)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();
    }
    let split_owner = format!("starring_authority_split_{}", suffix());
    assert!(
        split_owner.len() <= 63
            && split_owner
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    sqlx::query(&format!(
        "CREATE ROLE {split_owner} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        sqlx::query(&format!(
            "ALTER TABLE public.product_tenants OWNER TO {split_owner}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_014)
            .unwrap();
        let mut transaction = database.pool.begin().await.unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .expect_err("split relation ownership must reject the migration");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("55000"));
        assert_eq!(
            database_error.message(),
            "installation authority relations require one owner"
        );
        transaction.rollback().await.unwrap();
        let function_exists = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.to_regprocedure( \
             'public.starring_product_installation_authority_read_v1(text,text,bytea)') \
             IS NOT NULL",
        )
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(!function_exists);
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
async fn installation_authority_read_migration_strips_hostile_default_grants() {
    let mut database = isolated_product_control_database("authority_grants").await;
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 202_607_190_013)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();
    }
    let role_suffix = suffix();
    let owner_role = format!("starring_authority_owner_{role_suffix}");
    let migrator_role = format!("starring_authority_migrator_{role_suffix}");
    let hostile_role = format!("starring_authority_hostile_{role_suffix}");
    let migrator_password = installation_authority_role_password();
    for role in [&owner_role, &migrator_role, &hostile_role] {
        assert!(
            role.len() <= 63
                && role
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_')
        );
    }
    sqlx::query(&format!(
        "CREATE ROLE {owner_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {hostile_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let password_literal =
        sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
            .bind(&migrator_password)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {migrator_role} LOGIN PASSWORD {password_literal} \
         NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
         NOBYPASSRLS CONNECTION LIMIT 2"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    for relation in [
        "product_principals",
        "product_auth_sessions",
        "product_tenants",
        "automation_installations",
        "automation_installation_authority_versions",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {} TO {migrator_role}",
        database.name
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE, CREATE ON SCHEMA public TO {owner_role}, {migrator_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!("GRANT {owner_role} TO {migrator_role}"))
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "ALTER DEFAULT PRIVILEGES FOR ROLE {migrator_role} IN SCHEMA public \
         GRANT EXECUTE ON FUNCTIONS TO {hostile_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let migrator_pool = installation_authority_login_pool(
        &database.name,
        &migrator_role,
        &migrator_password,
    )
    .await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let identity = sqlx::query_as::<_, (String, String)>(
            "SELECT current_user::TEXT, session_user::TEXT",
        )
        .fetch_one(&migrator_pool)
        .await
        .unwrap();
        assert_eq!(identity, (migrator_role.clone(), migrator_role.clone()));
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_014)
            .unwrap();
        let mut transaction = migrator_pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let function_contract = sqlx::query_as::<_, (String, bool, bool, bool)>(
            "SELECT pg_catalog.pg_get_userbyid(function_row.proowner), \
              function_row.prosecdef, \
              pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE'), \
              EXISTS ( \
               SELECT 1 \
               FROM pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, \
                pg_catalog.acldefault('f', function_row.proowner) \
               )) AS privilege \
               WHERE privilege.grantee = 0 \
                AND privilege.privilege_type = 'EXECUTE' \
              ) \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind("public.starring_product_installation_authority_read_v1(text,text,bytea)")
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            function_contract,
            (owner_role.clone(), true, false, false)
        );
        let unexpected_grants = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_proc AS function_row \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
              function_row.proacl, \
              pg_catalog.acldefault('f', function_row.proowner) \
             )) AS privilege \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
              AND privilege.grantee <> function_row.proowner",
        )
        .bind("public.starring_product_installation_authority_read_v1(text,text,bytea)")
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(unexpected_grants, 0);
    })
    .catch_unwind()
    .await;
    migrator_pool.close().await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    sqlx::query(&format!(
        "REVOKE {owner_role} FROM {migrator_role}"
    ))
    .execute(&mut database.administrator)
    .await
    .unwrap();
    for role in [&hostile_role, &migrator_role, &owner_role] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
