use futures::FutureExt as _;
use sqlx::Connection as _;

const SNAPSHOT_FUNCTION: &str =
    "public.starring_product_authorized_snapshot_read_v2(TEXT, TEXT, BYTEA, TEXT, TEXT)";
const SNAPSHOT_FUNCTION_IDENTITY: &str =
    "public.starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)";
const SNAPSHOT_DATABASE_IDENTITY_FUNCTION: &str =
    "public.starring_product_authorized_snapshot_reader_database_identity_v1()";
const SNAPSHOT_KEY_COVERAGE_FUNCTION: &str =
    "public.starring_product_authorized_snapshot_key_coverage_v1(TEXT[])";
const SNAPSHOT_RELATIONS: [&str; 7] = [
    "product_principals",
    "product_auth_sessions",
    "product_tenants",
    "automation_installations",
    "authoring_sessions",
    "authoring_session_generations",
    "automation_installation_authority_versions",
];
const SNAPSHOT_READINESS_RELATIONS: [&str; 8] = [
    "product_control_plane_identity",
    "product_principals",
    "product_auth_sessions",
    "product_tenants",
    "automation_installations",
    "authoring_sessions",
    "authoring_session_generations",
    "automation_installation_authority_versions",
];

struct SnapshotSecurityDatabase {
    name: String,
    administrator: sqlx::postgres::PgConnection,
    pool: PgPool,
}

struct SnapshotSecurityAuthentication {
    claims: AuthenticationClaimsV1,
}

impl AuthenticationPort for SnapshotSecurityAuthentication {
    type Credential = str;

    async fn authenticate(
        &self,
        _credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, authoring_application::AuthenticationError> {
        Ok(self.claims.clone())
    }
}

impl MutationAuthenticationPort for SnapshotSecurityAuthentication {
    type CsrfProof = str;

