use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::MIGRATOR;
use futures::FutureExt;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Connection;

const IDENTITY_LIFECYCLE_MIGRATION: i64 = 202_607_190_017;
const SESSION_ISSUE_RECONCILIATION_MIGRATION: i64 = 202_607_190_018;
const SESSION_ISSUE_FUNCTION: &str = "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)";
const IDENTITY_LIFECYCLE_FUNCTIONS: [&str; 10] = [
    "public.starring_product_oauth_database_identity_v1()",
    "public.starring_product_session_issuer_database_identity_v1()",
    "public.starring_product_session_api_database_identity_v1()",
    "public.starring_product_security_revoker_database_identity_v1()",
    "public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)",
    "public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])",
    "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)",
    "public.starring_product_session_logout_read_v1(bytea)",
    "public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)",
    "public.starring_product_session_security_revoke_v1(bytea)",
];
const IDENTITY_PROTECTED_FUNCTIONS: [&str; 15] = [
    "public.starring_product_oauth_database_identity_v1()",
    "public.starring_product_session_issuer_database_identity_v1()",
    "public.starring_product_session_api_database_identity_v1()",
    "public.starring_product_security_revoker_database_identity_v1()",
    "public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)",
    "public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])",
    "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)",
    "public.starring_product_session_logout_read_v1(bytea)",
    "public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)",
    "public.starring_product_session_security_revoke_v1(bytea)",
    "public.enforce_product_principal_transition()",
    "public.enforce_product_oauth_flow_transition()",
    "public.enforce_product_auth_session_oauth_binding()",
    "public.enforce_product_auth_session_transition()",
    "public.starring_purge_product_identity_v1(integer)",
];
const IDENTITY_PREEXISTING_FUNCTIONS: [&str; 8] = [
    "public.starring_product_session_read_v1(bytea)",
    "public.starring_product_session_mutation_read_v1(bytea)",
    "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)",
    "public.enforce_product_principal_transition()",
    "public.enforce_product_oauth_flow_transition()",
    "public.enforce_product_auth_session_oauth_binding()",
    "public.enforce_product_auth_session_transition()",
    "public.starring_purge_product_identity_v1(integer)",
];
const IDENTITY_RELATIONS: [(&str, &str); 4] = [
    ("public.product_control_plane_identity", "database_identity"),
    ("public.product_oauth_flows", "return_path"),
    ("public.product_principals", "display_profile"),
    ("public.product_auth_sessions", "revocation_reason"),
];
const OAUTH_ROLE_FUNCTIONS: [&str; 3] = [
    "public.starring_product_oauth_database_identity_v1()",
    "public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)",
    "public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])",
];
const ISSUER_ROLE_FUNCTIONS: [&str; 2] = [
    "public.starring_product_session_issuer_database_identity_v1()",
    "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)",
];
const SESSION_ROLE_FUNCTIONS: [&str; 6] = [
    "public.starring_product_session_api_database_identity_v1()",
    "public.starring_product_session_read_v1(bytea)",
    "public.starring_product_session_mutation_read_v1(bytea)",
    "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)",
    "public.starring_product_session_logout_read_v1(bytea)",
    "public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)",
];
const SECURITY_ROLE_FUNCTIONS: [&str; 2] = [
    "public.starring_product_security_revoker_database_identity_v1()",
    "public.starring_product_session_security_revoke_v1(bytea)",
];

static SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

struct IdentityMigrationTestDatabase {
    name: String,
    administrator: PgConnection,
    pool: PgPool,
}

fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let options = url
        .parse::<PgConnectOptions>()
        .expect("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL");
    let database = options
        .get_database()
        .expect("STARRING_TEST_DATABASE_URL must name a database");
    assert!(
        database.starts_with("starring_")
            && database.split('_').any(|segment| segment == "test")
            && database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "refusing to use a database outside the strict Starring test namespace"
    );
    url
}

fn suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{timestamp}{counter}")
}

