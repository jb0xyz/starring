use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::{
    OperatingSystemSecretGenerator, PostgresAuthorizedPromotionSnapshots,
    PostgresInstallationAuthoritySource, PostgresProductApiReadiness, PostgresProductControl,
    PostgresProductDeploymentOperationalStatusesV2, PostgresProductDeploymentStatuses,
    PostgresProductIdentityConfig, PostgresProductIdentityStore, PostgresProductPromotions,
    ProductActionDigestKeyV1, ProductActionDigestKeyringV1, ProductApiReadinessErrorV1,
    ProductDecisionDatabasePoolsV1, ProductIdentityDatabasePoolsV1,
    ProductIdentityReadinessErrorV1, SnapshotEnvelopeKeyV1, SnapshotEnvelopeKeyringV1,
    XChaCha20Poly1305SnapshotEnvelopeCipherV1, MIGRATOR,
};
use futures::FutureExt;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Connection;
use zeroize::Zeroizing;

const OAUTH_FLOW_WRITER: usize = 0;
const SESSION_ISSUER: usize = 1;
const SESSION_API: usize = 2;
const SECURITY_REVOKER: usize = 3;
const INSTALLATION_AUTHORITY: usize = 4;
const AUTHORIZED_SNAPSHOT: usize = 5;
const PROMOTION: usize = 6;
const DECISION_READER: usize = 7;
const APPROVAL_EXECUTOR: usize = 8;
const REJECTION_EXECUTOR: usize = 9;
const APPLY_EXECUTOR: usize = 10;
const DEPLOYMENT_STATUS: usize = 11;
const OPERATIONAL_DEPLOYMENT_STATUS: usize = 12;

const OAUTH_FLOW_FUNCTIONS: &[&str] = &[
    "public.starring_product_oauth_database_identity_v1()",
    "public.starring_product_oauth_flow_create_v1(bytea,bytea,text,text,double precision)",
    "public.starring_product_oauth_flow_consume_v1(bytea,bytea,text,text[])",
];
const SESSION_ISSUER_FUNCTIONS: &[&str] = &[
    "public.starring_product_session_issuer_database_identity_v1()",
    "public.starring_product_session_issue_v1(bytea,text,text,timestamp with time zone,text,text,bytea,bytea,double precision,double precision)",
];
const SESSION_API_FUNCTIONS: &[&str] = &[
    "public.starring_product_session_api_database_identity_v1()",
    "public.starring_product_session_read_v1(bytea)",
    "public.starring_product_session_mutation_read_v1(bytea)",
    "public.starring_product_session_touch_v1(bytea,timestamp with time zone,timestamp with time zone,timestamp with time zone,double precision)",
    "public.starring_product_session_logout_read_v1(bytea)",
    "public.starring_product_session_logout_commit_v1(bytea,bytea,timestamp with time zone)",
];
const SECURITY_REVOKER_FUNCTIONS: &[&str] = &[
    "public.starring_product_security_revoker_database_identity_v1()",
    "public.starring_product_session_security_revoke_v1(bytea)",
];
const INSTALLATION_AUTHORITY_FUNCTIONS: &[&str] = &[
    "public.starring_product_installation_authority_reader_database_identity_v1()",
    "public.starring_product_installation_authority_read_v1(text,text,bytea)",
];
const AUTHORIZED_SNAPSHOT_FUNCTIONS: &[&str] = &[
    "public.starring_product_authorized_snapshot_reader_database_identity_v1()",
    "public.starring_product_authorized_snapshot_read_v1(text,text,bytea,text,text)",
    "public.starring_product_authorized_snapshot_key_coverage_v1(text[])",
];
const PROMOTION_FUNCTIONS: &[&str] = &[
    "public.starring_product_promotion_executor_database_identity_v1()",
    "public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])",
    "public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    "public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)",
    "public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)",
    "public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_promotion_keyring_coverage_v1(text[],text[])",
];
const DECISION_READER_FUNCTIONS: &[&str] = &[
    "public.starring_product_decision_reader_database_identity_v1()",
    "public.starring_product_decision_read_v1(text,text,text,text,text,text,bytea)",
];
const APPROVAL_EXECUTOR_FUNCTIONS: &[&str] = &[
    "public.starring_product_approval_executor_database_identity_v1()",
    "public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text)",
    "public.starring_product_approval_keyring_coverage_v1(text[],text[])",
];
const REJECTION_EXECUTOR_FUNCTIONS: &[&str] = &[
    "public.starring_product_rejection_executor_database_identity_v1()",
    "public.starring_product_rejection_keyring_coverage_v1(text[],text[])",
    "public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)",
];
const APPLY_EXECUTOR_FUNCTIONS: &[&str] = &[
    "public.starring_product_apply_executor_database_identity_v1()",
    "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)",
    "public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)",
    "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)",
    "public.starring_product_apply_keyring_coverage_v1(text[],text[])",
];
const DEPLOYMENT_STATUS_FUNCTIONS: &[&str] = &[
    "public.starring_product_deployment_status_reader_database_identity_v1()",
    "public.starring_product_deployment_status_read_v1(text,text,text,text,text,text,text,text,bytea)",
];
const OPERATIONAL_DEPLOYMENT_STATUS_FUNCTIONS: &[&str] = &[
    "public.starring_product_deployment_status_reader_database_identity_v2()",
    "public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)",
];

