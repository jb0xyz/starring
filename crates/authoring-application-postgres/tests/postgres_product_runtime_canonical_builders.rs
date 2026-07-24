use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::MIGRATOR;
use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_controller::{
    RuntimeCanonicalProductDrainV2, RuntimeDeploymentScopeV1, RuntimeDrainIntentIdV2,
    RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2, RuntimeProductOperationIdV2,
    RuntimeProductSemanticRequestDigestV2, RuntimeServingSlotV2,
};
use automation_runtime_convergence::{
    BindingRevision, DeploymentId, DeploymentRevision, InstallationId, RuntimeDeploymentTargetV1,
    TenantId,
};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

const MIGRATION: &str =
    include_str!("../../../migrations/202607240008_add_product_runtime_canonical_builders.sql");
const PRODUCT_OPERATION_ID: &str = "00112233445566778899aabbccddeeff";
const DRAIN_INTENT_ID: &str = "ffeeddccbbaa99887766554433221100";
const PRODUCT_BUILDER: &str = "SELECT \
    starring_runtime_private_v2.starring_runtime_product_mutation_bytes_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15\
    )";
const DRAIN_BUILDER: &str = "SELECT \
    starring_runtime_private_v2.starring_runtime_drain_intent_bytes_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16\
    )";
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const EXECUTION_CAPABILITIES: [&str; 15] = [
    "public.starring_runtime_execution_database_readiness_v1()",
    "public.starring_runtime_execution_database_identity_v1()",
    "public.starring_runtime_execution_claim_next_v1(text,bigint)",
    "public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)",
    "public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)",
    "public.starring_runtime_execution_certify_prepare_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint)",
    "public.starring_runtime_execution_certify_commit_v1(text,text,text,bigint,text,bigint,bigint,bigint,jsonb,text,text,text,bigint,timestamp with time zone,jsonb,text,jsonb,text)",
    "public.starring_runtime_execution_recover_stale_live_v1()",
    "public.starring_runtime_observe_previous_serving_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,text,bigint,text,bigint,text,jsonb)",
    "public.starring_runtime_gateway_owner_observe_v1(text)",
    "public.starring_runtime_gateway_owner_acquire_v1(text,text,text,bigint)",
    "public.starring_runtime_gateway_owner_renew_v1(text,text,bigint,text,bigint,bigint)",
    "public.starring_runtime_gateway_owner_release_v1(text,text,bigint,text)",
    "public.starring_runtime_writer_fence_observe_v1()",
    "public.starring_runtime_product_drain_observe_v2(text,text,text,bigint,text,text)",
];

struct TestDatabase {
    name: String,
    administrator: PgConnection,
    options: PgConnectOptions,
    pool: PgPool,
}

#[derive(Clone)]
struct BuilderArguments {
    operation_id: String,
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    expected_revision: i64,
    slot_guild_id: String,
    slot_ruleset_key: String,
    target_guild_id: String,
    target_ruleset_key: String,
    target_version: i64,
    target_content_hash: String,
    target_binding_revision: i64,
    target_binding_fingerprint: String,
    mutation_kind: String,
    semantic_digest: String,
}

fn assert_test_database_name(database_name: &str) {
    assert!(
        database_name.starts_with("starring_")
            && database_name.split('_').any(|segment| segment == "test")
            && database_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "refusing to use a database outside the strict Starring test namespace"
    );
}

fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    let options = url
        .parse::<PgConnectOptions>()
        .unwrap_or_else(|_| panic!("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL"));
    let database_name = options
        .get_database()
        .unwrap_or_else(|| panic!("STARRING_TEST_DATABASE_URL must name a database"));
    assert_test_database_name(database_name);
    url
}

fn fixture_token() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{timestamp}_{sequence}")
}

async fn isolated_database(migrate: bool) -> TestDatabase {
    let database_name = format!("starring_builder_test_{}", fixture_token());
    assert_test_database_name(&database_name);
    let base_options = database_url().parse::<PgConnectOptions>().unwrap();
    let mut administrator = PgConnection::connect_with(&base_options.clone().database("postgres"))
        .await
        .expect("connect to PostgreSQL administrator database");
    sqlx::query(&format!("CREATE DATABASE {database_name}"))
        .execute(&mut administrator)
        .await
        .expect("create isolated canonical builder test database");
    let options = base_options.database(&database_name);
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect_with(options.clone())
        .await
        .expect("connect to isolated canonical builder test database");
    if migrate {
        MIGRATOR
            .run(&pool)
            .await
            .expect("run canonical builder migrations");
    }
    TestDatabase {
        name: database_name,
        administrator,
        options,
        pool,
    }
}

