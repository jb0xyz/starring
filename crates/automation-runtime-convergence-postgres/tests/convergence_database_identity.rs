use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use automation_runtime_convergence_postgres::{
    PostgresRuntimeConvergenceDatabaseIdentityReader, RuntimeConvergenceStoreError, MIGRATOR,
};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use sqlx::{Connection, PgPool};

const MIGRATION_VERSION: i64 = 202_607_220_025;
const FUNCTION_IDENTITY: &str = "public.starring_runtime_convergence_database_identity_v1()";

#[test]
fn convergence_database_identity_migration_is_registered_after_panel_database_scope() {
    let versions = MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let panel = versions
        .iter()
        .position(|version| *version == 202_607_220_024)
        .unwrap();
    let identity = versions
        .iter()
        .position(|version| *version == MIGRATION_VERSION)
        .unwrap();
    assert_eq!(identity, panel + 1);
}

#[test]
fn convergence_database_identity_is_one_private_fixed_resolution_capability() {
    let migration = include_str!(
        "../../../migrations/202607220025_scope_runtime_convergence_database_identity.sql"
    );
    assert_eq!(
        migration
            .matches("CREATE FUNCTION public.starring_runtime_convergence_database_identity_v1()")
            .count(),
        1
    );
    assert_eq!(migration.matches("CREATE FUNCTION public.").count(), 1);
    assert!(migration.contains("RETURNS TEXT\nLANGUAGE sql"));
    assert!(migration.contains("SECURITY DEFINER\nSET search_path = pg_catalog"));
    assert!(migration.contains("DO $preflight$"));
    assert!(migration.contains("DO $postflight$"));
    assert!(migration.contains("function_row.proowner <> common_owner"));
    assert!(migration.contains("ARRAY['search_path=pg_catalog']::TEXT[]"));
    assert!(migration.contains("privilege.grantee <> common_owner"));
    assert!(migration.contains(
        "REVOKE ALL PRIVILEGES ON FUNCTION\n        public.starring_runtime_convergence_database_identity_v1()\n    FROM PUBLIC CASCADE"
    ));
    assert!(migration.contains("FROM public.product_control_plane_identity AS identity"));
    assert!(migration.contains("^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"));
    for relation in [
        "product_control_plane_identity",
        "runtime_deployments",
        "runtime_attestations",
        "runtime_serving_leases",
    ] {
        assert!(migration.contains(relation));
    }
    for forbidden in [
        "CREATE ROLE",
        "GRANT EXECUTE",
        "GRANT SELECT",
        "WITH GRANT OPTION",
        "COMMENT ON",
    ] {
        assert!(!migration.contains(forbidden));
    }
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[test]
fn convergence_database_identity_adapter_uses_only_the_private_bounded_query() {
    let source = include_str!("../src/database_identity.rs");
    assert!(source.contains(FUNCTION_IDENTITY));
    assert!(source.contains("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY"));
    for setting in [
        "statement_timeout",
        "lock_timeout",
        "idle_in_transaction_session_timeout",
    ] {
        assert!(source.contains(setting));
    }
    for relation in [
        "product_control_plane_identity",
        "runtime_deployments",
        "runtime_attestations",
        "runtime_serving_leases",
    ] {
        assert!(!source.contains(relation));
    }
    for line in source.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
        assert!(!trimmed.starts_with('*'));
    }
}

struct IsolatedDatabase {
    name: String,
    administrator: PgConnection,
    connect_options: PgConnectOptions,
    pool: PgPool,
}

