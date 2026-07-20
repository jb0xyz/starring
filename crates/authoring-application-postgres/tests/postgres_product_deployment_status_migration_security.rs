use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::MIGRATOR;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Connection;

const STATUS_MIGRATION: i64 = 202_607_200_001;
const OPERATIONAL_STATUS_MIGRATION: i64 = 202_607_200_004;
const STATUS_IDENTITY_FUNCTION: &str =
    "public.starring_product_deployment_status_reader_database_identity_v1()";
const STATUS_READ_FUNCTION: &str = "public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)";
const OPERATIONAL_STATUS_IDENTITY_FUNCTION: &str =
    "public.starring_product_deployment_status_reader_database_identity_v2()";
const OPERATIONAL_STATUS_CORE_FUNCTION: &str = "public.starring_product_deployment_status_read_core_v2(text,text,text,text,text,text,text,text,bytea)";
const OPERATIONAL_STATUS_READ_FUNCTION: &str = "public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)";
const CANONICAL_HELPER: &str = "public.starring_canonical_json_v1(JSONB)";
const STATUS_RESULT: &str = "TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone)";

static SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

struct StatusMigrationTestDatabase {
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

async fn isolated_database(label: &str) -> StatusMigrationTestDatabase {
    assert!(
        !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    let label = label.chars().take(12).collect::<String>();
    let name = format!("starring_status_{label}_test_{}", suffix());
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
    StatusMigrationTestDatabase {
        name,
        administrator,
        pool,
    }
}

async fn drop_isolated_database(database: StatusMigrationTestDatabase, roles: &[String]) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
    for role in roles {
        assert_safe_identifier(role);
        sqlx::query(&format!("DROP ROLE IF EXISTS {role}"))
            .execute(&mut administrator)
            .await
            .unwrap();
    }
}

async fn apply_pre_status_migrations(pool: &PgPool) {
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version < STATUS_MIGRATION)
    {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }
}

fn status_migration() -> &'static sqlx::migrate::Migration {
    MIGRATOR
        .iter()
        .find(|migration| migration.version == STATUS_MIGRATION)
        .expect("deployment status migration must exist")
}

async fn apply_pre_operational_status_migrations(pool: &PgPool) {
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version < OPERATIONAL_STATUS_MIGRATION)
    {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }
}

fn operational_status_migration() -> &'static sqlx::migrate::Migration {
    MIGRATOR
        .iter()
        .find(|migration| migration.version == OPERATIONAL_STATUS_MIGRATION)
        .expect("operational deployment status migration must exist")
}