const CAPABILITIES: [(&str, &[&str]); 13] = [
    ("oauth", OAUTH_FLOW_FUNCTIONS),
    ("issuer", SESSION_ISSUER_FUNCTIONS),
    ("session", SESSION_API_FUNCTIONS),
    ("revoker", SECURITY_REVOKER_FUNCTIONS),
    ("authority", INSTALLATION_AUTHORITY_FUNCTIONS),
    ("snapshot", AUTHORIZED_SNAPSHOT_FUNCTIONS),
    ("promotion", PROMOTION_FUNCTIONS),
    ("decision", DECISION_READER_FUNCTIONS),
    ("approval", APPROVAL_EXECUTOR_FUNCTIONS),
    ("rejection", REJECTION_EXECUTOR_FUNCTIONS),
    ("apply", APPLY_EXECUTOR_FUNCTIONS),
    ("status", DEPLOYMENT_STATUS_FUNCTIONS),
    ("operational", OPERATIONAL_DEPLOYMENT_STATUS_FUNCTIONS),
];

static SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDatabase {
    name: String,
    administrator: PgConnection,
    owner_pool: PgPool,
}

struct RoleCredentials {
    name: String,
    password: String,
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
    let name = format!("starring_api_{label}_test_{}", suffix());
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
    let owner_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(base.database(&name))
        .await
        .unwrap();
    TestDatabase {
        name,
        administrator,
        owner_pool,
    }
}

fn primary_roles() -> Vec<RoleCredentials> {
    let shared_suffix = suffix();
    CAPABILITIES
        .iter()
        .map(|(label, _)| {
            let name = format!("starring_api_{label}_{shared_suffix}");
            assert_safe_identifier(&name);
            RoleCredentials {
                name,
                password: password(),
            }
        })
        .collect()
}

async fn create_owner(database: &mut TestDatabase, owner: &str) {
    assert_safe_identifier(owner);
    sqlx::query(&format!(
        "CREATE ROLE {owner} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 0"
    ))
    .execute(&mut database.administrator)
    .await
    .unwrap();
}

async fn create_login(database: &mut TestDatabase, role: &RoleCredentials) {
    assert_safe_identifier(&role.name);
    let password_literal = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_literal($1)")
        .bind(&role.password)
        .fetch_one(&database.owner_pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE ROLE {} LOGIN PASSWORD {password_literal} NOSUPERUSER NOCREATEDB \
         NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS CONNECTION LIMIT 4",
        role.name
    ))
    .execute(&mut database.administrator)
    .await
    .unwrap();
}

async fn normalize_public_ownership(database: &TestDatabase, owner: &str) {
    let relations = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.format('%I.%I', namespace.nspname, relation.relname) \
         FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
          ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname = 'public' AND relation.relkind IN ('r', 'p') \
         ORDER BY relation.oid",
    )
    .fetch_all(&database.owner_pool)
    .await
    .unwrap();
    for relation in relations {
        sqlx::query(&format!("ALTER TABLE {relation} OWNER TO {owner}"))
            .execute(&database.owner_pool)
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
         WHERE namespace.nspname = 'public' AND function_row.prokind = 'f' \
         ORDER BY function_row.oid",
    )
    .fetch_all(&database.owner_pool)
    .await
    .unwrap();
    for function in functions {
        sqlx::query(&format!("ALTER FUNCTION {function} OWNER TO {owner}"))
            .execute(&database.owner_pool)
            .await
            .unwrap();
    }
    sqlx::query(&format!("ALTER SCHEMA public OWNER TO {owner}"))
        .execute(&database.owner_pool)
        .await
        .unwrap();
}