fn assert_safe_identifier(identifier: &str) {
    assert!(
        !identifier.is_empty()
            && identifier.len() <= 63
            && identifier
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
}

async fn isolated_database(label: &str) -> IdentityMigrationTestDatabase {
    assert!(
        !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    let label = label.chars().take(16).collect::<String>();
    let name = format!("starring_identity_{label}_test_{}", suffix());
    assert_safe_identifier(&name);
    assert!(name.split('_').any(|segment| segment == "test"));
    let base = database_url().parse::<PgConnectOptions>().unwrap();
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(base.database(&name))
        .await
        .unwrap();
    IdentityMigrationTestDatabase {
        name,
        administrator,
        pool,
    }
}

async fn drop_isolated_database(database: IdentityMigrationTestDatabase) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
}

async fn apply_migrations_through_016(pool: &PgPool) {
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version < IDENTITY_LIFECYCLE_MIGRATION)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn apply_migrations_through_017(pool: &PgPool) {
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version < SESSION_ISSUE_RECONCILIATION_MIGRATION)
    {
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(pool)
            .await
            .unwrap();
    }
}

fn identity_lifecycle_migration() -> &'static sqlx::migrate::Migration {
    MIGRATOR
        .iter()
        .find(|migration| migration.version == IDENTITY_LIFECYCLE_MIGRATION)
        .expect("identity lifecycle migration must exist")
}

fn session_issue_reconciliation_migration() -> &'static sqlx::migrate::Migration {
    MIGRATOR
        .iter()
        .find(|migration| migration.version == SESSION_ISSUE_RECONCILIATION_MIGRATION)
        .expect("session issue reconciliation migration must exist")
}

async fn assert_no_lifecycle_functions(pool: &PgPool) {
    let relation_exists = sqlx::query_scalar::<_, bool>(
        "SELECT pg_catalog.to_regclass('public.product_control_plane_identity') IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(!relation_exists);
    for signature in IDENTITY_LIFECYCLE_FUNCTIONS {
        let exists =
            sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regprocedure($1) IS NOT NULL")
                .bind(signature)
                .fetch_one(pool)
                .await
                .unwrap();
        assert!(!exists, "unexpected lifecycle function {signature}");
    }
}

async fn assert_migration_rejected_atomically(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    let error = sqlx::raw_sql(identity_lifecycle_migration().sql.as_ref())
        .execute(&mut *transaction)
        .await
        .expect_err("identity lifecycle migration must reject unsafe ownership state");
    assert!(matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
    ));
    transaction.rollback().await.unwrap();
    assert_no_lifecycle_functions(pool).await;
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

fn database_role_password() -> String {
    let mut material = [0_u8; 32];
    getrandom::fill(&mut material).unwrap();
    lower_hex(&material)
}

async fn create_login_role(pool: &PgPool, role: &str, password: &str) {
    assert_safe_identifier(role);
    assert!(password.len() == 64 && password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let password_literal = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
        .bind(password)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {role} LOGIN PASSWORD {password_literal} \
         NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION \
         NOBYPASSRLS CONNECTION LIMIT 4"
    ))
    .execute(pool)
    .await
    .unwrap();
}