async fn cleanup_database(database: TestDatabase, login_role: &str) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .expect("drop isolated canonical builder test database");
    sqlx::query(&format!("DROP ROLE IF EXISTS {login_role}"))
        .execute(&mut administrator)
        .await
        .expect("drop canonical builder login role");
}

async fn migrate_through(pool: &PgPool, maximum_version: i64) {
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version <= maximum_version)
    {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }
}

async fn public_function_acl_fingerprint(pool: &PgPool) -> String {
    sqlx::query_scalar(
        r#"SELECT pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.string_agg(
                    pg_catalog.concat_ws(
                        '|',
                        function_row.oid::TEXT,
                        function_row.proowner::TEXT,
                        COALESCE(function_row.proacl::TEXT, '')
                    ),
                    E'\n'
                    ORDER BY function_row.oid
                ),
                'UTF8'
            )),
            'hex'
        )
        FROM pg_catalog.pg_proc AS function_row
        INNER JOIN pg_catalog.pg_namespace AS namespace
            ON namespace.oid = function_row.pronamespace
        WHERE namespace.nspname = 'public'"#,
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

fn mutation_tag(kind: RuntimeProductMutationKindV2) -> &'static str {
    match kind {
        RuntimeProductMutationKindV2::Apply => "apply",
        RuntimeProductMutationKindV2::Supersede => "supersede",
        RuntimeProductMutationKindV2::Cancel => "cancel",
        RuntimeProductMutationKindV2::AuthorityChange => "authority_change",
        RuntimeProductMutationKindV2::Teardown => "teardown",
    }
}

fn builder_arguments(kind: RuntimeProductMutationKindV2) -> BuilderArguments {
    BuilderArguments {
        operation_id: PRODUCT_OPERATION_ID.to_owned(),
        tenant_id: "tenant:1".to_owned(),
        installation_id: "installation:1".to_owned(),
        deployment_id: "deployment:1".to_owned(),
        expected_revision: 11,
        slot_guild_id: "9223372036854775808".to_owned(),
        slot_ruleset_key: "studyroom".to_owned(),
        target_guild_id: "9223372036854775808".to_owned(),
        target_ruleset_key: "studyroom".to_owned(),
        target_version: 1,
        target_content_hash: "b".repeat(64),
        target_binding_revision: 3,
        target_binding_fingerprint: "a".repeat(64),
        mutation_kind: mutation_tag(kind).to_owned(),
        semantic_digest: "c".repeat(64),
    }
}

fn rust_canonical(arguments: &BuilderArguments) -> RuntimeCanonicalProductDrainV2 {
    let target = RuntimeDeploymentTargetV1 {
        guild_id: GuildId(arguments.target_guild_id.parse().unwrap()),
        ruleset_key: RuleSetKey::parse(&arguments.target_ruleset_key).unwrap(),
        version: RuleSetVersionId::new(arguments.target_version.try_into().unwrap()).unwrap(),
        content_hash: RuleSetContentHash::parse_hex(&arguments.target_content_hash).unwrap(),
        binding_revision: BindingRevision::new(
            arguments.target_binding_revision.try_into().unwrap(),
        )
        .unwrap(),
        binding_fingerprint: ResourceBindingFingerprint::parse(
            &arguments.target_binding_fingerprint,
        )
        .unwrap(),
    };
    let product = RuntimeProductMutationPreimageV2 {
        operation_id: RuntimeProductOperationIdV2::parse(&arguments.operation_id).unwrap(),
        scope: RuntimeDeploymentScopeV1 {
            tenant_id: TenantId::parse(&arguments.tenant_id).unwrap(),
            installation_id: InstallationId::parse(&arguments.installation_id).unwrap(),
            deployment_id: DeploymentId::parse(&arguments.deployment_id).unwrap(),
        },
        expected_revision: DeploymentRevision::new(arguments.expected_revision.try_into().unwrap())
            .unwrap(),
        slot: RuntimeServingSlotV2::new(
            GuildId(arguments.slot_guild_id.parse().unwrap()),
            RuleSetKey::parse(&arguments.slot_ruleset_key).unwrap(),
        ),
        expected_target: target,
        mutation_kind: match arguments.mutation_kind.as_str() {
            "apply" => RuntimeProductMutationKindV2::Apply,
            "supersede" => RuntimeProductMutationKindV2::Supersede,
            "cancel" => RuntimeProductMutationKindV2::Cancel,
            "authority_change" => RuntimeProductMutationKindV2::AuthorityChange,
            "teardown" => RuntimeProductMutationKindV2::Teardown,
            _ => panic!("test arguments must use a valid mutation kind"),
        },
        product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2::parse(
            &arguments.semantic_digest,
        )
        .unwrap(),
    };
    RuntimeCanonicalProductDrainV2::new(
        product,
        RuntimeDrainIntentIdV2::parse(DRAIN_INTENT_ID).unwrap(),
    )
    .unwrap()
}