async fn seal_database(database: &TestDatabase) {
    for statement in [
        format!(
            "REVOKE ALL PRIVILEGES ON DATABASE {} FROM PUBLIC",
            database.name
        ),
        "REVOKE ALL PRIVILEGES ON SCHEMA public FROM PUBLIC".to_string(),
        "REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM PUBLIC".to_string(),
        "REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM PUBLIC".to_string(),
        "REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM PUBLIC".to_string(),
    ] {
        sqlx::query(&statement)
            .execute(&database.owner_pool)
            .await
            .unwrap();
    }
}

async fn grant_database_access(
    database: &TestDatabase,
    owner: &str,
    roles: impl IntoIterator<Item = String>,
) {
    let roles = roles.into_iter().collect::<Vec<_>>();
    for role in &roles {
        assert_safe_identifier(role);
    }
    let joined = roles.join(", ");
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {} TO {joined}",
        database.name
    ))
    .execute(&database.owner_pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT USAGE ON SCHEMA public TO {owner}, {joined}"
    ))
    .execute(&database.owner_pool)
    .await
    .unwrap();
}

async fn grant_functions(database: &TestDatabase, role: &str, functions: &[&str]) {
    assert_safe_identifier(role);
    for function in functions {
        sqlx::query(&format!("GRANT EXECUTE ON FUNCTION {function} TO {role}"))
            .execute(&database.owner_pool)
            .await
            .unwrap();
    }
}

async fn role_pool(database: &str, role: &RoleCredentials) -> PgPool {
    PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            database_url()
                .parse::<PgConnectOptions>()
                .unwrap()
                .database(database)
                .username(&role.name)
                .password(&role.password),
        )
        .await
        .unwrap()
}

async fn provision_primary(
    database: &mut TestDatabase,
    owner: &str,
    roles: &[RoleCredentials],
) -> Vec<PgPool> {
    MIGRATOR.run(&database.owner_pool).await.unwrap();
    create_owner(database, owner).await;
    for role in roles {
        create_login(database, role).await;
    }
    normalize_public_ownership(database, owner).await;
    seal_database(database).await;
    grant_database_access(database, owner, roles.iter().map(|role| role.name.clone())).await;
    for ((_, functions), role) in CAPABILITIES.iter().zip(roles) {
        grant_functions(database, &role.name, functions).await;
    }
    let mut pools = Vec::with_capacity(roles.len());
    for role in roles {
        pools.push(role_pool(&database.name, role).await);
    }
    pools
}

async fn provision_secondary(
    database: &mut TestDatabase,
    owner: &str,
    reused: &RoleCredentials,
    distinct: &RoleCredentials,
) -> (PgPool, PgPool) {
    MIGRATOR.run(&database.owner_pool).await.unwrap();
    create_owner(database, owner).await;
    create_login(database, distinct).await;
    normalize_public_ownership(database, owner).await;
    seal_database(database).await;
    grant_database_access(
        database,
        owner,
        [reused.name.clone(), distinct.name.clone()],
    )
    .await;
    grant_functions(
        database,
        &reused.name,
        OPERATIONAL_DEPLOYMENT_STATUS_FUNCTIONS,
    )
    .await;
    (
        role_pool(&database.name, reused).await,
        role_pool(&database.name, distinct).await,
    )
}

fn action_keyring() -> ProductActionDigestKeyringV1 {
    ProductActionDigestKeyringV1::new(
        ProductActionDigestKeyV1::from_bytes(
            "product-api-readiness-v1",
            std::array::from_fn(|index| 37_u8.wrapping_add(index as u8)),
        )
        .unwrap(),
        [],
    )
    .unwrap()
}

fn snapshot_cipher() -> XChaCha20Poly1305SnapshotEnvelopeCipherV1 {
    let key = SnapshotEnvelopeKeyV1::new(
        "snapshot-readiness-v1",
        Zeroizing::new(std::array::from_fn(|index| {
            103_u8.wrapping_add(index as u8)
        })),
    )
    .unwrap();
    XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(SnapshotEnvelopeKeyringV1::new(key, []).unwrap())
}

