use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::MIGRATOR;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Connection;

const REJECTION_MIGRATION: i64 = 202_607_200_005;
const REJECTION_IDENTITY_FUNCTION: &str =
    "public.starring_product_rejection_executor_database_identity_v1()";
const REJECTION_COVERAGE_FUNCTION: &str =
    "public.starring_product_rejection_keyring_coverage_v1(text[],text[])";
const REJECTION_EXECUTE_FUNCTION: &str = "public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)";
const UNRELATED_READER_FUNCTION: &str = "public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)";
const ACTIVATION_EXECUTOR_FUNCTION: &str = "public.enforce_product_activation_executor()";

static SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

struct RejectionMigrationTestDatabase {
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

async fn isolated_database(label: &str) -> RejectionMigrationTestDatabase {
    assert!(
        !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    let label = label.chars().take(12).collect::<String>();
    let name = format!("starring_rejection_{label}_test_{}", suffix());
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
    RejectionMigrationTestDatabase {
        name,
        administrator,
        pool,
    }
}

async fn drop_isolated_database(database: RejectionMigrationTestDatabase, roles: &[String]) {
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

async fn apply_pre_rejection_migrations(pool: &PgPool) {
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version < REJECTION_MIGRATION)
    {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }
}

fn rejection_migration() -> &'static sqlx::migrate::Migration {
    MIGRATOR
        .iter()
        .find(|migration| migration.version == REJECTION_MIGRATION)
        .expect("product rejection migration must exist")
}

fn migration_source() -> &'static str {
    include_str!("../../../migrations/202607200005_scope_product_rejection_execution.sql")
}

fn function_definition<'a>(source: &'a str, declaration: &str) -> &'a str {
    source
        .split(declaration)
        .nth(1)
        .unwrap_or_else(|| panic!("missing function declaration: {declaration}"))
        .split("$function$;")
        .next()
        .unwrap()
}

async fn apply_rejection_migration(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::raw_sql(rejection_migration().sql.as_ref())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await
}

async fn rejected_migration_error(pool: &PgPool) -> sqlx::Error {
    let mut transaction = pool.begin().await.unwrap();
    let error = sqlx::raw_sql(rejection_migration().sql.as_ref())
        .execute(&mut *transaction)
        .await
        .expect_err("hostile pre-migration state must reject the migration");
    transaction.rollback().await.unwrap();
    error
}

fn assert_object_state_error(error: &sqlx::Error, expected_message: &str) {
    let sqlx::Error::Database(database) = error else {
        panic!("{error:?}");
    };
    assert_eq!(database.code().as_deref(), Some("55000"));
    assert_eq!(database.message(), expected_message);
}

async fn function_exists(pool: &PgPool, signature: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regprocedure($1) IS NOT NULL")
        .bind(signature)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn constraint_definition(pool: &PgPool) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.pg_get_constraintdef(constraint_row.oid) \
         FROM pg_catalog.pg_constraint AS constraint_row \
         WHERE constraint_row.conrelid = 'public.activation_requests'::REGCLASS \
         AND constraint_row.conname = 'activation_requests_rejected_fields_valid'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn activation_executor_definition(pool: &PgPool) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure($1))",
    )
    .bind(ACTIVATION_EXECUTOR_FUNCTION)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn assert_no_rejection_residue(pool: &PgPool) {
    let signatures = vec![
        REJECTION_IDENTITY_FUNCTION.to_string(),
        REJECTION_COVERAGE_FUNCTION.to_string(),
        REJECTION_EXECUTE_FUNCTION.to_string(),
    ];
    let function_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.unnest($1::TEXT[]) AS expected(signature) \
         WHERE pg_catalog.to_regprocedure(expected.signature) IS NOT NULL",
    )
    .bind(&signatures)
    .fetch_one(pool)
    .await
    .unwrap();
    let index_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.unnest(ARRAY[ \
          'public.product_action_receipts_rejection_retention_index', \
          'public.product_action_aliases_rejection_receipt_retention_index' \
         ]::TEXT[]) AS expected(identity) \
         WHERE pg_catalog.to_regclass(expected.identity) IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(function_count, 0);
    assert_eq!(index_count, 0);
}