async fn database_role_login_pool(database_name: &str, role: &str, password: &str) -> PgPool {
    assert_safe_identifier(database_name);
    assert_safe_identifier(role);
    assert!(password.len() == 64 && password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    PgPoolOptions::new()
        .max_connections(2)
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

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn identity_lifecycle_migration_rejects_split_relation_owners_atomically() {
    let mut database = isolated_database("split_owner").await;
    apply_migrations_through_016(&database.pool).await;
    let split_owner = format!("starring_identity_split_{}", suffix());
    assert_safe_identifier(&split_owner);
    sqlx::query(&format!(
        "CREATE ROLE {split_owner} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "ALTER TABLE public.product_oauth_flows OWNER TO {split_owner}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        assert_migration_rejected_atomically(&database.pool).await;
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
async fn identity_lifecycle_migration_seals_new_capabilities_and_preserves_legacy_grants() {
    let mut database = isolated_database("hostile_grants").await;
    apply_migrations_through_016(&database.pool).await;
    let role_suffix = suffix();
    let owner_role = format!("starring_identity_owner_{role_suffix}");
    let migrator_role = format!("starring_identity_migrator_{role_suffix}");
    let hostile_role = format!("starring_identity_hostile_{role_suffix}");
    let oauth_role = format!("starring_identity_oauth_{role_suffix}");
    let issuer_role = format!("starring_identity_issuer_{role_suffix}");
    let session_role = format!("starring_identity_session_{role_suffix}");
    let security_role = format!("starring_identity_security_{role_suffix}");
    let migrator_password = database_role_password();
    let oauth_password = database_role_password();
    let issuer_password = database_role_password();
    let session_password = database_role_password();
    let security_password = database_role_password();
    for role in [
        &owner_role,
        &migrator_role,
        &hostile_role,
        &oauth_role,
        &issuer_role,
        &session_role,
        &security_role,
    ] {
        assert_safe_identifier(role);
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
    let password_literal = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
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
    for (role, password) in [
        (&oauth_role, &oauth_password),
        (&issuer_role, &issuer_password),
        (&session_role, &session_password),
        (&security_role, &security_password),
    ] {
        create_login_role(&database.pool, role, password).await;
    }
    for (relation, column) in [
        ("product_oauth_flows", "return_path"),
        ("product_principals", "display_profile"),
        ("product_auth_sessions", "revocation_reason"),
    ] {
        sqlx::query(&format!(
            "GRANT SELECT ON TABLE public.{relation} TO {hostile_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT UPDATE ({column}) ON TABLE public.{relation} TO {hostile_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "ALTER TABLE public.{relation} OWNER TO {owner_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    for signature in IDENTITY_PREEXISTING_FUNCTIONS {
        sqlx::query(&format!("ALTER FUNCTION {signature} OWNER TO {owner_role}"))
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
        "ALTER DEFAULT PRIVILEGES FOR ROLE {owner_role} IN SCHEMA public \
         GRANT ALL PRIVILEGES ON TABLES TO {hostile_role}"
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
        "ALTER DEFAULT PRIVILEGES FOR ROLE {owner_role} IN SCHEMA public \
         GRANT EXECUTE ON FUNCTIONS TO {hostile_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let migrator_pool =
        database_role_login_pool(&database.name, &migrator_role, &migrator_password).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let identity =
            sqlx::query_as::<_, (String, String)>("SELECT current_user::TEXT, session_user::TEXT")
                .fetch_one(&migrator_pool)
                .await
                .unwrap();
        assert_eq!(identity, (migrator_role.clone(), migrator_role.clone()));
        let mut transaction = migrator_pool.begin().await.unwrap();
        sqlx::query(&format!("SET LOCAL ROLE {owner_role}"))
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::raw_sql(identity_lifecycle_migration().sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        for (relation, column) in IDENTITY_RELATIONS {
            let contract = sqlx::query_as::<_, (String, bool, bool, bool, bool, i64)>(
                "SELECT pg_catalog.pg_get_userbyid(relation_row.relowner), \
                  relation_row.relrowsecurity, \
                  relation_row.relforcerowsecurity, \
                  pg_catalog.has_table_privilege($2, relation_row.oid, 'SELECT'), \
                  pg_catalog.has_column_privilege($2, relation_row.oid, $3, 'UPDATE'), \
                  (SELECT pg_catalog.count(*) \
                   FROM ( \
                    SELECT privilege.grantee \
                    FROM pg_catalog.aclexplode(COALESCE( \
                     relation_row.relacl, \
                     pg_catalog.acldefault('r', relation_row.relowner) \
                    )) AS privilege \
                    UNION ALL \
                    SELECT privilege.grantee \
                    FROM pg_catalog.pg_attribute AS attribute \
                    CROSS JOIN LATERAL pg_catalog.aclexplode( \
                     NULLIF(attribute.attacl, '{}'::ACLITEM[]) \
                    ) AS privilege \
                    WHERE attribute.attrelid = relation_row.oid \
                     AND attribute.attnum > 0 \
                     AND NOT attribute.attisdropped \
                   ) AS grant_entry \
                   WHERE grant_entry.grantee <> relation_row.relowner) \
                 FROM pg_catalog.pg_class AS relation_row \
                 WHERE relation_row.oid = pg_catalog.to_regclass($1) \
                  AND relation_row.relkind = 'r'",
            )
            .bind(relation)
            .bind(&hostile_role)
            .bind(column)
            .fetch_one(&database.pool)
            .await
            .unwrap();
            if relation == "public.product_control_plane_identity" {
                assert_eq!(
                    contract,
                    (owner_role.clone(), false, false, false, false, 0)
                );
            } else {
                assert_eq!(contract, (owner_role.clone(), false, false, true, true, 2));
            }
        }

        let mut identity_verification = migrator_pool.begin().await.unwrap();
        sqlx::query(&format!("SET LOCAL ROLE {owner_role}"))
            .execute(&mut *identity_verification)
            .await
            .unwrap();
        let identity_contract = sqlx::query_as::<_, (i64, i64, bool, bool)>(
            "SELECT pg_catalog.count(*), \
              pg_catalog.count(DISTINCT identity.database_identity), \
              pg_catalog.bool_and( \
               identity.singleton \
               AND identity.database_identity \
                <> '00000000-0000-0000-0000-000000000000'::UUID \
              ), \
              pg_catalog.bool_and(identity.created_at IS NOT NULL) \
             FROM public.product_control_plane_identity AS identity",
        )
        .fetch_one(&mut *identity_verification)
        .await
        .unwrap();
        assert_eq!(identity_contract, (1, 1, true, true));
        let database_identity = sqlx::query_scalar::<_, String>(
            "SELECT identity.database_identity::TEXT \
             FROM public.product_control_plane_identity AS identity \
             WHERE identity.singleton",
        )
        .fetch_one(&mut *identity_verification)
        .await
        .unwrap();
        for signature in &IDENTITY_PROTECTED_FUNCTIONS[..4] {
            let observed_identity = sqlx::query_scalar::<_, String>(&format!("SELECT {signature}"))
                .fetch_one(&mut *identity_verification)
                .await
                .unwrap();
            assert_eq!(observed_identity, database_identity);
        }
        identity_verification.rollback().await.unwrap();

        for signature in IDENTITY_PROTECTED_FUNCTIONS {
            let contract = sqlx::query_as::<_, (String, bool, bool, i64)>(
                "SELECT pg_catalog.pg_get_userbyid(function_row.proowner), \
                  pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE'), \
                  EXISTS ( \
                   SELECT 1 \
                   FROM pg_catalog.aclexplode(COALESCE( \
                    function_row.proacl, \
                    pg_catalog.acldefault('f', function_row.proowner) \
                   )) AS privilege \
                   WHERE privilege.grantee = 0 \
                    AND privilege.privilege_type = 'EXECUTE' \
                  ), \
                  (SELECT pg_catalog.count(*) \
                   FROM pg_catalog.aclexplode(COALESCE( \
                    function_row.proacl, \
                    pg_catalog.acldefault('f', function_row.proowner) \
                   )) AS privilege \
                   WHERE privilege.grantee <> function_row.proowner) \
                 FROM pg_catalog.pg_proc AS function_row \
                 WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
            )
            .bind(signature)
            .bind(&hostile_role)
            .fetch_one(&database.pool)
            .await
            .unwrap();
            assert_eq!(contract, (owner_role.clone(), false, false, 0));
        }

        sqlx::query(&format!("REVOKE {owner_role} FROM {migrator_role}"))
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
            "GRANT CONNECT ON DATABASE {} TO {oauth_role}, {issuer_role}, \
             {session_role}, {security_role}",
            database.name
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT USAGE ON SCHEMA public TO {owner_role}, {oauth_role}, {issuer_role}, \
             {session_role}, {security_role}"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        for (role, functions) in [
            (&oauth_role, OAUTH_ROLE_FUNCTIONS.as_slice()),
            (&issuer_role, ISSUER_ROLE_FUNCTIONS.as_slice()),
            (&session_role, SESSION_ROLE_FUNCTIONS.as_slice()),
            (&security_role, SECURITY_ROLE_FUNCTIONS.as_slice()),
        ] {
            for function in functions {
                sqlx::query(&format!("GRANT EXECUTE ON FUNCTION {function} TO {role}"))
                    .execute(&database.pool)
                    .await
                    .unwrap();
            }
        }
        let oauth_pool =
            database_role_login_pool(&database.name, &oauth_role, &oauth_password).await;
        let issuer_pool =
            database_role_login_pool(&database.name, &issuer_role, &issuer_password).await;
        let session_pool =
            database_role_login_pool(&database.name, &session_role, &session_password).await;
        let security_pool =
            database_role_login_pool(&database.name, &security_role, &security_password).await;
        let config = authoring_application_postgres::PostgresProductIdentityConfig::production(
            "https://starring.example/oauth/discord/callback",
            ["/".to_string(), "/app".to_string()],
        )
        .unwrap();
        let store = authoring_application_postgres::PostgresProductIdentityStore::production(
            authoring_application_postgres::ProductIdentityDatabasePoolsV1::new(
                oauth_pool.clone(),
                issuer_pool.clone(),
                session_pool.clone(),
                security_pool.clone(),
            ),
            config,
        );
        assert_eq!(
            store.verify_readiness().await,
            Err(authoring_application_postgres::ProductIdentityReadinessErrorV1::ExcessCapability)
        );
        for relation in [
            "product_oauth_flows",
            "product_principals",
            "product_auth_sessions",
        ] {
            sqlx::query(&format!(
                "REVOKE ALL PRIVILEGES ON TABLE public.{relation} FROM {hostile_role} CASCADE"
            ))
            .execute(&database.pool)
            .await
            .unwrap();
        }
        store.verify_readiness().await.unwrap();
        oauth_pool.close().await;
        issuer_pool.close().await;
        session_pool.close().await;
        security_pool.close().await;
    })
    .catch_unwind()
    .await;
    migrator_pool.close().await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    let _ = sqlx::query(&format!("REVOKE {owner_role} FROM {migrator_role}"))
        .execute(&mut database.administrator)
        .await;
    for role in [
        &hostile_role,
        &oauth_role,
        &issuer_role,
        &session_role,
        &security_role,
        &migrator_role,
        &owner_role,
    ] {
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
async fn identity_lifecycle_migration_rejects_rls_drift_atomically() {
    let database = isolated_database("rls_drift").await;
    apply_migrations_through_016(&database.pool).await;
    sqlx::query("ALTER TABLE public.product_principals ENABLE ROW LEVEL SECURITY")
        .execute(&database.pool)
        .await
        .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        assert_migration_rejected_atomically(&database.pool).await;
    })
    .catch_unwind()
    .await;
    drop_isolated_database(database).await;
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn session_issue_reconciliation_migration_seals_function_capability() {
    let mut database = isolated_database("reconcile_acl").await;
    apply_migrations_through_017(&database.pool).await;
    let hostile_role = format!("starring_identity_hostile_{}", suffix());
    assert_safe_identifier(&hostile_role);
    sqlx::query(&format!(
        "CREATE ROLE {hostile_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {SESSION_ISSUE_FUNCTION} TO {hostile_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        let definition_before = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_functiondef(function_row.oid) \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(SESSION_ISSUE_FUNCTION)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(definition_before.contains(
            "locked_flow.consumed_at > issue_now OR issue_now >= locked_flow.expires_at"
        ));
        let hostile_execute_before = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE') \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(SESSION_ISSUE_FUNCTION)
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(hostile_execute_before);

        sqlx::raw_sql(session_issue_reconciliation_migration().sql.as_ref())
            .execute(&database.pool)
            .await
            .unwrap();

        let relation_owner = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_userbyid(relation_row.relowner) \
             FROM pg_catalog.pg_class AS relation_row \
             WHERE relation_row.oid = pg_catalog.to_regclass('public.product_auth_sessions')",
        )
        .fetch_one(&database.pool)
        .await
        .unwrap();
        let contract = sqlx::query_as::<
            _,
            (
                String,
                bool,
                bool,
                String,
                String,
                Option<Vec<String>>,
                f64,
                bool,
                bool,
                i64,
                i64,
                String,
            ),
        >(
            "SELECT pg_catalog.pg_get_userbyid(function_row.proowner), \
              function_row.prosecdef, \
              function_row.proisstrict, \
              function_row.provolatile::TEXT, \
              function_row.proparallel::TEXT, \
              function_row.proconfig, \
              function_row.prorows::DOUBLE PRECISION, \
              pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE'), \
              EXISTS ( \
               SELECT 1 \
               FROM pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, \
                pg_catalog.acldefault('f', function_row.proowner) \
               )) AS privilege \
               WHERE privilege.grantee = 0 \
                AND privilege.privilege_type = 'EXECUTE' \
              ), \
              (SELECT pg_catalog.count(*) \
               FROM pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, \
                pg_catalog.acldefault('f', function_row.proowner) \
               )) AS privilege), \
              (SELECT pg_catalog.count(*) \
               FROM pg_catalog.aclexplode(COALESCE( \
                function_row.proacl, \
                pg_catalog.acldefault('f', function_row.proowner) \
               )) AS privilege \
               WHERE privilege.grantee <> function_row.proowner), \
              pg_catalog.pg_get_functiondef(function_row.oid) \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(SESSION_ISSUE_FUNCTION)
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(contract.0, relation_owner);
        assert!(contract.1);
        assert!(contract.2);
        assert_eq!(contract.3, "v");
        assert_eq!(contract.4, "u");
        assert_eq!(contract.5, Some(vec!["search_path=pg_catalog".to_string()]));
        assert_eq!(contract.6, 1.0);
        assert!(!contract.7);
        assert!(!contract.8);
        assert_eq!(contract.9, 1);
        assert_eq!(contract.10, 0);
        assert_ne!(contract.11, definition_before);
        let replay_lookup = contract.11.find("SELECT authentication_session.*").unwrap();
        let new_issue_expiry_check = contract
            .11
            .find("IF issue_now >= locked_flow.expires_at THEN")
            .unwrap();
        assert!(replay_lookup < new_issue_expiry_check);
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
async fn session_issue_reconciliation_migration_rolls_back_on_owner_drift() {
    let mut database = isolated_database("reconcile_drift").await;
    apply_migrations_through_017(&database.pool).await;
    let split_owner = format!("starring_identity_split_{}", suffix());
    let hostile_role = format!("starring_identity_hostile_{}", suffix());
    for role in [&split_owner, &hostile_role] {
        assert_safe_identifier(role);
        sqlx::query(&format!(
            "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "ALTER TABLE public.product_oauth_flows OWNER TO {split_owner}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {SESSION_ISSUE_FUNCTION} TO {hostile_role}"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        let before = sqlx::query_as::<_, (String, String, String)>(
            "SELECT pg_catalog.pg_get_functiondef(function_row.oid), \
              COALESCE(function_row.proacl::TEXT, ''), \
              pg_catalog.pg_get_userbyid(function_row.proowner) \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(SESSION_ISSUE_FUNCTION)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(before.0.contains(
            "locked_flow.consumed_at > issue_now OR issue_now >= locked_flow.expires_at"
        ));
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE') \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(SESSION_ISSUE_FUNCTION)
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await
        .unwrap());

        let mut transaction = database.pool.begin().await.unwrap();
        let error = sqlx::raw_sql(session_issue_reconciliation_migration().sql.as_ref())
            .execute(&mut *transaction)
            .await
            .expect_err("session issue reconciliation must reject split relation owners");
        assert!(matches!(
            error,
            sqlx::Error::Database(database_error)
                if database_error.code().as_deref() == Some("55000")
        ));
        transaction.rollback().await.unwrap();

        let after = sqlx::query_as::<_, (String, String, String)>(
            "SELECT pg_catalog.pg_get_functiondef(function_row.oid), \
              COALESCE(function_row.proacl::TEXT, ''), \
              pg_catalog.pg_get_userbyid(function_row.proowner) \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(SESSION_ISSUE_FUNCTION)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(after, before);
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($2, function_row.oid, 'EXECUTE') \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(SESSION_ISSUE_FUNCTION)
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await
        .unwrap());
        let observed_split_owner = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_userbyid(relation_row.relowner) \
             FROM pg_catalog.pg_class AS relation_row \
             WHERE relation_row.oid = pg_catalog.to_regclass('public.product_oauth_flows')",
        )
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(observed_split_owner, split_owner);
    })
    .catch_unwind()
    .await;
    database.pool.close().await;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut database.administrator)
        .await
        .unwrap();
    for role in [&hostile_role, &split_owner] {
        sqlx::query(&format!("DROP ROLE {role}"))
            .execute(&mut database.administrator)
            .await
            .unwrap();
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