async fn verify_api(
    pools: &[PgPool],
    operational_status_pool: PgPool,
) -> Result<(), ProductApiReadinessErrorV1> {
    let identity = PostgresProductIdentityStore::<OperatingSystemSecretGenerator>::production(
        ProductIdentityDatabasePoolsV1::new(
            pools[OAUTH_FLOW_WRITER].clone(),
            pools[SESSION_ISSUER].clone(),
            pools[SESSION_API].clone(),
            pools[SECURITY_REVOKER].clone(),
        ),
        PostgresProductIdentityConfig::production(
            "https://starring.example/oauth/discord/callback",
            ["/".to_string()],
        )
        .unwrap(),
    );
    let authority = PostgresInstallationAuthoritySource::new(pools[INSTALLATION_AUTHORITY].clone());
    let snapshots = PostgresAuthorizedPromotionSnapshots::new(
        pools[AUTHORIZED_SNAPSHOT].clone(),
        snapshot_cipher(),
    );
    let promotions =
        PostgresProductPromotions::new(pools[PROMOTION].clone(), action_keyring()).unwrap();
    let control = PostgresProductControl::new(
        ProductDecisionDatabasePoolsV1::new(
            pools[DECISION_READER].clone(),
            pools[APPROVAL_EXECUTOR].clone(),
            pools[APPLY_EXECUTOR].clone(),
        ),
        pools[REJECTION_EXECUTOR].clone(),
        action_keyring(),
    )
    .unwrap();
    let statuses = PostgresProductDeploymentStatuses::new(pools[DEPLOYMENT_STATUS].clone());
    let operational_statuses =
        PostgresProductDeploymentOperationalStatusesV2::new(operational_status_pool);
    PostgresProductApiReadiness::new(
        &identity,
        &authority,
        &snapshots,
        &promotions,
        &control,
        &statuses,
        &operational_statuses,
    )
    .verify_readiness()
    .await
}