    async fn authenticate_mutation(
        &self,
        _credential: &Self::Credential,
        _csrf: &Self::CsrfProof,
    ) -> Result<AuthenticationClaimsV1, authoring_application::AuthenticationError> {
        Ok(self.claims.clone())
    }
}

async fn isolated_snapshot_security_database(label: &str) -> SnapshotSecurityDatabase {
    assert!(
        !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let name = format!("starring_snapshot_{label}_test_{}", unique_suffix());
    assert!(
        name.len() <= 63
            && name.starts_with("starring_")
            && name.split('_').any(|segment| segment == "test")
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let base = database_url().parse::<PgConnectOptions>().unwrap();
    let mut administrator =
        sqlx::postgres::PgConnection::connect_with(&base.clone().database("postgres"))
            .await
            .unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(base.database(&name))
        .await
        .unwrap();
    SnapshotSecurityDatabase {
        name,
        administrator,
        pool,
    }
}

async fn drop_snapshot_security_database(mut database: SnapshotSecurityDatabase) {
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
}

fn snapshot_security_password() -> String {
    let mut material = [0_u8; 32];
    getrandom::fill(&mut material).unwrap();
    material.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn assert_snapshot_security_role(role: &str) {
    assert!(
        !role.is_empty()
            && role.len() <= 63
            && role
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
}

async fn create_snapshot_login_role(
    pool: &PgPool,
    role: &str,
    password: &str,
    connection_limit: u32,
) {
    assert_snapshot_security_role(role);
    assert!(password.len() == 64 && password.bytes().all(|byte| byte.is_ascii_hexdigit()));
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

async fn snapshot_role_pool(database_name: &str, role: &str, password: &str) -> PgPool {
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

async fn assert_snapshot_permission_denied(pool: &PgPool, statement: &str) {
    let error = sqlx::query(statement)
        .execute(pool)
        .await
        .expect_err("snapshot database capability must be denied");
    assert!(matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("42501")
    ));
}

async fn grant_snapshot_role_boundary(
    pool: &PgPool,
    database_name: &str,
    owner_role: &str,
    api_role: &str,
    denied_role: &str,
) {
    for relation in SNAPSHOT_READINESS_RELATIONS {
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "ALTER FUNCTION {SNAPSHOT_FUNCTION} OWNER TO {owner_role}"
    ))
    .execute(pool)
    .await
    .unwrap();
    for function in [
        SNAPSHOT_DATABASE_IDENTITY_FUNCTION,
        SNAPSHOT_KEY_COVERAGE_FUNCTION,
    ] {
        sqlx::query(&format!("ALTER FUNCTION {function} OWNER TO {owner_role}"))
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query(&format!(
        "REVOKE ALL ON DATABASE {database_name} FROM PUBLIC"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("REVOKE ALL ON SCHEMA public FROM PUBLIC")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {database_name} TO {api_role}, {denied_role}"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {owner_role}, {api_role}, {denied_role}"
    ))
    .execute(pool)
    .await
    .unwrap();
    for function in [
        SNAPSHOT_FUNCTION,
        SNAPSHOT_DATABASE_IDENTITY_FUNCTION,
        SNAPSHOT_KEY_COVERAGE_FUNCTION,
    ] {
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {function} TO {api_role}"
        ))
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn snapshot_function_count(
    pool: &PgPool,
    session_id: &str,
    principal_id: &str,
    session_digest: &[u8],
    tenant_id: &str,
    installation_id: &str,
) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.starring_product_authorized_snapshot_read_v2($1, $2, $3, $4, $5)",
    )
    .bind(session_id)
    .bind(principal_id)
    .bind(session_digest)
    .bind(tenant_id)
    .bind(installation_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn authorized_snapshot_is_exactly_scoped_for_a_non_owner_role() {
    let mut database = isolated_snapshot_security_database("acl").await;
    MIGRATOR.run(&database.pool).await.unwrap();
    let fixture = insert_product_fixture(&database.pool).await;
    let role_suffix = unique_suffix();
    let owner_role = format!("starring_snapshot_owner_{role_suffix}");
    let api_role = format!("starring_snapshot_api_{role_suffix}");
    let denied_role = format!("starring_snapshot_denied_{role_suffix}");
    let api_password = snapshot_security_password();
    let denied_password = snapshot_security_password();
    for role in [&owner_role, &api_role, &denied_role] {
        assert_snapshot_security_role(role);
    }
    sqlx::query(&format!(
        "CREATE ROLE {owner_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    create_snapshot_login_role(&database.pool, &api_role, &api_password, 4).await;
    create_snapshot_login_role(&database.pool, &denied_role, &denied_password, 4).await;
    grant_snapshot_role_boundary(
        &database.pool,
        &database.name,
        &owner_role,
        &api_role,
        &denied_role,
    )
    .await;
    let api_pool = snapshot_role_pool(&database.name, &api_role, &api_password).await;
    let denied_pool = snapshot_role_pool(&database.name, &denied_role, &denied_password).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let role_identity = sqlx::query_as::<_, (String, String)>(
            "SELECT current_user::TEXT, session_user::TEXT",
        )
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(role_identity, (api_role.clone(), api_role.clone()));

        let snapshots =
            PostgresAuthorizedPromotionSnapshots::new(api_pool.clone(), snapshot_test_cipher());
        snapshots.verify_readiness().await.unwrap();

        let missing_key = SnapshotEnvelopeKeyV1::new(
            "missing-key-v1",
            Zeroizing::new(std::array::from_fn(|index| {
                131_u8.wrapping_add((index as u8).wrapping_mul(23))
            })),
        )
        .unwrap();
        let missing_key_cipher = XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(
            SnapshotEnvelopeKeyringV1::new(missing_key, []).unwrap(),
        );
        let missing_key_readiness =
            PostgresAuthorizedPromotionSnapshots::new(api_pool.clone(), missing_key_cipher)
                .verify_readiness()
                .await;
        assert_eq!(
            missing_key_readiness,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::EncryptionKeyCoverageMissing)
        );
        let missing_key_rendered = format!("{missing_key_readiness:?}");
        assert!(!missing_key_rendered.contains("missing-key-v1"));
        assert!(!missing_key_rendered.contains(SNAPSHOT_TEST_KEY_ID));

        let topology = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT \
             public.starring_product_authorized_snapshot_reader_database_identity_v1(), \
             current_database()::TEXT, current_user::TEXT, session_user::TEXT",
        )
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(topology.1, database.name);
        assert_eq!(topology.2, api_role);
        assert_eq!(topology.2, topology.3);
        assert_eq!(topology.0.len(), 36);

        let session_digest = digest_opaque_session_credential_v1(&fixture.credential)
            .unwrap()
            .into_bytes();
        assert_eq!(
            snapshot_function_count(
                &api_pool,
                fixture.session_id.as_str(),
                fixture.principal_id.as_str(),
                &session_digest,
                fixture.tenant_id.as_str(),
                fixture.installation_id.as_str(),
            )
            .await,
            1
        );

        let authentication = SnapshotSecurityAuthentication {
            claims: AuthenticationClaimsV1::from_authentication(
                fixture.principal_id.clone(),
                AuthenticatedSessionFingerprintV1::from_sha256_digest(session_digest),
            ),
        };
        let guild_authority = TestGuildAuthority {
            tenant_id: fixture.tenant_id.clone(),
            installation_id: fixture.installation_id.clone(),
            application_id: fixture.application_id.clone(),
            guild_id: fixture.guild_id,
            user_id: fixture.user_id,
        };
        let promotions = PromotionCapture {
            captured: Mutex::new(None),
        };
        let application =
            AuthoringApplication::new(&authentication, &guild_authority, &snapshots, &promotions);
        let promoted = application
            .promote_owned_session(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse("snapshot-security-positive").unwrap(),
                &InstallationSelectorV1::new(fixture.installation_id.clone()),
                PromoteOwnedSessionV1 {
                    idempotency_key: ProductPromotionIdempotencyKeyV1::parse(
                        "snapshot-security-positive",
                    )
                    .unwrap(),
                    session_id: fixture.session_id.clone(),
                    expected_generation: SessionGeneration::new(1).unwrap(),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(
            promoted,
            authoring_application::AuthoringApplicationError::AuthorizedPromotion(
                AuthorizedPromotionSubmissionErrorV1::Indeterminate
            )
        );
        let captured = promotions.captured.lock().unwrap().take().unwrap();
        assert_eq!(captured.0, fixture.tenant_id.as_str());
        assert_eq!(captured.1, fixture.candidate_revision);
        assert_eq!(captured.2, fixture.binding_fingerprint);

        for (session_id, principal_id, digest, tenant_id, installation_id) in [
            (
                format!("missing-session-{}", unique_suffix()),
                fixture.principal_id.as_str().to_string(),
                session_digest.to_vec(),
                fixture.tenant_id.as_str().to_string(),
                fixture.installation_id.as_str().to_string(),
            ),
            (
                fixture.session_id.as_str().to_string(),
                format!("missing-principal-{}", unique_suffix()),
                session_digest.to_vec(),
                fixture.tenant_id.as_str().to_string(),
                fixture.installation_id.as_str().to_string(),
            ),
            (
                fixture.session_id.as_str().to_string(),
                fixture.principal_id.as_str().to_string(),
                vec![255_u8; 32],
                fixture.tenant_id.as_str().to_string(),
                fixture.installation_id.as_str().to_string(),
            ),
            (
                fixture.session_id.as_str().to_string(),
                fixture.principal_id.as_str().to_string(),
                session_digest.to_vec(),
                format!("missing-tenant-{}", unique_suffix()),
                fixture.installation_id.as_str().to_string(),
            ),
            (
                fixture.session_id.as_str().to_string(),
                fixture.principal_id.as_str().to_string(),
                session_digest.to_vec(),
                fixture.tenant_id.as_str().to_string(),
                format!("missing-installation-{}", unique_suffix()),
            ),
            (
                fixture.session_id.as_str().to_string(),
                fixture.principal_id.as_str().to_string(),
                vec![0_u8; 31],
                fixture.tenant_id.as_str().to_string(),
                fixture.installation_id.as_str().to_string(),
            ),
        ] {
            assert_eq!(
                snapshot_function_count(
                    &api_pool,
                    &session_id,
                    &principal_id,
                    &digest,
                    &tenant_id,
                    &installation_id,
                )
                .await,
                0
            );
        }

        let direct_privilege_count = sqlx::query_scalar::<_, i64>(
            "WITH relations(name) AS (VALUES \
              ('public.product_control_plane_identity'), \
              ('public.product_principals'), \
              ('public.product_auth_sessions'), \
              ('public.product_tenants'), \
              ('public.automation_installations'), \
              ('public.authoring_sessions'), \
              ('public.authoring_session_generations'), \
              ('public.automation_installation_authority_versions') \
             ), privileges(name) AS (VALUES \
              ('SELECT'), ('INSERT'), ('UPDATE'), ('DELETE'), \
              ('TRUNCATE'), ('REFERENCES'), ('TRIGGER') \
             ) \
             SELECT pg_catalog.count(*) FROM relations CROSS JOIN privileges \
             WHERE pg_catalog.has_table_privilege( \
              current_user, pg_catalog.to_regclass(relations.name), privileges.name)",
        )
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(direct_privilege_count, 0);
        let column_privilege_count = sqlx::query_scalar::<_, i64>(
            "WITH relations(name) AS (VALUES \
              ('public.product_control_plane_identity'), \
              ('public.product_principals'), \
              ('public.product_auth_sessions'), \
              ('public.product_tenants'), \
              ('public.automation_installations'), \
              ('public.authoring_sessions'), \
              ('public.authoring_session_generations'), \
              ('public.automation_installation_authority_versions') \
             ), privileges(name) AS (VALUES \
              ('SELECT'), ('INSERT'), ('UPDATE'), ('REFERENCES') \
             ) \
             SELECT pg_catalog.count(*) FROM relations CROSS JOIN privileges \
             WHERE pg_catalog.has_any_column_privilege( \
              current_user, pg_catalog.to_regclass(relations.name), privileges.name)",
        )
        .fetch_one(&api_pool)
        .await
        .unwrap();
        assert_eq!(column_privilege_count, 0);
        for statement in [
            "SELECT snapshot_ciphertext FROM public.authoring_session_generations LIMIT 1",
            "INSERT INTO public.product_tenants (tenant_id, lifecycle_state, display_name) \
             VALUES ('snapshot-forbidden', 'active', 'snapshot forbidden')",
            "UPDATE public.authoring_sessions SET lifecycle_state = lifecycle_state WHERE FALSE",
            "DELETE FROM public.automation_installation_authority_versions WHERE FALSE",
            "CREATE TABLE public.snapshot_escape(value INTEGER)",
            "CREATE TEMPORARY TABLE snapshot_escape(value INTEGER)",
            "SELECT * FROM public.starring_product_session_read_v1( \
             decode(repeat('00', 32), 'hex'))",
            "SELECT * FROM public.starring_product_installation_authority_read_v1( \
             'missing', 'missing', decode(repeat('00', 32), 'hex'))",
        ] {
            assert_snapshot_permission_denied(&api_pool, statement).await;
        }
        assert_snapshot_permission_denied(
            &denied_pool,
            "SELECT * FROM public.starring_product_authorized_snapshot_read_v2( \
             'missing', 'missing', decode(repeat('00', 32), 'hex'), 'missing', 'missing')",
        )
        .await;

        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {SNAPSHOT_FUNCTION} FROM {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::CapabilityMissing)
        );
        let unavailable = application
            .promote_owned_session(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse("snapshot-security-revoked").unwrap(),
                &InstallationSelectorV1::new(fixture.installation_id.clone()),
                PromoteOwnedSessionV1 {
                    idempotency_key: ProductPromotionIdempotencyKeyV1::parse(
                        "snapshot-security-revoked",
                    )
                    .unwrap(),
                    session_id: fixture.session_id.clone(),
                    expected_generation: SessionGeneration::new(1).unwrap(),
                },
            )
            .await
            .unwrap_err();
        let rendered = format!("{unavailable:?}");
        for sensitive in [
            api_role.as_str(),
            fixture.credential.as_str(),
            fixture.csrf.as_str(),
            fixture.principal_id.as_str(),
            fixture.tenant_id.as_str(),
            fixture.installation_id.as_str(),
            fixture.session_id.as_str(),
        ] {
            assert!(!rendered.contains(sensitive));
        }
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {SNAPSHOT_FUNCTION} TO {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SELECT(snapshot_ciphertext) \
             ON public.authoring_session_generations TO {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE SELECT(snapshot_ciphertext) \
             ON public.authoring_session_generations FROM {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {SNAPSHOT_FUNCTION} TO {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {SNAPSHOT_FUNCTION} FROM {denied_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {SNAPSHOT_FUNCTION} \
             TO {api_role} WITH GRANT OPTION"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::ExcessCapability)
        );
        sqlx::query(&format!(
            "REVOKE GRANT OPTION FOR EXECUTE ON FUNCTION {SNAPSHOT_FUNCTION} FROM {api_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {SNAPSHOT_FUNCTION} TO PUBLIC"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {SNAPSHOT_FUNCTION} FROM PUBLIC"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER FUNCTION {SNAPSHOT_FUNCTION} SECURITY INVOKER"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "ALTER FUNCTION {SNAPSHOT_FUNCTION} SECURITY DEFINER"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER FUNCTION {SNAPSHOT_DATABASE_IDENTITY_FUNCTION} SECURITY INVOKER"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "ALTER FUNCTION {SNAPSHOT_DATABASE_IDENTITY_FUNCTION} SECURITY DEFINER"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::raw_sql(
            "CREATE OR REPLACE FUNCTION \
             public.starring_product_authorized_snapshot_reader_database_identity_v1() \
             RETURNS TEXT LANGUAGE sql VOLATILE STRICT PARALLEL UNSAFE SECURITY DEFINER \
             SET search_path = pg_catalog AS $function$ \
             SELECT '00000000-0000-0000-0000-000000000000'::TEXT \
             $function$",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::ContractMismatch)
        );
        sqlx::raw_sql(
            "CREATE OR REPLACE FUNCTION \
             public.starring_product_authorized_snapshot_reader_database_identity_v1() \
             RETURNS TEXT LANGUAGE sql VOLATILE STRICT PARALLEL UNSAFE SECURITY DEFINER \
             SET search_path = pg_catalog AS $function$ \
             SELECT identity.database_identity::TEXT \
             FROM public.product_control_plane_identity AS identity \
             WHERE identity.singleton \
             $function$",
        )
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER FUNCTION {SNAPSHOT_FUNCTION} SET search_path = public"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(&format!(
            "ALTER FUNCTION {SNAPSHOT_FUNCTION} SET search_path = pg_catalog"
        ))
        .execute(&database.pool)
        .await
        .unwrap();

        sqlx::query("ALTER TABLE public.authoring_sessions ENABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();
        assert_eq!(
            snapshots.verify_readiness().await,
            Err(authoring_application_postgres::AuthorizedSnapshotReadinessErrorV1::ContractMismatch)
        );
        sqlx::query("ALTER TABLE public.authoring_sessions DISABLE ROW LEVEL SECURITY")
            .execute(&database.pool)
            .await
            .unwrap();

        snapshots.verify_readiness().await.unwrap();

        let mut authorization_transaction = api_pool.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY")
            .execute(&mut *authorization_transaction)
            .await
            .unwrap();
        sqlx::query(
            "SELECT pg_catalog.set_config('statement_timeout', '2s', true), \
             pg_catalog.set_config('lock_timeout', '2s', true), \
             pg_catalog.set_config('idle_in_transaction_session_timeout', '2s', true)",
        )
        .execute(&mut *authorization_transaction)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE public.product_auth_sessions \
             SET revoked_at = GREATEST(pg_catalog.clock_timestamp(), last_seen_at), \
              revocation_reason = 'snapshot_linearization_test' \
             WHERE session_digest = $1",
        )
        .bind(session_digest.as_slice())
        .execute(&database.pool)
        .await
        .unwrap();
        let post_configuration_rows = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.starring_product_authorized_snapshot_read_v2(\
              $1, $2, $3, $4, $5)",
        )
        .bind(fixture.session_id.as_str())
        .bind(fixture.principal_id.as_str())
        .bind(session_digest.as_slice())
        .bind(fixture.tenant_id.as_str())
        .bind(fixture.installation_id.as_str())
        .fetch_one(&mut *authorization_transaction)
        .await
        .unwrap();
        assert_eq!(post_configuration_rows, 0);
        authorization_transaction.rollback().await.unwrap();
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
async fn authorized_snapshot_migration_rejects_split_ownership() {
    let database = isolated_snapshot_security_database("owner").await;
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 202_607_190_015)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();
    }
    let split_owner = format!("starring_snapshot_split_{}", unique_suffix());
    assert_snapshot_security_role(&split_owner);
    sqlx::query(&format!(
        "CREATE ROLE {split_owner} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        sqlx::query(&format!(
            "ALTER TABLE public.authoring_sessions OWNER TO {split_owner}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_016)
            .unwrap();
        let mut transaction = database.pool.begin().await.unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .expect_err("split snapshot relation ownership must reject the migration");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected database error");
        };
        assert_eq!(database_error.code().as_deref(), Some("55000"));
        assert_eq!(
            database_error.message(),
            "authorized snapshot relations require one non-RLS owner"
        );
        transaction.rollback().await.unwrap();
        let function_exists =
            sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regprocedure($1) IS NOT NULL")
                .bind(SNAPSHOT_FUNCTION_IDENTITY)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        assert!(!function_exists);
    })
    .catch_unwind()
    .await;
    drop_snapshot_security_database(database).await;
    let mut administrator = sqlx::postgres::PgConnection::connect_with(
        &database_url()
            .parse::<PgConnectOptions>()
            .unwrap()
            .database("postgres"),
    )
    .await
    .unwrap();
    sqlx::query(&format!("DROP ROLE {split_owner}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn authorized_snapshot_migration_strips_hostile_default_grants() {
    let mut database = isolated_snapshot_security_database("grants").await;
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 202_607_190_015)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();
    }
    let role_suffix = unique_suffix();
    let owner_role = format!("starring_snapshot_owner_{role_suffix}");
    let migrator_role = format!("starring_snapshot_migrator_{role_suffix}");
    let hostile_role = format!("starring_snapshot_hostile_{role_suffix}");
    let migrator_password = snapshot_security_password();
    for role in [&owner_role, &migrator_role, &hostile_role] {
        assert_snapshot_security_role(role);
    }
    for role in [&owner_role, &hostile_role] {
        sqlx::query(&format!(
            "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    create_snapshot_login_role(&database.pool, &migrator_role, &migrator_password, 2).await;
    for relation in SNAPSHOT_RELATIONS {
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
    let migrator_pool =
        snapshot_role_pool(&database.name, &migrator_role, &migrator_password).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let identity =
            sqlx::query_as::<_, (String, String)>("SELECT current_user::TEXT, session_user::TEXT")
                .fetch_one(&migrator_pool)
                .await
                .unwrap();
        assert_eq!(identity, (migrator_role.clone(), migrator_role.clone()));
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_016)
            .unwrap();
        let mut transaction = migrator_pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let contract = sqlx::query_as::<_, (String, bool, bool, bool)>(
            "SELECT pg_catalog.pg_get_userbyid(function_row.proowner), \
              function_row.prosecdef, \
              pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE'), \
              EXISTS ( \
               SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
               )) AS privilege \
               WHERE privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE' \
              ) \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(SNAPSHOT_FUNCTION_IDENTITY)
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(contract, (owner_role.clone(), true, false, false));
        let unexpected_grants = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_proc AS function_row \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
              function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
             )) AS privilege \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
              AND privilege.grantee <> function_row.proowner",
        )
        .bind(SNAPSHOT_FUNCTION_IDENTITY)
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
    sqlx::query(&format!("REVOKE {owner_role} FROM {migrator_role}"))
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

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn authority_snapshot_readiness_migration_seals_identity_and_key_coverage() {
    let mut database = isolated_snapshot_security_database("readiness_migration").await;
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 202_607_200_005)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();
    }
    let hostile_role = format!("starring_readiness_hostile_{}", unique_suffix());
    assert_snapshot_security_role(&hostile_role);
    sqlx::query(&format!(
        "CREATE ROLE {hostile_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
         GRANT EXECUTE ON FUNCTIONS TO {hostile_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_200_006)
            .unwrap();
        let mut transaction = database.pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        let contracts = sqlx::query_as::<_, (String, bool, bool, bool, String, String)>(
            "SELECT expected.identity, function_row.prosecdef, \
              pg_catalog.has_function_privilege($1, function_row.oid, 'EXECUTE'), \
              EXISTS ( \
               SELECT 1 FROM pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
               )) AS privilege \
               WHERE privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE' \
              ), pg_catalog.pg_get_function_result(function_row.oid), \
              pg_catalog.pg_get_function_identity_arguments(function_row.oid) \
             FROM (VALUES ($2), ($3), ($4)) AS expected(identity) \
             INNER JOIN pg_catalog.pg_proc AS function_row \
              ON function_row.oid = pg_catalog.to_regprocedure(expected.identity) \
             ORDER BY expected.identity",
        )
        .bind(&hostile_role)
        .bind(SNAPSHOT_DATABASE_IDENTITY_FUNCTION)
        .bind(SNAPSHOT_KEY_COVERAGE_FUNCTION.to_ascii_lowercase())
        .bind("public.starring_product_installation_authority_reader_database_identity_v1()")
        .fetch_all(&database.pool)
        .await
        .unwrap();
        assert_eq!(contracts.len(), 3);
        for (_, security_definer, hostile_execute, public_execute, _, _) in &contracts {
            assert!(*security_definer);
            assert!(!*hostile_execute);
            assert!(!*public_execute);
        }
        assert!(contracts.iter().any(|contract| {
            contract.4 == "TABLE(covered boolean)"
                && contract.5 == "configured_encryption_key_ids text[]"
        }));

        let default_execute = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS ( \
              SELECT 1 \
              FROM pg_catalog.pg_default_acl AS default_acl \
              CROSS JOIN LATERAL pg_catalog.aclexplode(default_acl.defaclacl) AS privilege \
              WHERE default_acl.defaclobjtype = 'f' \
               AND privilege.grantee = pg_catalog.to_regrole($1) \
               AND privilege.privilege_type = 'EXECUTE' \
             )",
        )
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(!default_execute);

        let coverage = sqlx::query_scalar::<_, bool>(
            "SELECT covered \
             FROM public.starring_product_authorized_snapshot_key_coverage_v1($1)",
        )
        .bind(vec![SNAPSHOT_TEST_KEY_ID])
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(coverage);
        let invalid_coverage = sqlx::query_scalar::<_, bool>(
            "SELECT covered \
             FROM public.starring_product_authorized_snapshot_key_coverage_v1($1)",
        )
        .bind(Vec::<String>::new())
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(!invalid_coverage);
        let oversized_coverage = sqlx::query_scalar::<_, bool>(
            "SELECT covered \
             FROM public.starring_product_authorized_snapshot_key_coverage_v1( \
              pg_catalog.array_fill('valid-key'::TEXT, ARRAY[9]))",
        )
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(!oversized_coverage);
        let oversized_key_coverage = sqlx::query_scalar::<_, bool>(
            "SELECT covered \
             FROM public.starring_product_authorized_snapshot_key_coverage_v1( \
              ARRAY[pg_catalog.repeat('a', 129)])",
        )
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(!oversized_key_coverage);
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