async fn product_bytes(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    arguments: &BuilderArguments,
) -> Result<Vec<u8>, sqlx::Error> {
    sqlx::query_scalar(PRODUCT_BUILDER)
        .bind(&arguments.operation_id)
        .bind(&arguments.tenant_id)
        .bind(&arguments.installation_id)
        .bind(&arguments.deployment_id)
        .bind(arguments.expected_revision)
        .bind(&arguments.slot_guild_id)
        .bind(&arguments.slot_ruleset_key)
        .bind(&arguments.target_guild_id)
        .bind(&arguments.target_ruleset_key)
        .bind(arguments.target_version)
        .bind(&arguments.target_content_hash)
        .bind(arguments.target_binding_revision)
        .bind(&arguments.target_binding_fingerprint)
        .bind(&arguments.mutation_kind)
        .bind(&arguments.semantic_digest)
        .fetch_one(executor)
        .await
}

async fn drain_bytes(
    executor: impl Executor<'_, Database = sqlx::Postgres>,
    arguments: &BuilderArguments,
) -> Result<Vec<u8>, sqlx::Error> {
    sqlx::query_scalar(DRAIN_BUILDER)
        .bind(DRAIN_INTENT_ID)
        .bind(&arguments.operation_id)
        .bind(&arguments.tenant_id)
        .bind(&arguments.installation_id)
        .bind(&arguments.deployment_id)
        .bind(arguments.expected_revision)
        .bind(&arguments.slot_guild_id)
        .bind(&arguments.slot_ruleset_key)
        .bind(&arguments.target_guild_id)
        .bind(&arguments.target_ruleset_key)
        .bind(arguments.target_version)
        .bind(&arguments.target_content_hash)
        .bind(arguments.target_binding_revision)
        .bind(&arguments.target_binding_fingerprint)
        .bind(&arguments.mutation_kind)
        .bind(&arguments.semantic_digest)
        .fetch_one(executor)
        .await
}