async fn destroy_databases(
    primary: TestDatabase,
    secondary: TestDatabase,
    roles: impl IntoIterator<Item = String>,
) {
    primary.owner_pool.close().await;
    secondary.owner_pool.close().await;
    let TestDatabase {
        name: secondary_name,
        administrator: mut secondary_administrator,
        ..
    } = secondary;
    sqlx::query(&format!("DROP DATABASE {secondary_name} WITH (FORCE)"))
        .execute(&mut secondary_administrator)
        .await
        .unwrap();
    let TestDatabase {
        name: primary_name,
        administrator: mut primary_administrator,
        ..
    } = primary;
    sqlx::query(&format!("DROP DATABASE {primary_name} WITH (FORCE)"))
        .execute(&mut primary_administrator)
        .await
        .unwrap();
    for role in roles {
        assert_safe_identifier(&role);
        sqlx::query(&format!("DROP ROLE IF EXISTS {role}"))
            .execute(&mut primary_administrator)
            .await
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_api_readiness_enforces_thirteen_isolated_database_capabilities() {
    let mut primary = isolated_database("primary").await;
    let mut secondary = isolated_database("secondary").await;
    let roles = primary_roles();
    let primary_owner = format!("starring_api_owner_{}", suffix());
    let secondary_owner = format!("starring_api_secondary_owner_{}", suffix());
    let distinct_secondary = RoleCredentials {
        name: format!("starring_api_secondary_status_{}", suffix()),
        password: password(),
    };
    for identifier in [
        primary_owner.as_str(),
        secondary_owner.as_str(),
        distinct_secondary.name.as_str(),
    ] {
        assert_safe_identifier(identifier);
    }
    let cleanup_roles = roles
        .iter()
        .map(|role| role.name.clone())
        .chain([
            distinct_secondary.name.clone(),
            primary_owner.clone(),
            secondary_owner.clone(),
        ])
        .collect::<Vec<_>>();
    let outcome = AssertUnwindSafe(async {
        let primary_pools = provision_primary(&mut primary, &primary_owner, &roles).await;
        let reused = &roles[DEPLOYMENT_STATUS];
        let (reused_secondary_pool, distinct_secondary_pool) = provision_secondary(
            &mut secondary,
            &secondary_owner,
            reused,
            &distinct_secondary,
        )
        .await;

        verify_api(
            &primary_pools,
            primary_pools[OPERATIONAL_DEPLOYMENT_STATUS].clone(),
        )
        .await
        .unwrap();

        let reused_role = verify_api(&primary_pools, reused_secondary_pool.clone()).await;
        assert!(
            matches!(
                reused_role,
                Err(ProductApiReadinessErrorV1::TopologyMismatch)
            ),
            "reused role returned {reused_role:?}"
        );

        for function in OPERATIONAL_DEPLOYMENT_STATUS_FUNCTIONS {
            sqlx::query(&format!(
                "REVOKE EXECUTE ON FUNCTION {function} FROM {}",
                reused.name
            ))
            .execute(&secondary.owner_pool)
            .await
            .unwrap();
        }
        grant_functions(
            &secondary,
            &distinct_secondary.name,
            OPERATIONAL_DEPLOYMENT_STATUS_FUNCTIONS,
        )
        .await;
        let mixed_database = verify_api(&primary_pools, distinct_secondary_pool.clone()).await;
        assert!(
            matches!(
                mixed_database,
                Err(ProductApiReadinessErrorV1::TopologyMismatch)
            ),
            "mixed database returned {mixed_database:?}"
        );

        sqlx::query(&format!(
            "GRANT SELECT ON TABLE public.product_principals TO {}",
            roles[OAUTH_FLOW_WRITER].name
        ))
        .execute(&primary.owner_pool)
        .await
        .unwrap();
        let excess = verify_api(
            &primary_pools,
            primary_pools[OPERATIONAL_DEPLOYMENT_STATUS].clone(),
        )
        .await;
        assert!(matches!(
            excess,
            Err(ProductApiReadinessErrorV1::Identity(
                ProductIdentityReadinessErrorV1::ExcessCapability
            ))
        ));
        sqlx::query(&format!(
            "REVOKE SELECT ON TABLE public.product_principals FROM {}",
            roles[OAUTH_FLOW_WRITER].name
        ))
        .execute(&primary.owner_pool)
        .await
        .unwrap();
        verify_api(
            &primary_pools,
            primary_pools[OPERATIONAL_DEPLOYMENT_STATUS].clone(),
        )
        .await
        .unwrap();

        sqlx::query(&format!(
            "GRANT SET ON PARAMETER session_replication_role TO {}",
            roles[OAUTH_FLOW_WRITER].name
        ))
        .execute(&primary.owner_pool)
        .await
        .unwrap();
        let parameter_excess = verify_api(
            &primary_pools,
            primary_pools[OPERATIONAL_DEPLOYMENT_STATUS].clone(),
        )
        .await;
        assert!(matches!(
            parameter_excess,
            Err(ProductApiReadinessErrorV1::Identity(
                ProductIdentityReadinessErrorV1::ExcessCapability
            ))
        ));
        sqlx::query(&format!(
            "REVOKE SET ON PARAMETER session_replication_role FROM {}",
            roles[OAUTH_FLOW_WRITER].name
        ))
        .execute(&primary.owner_pool)
        .await
        .unwrap();
        verify_api(
            &primary_pools,
            primary_pools[OPERATIONAL_DEPLOYMENT_STATUS].clone(),
        )
        .await
        .unwrap();

        sqlx::query(&format!(
            "ALTER ROLE {} SET session_replication_role = 'replica'",
            roles[OAUTH_FLOW_WRITER].name
        ))
        .execute(&primary.owner_pool)
        .await
        .unwrap();
        let role_setting_excess = verify_api(
            &primary_pools,
            primary_pools[OPERATIONAL_DEPLOYMENT_STATUS].clone(),
        )
        .await;
        assert!(matches!(
            role_setting_excess,
            Err(ProductApiReadinessErrorV1::Identity(
                ProductIdentityReadinessErrorV1::ExcessCapability
            ))
        ));
        sqlx::query(&format!(
            "ALTER ROLE {} RESET ALL",
            roles[OAUTH_FLOW_WRITER].name
        ))
        .execute(&primary.owner_pool)
        .await
        .unwrap();
        verify_api(
            &primary_pools,
            primary_pools[OPERATIONAL_DEPLOYMENT_STATUS].clone(),
        )
        .await
        .unwrap();

        for pool in primary_pools {
            pool.close().await;
        }
        reused_secondary_pool.close().await;
        distinct_secondary_pool.close().await;
    })
    .catch_unwind()
    .await;
    destroy_databases(primary, secondary, cleanup_roles).await;
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}