async fn isolated_database() -> IsolatedDatabase {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let base = url
        .parse::<PgConnectOptions>()
        .expect("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL");
    let configured_database = base
        .get_database()
        .expect("STARRING_TEST_DATABASE_URL must name a database");
    assert!(
        configured_database.starts_with("starring_")
            && configured_database
                .split('_')
                .any(|segment| segment == "test")
            && configured_database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("starring_runtime_identity_test_{suffix}");
    assert!(
        name.len() <= 63
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    let mut administrator = PgConnection::connect_with(&base.clone().database("postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let connect_options = base.database(&name);
    let pool = match PgPoolOptions::new()
        .max_connections(4)
        .connect_with(connect_options.clone())
        .await
    {
        Ok(pool) => pool,
        Err(error) => {
            let cleanup = sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
                .execute(&mut administrator)
                .await;
            match cleanup {
                Ok(_) => panic!("isolated database pool connection failed: {error}"),
                Err(cleanup_error) => panic!(
                    "isolated database pool connection failed: {error}; cleanup failed: {cleanup_error}"
                ),
            }
        }
    };
    IsolatedDatabase {
        name,
        administrator,
        connect_options,
        pool,
    }
}

async fn run_isolated_database_test<F, Fut>(test: F)
where
    F: FnOnce(PgPool, PgConnectOptions, String, String) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let database = isolated_database().await;
    let role = object_capability_role(&database.name);
    let outcome = tokio::spawn(test(
        database.pool.clone(),
        database.connect_options.clone(),
        database.name.clone(),
        role.clone(),
    ))
    .await;
    cleanup_isolated_database(database, &role).await;
    outcome.expect("convergence database identity object capability test must complete");
}

fn object_capability_role(database_name: &str) -> String {
    let suffix = database_name.rsplit('_').next().unwrap();
    let suffix = &suffix[suffix.len().saturating_sub(18)..];
    let role = format!("srt_convergence_identity_{suffix}");
    assert!(
        role.len() <= 63
            && role
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    role
}

async fn cleanup_isolated_database(database: IsolatedDatabase, role: &str) {
    let IsolatedDatabase {
        name,
        mut administrator,
        connect_options: _,
        pool,
    } = database;
    let mut cleanup_errors = Vec::new();
    let role_exists =
        match sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regrole($1) IS NOT NULL")
            .bind(role)
            .fetch_one(&mut administrator)
            .await
        {
            Ok(exists) => exists,
            Err(error) => {
                cleanup_errors.push(format!("role lookup failed: {error}"));
                true
            }
        };
    if role_exists {
        if let Err(error) = sqlx::query(
            "SELECT pg_catalog.pg_terminate_backend(activity.pid) \
             FROM pg_catalog.pg_stat_activity AS activity \
             WHERE activity.usename = $1 \
                AND activity.pid <> pg_catalog.pg_backend_pid()",
        )
        .bind(role)
        .execute(&mut administrator)
        .await
        {
            cleanup_errors.push(format!("role session cleanup failed: {error}"));
        }
        if let Err(error) = sqlx::query(&format!("DROP OWNED BY {role}"))
            .execute(&pool)
            .await
        {
            cleanup_errors.push(format!("role object cleanup failed: {error}"));
        }
    }
    pool.close().await;
    if let Err(error) = sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&mut administrator)
        .await
    {
        cleanup_errors.push(format!("database cleanup failed: {error}"));
    }
    if let Err(error) = sqlx::query(&format!("DROP ROLE IF EXISTS {role}"))
        .execute(&mut administrator)
        .await
    {
        cleanup_errors.push(format!("role cleanup failed: {error}"));
    }
    assert!(
        cleanup_errors.is_empty(),
        "isolated database cleanup failed: {}",
        cleanup_errors.join("; ")
    );
}

async fn install_object_capability_role(
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
        "REVOKE CONNECT, TEMPORARY ON DATABASE {database_name} FROM PUBLIC"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {role} LOGIN PASSWORD {password_literal} NOINHERIT NOSUPERUSER NOCREATEDB \
         NOCREATEROLE NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("CREATE SEQUENCE public.runtime_convergence_identity_acl_probe")
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
        "GRANT EXECUTE ON FUNCTION {FUNCTION_IDENTITY} TO {role}"
    ))
    .execute(pool)
    .await
    .unwrap();
}

async fn assert_object_capability_boundary(pool: &PgPool, role: &str) {
    let role_contract = sqlx::query_as::<_, (bool, bool, bool, bool, bool, bool, bool, i32, i64)>(
        "SELECT role.rolcanlogin, role.rolinherit, role.rolsuper, role.rolcreatedb, \
                role.rolcreaterole, role.rolreplication, role.rolbypassrls, role.rolconnlimit, \
                COALESCE(pg_catalog.cardinality(role.rolconfig), 0)::BIGINT \
         FROM pg_catalog.pg_roles AS role \
         WHERE role.rolname = $1",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        role_contract,
        (true, false, false, false, false, false, false, 4, 0)
    );
    let database_and_schema_scope = sqlx::query_as::<_, (bool, bool, bool, bool, bool)>(
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
    assert_eq!(database_and_schema_scope, (true, false, false, true, false));
    let unexpected_schema_privileges = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_namespace AS namespace \
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
    assert_eq!(unexpected_schema_privileges, 0);
    let membership_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_auth_members AS membership \
         WHERE membership.roleid = pg_catalog.to_regrole($1) \
            OR membership.member = pg_catalog.to_regrole($1)",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(membership_count, 0);
    let role_setting_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_db_role_setting AS setting \
         WHERE setting.setrole = pg_catalog.to_regrole($1) \
            OR (setting.setrole = 0 AND setting.setdatabase = ( \
                SELECT database_row.oid \
                FROM pg_catalog.pg_database AS database_row \
                WHERE database_row.datname = pg_catalog.current_database() \
            ))",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(role_setting_count, 0);
    let relation_privileges = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname <> 'information_schema' \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND relation.relkind IN ('r', 'p', 'v', 'm', 'f') \
            AND (pg_catalog.has_table_privilege( \
                    $1, relation.oid, \
                    'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER' \
                ) \
                OR pg_catalog.has_any_column_privilege( \
                    $1, relation.oid, 'SELECT,INSERT,UPDATE,REFERENCES' \
                ))",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(relation_privileges, 0);
    let sequence_privileges = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_sequences AS sequence \
         WHERE sequence.schemaname <> 'information_schema' \
            AND pg_catalog.left(sequence.schemaname::TEXT, 3) <> 'pg_' \
            AND pg_catalog.has_sequence_privilege( \
                $1, \
                pg_catalog.format('%I.%I', sequence.schemaname, sequence.sequencename), \
                'USAGE,SELECT,UPDATE' \
            )",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(sequence_privileges, 0);
    let executable_routines = sqlx::query_scalar::<_, Vec<String>>(
        "SELECT COALESCE(pg_catalog.array_agg( \
                    pg_catalog.format( \
                        '%I.%I(%s)', \
                        namespace.nspname, \
                        function_row.proname, \
                        pg_catalog.pg_get_function_identity_arguments(function_row.oid) \
                    ) \
                    ORDER BY namespace.nspname, function_row.proname, function_row.oid \
                ), ARRAY[]::TEXT[]) \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = function_row.pronamespace \
         WHERE namespace.nspname <> 'information_schema' \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND pg_catalog.has_function_privilege($1, function_row.oid, 'EXECUTE')",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(executable_routines, vec![FUNCTION_IDENTITY.to_string()]);
    let grant_option_routines = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
            ON namespace.oid = function_row.pronamespace \
         WHERE namespace.nspname <> 'information_schema' \
            AND pg_catalog.left(namespace.nspname::TEXT, 3) <> 'pg_' \
            AND pg_catalog.has_function_privilege( \
                $1, function_row.oid, 'EXECUTE WITH GRANT OPTION' \
            )",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(grant_option_routines, 0);
    let identity_acl = sqlx::query_as::<_, (i64, i64)>(
        "SELECT pg_catalog.count(*), \
                pg_catalog.count(*) FILTER (WHERE privilege.is_grantable) \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
            function_row.proacl, \
            pg_catalog.acldefault('f', function_row.proowner) \
         )) AS privilege \
         WHERE function_row.oid = pg_catalog.to_regprocedure($2) \
            AND privilege.grantee = pg_catalog.to_regrole($1) \
            AND privilege.privilege_type = 'EXECUTE'",
    )
    .bind(role)
    .bind(FUNCTION_IDENTITY)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(identity_acl, (1, 0));
    let parameter_privileges = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_parameter_acl AS parameter_acl \
         CROSS JOIN LATERAL pg_catalog.aclexplode(parameter_acl.paracl) AS privilege \
         WHERE privilege.grantee IN (0, pg_catalog.to_regrole($1)) \
            AND privilege.privilege_type IN ('SET', 'ALTER SYSTEM')",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(parameter_privileges, 0);
    let large_object_privileges = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_largeobject_metadata AS large_object \
         WHERE large_object.lomowner = pg_catalog.to_regrole($1) \
            OR EXISTS ( \
                SELECT 1 \
                FROM pg_catalog.aclexplode(COALESCE( \
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
    assert_eq!(large_object_privileges, 0);
    for denied in [
        "public.starring_runtime_mutation_clock()",
        "public.starring_runtime_exact_target_reader_database_identity_v1()",
        "public.starring_runtime_panel_database_readiness_v1()",
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

async fn assert_object_permission_denied(pool: &PgPool, statement: &str) {
    let error = sqlx::query(statement)
        .execute(pool)
        .await
        .expect_err("restricted runtime convergence object capability must be denied");
    assert!(matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("42501")
    ));
}

#[tokio::test]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn convergence_database_identity_is_an_exact_object_capability() {
    run_isolated_database_test(|pool, connect_options, database_name, role| async move {
        MIGRATOR.run(&pool).await.unwrap();
        let contract = sqlx::query_as::<_, (i64, i64, bool, bool, bool)>(
            "SELECT function_row.proowner::BIGINT, \
                        pg_catalog.min(relation.relowner::BIGINT), \
                        pg_catalog.count(DISTINCT relation.relowner) = 1, \
                        function_row.prosecdef, \
                        function_row.proconfig \
                            IS NOT DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[] \
                 FROM pg_catalog.pg_proc AS function_row \
                 CROSS JOIN (VALUES \
                     (pg_catalog.to_regclass('public.product_control_plane_identity')), \
                     (pg_catalog.to_regclass('public.runtime_deployments')), \
                     (pg_catalog.to_regclass('public.runtime_attestations')), \
                     (pg_catalog.to_regclass('public.runtime_serving_leases')) \
                 ) AS expected(relation_oid) \
                 INNER JOIN pg_catalog.pg_class AS relation \
                    ON relation.oid = expected.relation_oid \
                 WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
                 GROUP BY function_row.proowner, function_row.prosecdef, function_row.proconfig",
        )
        .bind(FUNCTION_IDENTITY)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(contract.0, contract.1);
        assert_eq!((contract.2, contract.3, contract.4), (true, true, true));

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
        .bind(FUNCTION_IDENTITY)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(unexpected_grants, 0);

        let expected = sqlx::query_scalar::<_, String>(
            "SELECT identity.database_identity::TEXT \
                 FROM public.product_control_plane_identity AS identity \
                 WHERE identity.singleton",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let large_object_oid = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.lo_from_bytea( \
                0, pg_catalog.convert_to('runtime-convergence-identity-secret', 'UTF8') \
             )::BIGINT",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let password = format!("{role}_object_capability_password");
        install_object_capability_role(&pool, &database_name, &role, &password).await;
        assert_object_capability_boundary(&pool, &role).await;

        let restricted_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(connect_options.username(&role).password(&password))
            .await
            .unwrap();
        let reader = PostgresRuntimeConvergenceDatabaseIdentityReader::new(restricted_pool.clone());
        let observed = reader.database_identity().await.unwrap();
        assert_eq!(observed, expected);
        for statement in [
            "SELECT * FROM public.runtime_deployments LIMIT 1",
            "SELECT identity.database_identity \
                 FROM public.product_control_plane_identity AS identity",
            "SELECT pg_catalog.nextval('public.runtime_convergence_identity_acl_probe')",
            "SELECT public.starring_runtime_exact_target_reader_database_identity_v1()",
            "SELECT public.starring_runtime_mutation_clock()",
        ] {
            assert_object_permission_denied(&restricted_pool, statement).await;
        }
        assert_object_permission_denied(
            &restricted_pool,
            &format!("SELECT pg_catalog.lo_get({large_object_oid}::OID)"),
        )
        .await;
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {FUNCTION_IDENTITY} FROM {role}"
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert!(matches!(
            reader.database_identity().await.unwrap_err(),
            RuntimeConvergenceStoreError::DatabaseFailure
        ));
        restricted_pool.close().await;
    })
    .await;
}
