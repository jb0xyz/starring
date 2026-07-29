use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::{
    PostgresProductControl, PostgresProductDecisions, PostgresProductLifecycleCancellations,
    PostgresProductRejections, ProductDecisionDatabasePoolsV1, ProductDecisionDigestKeyV1,
    ProductDecisionDigestKeyringV1, ProductDecisionReadinessErrorV1, MIGRATOR,
};
use futures::FutureExt;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Connection;

const READER_FUNCTIONS: [&str; 2] = [
    "public.starring_product_decision_reader_database_identity_v1()",
    "public.starring_product_decision_read_v1(text,text,text,text,text,text,bytea)",
];
const APPROVAL_FUNCTIONS: [&str; 3] = [
    "public.starring_product_approval_executor_database_identity_v1()",
    "public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_approval_keyring_coverage_v1(text[],text[])",
];
const REJECTION_FUNCTIONS: [&str; 3] = [
    "public.starring_product_rejection_executor_database_identity_v1()",
    "public.starring_product_rejection_keyring_coverage_v1(text[],text[])",
    "public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)",
];
const APPLY_FUNCTIONS: [&str; 7] = [
    "public.starring_product_apply_executor_database_identity_v1()",
    "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    "public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)",
    "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)",
    "public.starring_product_apply_keyring_coverage_v1(text[],text[])",
    "public.starring_product_apply_begin_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text)",
    "public.starring_product_apply_consume_runtime_drain_v2(text,text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,bigint,bytea,text,text,text,bigint,text,text,bytea,text,bytea,text,text,bytea)",
];
const CANCELLATION_FUNCTIONS: [&str; 3] = [
    "public.starring_product_lifecycle_cancellation_executor_database_identity_v1()",
    "public.starring_product_lifecycle_cancellation_keyring_coverage_v1(text[],text[])",
    "public.starring_product_cancel_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text,bigint,text,text,bigint)",
];

static SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDatabase {
    name: String,
    administrator: PgConnection,
    pool: PgPool,
}

struct BoundaryRoles {
    owner: String,
    reader: String,
    approval: String,
    rejection: String,
    apply: String,
    cancellation: String,
    reader_password: String,
    approval_password: String,
    rejection_password: String,
    apply_password: String,
    cancellation_password: String,
}

impl BoundaryRoles {
    fn new(label: &str) -> Self {
        let suffix = suffix();
        let roles = Self {
            owner: format!("starring_pcr_{label}_owner_{suffix}"),
            reader: format!("starring_pcr_{label}_reader_{suffix}"),
            approval: format!("starring_pcr_{label}_approval_{suffix}"),
            rejection: format!("starring_pcr_{label}_rejection_{suffix}"),
            apply: format!("starring_pcr_{label}_apply_{suffix}"),
            cancellation: format!("starring_pcr_{label}_cancellation_{suffix}"),
            reader_password: password(),
            approval_password: password(),
            rejection_password: password(),
            apply_password: password(),
            cancellation_password: password(),
        };
        for role in roles.names() {
            assert_safe_identifier(&role);
        }
        roles
    }

    fn names(&self) -> Vec<String> {
        vec![
            self.owner.clone(),
            self.reader.clone(),
            self.approval.clone(),
            self.rejection.clone(),
            self.apply.clone(),
            self.cancellation.clone(),
        ]
    }
}