#[test]
fn rejection_migration_source_contract_is_explicit_and_fail_closed() {
    let source = migration_source();
    let signature = source
        .split("CREATE FUNCTION public.starring_product_reject_v1(\n")
        .nth(1)
        .unwrap()
        .split("\n)\nRETURNS TABLE(")
        .next()
        .unwrap();
    let arguments = signature.lines().map(str::trim).collect::<Vec<_>>();
    assert_eq!(
        arguments,
        [
            "expected_tenant_id TEXT,",
            "expected_installation_id TEXT,",
            "expected_promotion_id TEXT,",
            "expected_product_revision BIGINT,",
            "expected_payload_digest TEXT,",
            "expected_principal_id TEXT,",
            "expected_product_session_digest BYTEA,",
            "session_subject_digest BYTEA,",
            "expected_acting_user_id TEXT,",
            "expected_discord_application_id TEXT,",
            "expected_guild_id TEXT,",
            "expected_capability TEXT,",
            "expected_authority_revision BIGINT,",
            "expected_authority_payload_digest TEXT,",
            "expected_authority_observation_digest TEXT,",
            "expected_authority_observed_at TIMESTAMPTZ,",
            "expected_authority_expires_at TIMESTAMPTZ,",
            "expected_effective_permission_bits TEXT,",
            "expected_guild_owner BOOLEAN,",
            "product_request_id TEXT,",
            "active_idempotency_key_digest TEXT,",
            "idempotency_key_digest_candidates TEXT[],",
            "idempotency_digest_key_id_candidates TEXT[],",
            "idempotency_digest_key_fingerprint_candidates TEXT[],",
            "idempotency_digest_key_id TEXT,",
            "semantic_request_digest TEXT,",
            "new_receipt_id TEXT,",
            "new_audit_event_id TEXT,",
            "expected_rejection_reason TEXT",
        ]
    );
    assert_eq!(arguments.len(), 29);
    assert!(source.contains(
        "RETURNS TABLE(\n    outcome TEXT,\n    resulting_revision BIGINT,\n    resulting_state TEXT,\n    exact_replay BOOLEAN,\n    guild_id TEXT\n)"
    ));
    assert!(!source
        .lines()
        .any(|line| line.trim_start().starts_with("--")));
    assert!(!source.contains("/*"));
    assert!(!source.contains("CREATE ROLE"));
    assert!(!source.contains("GRANT EXECUTE"));

    for declaration in [
        "CREATE FUNCTION public.starring_product_rejection_executor_database_identity_v1()",
        "CREATE FUNCTION public.starring_product_rejection_keyring_coverage_v1(",
        "CREATE FUNCTION public.starring_product_reject_v1(",
    ] {
        let definition = function_definition(source, declaration);
        for required in [
            "VOLATILE",
            "STRICT",
            "SECURITY DEFINER",
            "PARALLEL UNSAFE",
            "SET search_path = pg_catalog",
        ] {
            assert!(
                definition.contains(required),
                "missing function metadata {required}: {declaration}"
            );
        }
    }
    assert!(function_definition(
        source,
        "CREATE FUNCTION public.starring_product_rejection_keyring_coverage_v1("
    )
    .contains("LANGUAGE plpgsql"));

    for declaration in [
        "CREATE OR REPLACE FUNCTION public.enforce_product_action_receipt_retention()",
        "CREATE OR REPLACE FUNCTION public.enforce_product_action_receipt_alias_retention()",
        "CREATE OR REPLACE FUNCTION public.assert_product_approval_receipt_audit()",
        "CREATE OR REPLACE FUNCTION public.capture_product_action_receipt_audit_evidence()",
        "CREATE OR REPLACE FUNCTION public.starring_purge_product_action_receipts_v1(",
    ] {
        let definition = function_definition(source, declaration);
        assert!(definition.contains("'product_reject_v1'"));
        assert!(definition.contains("'promotion.reject'"));
    }
    let alias_definition = function_definition(
        source,
        "CREATE OR REPLACE FUNCTION public.assert_product_approval_receipt_alias()",
    );
    assert!(alias_definition.contains("'product_reject_v1'"));
    for required in [
        "product_action_receipts_approval_key_identity_required",
        "'product_approve_v1',\n        'product_apply_v1',\n        'product_promote_v1',\n        'product_reject_v1'",
        "product_action_receipts_rejection_retention_index",
        "product_action_aliases_rejection_receipt_retention_index",
        "state = 'rejected'",
        "rejected_at >= created_at",
        "rejected_at < expires_at",
        "pg_catalog.char_length(rejection_reason) BETWEEN 1 AND 1000",
        "pg_catalog.octet_length(rejection_reason) <= 4000",
        r"rejection_reason !~ U&'[\0001-\001F\007F-\009F]'",
        "REVOKE ALL PRIVILEGES ON ROUTINE %s FROM PUBLIC CASCADE",
        "REVOKE ALL PRIVILEGES ON FUNCTION public.starring_product_rejection_executor_database_identity_v1() FROM %I CASCADE",
        "REVOKE ALL PRIVILEGES ON FUNCTION public.starring_product_rejection_keyring_coverage_v1(text[],text[]) FROM %I CASCADE",
        "REVOKE ALL PRIVILEGES ON FUNCTION public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text) FROM %I CASCADE",
        "ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC",
        "ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE ALL PRIVILEGES ON FUNCTIONS FROM %s",
    ] {
        assert!(source.contains(required), "missing migration guard: {required}");
    }

    let executor = function_definition(
        source,
        "CREATE OR REPLACE FUNCTION public.enforce_product_activation_executor()",
    );
    assert!(executor.contains("VOLATILE\nSTRICT\nSECURITY DEFINER"));
    for required in [
        "OLD.state = 'rejected'",
        "NEW.state = 'rejected'",
        "OLD.state <> 'rejected'",
        "OLD.state <> 'pending'",
        "'starring.product_rejection_gate'",
        "NEW.product_revision IS DISTINCT FROM OLD.product_revision + 1",
        "- 'state'",
        "- 'product_revision'",
        "- 'rejected_at'",
        "- 'rejected_by'",
        "- 'rejection_reason'",
        "ERRCODE = '23514'",
    ] {
        assert!(
            executor.contains(required),
            "missing transition gate: {required}"
        );
    }
    let purge = function_definition(
        source,
        "CREATE OR REPLACE FUNCTION public.starring_purge_product_action_receipts_v1(",
    );
    assert!(purge.contains("VOLATILE\nCALLED ON NULL INPUT\nSECURITY DEFINER"));
    assert!(!source.lines().any(|line| {
        line.trim_start()
            .starts_with("CREATE TRIGGER activation_requests_enforce_product_executor")
    }));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn rejection_migration_applies_fresh_seals_new_capabilities_and_preserves_reader_acl() {
    let mut database = isolated_database("fresh").await;
    let legacy_role = format!("rejection_legacy_{}", suffix());
    let hostile_role = format!("rejection_hostile_{}", suffix());
    assert_safe_identifier(&legacy_role);
    assert_safe_identifier(&hostile_role);
    let outcome = async {
        apply_pre_rejection_migrations(&database.pool).await;
        for role in [&legacy_role, &hostile_role] {
            sqlx::query(&format!("CREATE ROLE {role} NOLOGIN"))
                .execute(&mut database.administrator)
                .await?;
        }
        sqlx::query(&format!(
            "GRANT EXECUTE ON FUNCTION {UNRELATED_READER_FUNCTION} TO {legacy_role}"
        ))
        .execute(&database.pool)
        .await?;
        sqlx::query(&format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             GRANT EXECUTE ON FUNCTIONS TO {hostile_role}"
        ))
        .execute(&database.pool)
        .await?;
        let reader_acl_before = sqlx::query_scalar::<_, Option<String>>(
            "SELECT function_row.proacl::TEXT \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(UNRELATED_READER_FUNCTION)
        .fetch_one(&database.pool)
        .await?;

        apply_rejection_migration(&database.pool).await?;

        let reader_acl_after = sqlx::query_scalar::<_, Option<String>>(
            "SELECT function_row.proacl::TEXT \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(UNRELATED_READER_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(reader_acl_before, reader_acl_after);
        let legacy_reader_execute = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
        )
        .bind(&legacy_role)
        .bind(UNRELATED_READER_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert!(legacy_reader_execute);

        let signatures = vec![
            REJECTION_IDENTITY_FUNCTION.to_string(),
            REJECTION_COVERAGE_FUNCTION.to_string(),
            REJECTION_EXECUTE_FUNCTION.to_string(),
        ];
        let exact_metadata_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.unnest($1::TEXT[]) AS expected(signature) \
             INNER JOIN pg_catalog.pg_proc AS function_row \
              ON function_row.oid = pg_catalog.to_regprocedure(expected.signature) \
             WHERE function_row.prokind = 'f' \
              AND function_row.provolatile = 'v' \
              AND function_row.proisstrict \
              AND function_row.proparallel = 'u' \
              AND function_row.prosecdef \
              AND function_row.proconfig = ARRAY['search_path=pg_catalog']::TEXT[]",
        )
        .bind(&signatures)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(exact_metadata_count, 3);
        let leaked_acl_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.unnest($1::TEXT[]) AS expected(signature) \
             INNER JOIN pg_catalog.pg_proc AS function_row \
              ON function_row.oid = pg_catalog.to_regprocedure(expected.signature) \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
              function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
             )) AS privilege \
             WHERE privilege.grantee <> function_row.proowner",
        )
        .bind(&signatures)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(leaked_acl_count, 0);
        for signature in &signatures {
            let hostile_execute = sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
            )
            .bind(&hostile_role)
            .bind(signature)
            .fetch_one(&database.pool)
            .await?;
            assert!(!hostile_execute, "hostile grant leaked to {signature}");
        }
        let leaked_default_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_default_acl AS defaults \
             INNER JOIN pg_catalog.pg_namespace AS namespace \
              ON namespace.oid = defaults.defaclnamespace \
             CROSS JOIN LATERAL pg_catalog.aclexplode(defaults.defaclacl) AS privilege \
             WHERE namespace.nspname = 'public' \
              AND defaults.defaclobjtype = 'f' \
              AND privilege.privilege_type = 'EXECUTE' \
              AND privilege.grantee <> defaults.defaclrole",
        )
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(leaked_default_count, 0);
        sqlx::query(
            "CREATE FUNCTION public.rejection_default_privilege_probe() RETURNS INTEGER \
             LANGUAGE sql VOLATILE STRICT SET search_path = pg_catalog AS 'SELECT 1'",
        )
        .execute(&database.pool)
        .await?;
        let hostile_probe_execute = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.has_function_privilege( \
             $1, 'public.rejection_default_privilege_probe()', 'EXECUTE')",
        )
        .bind(&hostile_role)
        .fetch_one(&database.pool)
        .await?;
        assert!(!hostile_probe_execute);

        let trigger_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_trigger AS trigger_row \
             WHERE trigger_row.tgrelid = 'public.activation_requests'::REGCLASS \
              AND trigger_row.tgname = 'activation_requests_enforce_product_executor' \
              AND NOT trigger_row.tgisinternal \
              AND trigger_row.tgfoid = pg_catalog.to_regprocedure($1)",
        )
        .bind(ACTIVATION_EXECUTOR_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(trigger_count, 1);
        let rejection_constraint = constraint_definition(&database.pool).await;
        assert!(rejection_constraint.contains("char_length(rejection_reason)"));
        assert!(rejection_constraint.contains("octet_length(rejection_reason)"));
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &[legacy_role, hostile_role]).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn rejection_migration_collision_rolls_back_without_residue() {
    let database = isolated_database("collision").await;
    let outcome = async {
        apply_pre_rejection_migrations(&database.pool).await;
        let constraint_before = constraint_definition(&database.pool).await;
        let executor_before = activation_executor_definition(&database.pool).await;
        sqlx::query(
            "CREATE FUNCTION public.starring_product_rejection_executor_database_identity_v1(TEXT) \
             RETURNS TEXT LANGUAGE sql AS 'SELECT $1'",
        )
        .execute(&database.pool)
        .await?;

        let error = rejected_migration_error(&database.pool).await;
        assert_object_state_error(
            &error,
            "product rejection object identity collides with existing state",
        );
        assert_eq!(
            constraint_definition(&database.pool).await,
            constraint_before
        );
        assert_eq!(
            activation_executor_definition(&database.pool).await,
            executor_before
        );
        assert_no_rejection_residue(&database.pool).await;
        assert!(
            function_exists(
                &database.pool,
                "public.starring_product_rejection_executor_database_identity_v1(text)"
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
async fn rejection_migration_rejects_owner_and_support_metadata_drift_atomically() {
    let mut database = isolated_database("drift").await;
    let hostile_role = format!("rejection_owner_{}", suffix());
    assert_safe_identifier(&hostile_role);
    let outcome = async {
        apply_pre_rejection_migrations(&database.pool).await;
        sqlx::query(&format!("CREATE ROLE {hostile_role} NOLOGIN"))
            .execute(&mut database.administrator)
            .await?;
        let common_owner = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_userbyid(relation.relowner) \
             FROM pg_catalog.pg_class AS relation \
             WHERE relation.oid = 'public.activation_request_approvals'::REGCLASS",
        )
        .fetch_one(&database.pool)
        .await?;
        assert_safe_identifier(&common_owner);
        let constraint_before = constraint_definition(&database.pool).await;
        let executor_before = activation_executor_definition(&database.pool).await;

        sqlx::query(&format!(
            "ALTER TABLE public.activation_request_approvals OWNER TO {hostile_role}"
        ))
        .execute(&database.pool)
        .await?;
        let owner_error = rejected_migration_error(&database.pool).await;
        assert_object_state_error(
            &owner_error,
            "product rejection relations require their common owner",
        );
        assert_eq!(
            constraint_definition(&database.pool).await,
            constraint_before
        );
        assert_eq!(
            activation_executor_definition(&database.pool).await,
            executor_before
        );
        assert_no_rejection_residue(&database.pool).await;
        sqlx::query(&format!(
            "ALTER TABLE public.activation_request_approvals OWNER TO {common_owner}"
        ))
        .execute(&database.pool)
        .await?;

        sqlx::query(&format!(
            "ALTER FUNCTION {ACTIVATION_EXECUTOR_FUNCTION} SECURITY INVOKER"
        ))
        .execute(&database.pool)
        .await?;
        let metadata_error = rejected_migration_error(&database.pool).await;
        assert_object_state_error(
            &metadata_error,
            "product rejection support function contract is invalid",
        );
        let security_definer = sqlx::query_scalar::<_, bool>(
            "SELECT function_row.prosecdef \
             FROM pg_catalog.pg_proc AS function_row \
             WHERE function_row.oid = pg_catalog.to_regprocedure($1)",
        )
        .bind(ACTIVATION_EXECUTOR_FUNCTION)
        .fetch_one(&database.pool)
        .await?;
        assert!(!security_definer);
        assert_eq!(
            constraint_definition(&database.pool).await,
            constraint_before
        );
        assert_no_rejection_residue(&database.pool).await;
        sqlx::query(&format!(
            "ALTER FUNCTION {ACTIVATION_EXECUTOR_FUNCTION} SECURITY DEFINER"
        ))
        .execute(&database.pool)
        .await?;
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &[hostile_role]).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn rejection_migration_rejects_invalid_historical_rejection_atomically() {
    let database = isolated_database("history").await;
    let outcome = async {
        apply_pre_rejection_migrations(&database.pool).await;
        let constraint_before = constraint_definition(&database.pool).await;
        let executor_before = activation_executor_definition(&database.pool).await;
        sqlx::query(
            "INSERT INTO public.activation_requests ( \
              id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
              required_approvals, state, created_at, expires_at, rejected_at, rejected_by, \
              rejection_reason \
             ) VALUES ( \
              'historical_invalid_rejection', '1', 'historical_ruleset', 1, \
              pg_catalog.repeat('a', 64), '2', 1, 'rejected', \
              pg_catalog.clock_timestamp() - INTERVAL '1 minute', \
              pg_catalog.clock_timestamp() + INTERVAL '1 hour', \
              pg_catalog.clock_timestamp(), '2', NULL \
             )",
        )
        .execute(&database.pool)
        .await?;

        let error = rejected_migration_error(&database.pool).await;
        assert_object_state_error(
            &error,
            "product rejection migration found invalid rejection state",
        );
        assert_eq!(
            constraint_definition(&database.pool).await,
            constraint_before
        );
        assert_eq!(
            activation_executor_definition(&database.pool).await,
            executor_before
        );
        assert_no_rejection_residue(&database.pool).await;
        let historical_row_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM public.activation_requests \
             WHERE id = 'historical_invalid_rejection' AND rejection_reason IS NULL",
        )
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(historical_row_count, 1);

        sqlx::query(
            "DELETE FROM public.activation_requests WHERE id = 'historical_invalid_rejection'",
        )
        .execute(&database.pool)
        .await?;
        apply_rejection_migration(&database.pool).await?;
        assert!(function_exists(&database.pool, REJECTION_EXECUTE_FUNCTION).await);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database, &[]).await;
    outcome.unwrap();
}