async fn function_exists(pool: &PgPool, signature: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regprocedure($1) IS NOT NULL")
        .bind(signature)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn deployment_status_migration_is_bounded_lock_free_and_explicit() {
    let migration =
        include_str!("../../../migrations/202607200001_scope_product_deployment_status_reads.sql");
    let read_body = migration
        .split("CREATE FUNCTION public.starring_product_deployment_status_read_v1(")
        .nth(1)
        .unwrap()
        .split("$function$\n$definition$;")
        .next()
        .unwrap();
    for required in [
        "expected_deployment_id TEXT",
        "expected_promotion_id TEXT",
        "expected_desired_target_digest TEXT",
        "expected_principal_id TEXT",
        "expected_acting_discord_user_id TEXT",
        "expected_product_session_digest BYTEA",
        "pg_catalog.statement_timestamp()",
        "pg_catalog.octet_length(product_session.csrf_digest) = 32",
        "pg_catalog.octet_length(product_session.oauth_state_digest) = 32",
        "product_session.revoked_at IS NULL",
        "product_session.revocation_reason IS NULL",
        "product_session.authenticated_at = product_session.created_at",
        "product_session.last_seen_at + INTERVAL '30 minutes'",
        "product_session.authenticated_at + INTERVAL '12 hours'",
        "valid_request.database_now < product_session.idle_expires_at",
        "ON actor_deployment.request_matches",
        "'request_mismatch'::TEXT",
        "'record_authority_binding_revision'",
        "'record_activation_target_version'",
        "'authority_payload_digest', current_authority.authority_payload_digest",
        "LIMIT 2;",
    ] {
        assert!(
            read_body.contains(required),
            "missing status guard: {required}"
        );
    }
    for forbidden in [
        "FOR SHARE",
        "FOR UPDATE",
        "starring_runtime_lock_current_authority",
        "to_jsonb(",
        "'record', promotion.record",
        "INSERT INTO",
        "UPDATE public.",
        "DELETE FROM",
    ] {
        assert!(
            !read_body.contains(forbidden),
            "forbidden status capability behavior: {forbidden}"
        );
    }
    assert_eq!(read_body.matches("'evidence_format_version', 1").count(), 9);
    assert_eq!(
        read_body
            .matches("ON actor_deployment.request_matches")
            .count(),
        10
    );
    assert_eq!(migration.matches("\"relation\":").count(), 10);
    assert!(!migration.contains("CREATE ROLE"));
    assert!(!migration.contains("GRANT EXECUTE"));
    assert!(!migration.contains("REVOKE ALL PRIVILEGES ON TABLE"));
}

#[test]
fn operational_status_migration_preserves_v1_and_isolates_the_single_query_core() {
    let legacy =
        include_str!("../../../migrations/202607200001_scope_product_deployment_status_reads.sql");
    let migration = include_str!(
        "../../../migrations/202607200004_scope_product_operational_deployment_status_reads.sql"
    );
    let legacy_body = legacy
        .split("CREATE FUNCTION public.starring_product_deployment_status_read_v1(")
        .nth(1)
        .unwrap()
        .split("$function$\n$definition$;")
        .next()
        .unwrap();
    let core_body = migration
        .split("CREATE FUNCTION public.starring_product_deployment_status_read_core_v2(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    let v1_body = migration
        .split("CREATE OR REPLACE FUNCTION public.starring_product_deployment_status_read_v1(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    let v2_body = migration
        .split("CREATE FUNCTION public.starring_product_deployment_status_read_v2(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    let legacy_projection = legacy_body
        .split("    WITH request_clock AS MATERIALIZED")
        .nth(1)
        .unwrap()
        .split("        actor_deployment.database_now")
        .next()
        .unwrap();
    let core_projection = core_body
        .split("    WITH request_clock AS MATERIALIZED")
        .nth(1)
        .unwrap()
        .split("        actor_deployment.database_now")
        .next()
        .unwrap();
    let legacy_joins = legacy_body
        .split("    FROM actor_deployment")
        .nth(1)
        .unwrap();
    let core_joins = core_body.split("    FROM actor_deployment").nth(1).unwrap();
    assert_eq!(legacy_projection, core_projection);
    assert_eq!(legacy_joins, core_joins);
    assert_eq!(core_body.matches("'evidence_format_version', 1").count(), 9);
    for scalar in [
        "deployment_convergence_attempt_no BIGINT",
        "deployment_last_failure_attempt_no BIGINT",
        "attestation_convergence_attempt_no BIGINT",
        "actor_deployment.convergence_attempt_no",
        "actor_deployment.last_failure_attempt_no",
        "attestation.convergence_attempt_no",
    ] {
        assert!(
            core_body.contains(scalar),
            "missing operational scalar: {scalar}"
        );
    }
    for wrapper in [v1_body, v2_body] {
        assert_eq!(
            wrapper
                .matches("starring_product_deployment_status_read_core_v2(")
                .count(),
            1
        );
        for relation in [
            "public.runtime_deployments",
            "public.runtime_attestations",
            "public.runtime_serving_leases",
            "public.activation_requests",
            "public.authoring_promotions",
            "public.automation_ruleset_versions",
        ] {
            assert!(
                !wrapper.contains(relation),
                "wrapper reads relation: {relation}"
            );
        }
    }
    assert!(!v1_body.contains("deployment_convergence_attempt_no BIGINT"));
    assert!(!v1_body.contains("status.deployment_convergence_attempt_no"));
    assert!(v2_body.contains("status.deployment_convergence_attempt_no"));
    assert!(v2_body.contains("status.deployment_last_failure_attempt_no"));
    assert!(v2_body.contains("status.attestation_convergence_attempt_no"));
    assert!(migration.contains("IN ACCESS SHARE MODE"));
    assert!(migration.contains("CREATE OR REPLACE FUNCTION"));
    assert!(migration.contains(OPERATIONAL_STATUS_IDENTITY_FUNCTION));
    assert!(migration.contains("REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE"));
    assert!(migration.contains("REVOKE ALL PRIVILEGES ON ROUTINE %s FROM PUBLIC CASCADE"));
    assert!(migration
        .contains("ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC"));
    assert!(migration.contains("REVOKE ALL PRIVILEGES ON FUNCTIONS FROM %s"));
    assert!(migration.contains("user routine execution defaults are not sealed"));
    assert!(migration.contains("privilege.grantee <> common_owner"));
    assert!(migration.contains("function_row.proconfig\n            IS DISTINCT FROM ARRAY['search_path=pg_catalog']::TEXT[]"));
    assert!(!migration.contains("CREATE ROLE"));
    assert!(!migration.contains("GRANT EXECUTE"));
    assert!(!migration
        .lines()
        .any(|line| line.trim_start().starts_with("--")));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn operational_migration_preserves_v1_acl_and_strips_new_function_defaults() {
    let mut database = isolated_database("operational").await;
    let legacy_role = format!("status_legacy_{}", suffix());
    let hostile_role = format!("status_hostile_{}", suffix());
    assert_safe_identifier(&legacy_role);
    assert_safe_identifier(&hostile_role);
    let outcome = async {
        apply_pre_operational_status_migrations(&database.pool).await;
        for role in [&legacy_role, &hostile_role] {
            sqlx::query(&format!("CREATE ROLE {role} NOLOGIN"))
                .execute(&mut database.administrator)
                .await?;
        }
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {STATUS_IDENTITY_FUNCTION}, \
             {STATUS_READ_FUNCTION} TO {legacy_role}"
        ))
        .execute(&database.pool)
        .await?;
        sqlx::query(&format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             GRANT EXECUTE ON FUNCTIONS TO {hostile_role}"
        ))
        .execute(&database.pool)
        .await?;
        let legacy_acl_before = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT identity_function.proacl::TEXT, status_function.proacl::TEXT \
             FROM pg_catalog.pg_proc AS identity_function \
             CROSS JOIN pg_catalog.pg_proc AS status_function \
             WHERE identity_function.oid = pg_catalog.to_regprocedure($1) \
             AND status_function.oid = pg_catalog.to_regprocedure($2)",
        )
        .bind(STATUS_IDENTITY_FUNCTION)
        .bind(STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        let mut transaction = database.pool.begin().await?;
        sqlx::raw_sql(operational_status_migration().sql.as_ref())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        let legacy_acl_after = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT identity_function.proacl::TEXT, status_function.proacl::TEXT \
             FROM pg_catalog.pg_proc AS identity_function \
             CROSS JOIN pg_catalog.pg_proc AS status_function \
             WHERE identity_function.oid = pg_catalog.to_regprocedure($1) \
             AND status_function.oid = pg_catalog.to_regprocedure($2)",
        )
        .bind(STATUS_IDENTITY_FUNCTION)
        .bind(STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(legacy_acl_before, legacy_acl_after);
        for function in [
            OPERATIONAL_STATUS_IDENTITY_FUNCTION,
            OPERATIONAL_STATUS_CORE_FUNCTION,
            OPERATIONAL_STATUS_READ_FUNCTION,
        ] {
            let non_owner_acl = sqlx::query_scalar::<_, i64>(
                "SELECT pg_catalog.count(*) \
                 FROM pg_catalog.pg_proc AS function_row \
                 CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
                    function_row.proacl, \
                    pg_catalog.acldefault('f', function_row.proowner) \
                 )) AS privilege \
                 WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
                 AND privilege.grantee <> function_row.proowner",
            )
            .bind(function)
            .fetch_one(&database.pool)
            .await?;
            assert_eq!(non_owner_acl, 0);
            let hostile_execute = sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
            )
            .bind(&hostile_role)
            .bind(function)
            .fetch_one(&database.pool)
            .await?;
            assert!(!hostile_execute);
        }
        let legacy_v1_execute = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
        )
        .bind(&legacy_role)
        .bind(STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        let legacy_v2_execute = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
        )
        .bind(&legacy_role)
        .bind(OPERATIONAL_STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert!(legacy_v1_execute);
        assert!(!legacy_v2_execute);
        sqlx::query(
            "CREATE FUNCTION public.operational_plain_invoker_probe() RETURNS INTEGER \
             LANGUAGE sql VOLATILE STRICT SET search_path = pg_catalog AS 'SELECT 1'",
        )
        .execute(&database.pool)
        .await?;
        let hostile_plain_execute = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege( \
             $1, 'public.operational_plain_invoker_probe()', 'EXECUTE')",
        )
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await?;
        assert!(!hostile_plain_execute);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &[legacy_role, hostile_role]).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn operational_migration_collision_rolls_back_without_mutating_v1() {
    let database = isolated_database("opcollision").await;
    let outcome = async {
        apply_pre_operational_status_migrations(&database.pool).await;
        let legacy_definition = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure($1))",
        )
        .bind(STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        sqlx::query(
            "CREATE FUNCTION public.starring_product_deployment_status_read_core_v2(TEXT) \
             RETURNS TEXT LANGUAGE sql AS 'SELECT $1'",
        )
        .execute(&database.pool)
        .await?;
        let mut transaction = database.pool.begin().await?;
        let error = sqlx::raw_sql(operational_status_migration().sql.as_ref())
            .execute(&mut *transaction)
            .await
            .expect_err("migration must reject an operational status function name collision");
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
        ));
        transaction.rollback().await?;
        let preserved_definition = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure($1))",
        )
        .bind(STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(legacy_definition, preserved_definition);
        assert!(
            function_exists(
                &database.pool,
                "public.starring_product_deployment_status_read_core_v2(text)"
            )
            .await
        );
        assert!(!function_exists(&database.pool, OPERATIONAL_STATUS_CORE_FUNCTION).await);
        assert!(!function_exists(&database.pool, OPERATIONAL_STATUS_READ_FUNCTION).await);
        assert!(!function_exists(&database.pool, OPERATIONAL_STATUS_IDENTITY_FUNCTION).await);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &[]).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn operational_migration_rejects_support_function_metadata_drift() {
    let database = isolated_database("opsupport").await;
    let outcome = async {
        apply_pre_operational_status_migrations(&database.pool).await;

        sqlx::query(&format!("ALTER FUNCTION {CANONICAL_HELPER} VOLATILE"))
            .execute(&database.pool)
            .await?;
        let mut transaction = database.pool.begin().await?;
        let error = sqlx::raw_sql(operational_status_migration().sql.as_ref())
            .execute(&mut *transaction)
            .await
            .expect_err("migration must reject support helper volatility drift");
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
        ));
        transaction.rollback().await?;
        sqlx::query(&format!("ALTER FUNCTION {CANONICAL_HELPER} IMMUTABLE"))
            .execute(&database.pool)
            .await?;

        sqlx::query(
            "ALTER FUNCTION public.validate_runtime_deployment_projection() \
             SECURITY INVOKER",
        )
        .execute(&database.pool)
        .await?;
        let mut transaction = database.pool.begin().await?;
        let error = sqlx::raw_sql(operational_status_migration().sql.as_ref())
            .execute(&mut *transaction)
            .await
            .expect_err("migration must reject support function security drift");
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
        ));
        transaction.rollback().await?;
        sqlx::query(
            "ALTER FUNCTION public.validate_runtime_deployment_projection() \
             SECURITY DEFINER",
        )
        .execute(&database.pool)
        .await?;

        sqlx::query("ALTER FUNCTION public.validate_runtime_deployment_projection() RESET ALL")
            .execute(&database.pool)
            .await?;
        let mut transaction = database.pool.begin().await?;
        let error = sqlx::raw_sql(operational_status_migration().sql.as_ref())
            .execute(&mut *transaction)
            .await
            .expect_err("migration must reject support function search path drift");
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
        ));
        transaction.rollback().await?;
        sqlx::query(
            "ALTER FUNCTION public.validate_runtime_deployment_projection() \
             SET search_path = pg_catalog",
        )
        .execute(&database.pool)
        .await?;

        let mut transaction = database.pool.begin().await?;
        sqlx::raw_sql(operational_status_migration().sql.as_ref())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        assert!(function_exists(&database.pool, OPERATIONAL_STATUS_IDENTITY_FUNCTION).await);
        assert!(function_exists(&database.pool, OPERATIONAL_STATUS_CORE_FUNCTION).await);
        assert!(function_exists(&database.pool, OPERATIONAL_STATUS_READ_FUNCTION).await);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &[]).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn migration_collision_rolls_back_without_status_function_residue() {
    let database = isolated_database("collision").await;
    let outcome = async {
        apply_pre_status_migrations(&database.pool).await;
        sqlx::query(
            "CREATE FUNCTION public.starring_product_deployment_status_read_v1(TEXT) \
             RETURNS TEXT LANGUAGE sql AS 'SELECT $1'",
        )
        .execute(&database.pool)
        .await?;
        let error = sqlx::raw_sql(status_migration().sql.as_ref())
            .execute(&database.pool)
            .await
            .expect_err("migration must reject a status function name collision");
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
        ));
        assert!(!function_exists(&database.pool, STATUS_IDENTITY_FUNCTION).await);
        assert!(!function_exists(&database.pool, STATUS_READ_FUNCTION).await);
        assert!(
            function_exists(
                &database.pool,
                "public.starring_product_deployment_status_read_v1(text)"
            )
            .await
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &[]).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn migration_rejects_runtime_trigger_drift_without_function_residue() {
    let database = isolated_database("trigger").await;
    let outcome = async {
        apply_pre_status_migrations(&database.pool).await;
        sqlx::query(
            "ALTER TABLE public.runtime_deployments \
             DISABLE TRIGGER runtime_deployments_validate_projection",
        )
        .execute(&database.pool)
        .await?;
        let error = sqlx::raw_sql(status_migration().sql.as_ref())
            .execute(&database.pool)
            .await
            .expect_err("migration must reject disabled persisted-evidence protection");
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
        ));
        assert!(!function_exists(&database.pool, STATUS_IDENTITY_FUNCTION).await);
        assert!(!function_exists(&database.pool, STATUS_READ_FUNCTION).await);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &[]).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn hostile_defaults_are_stripped_and_restricted_role_has_only_status_capability() {
    let mut database = isolated_database("restricted").await;
    let hostile_role = format!("status_hostile_{}", suffix());
    let reader_role = format!("status_reader_{}", suffix());
    assert_safe_identifier(&hostile_role);
    assert_safe_identifier(&reader_role);
    let outcome = async {
        apply_pre_status_migrations(&database.pool).await;
        sqlx::query(&format!("CREATE ROLE {hostile_role} NOLOGIN"))
            .execute(&mut database.administrator)
            .await?;
        sqlx::query(&format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             GRANT EXECUTE ON FUNCTIONS TO {hostile_role}"
        ))
        .execute(&database.pool)
        .await?;
        let mut settings_connection = database.pool.acquire().await?;
        sqlx::query("SET search_path TO public, pg_catalog")
            .execute(&mut *settings_connection)
            .await?;
        sqlx::query("SET quote_all_identifiers TO on")
            .execute(&mut *settings_connection)
            .await?;
        sqlx::raw_sql(status_migration().sql.as_ref())
            .execute(&mut *settings_connection)
            .await?;
        let settings = sqlx::query_as::<_, (String, String)>(
            "SELECT pg_catalog.current_setting('search_path'), \
             pg_catalog.current_setting('quote_all_identifiers')",
        )
        .fetch_one(&mut *settings_connection)
        .await?;
        assert_eq!(settings, ("public, pg_catalog".to_string(), "on".to_string()));
        sqlx::query("RESET search_path")
            .execute(&mut *settings_connection)
            .await?;
        sqlx::query("SET quote_all_identifiers TO off")
            .execute(&mut *settings_connection)
            .await?;
        drop(settings_connection);
        let contract = sqlx::query_as::<_, (String, String, String, bool, bool, String, bool, f32)>(
            "SELECT language_row.lanname::TEXT, function_row.provolatile::TEXT, \
             function_row.proparallel::TEXT, function_row.proisstrict, \
             function_row.prosecdef, \
             pg_catalog.pg_get_function_identity_arguments(function_row.oid), \
             function_row.proretset, function_row.prorows \
             FROM pg_catalog.pg_proc AS function_row \
             INNER JOIN pg_catalog.pg_language AS language_row \
               ON language_row.oid = function_row.prolang \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(contract.0, "sql");
        assert_eq!(contract.1, "v");
        assert_eq!(contract.2, "u");
        assert!(contract.3);
        assert!(contract.4);
        assert_eq!(contract.5, "expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea");
        assert!(contract.6);
        assert_eq!(contract.7, 1.0);
        let result = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_function_result(pg_catalog.to_regprocedure($1))",
        )
        .bind(STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(result, STATUS_RESULT);
        for signature in [STATUS_IDENTITY_FUNCTION, STATUS_READ_FUNCTION] {
            let leaked = sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
            )
            .bind(&hostile_role)
            .bind(signature)
            .fetch_one(&database.pool)
            .await?;
            assert!(!leaked, "hostile default leaked {signature}");
        }
        let non_owner_acl_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_proc AS function_row \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(\
              function_row.proacl, pg_catalog.acldefault('f', function_row.proowner)\
             )) AS privilege \
             WHERE function_row.oid IN (\
              pg_catalog.to_regprocedure($1), pg_catalog.to_regprocedure($2)\
             ) AND privilege.grantee <> function_row.proowner",
        )
        .bind(STATUS_IDENTITY_FUNCTION)
        .bind(STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(non_owner_acl_count, 0);
        sqlx::query(&format!(
            "CREATE ROLE {reader_role} LOGIN NOINHERIT NOSUPERUSER NOCREATEDB \
             NOCREATEROLE NOREPLICATION NOBYPASSRLS"
        ))
        .execute(&mut database.administrator)
        .await?;
        sqlx::query(&format!(
            "REVOKE TEMPORARY ON DATABASE {} FROM PUBLIC",
            database.name
        ))
        .execute(&database.pool)
        .await?;
        sqlx::query(&format!(
            "GRANT CONNECT ON DATABASE {} TO {reader_role}",
            database.name
        ))
        .execute(&database.pool)
        .await?;
        sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {reader_role}"))
            .execute(&database.pool)
            .await?;
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {STATUS_IDENTITY_FUNCTION}, \
             {STATUS_READ_FUNCTION} TO {reader_role}"
        ))
        .execute(&database.pool)
        .await?;
        let role_contract = sqlx::query_as::<_, (bool, bool, bool, bool, bool, bool, bool)>(
            "SELECT rolcanlogin, rolinherit, rolsuper, rolcreatedb, rolcreaterole, \
             rolreplication, rolbypassrls \
             FROM pg_catalog.pg_roles WHERE rolname = $1",
        )
        .bind(&reader_role)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(role_contract, (true, false, false, false, false, false, false));
        let role_scope = sqlx::query_as::<_, (bool, bool, bool, bool, bool)>(
            "SELECT pg_catalog.has_database_privilege($1, pg_catalog.current_database(), 'CONNECT'), \
             pg_catalog.has_database_privilege($1, pg_catalog.current_database(), 'CREATE'), \
             pg_catalog.has_database_privilege($1, pg_catalog.current_database(), 'TEMPORARY'), \
             pg_catalog.has_schema_privilege($1, 'public', 'USAGE'), \
             pg_catalog.has_schema_privilege($1, 'public', 'CREATE')",
        )
        .bind(&reader_role)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(role_scope, (true, false, false, true, false));
        let membership_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM pg_catalog.pg_auth_members \
             WHERE roleid = pg_catalog.to_regrole($1) \
                OR member = pg_catalog.to_regrole($1)",
        )
        .bind(&reader_role)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(membership_count, 0);
        let executable_names = sqlx::query_scalar::<_, Vec<String>>(
            "SELECT COALESCE(pg_catalog.array_agg(function_row.proname::TEXT \
              ORDER BY function_row.proname), ARRAY[]::TEXT[]) \
             FROM pg_catalog.pg_proc AS function_row \
             INNER JOIN pg_catalog.pg_namespace AS namespace \
               ON namespace.oid = function_row.pronamespace \
             WHERE namespace.nspname = 'public' \
               AND (function_row.proname LIKE 'starring_%' OR function_row.prosecdef) \
               AND pg_catalog.has_function_privilege($1, function_row.oid, 'EXECUTE')",
        )
        .bind(&reader_role)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(
            executable_names,
            vec![
                "starring_product_deployment_status_read_v1".to_string(),
                "starring_product_deployment_status_reader_database_identity_v1".to_string(),
            ]
        );
        let grant_option_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_proc AS function_row \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(\
              function_row.proacl, pg_catalog.acldefault('f', function_row.proowner)\
             )) AS privilege \
             WHERE function_row.oid IN (\
              pg_catalog.to_regprocedure($2), pg_catalog.to_regprocedure($3)\
             ) AND privilege.grantee = pg_catalog.to_regrole($1) \
               AND privilege.is_grantable",
        )
        .bind(&reader_role)
        .bind(STATUS_IDENTITY_FUNCTION)
        .bind(STATUS_READ_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(grant_option_count, 0);
        let relation_privilege_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM (VALUES \
              ('public.product_control_plane_identity'), \
              ('public.product_principals'), \
              ('public.product_auth_sessions'), \
              ('public.runtime_deployments'), \
              ('public.activation_requests'), \
              ('public.authoring_promotions'), \
              ('public.product_tenants'), \
              ('public.automation_installations'), \
              ('public.automation_installation_authority_versions'), \
              ('public.automation_ruleset_activations'), \
              ('public.automation_ruleset_versions'), \
              ('public.runtime_attestations'), \
              ('public.runtime_serving_leases')\
             ) AS relation(name) \
             WHERE pg_catalog.has_table_privilege($1, relation.name, \
              'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') \
                OR pg_catalog.has_any_column_privilege($1, relation.name, \
                 'SELECT,INSERT,UPDATE,REFERENCES')",
        )
        .bind(&reader_role)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(relation_privilege_count, 0);
        let mut capability = database.pool.begin().await?;
        sqlx::query(&format!("SET LOCAL ROLE {reader_role}"))
            .execute(&mut *capability)
            .await?;
        let identity = sqlx::query_scalar::<_, String>(
            "SELECT public.starring_product_deployment_status_reader_database_identity_v1()",
        )
        .fetch_one(&mut *capability)
        .await?;
        assert!(!identity.is_empty());
        let rows = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.starring_product_deployment_status_read_v1(\
              '', '', '', '', '', '', '', '', pg_catalog.decode('', 'hex'))",
        )
        .fetch_one(&mut *capability)
        .await?;
        assert_eq!(rows, 0);
        let relation_privilege = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_table_privilege(\
              current_user, 'public.runtime_deployments', 'SELECT') \
             OR pg_catalog.has_any_column_privilege(\
              current_user, 'public.runtime_deployments', 'SELECT')",
        )
        .fetch_one(&mut *capability)
        .await?;
        assert!(!relation_privilege);
        capability.commit().await?;
        let mut denied = database.pool.begin().await?;
        sqlx::query(&format!("SET LOCAL ROLE {reader_role}"))
            .execute(&mut *denied)
            .await?;
        let error = sqlx::query("SELECT deployment_id FROM public.runtime_deployments LIMIT 1")
            .execute(&mut *denied)
            .await
            .expect_err("status reader must not select a protected relation");
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("42501")
        ));
        denied.rollback().await?;
        let mut unrelated = database.pool.begin().await?;
        sqlx::query(&format!("SET LOCAL ROLE {reader_role}"))
            .execute(&mut *unrelated)
            .await?;
        let error = sqlx::query(
            "SELECT public.starring_product_apply_executor_database_identity_v1()",
        )
        .execute(&mut *unrelated)
        .await
        .expect_err("status reader must not execute an unrelated protected function");
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("42501")
        ));
        unrelated.rollback().await?;
        let mut temporary = database.pool.begin().await?;
        sqlx::query(&format!("SET LOCAL ROLE {reader_role}"))
            .execute(&mut *temporary)
            .await?;
        let error = sqlx::query("CREATE TEMPORARY TABLE status_reader_escape(value INTEGER)")
            .execute(&mut *temporary)
            .await
            .expect_err("status reader must not create temporary objects");
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("42501")
        ));
        temporary.rollback().await?;
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &[hostile_role, reader_role]).await;
    outcome.unwrap();
}