struct BoundaryPools {
    reader: PgPool,
    approval: PgPool,
    rejection: PgPool,
    apply: PgPool,
    cancellation: PgPool,
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

fn password() -> String {
    let mut material = [0_u8; 32];
    getrandom::fill(&mut material).unwrap();
    material.iter().map(|byte| format!("{byte:02x}")).collect()
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

async fn isolated_database(label: &str) -> TestDatabase {
    assert_safe_identifier(label);
    let name = format!("starring_control_{label}_test_{}", suffix());
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
        .max_connections(8)
        .connect_with(base.database(&name))
        .await
        .unwrap();
    TestDatabase {
        name,
        administrator,
        pool,
    }
}

async fn destroy_database(database: TestDatabase, roles: &[String]) {
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

async fn create_owner(pool: &PgPool, role: &str) {
    sqlx::query(&format!(
        "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(pool)
    .await
    .unwrap();
}

async fn create_login(pool: &PgPool, role: &str, password: &str) {
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
}

async fn normalize_public_ownership(pool: &PgPool, owner: &str) {
    let relations = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.format('%I.%I', namespace.nspname, relation.relname) \
         FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
          ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname = 'public' AND relation.relkind IN ('r', 'p') \
         ORDER BY relation.oid",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    for relation in relations {
        sqlx::query(&format!("ALTER TABLE {relation} OWNER TO {owner}"))
            .execute(pool)
            .await
            .unwrap();
    }
    let functions = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.format( \
          '%I.%I(%s)', namespace.nspname, function_row.proname, \
          pg_catalog.pg_get_function_identity_arguments(function_row.oid)) \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
          ON namespace.oid = function_row.pronamespace \
         WHERE namespace.nspname IN ('public', 'starring_runtime_private_v2') \
          AND function_row.prokind = 'f' \
         ORDER BY function_row.oid",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    for function in functions {
        sqlx::query(&format!("ALTER FUNCTION {function} OWNER TO {owner}"))
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query(&format!("ALTER SCHEMA public OWNER TO {owner}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "ALTER SCHEMA starring_runtime_private_v2 OWNER TO {owner}"
    ))
    .execute(pool)
    .await
    .unwrap();
}

async fn grant_functions(pool: &PgPool, role: &str, functions: &[&str]) {
    for function in functions {
        sqlx::query(&format!("GRANT EXECUTE ON FUNCTION {function} TO {role}"))
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn provision_boundary(database: &TestDatabase, roles: &BoundaryRoles) {
    MIGRATOR.run(&database.pool).await.unwrap();
    create_owner(&database.pool, &roles.owner).await;
    for (role, password) in [
        (&roles.reader, &roles.reader_password),
        (&roles.approval, &roles.approval_password),
        (&roles.rejection, &roles.rejection_password),
        (&roles.apply, &roles.apply_password),
        (&roles.cancellation, &roles.cancellation_password),
    ] {
        create_login(&database.pool, role, password).await;
    }
    normalize_public_ownership(&database.pool, &roles.owner).await;
    sqlx::query(&format!(
        "REVOKE ALL PRIVILEGES ON DATABASE {} FROM PUBLIC",
        database.name
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query("REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC")
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query("REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM PUBLIC")
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query("REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM PUBLIC")
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query("REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC")
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {} TO {}, {}, {}, {}, {}",
        database.name,
        roles.reader,
        roles.approval,
        roles.rejection,
        roles.apply,
        roles.cancellation
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {}, {}, {}, {}, {}, {}",
        roles.owner, roles.reader, roles.approval, roles.rejection, roles.apply, roles.cancellation
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    grant_functions(&database.pool, &roles.reader, &READER_FUNCTIONS).await;
    grant_functions(&database.pool, &roles.approval, &APPROVAL_FUNCTIONS).await;
    grant_functions(&database.pool, &roles.rejection, &REJECTION_FUNCTIONS).await;
    grant_functions(&database.pool, &roles.apply, &APPLY_FUNCTIONS).await;
    grant_functions(&database.pool, &roles.cancellation, &CANCELLATION_FUNCTIONS).await;
}

async fn role_pool(database: &str, role: &str, password: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(4)
        .connect_with(
            database_url()
                .parse::<PgConnectOptions>()
                .unwrap()
                .database(database)
                .username(role)
                .password(password),
        )
        .await
        .unwrap()
}

async fn boundary_pools(database: &TestDatabase, roles: &BoundaryRoles) -> BoundaryPools {
    BoundaryPools {
        reader: role_pool(&database.name, &roles.reader, &roles.reader_password).await,
        approval: role_pool(&database.name, &roles.approval, &roles.approval_password).await,
        rejection: role_pool(&database.name, &roles.rejection, &roles.rejection_password).await,
        apply: role_pool(&database.name, &roles.apply, &roles.apply_password).await,
        cancellation: role_pool(
            &database.name,
            &roles.cancellation,
            &roles.cancellation_password,
        )
        .await,
    }
}

fn keyring() -> ProductDecisionDigestKeyringV1 {
    ProductDecisionDigestKeyringV1::new(
        ProductDecisionDigestKeyV1::from_bytes(
            "control-readiness-v1",
            std::array::from_fn(|index| 73_u8.wrapping_add(index as u8)),
        )
        .unwrap(),
        [],
    )
    .unwrap()
}

fn control(
    reader: PgPool,
    approval: PgPool,
    rejection: PgPool,
    apply: PgPool,
    cancellation: PgPool,
) -> PostgresProductControl {
    PostgresProductControl::new(
        ProductDecisionDatabasePoolsV1::new(reader, approval, apply),
        rejection,
        cancellation,
        keyring(),
    )
    .unwrap()
}

async fn assert_excess(control: &PostgresProductControl) {
    assert_eq!(
        control.verify_readiness().await,
        Err(ProductDecisionReadinessErrorV1::ExcessCapability)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_control_readiness_requires_five_exact_direct_login_capabilities() {
    let primary = isolated_database("readiness_a").await;
    let secondary = isolated_database("readiness_b").await;
    let primary_roles = BoundaryRoles::new("a");
    let secondary_roles = BoundaryRoles::new("b");
    let primary_role_names = primary_roles.names();
    let secondary_role_names = secondary_roles.names();

    let outcome = AssertUnwindSafe(async {
        provision_boundary(&primary, &primary_roles).await;
        provision_boundary(&secondary, &secondary_roles).await;
        let primary_pools = boundary_pools(&primary, &primary_roles).await;
        let secondary_pools = boundary_pools(&secondary, &secondary_roles).await;
        let decisions = PostgresProductDecisions::new(
            ProductDecisionDatabasePoolsV1::new(
                primary_pools.reader.clone(),
                primary_pools.approval.clone(),
                primary_pools.apply.clone(),
            ),
            keyring(),
        )
        .unwrap();
        decisions.verify_decision_reader_readiness().await.unwrap();
        decisions
            .verify_approval_executor_readiness()
            .await
            .unwrap();
        decisions.verify_apply_executor_readiness().await.unwrap();
        PostgresProductRejections::new(primary_pools.rejection.clone(), keyring())
            .unwrap()
            .verify_product_rejection_readiness()
            .await
            .unwrap();
        PostgresProductLifecycleCancellations::new(primary_pools.cancellation.clone(), keyring())
            .unwrap()
            .verify_product_lifecycle_cancellation_readiness()
            .await
            .unwrap();
        let primary_control = control(
            primary_pools.reader.clone(),
            primary_pools.approval.clone(),
            primary_pools.rejection.clone(),
            primary_pools.apply.clone(),
            primary_pools.cancellation.clone(),
        );
        primary_control.verify_readiness().await.unwrap();
        sqlx::query(
            "ALTER FUNCTION \
             starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2( \
                TEXT, TEXT[] \
             ) PARALLEL UNSAFE",
        )
        .execute(&primary.pool)
        .await
        .unwrap();
        assert_eq!(
            primary_control.verify_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );
        sqlx::query(
            "ALTER FUNCTION \
             starring_runtime_private_v2.starring_product_lifecycle_cancellation_unkeyed_digest_v2( \
                TEXT, TEXT[] \
             ) PARALLEL SAFE",
        )
        .execute(&primary.pool)
        .await
        .unwrap();
        primary_control.verify_readiness().await.unwrap();
        control(
            secondary_pools.reader.clone(),
            secondary_pools.approval.clone(),
            secondary_pools.rejection.clone(),
            secondary_pools.apply.clone(),
            secondary_pools.cancellation.clone(),
        )
        .verify_readiness()
        .await
        .unwrap();

        let reused_role = control(
            primary_pools.reader.clone(),
            primary_pools.approval.clone(),
            primary_pools.approval.clone(),
            primary_pools.apply.clone(),
            primary_pools.cancellation.clone(),
        );
        assert_eq!(
            reused_role.verify_readiness().await,
            Err(ProductDecisionReadinessErrorV1::CapabilityMissing)
        );

        let reused_cancellation_role = control(
            primary_pools.reader.clone(),
            primary_pools.approval.clone(),
            primary_pools.rejection.clone(),
            primary_pools.apply.clone(),
            primary_pools.apply.clone(),
        );
        assert_eq!(
            reused_cancellation_role.verify_readiness().await,
            Err(ProductDecisionReadinessErrorV1::CapabilityMissing)
        );

        let mixed_database = control(
            primary_pools.reader.clone(),
            primary_pools.approval.clone(),
            primary_pools.rejection.clone(),
            secondary_pools.apply.clone(),
            primary_pools.cancellation.clone(),
        );
        assert_eq!(
            mixed_database.verify_readiness().await,
            Err(ProductDecisionReadinessErrorV1::ContractMismatch)
        );

        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION {} FROM {}",
            CANCELLATION_FUNCTIONS[2], primary_roles.cancellation
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        assert_eq!(
            primary_control.verify_readiness().await,
            Err(ProductDecisionReadinessErrorV1::CapabilityMissing)
        );
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {} TO {}",
            CANCELLATION_FUNCTIONS[2], primary_roles.cancellation
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        primary_control.verify_readiness().await.unwrap();

        sqlx::query(&format!(
            "GRANT SELECT ON TABLE public.runtime_product_drain_terminal_actions_v2 TO {}",
            primary_roles.cancellation
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        assert_excess(&primary_control).await;
        sqlx::query(&format!(
            "REVOKE SELECT ON TABLE public.runtime_product_drain_terminal_actions_v2 FROM {}",
            primary_roles.cancellation
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        primary_control.verify_readiness().await.unwrap();

        let unrelated_function = format!("facade_unrelated_{}", suffix());
        assert_safe_identifier(&unrelated_function);
        sqlx::query(&format!(
            "CREATE FUNCTION public.{unrelated_function}() RETURNS BIGINT LANGUAGE SQL \
             IMMUTABLE STRICT PARALLEL SAFE SET search_path = pg_catalog AS 'SELECT 1::BIGINT'"
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION public.{unrelated_function}() FROM PUBLIC"
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION public.{unrelated_function}() TO {}",
            primary_roles.reader
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        assert_excess(&primary_control).await;
        sqlx::query(&format!(
            "REVOKE EXECUTE ON FUNCTION public.{unrelated_function}() FROM {}",
            primary_roles.reader
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        primary_control.verify_readiness().await.unwrap();

        sqlx::query(&format!(
            "GRANT SELECT ON TABLE public.product_tenants TO {}",
            primary_roles.approval
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        assert_excess(&primary_control).await;
        sqlx::query(&format!(
            "REVOKE SELECT ON TABLE public.product_tenants FROM {}",
            primary_roles.approval
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        primary_control.verify_readiness().await.unwrap();

        let shadow_schema = format!("facade_shadow_{}", suffix());
        assert_safe_identifier(&shadow_schema);
        sqlx::query(&format!(
            "CREATE SCHEMA {shadow_schema} AUTHORIZATION {}",
            primary_roles.owner
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "REVOKE ALL PRIVILEGES ON SCHEMA {shadow_schema} FROM PUBLIC"
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        sqlx::query(&format!(
            "GRANT USAGE ON SCHEMA {shadow_schema} TO {}",
            primary_roles.reader
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        assert_excess(&primary_control).await;
        sqlx::query(&format!(
            "REVOKE USAGE ON SCHEMA {shadow_schema} FROM {}",
            primary_roles.reader
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        primary_control.verify_readiness().await.unwrap();

        sqlx::query(&format!(
            "GRANT TEMPORARY ON DATABASE {} TO {}",
            primary.name, primary_roles.apply
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        assert_excess(&primary_control).await;
        sqlx::query(&format!(
            "REVOKE TEMPORARY ON DATABASE {} FROM {}",
            primary.name, primary_roles.apply
        ))
        .execute(&primary.pool)
        .await
        .unwrap();
        primary_control.verify_readiness().await.unwrap();
    })
    .catch_unwind()
    .await;

    destroy_database(secondary, &secondary_role_names).await;
    destroy_database(primary, &primary_role_names).await;
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