fn framed_digest(domain: &[u8], payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(u64::try_from(domain.len()).unwrap().to_be_bytes());
    hasher.update(domain);
    hasher.update(u64::try_from(payload.len()).unwrap().to_be_bytes());
    hasher.update(payload);
    let bytes = hasher.finalize();
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

fn assert_runtime_input_error(error: sqlx::Error) {
    let sqlx::Error::Database(database) = error else {
        panic!("{error:?}");
    };
    assert_eq!(database.code().as_deref(), Some("RX002"));
}

async fn catalog_fingerprint(pool: &PgPool) -> String {
    sqlx::query_scalar(
        r#"WITH contract(value) AS (
            SELECT pg_catalog.concat_ws(
                '|',
                namespace.nspname,
                function_row.oid::TEXT,
                function_row.proowner::TEXT,
                COALESCE(function_row.proacl::TEXT, ''),
                pg_catalog.pg_get_functiondef(function_row.oid)
            )
            FROM pg_catalog.pg_proc AS function_row
            INNER JOIN pg_catalog.pg_namespace AS namespace
                ON namespace.oid = function_row.pronamespace
            WHERE namespace.nspname = 'starring_runtime_private_v2'
            UNION ALL
            SELECT pg_catalog.concat_ws(
                '|',
                namespace.nspname,
                namespace.nspowner::TEXT,
                COALESCE(namespace.nspacl::TEXT, '')
            )
            FROM pg_catalog.pg_namespace AS namespace
            WHERE namespace.nspname = 'starring_runtime_private_v2'
            UNION ALL
            SELECT pg_catalog.pg_get_functiondef(function_row.oid)
            FROM pg_catalog.pg_proc AS function_row
            WHERE function_row.oid IN (
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_schema_manifest_v1()'
                ),
                pg_catalog.to_regprocedure(
                    'public.starring_runtime_execution_database_readiness_v1()'
                )
            )
        )
        SELECT pg_catalog.encode(
            pg_catalog.sha256(pg_catalog.convert_to(
                pg_catalog.string_agg(value, E'\n' ORDER BY value),
                'UTF8'
            )),
            'hex'
        )
        FROM contract"#,
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn product_runtime_canonical_builders_match_rust_and_reject_unsafe_access() {
    let database = isolated_database(true).await;
    let login_role = format!("builder_login_{}", fixture_token());
    let login_password = format!("BuilderPassword{}x", fixture_token());

    for kind in [
        RuntimeProductMutationKindV2::Apply,
        RuntimeProductMutationKindV2::Supersede,
        RuntimeProductMutationKindV2::Cancel,
        RuntimeProductMutationKindV2::AuthorityChange,
        RuntimeProductMutationKindV2::Teardown,
    ] {
        let arguments = builder_arguments(kind);
        let rust = rust_canonical(&arguments);
        let sql_product = product_bytes(&database.pool, &arguments).await.unwrap();
        let sql_drain = drain_bytes(&database.pool, &arguments).await.unwrap();
        assert_eq!(sql_product, rust.product_mutation_request_bytes());
        assert_eq!(sql_drain, rust.drain_intent_request_bytes());
        let product_digest: String = sqlx::query_scalar(
            "SELECT \
             starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2($1)",
        )
        .bind(&sql_product)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        let drain_digest: String = sqlx::query_scalar(
            "SELECT \
             starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2($1)",
        )
        .bind(&sql_drain)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(product_digest, rust.product_mutation_digest().as_str());
        assert_eq!(drain_digest, rust.drain_intent_digest().as_str());
    }

    let mut controls = String::new();
    for codepoint in 1_u32..=31 {
        controls.push(char::from_u32(codepoint).unwrap());
    }
    for value in [
        String::new(),
        "\"".to_owned(),
        "\\".to_owned(),
        controls,
        "한글 Ω".to_owned(),
        "🦀😀".to_owned(),
    ] {
        let sql_bytes: Vec<u8> = sqlx::query_scalar(
            "SELECT \
             starring_runtime_private_v2.starring_runtime_json_string_bytes_v2($1)",
        )
        .bind(&value)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(sql_bytes, serde_json::to_vec(&value).unwrap());
    }

    let payload = br#"{"format_version":2}"#;
    let product_digest: String = sqlx::query_scalar(
        "SELECT \
         starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2($1)",
    )
    .bind(payload.as_slice())
    .fetch_one(&database.pool)
    .await
    .unwrap();
    let drain_digest: String = sqlx::query_scalar(
        "SELECT \
         starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2($1)",
    )
    .bind(payload.as_slice())
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(
        product_digest,
        "558cb8a7f9190dfc7a7784750bf4e0d053ed7c2bb6c36c6ba6b7fd80c39bff81"
    );
    assert_eq!(
        drain_digest,
        "08ae4fb2781f1d8f841912af5b0397468ba19fb2f41278933cce30f229943564"
    );
    assert_eq!(
        product_digest,
        framed_digest(b"starring.runtime.product_mutation.v2\0", payload)
    );
    assert_eq!(
        drain_digest,
        framed_digest(b"starring.runtime.drain_intent.v2\0", payload)
    );
    assert_ne!(product_digest, drain_digest);

    let mut upper = builder_arguments(RuntimeProductMutationKindV2::Apply);
    upper.expected_revision = i64::MAX;
    upper.target_binding_revision = i64::MAX;
    upper.target_version = i64::from(u32::MAX);
    upper.slot_guild_id = u64::MAX.to_string();
    upper.target_guild_id = u64::MAX.to_string();
    let upper_rust = rust_canonical(&upper);
    assert_eq!(
        product_bytes(&database.pool, &upper).await.unwrap(),
        upper_rust.product_mutation_request_bytes()
    );
    assert_eq!(
        drain_bytes(&database.pool, &upper).await.unwrap(),
        upper_rust.drain_intent_request_bytes()
    );

    let mut minimum_snowflake = builder_arguments(RuntimeProductMutationKindV2::Apply);
    minimum_snowflake.slot_guild_id = "1".to_owned();
    minimum_snowflake.target_guild_id = "1".to_owned();
    let minimum_snowflake_rust = rust_canonical(&minimum_snowflake);
    assert_eq!(
        product_bytes(&database.pool, &minimum_snowflake)
            .await
            .unwrap(),
        minimum_snowflake_rust.product_mutation_request_bytes()
    );
    assert_eq!(
        drain_bytes(&database.pool, &minimum_snowflake)
            .await
            .unwrap(),
        minimum_snowflake_rust.drain_intent_request_bytes()
    );

    let valid = builder_arguments(RuntimeProductMutationKindV2::Apply);
    let expected_valid_product = product_bytes(&database.pool, &valid).await.unwrap();
    let mut hostile_search_path = database.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL search_path = pg_temp, public, pg_catalog")
        .execute(&mut *hostile_search_path)
        .await
        .unwrap();
    assert_eq!(
        product_bytes(&mut *hostile_search_path, &valid)
            .await
            .unwrap(),
        expected_valid_product
    );
    hostile_search_path.rollback().await.unwrap();

    let mut invalid_cases = Vec::new();
    let mut invalid = valid.clone();
    invalid.expected_revision = 0;
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.target_version = 0;
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.target_version = i64::from(u32::MAX) + 1;
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.target_binding_revision = 0;
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.target_guild_id = "18446744073709551616".to_owned();
    invalid.slot_guild_id = invalid.target_guild_id.clone();
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.target_guild_id = "0".to_owned();
    invalid.slot_guild_id = invalid.target_guild_id.clone();
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.target_guild_id = "01".to_owned();
    invalid.slot_guild_id = invalid.target_guild_id.clone();
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.slot_ruleset_key = "other".to_owned();
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.operation_id = "A".repeat(32);
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.mutation_kind = "unknown".to_owned();
    invalid_cases.push(invalid);
    let mut invalid = valid.clone();
    invalid.semantic_digest = "C".repeat(64);
    invalid_cases.push(invalid);

    for invalid in invalid_cases {
        assert_runtime_input_error(product_bytes(&database.pool, &invalid).await.unwrap_err());
        assert_runtime_input_error(drain_bytes(&database.pool, &invalid).await.unwrap_err());
    }

    for (query, payload) in [
        (
            "SELECT \
             starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2($1)",
            Vec::new(),
        ),
        (
            "SELECT \
             starring_runtime_private_v2.starring_runtime_product_mutation_digest_v2($1)",
            vec![0_u8; 32769],
        ),
        (
            "SELECT \
             starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2($1)",
            Vec::new(),
        ),
        (
            "SELECT \
             starring_runtime_private_v2.starring_runtime_drain_intent_digest_v2($1)",
            vec![0_u8; 65537],
        ),
    ] {
        let error = sqlx::query_scalar::<_, String>(query)
            .bind(payload)
            .fetch_one(&database.pool)
            .await
            .unwrap_err();
        assert_runtime_input_error(error);
    }

    let before_rerun = catalog_fingerprint(&database.pool).await;
    let mut transaction = database.pool.begin().await.unwrap();
    let rerun_error = sqlx::raw_sql(MIGRATION)
        .execute(&mut *transaction)
        .await
        .expect_err("canonical builder migration rerun must be rejected");
    transaction.rollback().await.unwrap();
    let sqlx::Error::Database(rerun_database_error) = rerun_error else {
        panic!("{rerun_error:?}");
    };
    assert_eq!(rerun_database_error.code().as_deref(), Some("RE001"));
    assert_eq!(
        rerun_database_error.message(),
        "runtime_canonical_builders_preflight_drift"
    );
    assert_eq!(catalog_fingerprint(&database.pool).await, before_rerun);

    sqlx::query(&format!(
        "CREATE ROLE {login_role} LOGIN PASSWORD '{login_password}'"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {} TO {login_role}",
        database.name
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let (schema_usage, schema_create, helper_execute, denied_helper_count) =
        sqlx::query_as::<_, (bool, bool, bool, i64)>(
            r#"SELECT
                pg_catalog.has_schema_privilege(
                    pg_catalog.to_regrole($1),
                    'starring_runtime_private_v2',
                    'USAGE'
                ),
                pg_catalog.has_schema_privilege(
                    pg_catalog.to_regrole($1),
                    'starring_runtime_private_v2',
                    'CREATE'
                ),
                pg_catalog.has_function_privilege(
                    pg_catalog.to_regrole($1),
                    pg_catalog.to_regprocedure(
                        'starring_runtime_private_v2.starring_runtime_json_string_bytes_v2(text)'
                    ),
                    'EXECUTE'
                ),
                (SELECT pg_catalog.count(*)
                 FROM pg_catalog.pg_proc AS function_row
                 INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = function_row.pronamespace
                 WHERE namespace.nspname = 'starring_runtime_private_v2'
                    AND NOT pg_catalog.has_function_privilege(
                        pg_catalog.to_regrole($1),
                        function_row.oid,
                        'EXECUTE'
                    ))"#,
        )
        .bind(&login_role)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert!(!schema_usage);
    assert!(!schema_create);
    assert!(!helper_execute);
    assert_eq!(denied_helper_count, 6);
    let login_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            database
                .options
                .clone()
                .username(&login_role)
                .password(&login_password),
        )
        .await
        .unwrap();
    sqlx::query("SET search_path = pg_temp, public, starring_runtime_private_v2, pg_catalog")
        .execute(&login_pool)
        .await
        .unwrap();
    let denied = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT \
         starring_runtime_private_v2.starring_runtime_json_string_bytes_v2('denied')",
    )
    .fetch_one(&login_pool)
    .await
    .unwrap_err();
    let sqlx::Error::Database(denied) = denied else {
        panic!("{denied:?}");
    };
    assert_eq!(denied.code().as_deref(), Some("42501"));
    login_pool.close().await;

    cleanup_database(database, &login_role).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn canonical_builder_upgrade_preserves_the_existing_executor_acl_set() {
    let database = isolated_database(false).await;
    let login_role = format!("builder_upgrade_{}", fixture_token());
    let login_password = format!("BuilderUpgradePassword{}x", fixture_token());
    migrate_through(&database.pool, 202_607_240_007).await;
    sqlx::query(&format!(
        "CREATE ROLE {login_role} LOGIN PASSWORD '{login_password}'"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {} TO {login_role}",
        EXECUTION_CAPABILITIES.join(",")
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let before_upgrade = public_function_acl_fingerprint(&database.pool).await;

    let mut transaction = database.pool.begin().await.unwrap();
    sqlx::raw_sql(MIGRATION)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    assert_eq!(
        public_function_acl_fingerprint(&database.pool).await,
        before_upgrade
    );
    let preserved_capability_count: i64 = sqlx::query_scalar(
        r#"SELECT pg_catalog.count(*)
        FROM pg_catalog.pg_proc AS function_row
        WHERE function_row.oid IN (
            SELECT pg_catalog.to_regprocedure(expected.identity)
            FROM pg_catalog.unnest($1::TEXT[]) AS expected(identity)
        )
            AND pg_catalog.has_function_privilege(
                pg_catalog.to_regrole($2),
                function_row.oid,
                'EXECUTE'
            )"#,
    )
    .bind(EXECUTION_CAPABILITIES.as_slice())
    .bind(&login_role)
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(preserved_capability_count, 15);
    let (schema_usage, schema_create, private_external_acl_count, manifest_valid) =
        sqlx::query_as::<_, (bool, bool, i64, bool)>(
            r#"SELECT
                pg_catalog.has_schema_privilege(
                    pg_catalog.to_regrole($1),
                    'starring_runtime_private_v2',
                    'USAGE'
                ),
                pg_catalog.has_schema_privilege(
                    pg_catalog.to_regrole($1),
                    'starring_runtime_private_v2',
                    'CREATE'
                ),
                (SELECT pg_catalog.count(*)
                 FROM pg_catalog.pg_proc AS function_row
                 INNER JOIN pg_catalog.pg_namespace AS namespace
                    ON namespace.oid = function_row.pronamespace
                 CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                    function_row.proacl,
                    pg_catalog.acldefault('f', function_row.proowner)
                 )) AS privilege
                 WHERE namespace.nspname = 'starring_runtime_private_v2'
                    AND privilege.grantee <> function_row.proowner),
                public.starring_runtime_execution_schema_manifest_v1()"#,
        )
        .bind(&login_role)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert!(!schema_usage);
    assert!(!schema_create);
    assert_eq!(private_external_acl_count, 0);
    assert!(manifest_valid);

    cleanup_database(database, &login_role).await;
}
