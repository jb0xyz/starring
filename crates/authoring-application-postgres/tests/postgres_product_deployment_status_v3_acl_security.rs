use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::MIGRATOR;
use sqlx::migrate::Migrate;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Connection;

const STATUS_V3_MIGRATION: i64 = 202_608_020_001;
const BASIC_IDENTITY: &str =
    "public.starring_product_deployment_status_reader_database_identity_v1()";
const OPERATIONAL_IDENTITY: &str =
    "public.starring_product_deployment_status_reader_database_identity_v2()";
const LEGACY_BASIC: &str = "public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)";
const LEGACY_OPERATIONAL: &str = "public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)";
const STATUS_CORE_V3: &str = "public.starring_product_deployment_status_read_core_v3(text,text,text,text,text,text,text,text,bytea)";
const STATUS_BASIC_V3: &str = "public.starring_product_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)";
const STATUS_OPERATIONAL_V3: &str = "public.starring_product_operational_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)";

static SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDatabase {
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
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
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

async fn isolated_database() -> TestDatabase {
    let name = format!("starring_status_v3_acl_test_{}", suffix());
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
        .max_connections(1)
        .connect_with(base.database(&name))
        .await
        .unwrap();
    TestDatabase {
        name,
        administrator,
        pool,
    }
}

async fn drop_isolated_database(database: TestDatabase, roles: &[String]) {
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

async fn apply_pre_v3_migrations(pool: &PgPool) {
    let mut connection = pool.acquire().await.unwrap();
    let connection = &mut *connection;
    connection.ensure_migrations_table().await.unwrap();
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version < STATUS_V3_MIGRATION)
    {
        connection.apply(migration).await.unwrap();
    }
}

fn status_v3_migration() -> &'static sqlx::migrate::Migration {
    MIGRATOR
        .iter()
        .find(|migration| migration.version == STATUS_V3_MIGRATION)
        .expect("deployment status V3 migration must exist")
}

async fn function_exists(pool: &PgPool, signature: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regprocedure($1) IS NOT NULL")
        .bind(signature)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn direct_acl(pool: &PgPool, signature: &str, role: &str) -> (i64, bool) {
    sqlx::query_as::<_, (i64, bool)>(
        "SELECT pg_catalog.count(*), \
         COALESCE(pg_catalog.bool_or(privilege.is_grantable), FALSE) \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(\
            function_row.proacl, \
            pg_catalog.acldefault('f', function_row.proowner) \
         )) AS privilege \
         WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
         AND privilege.grantee = pg_catalog.to_regrole($2)",
    )
    .bind(signature)
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn public_acl_count(pool: &PgPool, signature: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(\
            function_row.proacl, \
            pg_catalog.acldefault('f', function_row.proowner) \
         )) AS privilege \
         WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
         AND privilege.grantee = 0",
    )
    .bind(signature)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn non_owner_acl_count(pool: &PgPool, signature: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_proc AS function_row \
         CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(\
            function_row.proacl, \
            pg_catalog.acldefault('f', function_row.proowner) \
         )) AS privilege \
         WHERE function_row.oid = pg_catalog.to_regprocedure($1) \
         AND privilege.grantee <> function_row.proowner",
    )
    .bind(signature)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_v3_absent(pool: &PgPool) {
    for signature in [STATUS_CORE_V3, STATUS_BASIC_V3, STATUS_OPERATIONAL_V3] {
        assert!(!function_exists(pool, signature).await);
    }
}

async fn reject_v3_migration(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let error = sqlx::raw_sql(status_v3_migration().sql.as_ref())
        .execute(&mut *transaction)
        .await
        .expect_err("unsafe ACL migration must fail closed");
    assert!(matches!(
        error,
        sqlx::Error::Database(ref database) if database.code().as_deref() == Some("RE001")
    ));
    transaction.rollback().await?;
    assert_v3_absent(pool).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn v3_acl_transfer_is_exact_and_rejects_unsafe_shapes_atomically() {
    let mut database = isolated_database().await;
    let basic_role = format!("status_v3_basic_{}", suffix());
    let operational_role = format!("status_v3_operational_{}", suffix());
    let ambiguous_role = format!("status_v3_ambiguous_{}", suffix());
    let roles = [
        basic_role.clone(),
        operational_role.clone(),
        ambiguous_role.clone(),
    ];
    for role in &roles {
        assert_safe_identifier(role);
    }
    let outcome = async {
        apply_pre_v3_migrations(&database.pool).await;
        for role in &roles {
            sqlx::query(&format!("CREATE ROLE {role} NOLOGIN"))
                .execute(&mut database.administrator)
                .await?;
        }

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {LEGACY_BASIC} TO PUBLIC"
        ))
        .execute(&database.pool)
        .await?;
        reject_v3_migration(&database.pool).await?;
        assert_eq!(public_acl_count(&database.pool, LEGACY_BASIC).await, 1);
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {LEGACY_BASIC} FROM PUBLIC"
        ))
        .execute(&database.pool)
        .await?;

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {BASIC_IDENTITY} TO {basic_role}"
        ))
        .execute(&database.pool)
        .await?;
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {LEGACY_BASIC} TO {basic_role} WITH GRANT OPTION"
        ))
        .execute(&database.pool)
        .await?;
        reject_v3_migration(&database.pool).await?;
        assert_eq!(direct_acl(&database.pool, LEGACY_BASIC, &basic_role).await, (1, true));
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION {LEGACY_BASIC}, {BASIC_IDENTITY} FROM {basic_role} CASCADE"
        ))
        .execute(&database.pool)
        .await?;

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {BASIC_IDENTITY}, {OPERATIONAL_IDENTITY}, \
             {LEGACY_BASIC}, {LEGACY_OPERATIONAL} TO {ambiguous_role}"
        ))
        .execute(&database.pool)
        .await?;
        reject_v3_migration(&database.pool).await?;
        assert_eq!(direct_acl(&database.pool, LEGACY_BASIC, &ambiguous_role).await, (1, false));
        assert_eq!(
            direct_acl(&database.pool, LEGACY_OPERATIONAL, &ambiguous_role).await,
            (1, false)
        );
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION {BASIC_IDENTITY}, {OPERATIONAL_IDENTITY}, \
             {LEGACY_BASIC}, {LEGACY_OPERATIONAL} FROM {ambiguous_role} CASCADE"
        ))
        .execute(&database.pool)
        .await?;

        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {BASIC_IDENTITY}, {LEGACY_BASIC} TO {basic_role}"
        ))
        .execute(&database.pool)
        .await?;
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {OPERATIONAL_IDENTITY}, {LEGACY_OPERATIONAL} \
             TO {operational_role}"
        ))
        .execute(&database.pool)
        .await?;
        let mut transaction = database.pool.begin().await?;
        sqlx::raw_sql(status_v3_migration().sql.as_ref())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        for signature in [STATUS_CORE_V3, STATUS_BASIC_V3, STATUS_OPERATIONAL_V3] {
            assert!(function_exists(&database.pool, signature).await);
        }
        assert_eq!(non_owner_acl_count(&database.pool, STATUS_CORE_V3).await, 0);
        assert_eq!(non_owner_acl_count(&database.pool, LEGACY_BASIC).await, 0);
        assert_eq!(
            non_owner_acl_count(&database.pool, LEGACY_OPERATIONAL).await,
            0
        );
        assert_eq!(direct_acl(&database.pool, STATUS_BASIC_V3, &basic_role).await, (1, false));
        assert_eq!(
            direct_acl(
                &database.pool,
                STATUS_OPERATIONAL_V3,
                &operational_role
            )
            .await,
            (1, false)
        );
        assert_eq!(
            direct_acl(&database.pool, STATUS_OPERATIONAL_V3, &basic_role).await,
            (0, false)
        );
        assert_eq!(
            direct_acl(&database.pool, STATUS_BASIC_V3, &operational_role).await,
            (0, false)
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &roles).await;
    outcome.unwrap();
}
