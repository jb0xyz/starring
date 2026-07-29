use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::MIGRATOR;
use futures::future;
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::types::Json;
use sqlx::Connection;

#[path = "postgres_authoring_writer/migration_security.rs"]
mod migration_security;
#[path = "postgres_authoring_writer/read_security.rs"]
mod read_security;
#[path = "postgres_authoring_writer/write_security.rs"]
mod write_security;

const COMMIT_QUERY: &str = "SELECT * FROM public.starring_authoring_session_writer_commit_v1(\
     $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,\
     $21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31)";
const CHECK_QUERY: &str =
    "SELECT * FROM public.starring_authoring_session_writer_check_v1($1,$2,$3,$4,$5,$6,$7,$8,$9)";
const LOAD_QUERY: &str =
    "SELECT * FROM public.starring_authoring_session_writer_load_v1($1,$2,$3,$4,$5)";
const WRITER_FUNCTIONS: [&str; 5] = [
    "public.starring_authoring_session_writer_database_identity_v1()",
    "public.starring_authoring_session_writer_check_v1(text,text,text,text,bigint,text[],text[],text[],text[])",
    "public.starring_authoring_session_writer_commit_v1(text,text,text,text,bigint,text[],text[],text[],text[],text,text,text,text,bigint,bytea,bytea,text,text,smallint,text,jsonb,text,bigint,text,jsonb,text,bigint,text,bytea,text,bigint)",
    "public.starring_authoring_session_writer_key_coverage_v1(text[],text[],text[])",
    "public.starring_authoring_session_writer_load_v1(text,text,text,text,bigint)",
];
const WRITER_FUNCTION_NAMES: [&str; 5] = [
    "starring_authoring_session_writer_check_v1",
    "starring_authoring_session_writer_commit_v1",
    "starring_authoring_session_writer_database_identity_v1",
    "starring_authoring_session_writer_key_coverage_v1",
    "starring_authoring_session_writer_load_v1",
];

#[derive(Clone)]
struct Scope {
    tenant_id: String,
    installation_id: String,
    principal_id: String,
    session_id: String,
    bindings: Value,
    binding_fingerprint: String,
    authority_revision: i64,
    authority_digest: String,
}

#[derive(Clone)]
struct CommitInput {
    request_digest: String,
    semantic_digest: String,
    writer_key_id: String,
    writer_key_fingerprint: String,
    snapshot_schema_version: i64,
    ciphertext: Vec<u8>,
    nonce: Vec<u8>,
    encryption_key_id: String,
    metadata_digest: String,
    summary: Value,
    stage: String,
    candidate_revision: Option<i64>,
    candidate_hash: Option<String>,
    projection: Vec<u8>,
    projection_digest: String,
}

#[derive(sqlx::FromRow)]
struct CommitRow {
    outcome_code: String,
    current_generation: Option<i64>,
    committed_generation: Option<i64>,
    safe_turn_projection: Option<Vec<u8>>,
    safe_turn_projection_digest: Option<String>,
}

#[derive(sqlx::FromRow)]
struct CheckRow {
    outcome_code: String,
    current_generation: Option<i64>,
    matched_generation: Option<i64>,
    safe_turn_projection: Option<Vec<u8>>,
    safe_turn_projection_digest: Option<String>,
}

#[derive(sqlx::FromRow)]
struct LoadRow {
    outcome_code: String,
    head_generation: Option<i64>,
    snapshot_ciphertext: Option<Vec<u8>>,
    snapshot_nonce: Option<Vec<u8>>,
    encryption_key_id: Option<String>,
    authenticated_metadata_digest: Option<String>,
    resource_bindings: Option<Json<Value>>,
    binding_fingerprint: Option<String>,
    installation_authority_revision: Option<i64>,
    authority_payload_digest: Option<String>,
    writer_request_digest: Option<String>,
    writer_semantic_request_digest: Option<String>,
    writer_digest_key_id: Option<String>,
    writer_digest_key_fingerprint: Option<String>,
    safe_turn_projection: Option<Vec<u8>>,
    safe_turn_projection_digest: Option<String>,
    current_authority_revision: Option<i64>,
    current_authority_payload_digest: Option<String>,
    current_resource_bindings: Option<Json<Value>>,
    current_binding_fingerprint: Option<String>,
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

fn unique_suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string()
}

fn digest(seed: impl AsRef<[u8]>) -> String {
    let bytes = Sha256::digest(seed);
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

async fn temporary_database(name: &str) -> (PgConnection, PgPool) {
    assert!(
        name.starts_with("starring_")
            && name.split('_').any(|segment| segment == "test")
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    let options = database_url().parse::<PgConnectOptions>().unwrap();
    let mut administrator = PgConnection::connect_with(&options.clone().database("postgres"))
        .await
        .unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .execute(&mut administrator)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.database(name))
        .await
        .unwrap();
    (administrator, pool)
}

async fn application_pool(name: &str) -> PgPool {
    let options = database_url().parse::<PgConnectOptions>().unwrap();
    PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options.database(name))
        .await
        .unwrap()
}

async fn apply_fresh_migrations(pool: &PgPool) {
    for migration in MIGRATOR.iter() {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }
}

async fn grant_writer_capability(pool: &PgPool, administrator: &mut PgConnection, role: &str) {
    sqlx::query(&format!(
        "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS"
    ))
    .execute(&mut *administrator)
    .await
    .unwrap();
    sqlx::query(&format!("GRANT {role} TO CURRENT_USER"))
        .execute(&mut *administrator)
        .await
        .unwrap();
    sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {role}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!("REVOKE CREATE ON SCHEMA public FROM {role}"))
        .execute(pool)
        .await
        .unwrap();
    for identity in WRITER_FUNCTIONS {
        sqlx::query(&format!("GRANT EXECUTE ON FUNCTION {identity} TO {role}"))
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn seed_scope(pool: &PgPool, suffix: &str) -> Scope {
    let scope = Scope {
        tenant_id: format!("tenant-{suffix}"),
        installation_id: format!("installation-{suffix}"),
        principal_id: format!("principal-{suffix}"),
        session_id: format!("session-{suffix}"),
        bindings: json!({"role_bindings": {}, "channel_bindings": {}}),
        binding_fingerprint: resource_binding_fingerprint_v2(&ResourceBindingMap::default())
            .into_string(),
        authority_revision: 1,
        authority_digest: digest(format!("authority:{suffix}:1")),
    };
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals \
         (principal_id, discord_user_id, display_profile) \
         VALUES ($1, $2, '{}'::JSONB)",
    )
    .bind(&scope.principal_id)
    .bind("900000000000000001")
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) \
         VALUES ($1, 'active', 'Authoring writer integration')",
    )
    .bind(&scope.tenant_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, ruleset_key, \
          lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, '900000000000000002', '900000000000000003', \
          'authoring_writer_test', 'active', 1)",
    )
    .bind(&scope.installation_id)
    .bind(&scope.tenant_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, 1, $3, $4, 1, 1, 3600, $5, $6, $7)",
    )
    .bind(&scope.installation_id)
    .bind(&scope.tenant_id)
    .bind(Json(&scope.bindings))
    .bind(&scope.binding_fingerprint)
    .bind(&scope.authority_digest)
    .bind(&scope.principal_id)
    .bind(digest(format!("authority-request:{suffix}:1")))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    scope
}

fn commit_input(seed: &str, projection: &[u8]) -> CommitInput {
    CommitInput {
        request_digest: digest(format!("request:{seed}")),
        semantic_digest: digest(format!("semantic:{seed}")),
        writer_key_id: "writer-v1".to_string(),
        writer_key_fingerprint: digest("writer-v1-fingerprint"),
        snapshot_schema_version: 1,
        ciphertext: vec![seed.as_bytes()[0]; 48],
        nonce: vec![seed.as_bytes()[0]; 24],
        encryption_key_id: "snapshot-v1".to_string(),
        metadata_digest: digest(format!("metadata:{seed}")),
        summary: json!({"turn": seed}),
        stage: "discussion".to_string(),
        candidate_revision: None,
        candidate_hash: None,
        projection: projection.to_vec(),
        projection_digest: digest(projection),
    }
}

async fn set_local_role(transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>, role: &str) {
    sqlx::query(&format!("SET LOCAL ROLE {role}"))
        .execute(&mut **transaction)
        .await
        .unwrap();
}

async fn assert_role_statement_denied(pool: &PgPool, role: &str, statement: &str) {
    let mut transaction = pool.begin().await.unwrap();
    set_local_role(&mut transaction, role).await;
    let error = sqlx::query(statement)
        .execute(&mut *transaction)
        .await
        .expect_err("restricted role statement must fail");
    assert_eq!(
        error.as_database_error().and_then(|error| error.code()),
        Some(std::borrow::Cow::Borrowed("42501"))
    );
    transaction.rollback().await.unwrap();
}

async fn execute_commit(
    pool: &PgPool,
    role: &str,
    scope: &Scope,
    expected_generation: i64,
    input: &CommitInput,
    persist: bool,
) -> CommitRow {
    execute_commit_with_candidates(
        pool,
        role,
        scope,
        expected_generation,
        input,
        &[input],
        persist,
    )
    .await
}

async fn execute_commit_with_candidates(
    pool: &PgPool,
    role: &str,
    scope: &Scope,
    expected_generation: i64,
    input: &CommitInput,
    candidates: &[&CommitInput],
    persist: bool,
) -> CommitRow {
    let mut transaction = pool.begin().await.unwrap();
    set_local_role(&mut transaction, role).await;
    let row = sqlx::query_as::<_, CommitRow>(COMMIT_QUERY)
        .bind(&scope.tenant_id)
        .bind(&scope.installation_id)
        .bind(&scope.principal_id)
        .bind(&scope.session_id)
        .bind(expected_generation)
        .bind(
            candidates
                .iter()
                .map(|candidate| candidate.request_digest.clone())
                .collect::<Vec<_>>(),
        )
        .bind(
            candidates
                .iter()
                .map(|candidate| candidate.semantic_digest.clone())
                .collect::<Vec<_>>(),
        )
        .bind(
            candidates
                .iter()
                .map(|candidate| candidate.writer_key_id.clone())
                .collect::<Vec<_>>(),
        )
        .bind(
            candidates
                .iter()
                .map(|candidate| candidate.writer_key_fingerprint.clone())
                .collect::<Vec<_>>(),
        )
        .bind(&input.request_digest)
        .bind(&input.semantic_digest)
        .bind(&input.writer_key_id)
        .bind(&input.writer_key_fingerprint)
        .bind(input.snapshot_schema_version)
        .bind(&input.ciphertext)
        .bind(&input.nonce)
        .bind(&input.encryption_key_id)
        .bind("xchacha20_poly1305")
        .bind(1_i16)
        .bind(&input.metadata_digest)
        .bind(Json(&scope.bindings))
        .bind(&scope.binding_fingerprint)
        .bind(scope.authority_revision)
        .bind(&scope.authority_digest)
        .bind(Json(&input.summary))
        .bind(&input.stage)
        .bind(input.candidate_revision)
        .bind(input.candidate_hash.as_deref())
        .bind(&input.projection)
        .bind(&input.projection_digest)
        .bind(1_i64)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    if persist {
        transaction.commit().await.unwrap();
    } else {
        transaction.rollback().await.unwrap();
    }
    row
}

async fn commit(
    pool: &PgPool,
    role: &str,
    scope: &Scope,
    expected_generation: i64,
    input: &CommitInput,
) -> CommitRow {
    execute_commit(pool, role, scope, expected_generation, input, true).await
}

async fn commit_with_candidates(
    pool: &PgPool,
    role: &str,
    scope: &Scope,
    expected_generation: i64,
    input: &CommitInput,
    candidates: &[&CommitInput],
) -> CommitRow {
    execute_commit_with_candidates(
        pool,
        role,
        scope,
        expected_generation,
        input,
        candidates,
        true,
    )
    .await
}

async fn commit_then_rollback(
    pool: &PgPool,
    role: &str,
    scope: &Scope,
    expected_generation: i64,
    input: &CommitInput,
) -> CommitRow {
    execute_commit(pool, role, scope, expected_generation, input, false).await
}

async fn check(
    pool: &PgPool,
    role: &str,
    scope: &Scope,
    expected_generation: i64,
    input: &CommitInput,
) -> CheckRow {
    check_with_candidates(pool, role, scope, expected_generation, &[input]).await
}

async fn check_with_candidates(
    pool: &PgPool,
    role: &str,
    scope: &Scope,
    expected_generation: i64,
    candidates: &[&CommitInput],
) -> CheckRow {
    let mut transaction = pool.begin().await.unwrap();
    set_local_role(&mut transaction, role).await;
    let row = sqlx::query_as::<_, CheckRow>(CHECK_QUERY)
        .bind(&scope.tenant_id)
        .bind(&scope.installation_id)
        .bind(&scope.principal_id)
        .bind(&scope.session_id)
        .bind(expected_generation)
        .bind(
            candidates
                .iter()
                .map(|candidate| candidate.request_digest.clone())
                .collect::<Vec<_>>(),
        )
        .bind(
            candidates
                .iter()
                .map(|candidate| candidate.semantic_digest.clone())
                .collect::<Vec<_>>(),
        )
        .bind(
            candidates
                .iter()
                .map(|candidate| candidate.writer_key_id.clone())
                .collect::<Vec<_>>(),
        )
        .bind(
            candidates
                .iter()
                .map(|candidate| candidate.writer_key_fingerprint.clone())
                .collect::<Vec<_>>(),
        )
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    row
}

async fn load(pool: &PgPool, role: &str, scope: &Scope, requested_generation: i64) -> LoadRow {
    let mut transaction = pool.begin().await.unwrap();
    set_local_role(&mut transaction, role).await;
    let row = sqlx::query_as::<_, LoadRow>(LOAD_QUERY)
        .bind(&scope.tenant_id)
        .bind(&scope.installation_id)
        .bind(&scope.principal_id)
        .bind(&scope.session_id)
        .bind(requested_generation)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    row
}

async fn key_coverage(
    pool: &PgPool,
    role: &str,
    encryption_key_ids: Vec<String>,
    writer_key_ids: Vec<String>,
    writer_key_fingerprints: Vec<String>,
) -> bool {
    let mut transaction = pool.begin().await.unwrap();
    set_local_role(&mut transaction, role).await;
    let covered = sqlx::query_scalar::<_, bool>(
        "SELECT covered \
         FROM public.starring_authoring_session_writer_key_coverage_v1($1,$2,$3)",
    )
    .bind(encryption_key_ids)
    .bind(writer_key_ids)
    .bind(writer_key_fingerprints)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    covered
}

async fn rotate_authority(pool: &PgPool, scope: &mut Scope, suffix: &str) {
    let next_digest = digest(format!("authority:{suffix}:2"));
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 2, $2, 1, $3, $4, 2, 1, 3600, $5, $6, $7)",
    )
    .bind(&scope.installation_id)
    .bind(&scope.tenant_id)
    .bind(Json(&scope.bindings))
    .bind(&scope.binding_fingerprint)
    .bind(&next_digest)
    .bind(&scope.principal_id)
    .bind(digest(format!("authority-request:{suffix}:2")))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.automation_installations \
         SET current_authority_revision = 2, \
          updated_at = GREATEST( \
           pg_catalog.clock_timestamp(), updated_at + INTERVAL '1 microsecond') \
         WHERE tenant_id = $1 AND installation_id = $2",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    scope.authority_revision = 2;
    scope.authority_digest = next_digest;
}

async fn assert_writer_least_privilege(pool: &PgPool, role: &str) {
    let executable = sqlx::query_scalar::<_, String>(
        "SELECT function_row.proname::TEXT \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
          ON namespace.oid = function_row.pronamespace \
         WHERE namespace.nspname = 'public' \
          AND function_row.proname LIKE 'starring_authoring_session_writer_%' \
          AND pg_catalog.has_function_privilege($1, function_row.oid, 'EXECUTE') \
         ORDER BY function_row.proname",
    )
    .bind(role)
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(executable, WRITER_FUNCTION_NAMES);
    let unexpected_executable = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_proc AS function_row \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
          ON namespace.oid = function_row.pronamespace \
         WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema') \
          AND pg_catalog.has_function_privilege($1, function_row.oid, 'EXECUTE') \
          AND NOT ( \
           namespace.nspname = 'public' \
           AND function_row.proname = ANY($2::TEXT[]))",
    )
    .bind(role)
    .bind(
        WRITER_FUNCTION_NAMES
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(unexpected_executable, 0);
    let can_read_promotion = sqlx::query_scalar::<_, bool>(
        "SELECT pg_catalog.has_function_privilege( \
         $1, \
         'public.starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)', \
         'EXECUTE')",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(!can_read_promotion);
    let relation_privileges = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
          ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname = 'public' \
          AND relation.relkind IN ('r','p','v','m','S','f') \
          AND pg_catalog.has_table_privilege( \
           $1, relation.oid, \
           'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(relation_privileges, 0);
    let column_privileges = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_class AS relation \
         INNER JOIN pg_catalog.pg_namespace AS namespace \
          ON namespace.oid = relation.relnamespace \
         WHERE namespace.nspname = 'public' \
          AND relation.relkind IN ('r','p','v','m','f') \
          AND pg_catalog.has_any_column_privilege( \
           $1, relation.oid, 'SELECT,INSERT,UPDATE,REFERENCES')",
    )
    .bind(role)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(column_privileges, 0);

    let expected_database_identity = sqlx::query_scalar::<_, String>(
        "SELECT database_identity::TEXT \
         FROM public.product_control_plane_identity \
         WHERE singleton",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    set_local_role(&mut transaction, role).await;
    let actual_database_identity = sqlx::query_scalar::<_, String>(
        "SELECT public.starring_authoring_session_writer_database_identity_v1()",
    )
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(actual_database_identity, expected_database_identity);
    transaction.rollback().await.unwrap();

    for statement in [
        "SELECT snapshot_ciphertext FROM public.authoring_session_generations",
        "INSERT INTO public.authoring_session_generations DEFAULT VALUES",
        "UPDATE public.authoring_session_generations \
         SET stage = 'discussion' WHERE FALSE",
        "DELETE FROM public.authoring_session_generations WHERE FALSE",
    ] {
        assert_role_statement_denied(pool, role, statement).await;
    }

    let adjacent_api_function =
        "public.starring_product_deployment_status_reader_database_identity_v1()";
    let adjacent_reader_function =
        "public.starring_product_authorized_snapshot_reader_database_identity_v1()";
    let api_role = format!("{role}_api");
    let reader_role = format!("{role}_reader");
    let public_role = format!("{role}_public");
    let unrelated_role = format!("{role}_unrelated");
    for restricted_role in [&api_role, &reader_role, &public_role, &unrelated_role] {
        sqlx::query(&format!(
            "CREATE ROLE {restricted_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOINHERIT NOREPLICATION NOBYPASSRLS"
        ))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(&format!("GRANT {restricted_role} TO CURRENT_USER"))
            .execute(pool)
            .await
            .unwrap();
    }
    for restricted_role in [&api_role, &reader_role, &unrelated_role] {
        sqlx::query(&format!(
            "GRANT USAGE ON SCHEMA public TO {restricted_role}"
        ))
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {adjacent_api_function} TO {api_role}"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "GRANT EXECUTE ON FUNCTION {adjacent_reader_function} TO {reader_role}"
    ))
    .execute(pool)
    .await
    .unwrap();

    for restricted_role in [&api_role, &reader_role, &public_role, &unrelated_role] {
        for writer_function in WRITER_FUNCTIONS {
            let can_execute = sqlx::query_scalar::<_, bool>(
                "SELECT pg_catalog.has_function_privilege($1, $2, 'EXECUTE')",
            )
            .bind(restricted_role)
            .bind(writer_function)
            .fetch_one(pool)
            .await
            .unwrap();
            assert!(!can_execute);
        }
        assert_role_statement_denied(
            pool,
            restricted_role,
            "SELECT public.starring_authoring_session_writer_database_identity_v1()",
        )
        .await;
    }

    for restricted_role in [&api_role, &reader_role, &public_role, &unrelated_role] {
        sqlx::query(&format!("DROP OWNED BY {restricted_role}"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(&format!("DROP ROLE {restricted_role}"))
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn cleanup(mut administrator: PgConnection, pool: PgPool, database_name: &str, role: &str) {
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database_name} WITH (FORCE)"))
        .execute(&mut administrator)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {role}"))
        .execute(&mut administrator)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn trusted_authoring_writer_is_atomic_replay_safe_and_relation_blind() {
    let suffix = unique_suffix();
    let tail = &suffix[suffix.len().saturating_sub(14)..];
    let database_name = format!("starring_authoring_writer_test_{tail}");
    let role = format!("starring_authoring_writer_test_{tail}");
    let (mut administrator, migration_pool) = temporary_database(&database_name).await;
    apply_fresh_migrations(&migration_pool).await;
    migration_pool.close().await;
    let pool = application_pool(&database_name).await;
    grant_writer_capability(&pool, &mut administrator, &role).await;
    let mut scope = seed_scope(&pool, tail).await;
    assert_writer_least_privilege(&pool, &role).await;

    let projection_one = br#"{"generation":1,"state":"discussion","text":"first"}"#;
    let generation_one = commit_input("first", projection_one);
    let first = commit(&pool, &role, &scope, 0, &generation_one).await;
    assert_eq!(first.outcome_code, "committed");
    assert_eq!(first.current_generation, Some(1));
    assert_eq!(first.committed_generation, Some(1));
    assert_eq!(
        first.safe_turn_projection.as_deref(),
        Some(projection_one.as_slice())
    );
    assert_eq!(
        first.safe_turn_projection_digest.as_deref(),
        Some(generation_one.projection_digest.as_str())
    );

    let replay = commit(&pool, &role, &scope, 0, &generation_one).await;
    assert_eq!(replay.outcome_code, "exact_replay");
    assert_eq!(replay.current_generation, Some(1));
    assert_eq!(replay.committed_generation, Some(1));
    assert_eq!(
        replay.safe_turn_projection.as_deref(),
        Some(projection_one.as_slice())
    );
    assert_eq!(
        replay.safe_turn_projection_digest.as_deref(),
        Some(generation_one.projection_digest.as_str())
    );

    let mut rotated_active = commit_input("rotated-active", projection_one);
    rotated_active.writer_key_id = "writer-v2".to_string();
    rotated_active.writer_key_fingerprint = digest("writer-v2-fingerprint");
    let active_only = check(&pool, &role, &scope, 0, &rotated_active).await;
    assert_eq!(active_only.outcome_code, "generation_conflict");
    assert_eq!(active_only.current_generation, Some(1));
    assert!(active_only.matched_generation.is_none());
    assert!(active_only.safe_turn_projection.is_none());
    assert!(active_only.safe_turn_projection_digest.is_none());
    let rotated_candidates = [&rotated_active, &generation_one];
    let retired_check = check_with_candidates(&pool, &role, &scope, 0, &rotated_candidates).await;
    assert_eq!(retired_check.outcome_code, "exact_replay");
    assert_eq!(retired_check.current_generation, Some(1));
    assert_eq!(retired_check.matched_generation, Some(1));
    assert_eq!(
        retired_check.safe_turn_projection.as_deref(),
        Some(projection_one.as_slice())
    );
    assert_eq!(
        retired_check.safe_turn_projection_digest.as_deref(),
        Some(generation_one.projection_digest.as_str())
    );
    let retired_commit = commit_with_candidates(
        &pool,
        &role,
        &scope,
        0,
        &rotated_active,
        &rotated_candidates,
    )
    .await;
    assert_eq!(retired_commit.outcome_code, "exact_replay");
    assert_eq!(retired_commit.current_generation, Some(1));
    assert_eq!(retired_commit.committed_generation, Some(1));
    assert_eq!(
        retired_commit.safe_turn_projection.as_deref(),
        Some(projection_one.as_slice())
    );
    assert_eq!(
        retired_commit.safe_turn_projection_digest.as_deref(),
        Some(generation_one.projection_digest.as_str())
    );

    let mut semantic_conflict = generation_one.clone();
    semantic_conflict.semantic_digest = digest("semantic-conflict");
    let conflict = commit(&pool, &role, &scope, 0, &semantic_conflict).await;
    assert_eq!(conflict.outcome_code, "idempotency_conflict");
    assert_eq!(conflict.current_generation, Some(1));
    assert_eq!(conflict.committed_generation, Some(1));
    assert!(conflict.safe_turn_projection.is_none());
    assert!(conflict.safe_turn_projection_digest.is_none());

    let mut key_metadata_mismatch = generation_one.clone();
    key_metadata_mismatch.writer_key_fingerprint = digest("different-writer-key");
    let invalid_metadata = check(&pool, &role, &scope, 0, &key_metadata_mismatch).await;
    assert_eq!(invalid_metadata.outcome_code, "invalid_state");
    assert_eq!(invalid_metadata.current_generation, Some(1));
    assert!(invalid_metadata.matched_generation.is_none());
    assert!(invalid_metadata.safe_turn_projection.is_none());
    assert!(invalid_metadata.safe_turn_projection_digest.is_none());

    let generation_two_a = commit_input(
        "second-a",
        br#"{"generation":2,"state":"discussion","text":"second-a"}"#,
    );
    let generation_two_b = commit_input(
        "second-b",
        br#"{"generation":2,"state":"discussion","text":"second-b"}"#,
    );
    let (second_a, second_b) = future::join(
        commit(&pool, &role, &scope, 1, &generation_two_a),
        commit(&pool, &role, &scope, 1, &generation_two_b),
    )
    .await;
    let mut outcomes = [
        second_a.outcome_code.as_str(),
        second_b.outcome_code.as_str(),
    ];
    outcomes.sort_unstable();
    assert_eq!(outcomes, ["committed", "generation_conflict"]);
    assert_eq!(
        [second_a.current_generation, second_b.current_generation],
        [Some(2), Some(2)]
    );
    assert_eq!(
        [second_a.committed_generation, second_b.committed_generation]
            .into_iter()
            .filter(|generation| generation.is_some())
            .collect::<Vec<_>>(),
        vec![Some(2)]
    );

    let historical = load(&pool, &role, &scope, 1).await;
    assert_eq!(historical.outcome_code, "loaded");
    assert_eq!(historical.head_generation, Some(1));
    assert_eq!(
        historical.snapshot_ciphertext.as_deref(),
        Some(generation_one.ciphertext.as_slice())
    );
    assert_eq!(
        historical.snapshot_nonce.as_deref(),
        Some(generation_one.nonce.as_slice())
    );
    assert_eq!(
        historical.encryption_key_id.as_deref(),
        Some(generation_one.encryption_key_id.as_str())
    );
    assert_eq!(
        historical.authenticated_metadata_digest.as_deref(),
        Some(generation_one.metadata_digest.as_str())
    );
    assert_eq!(
        historical.resource_bindings.as_ref().map(|value| &value.0),
        Some(&scope.bindings)
    );
    assert_eq!(
        historical.binding_fingerprint.as_deref(),
        Some(scope.binding_fingerprint.as_str())
    );
    assert_eq!(historical.installation_authority_revision, Some(1));
    assert_eq!(
        historical.authority_payload_digest.as_deref(),
        Some(scope.authority_digest.as_str())
    );
    assert_eq!(
        historical.writer_request_digest.as_deref(),
        Some(generation_one.request_digest.as_str())
    );
    assert_eq!(
        historical.writer_semantic_request_digest.as_deref(),
        Some(generation_one.semantic_digest.as_str())
    );
    assert_eq!(
        historical.writer_digest_key_id.as_deref(),
        Some(generation_one.writer_key_id.as_str())
    );
    assert_eq!(
        historical.writer_digest_key_fingerprint.as_deref(),
        Some(generation_one.writer_key_fingerprint.as_str())
    );
    assert_eq!(
        historical.safe_turn_projection.as_deref(),
        Some(projection_one.as_slice())
    );
    assert_eq!(
        historical.safe_turn_projection_digest.as_deref(),
        Some(generation_one.projection_digest.as_str())
    );

    let authority_one_digest = scope.authority_digest.clone();
    rotate_authority(&pool, &mut scope, tail).await;
    let after_rotation = load(&pool, &role, &scope, 1).await;
    assert_eq!(after_rotation.installation_authority_revision, Some(1));
    assert_eq!(
        after_rotation.authority_payload_digest.as_deref(),
        Some(authority_one_digest.as_str())
    );
    assert_eq!(after_rotation.current_authority_revision, Some(2));
    assert_eq!(
        after_rotation.current_authority_payload_digest.as_deref(),
        Some(scope.authority_digest.as_str())
    );
    assert_eq!(
        after_rotation
            .current_resource_bindings
            .as_ref()
            .map(|value| &value.0),
        Some(&scope.bindings)
    );
    assert_eq!(
        after_rotation.current_binding_fingerprint.as_deref(),
        Some(scope.binding_fingerprint.as_str())
    );
    assert_eq!(
        after_rotation.safe_turn_projection.as_deref(),
        Some(projection_one.as_slice())
    );

    let replay_after_rotation = commit(&pool, &role, &scope, 0, &generation_one).await;
    assert_eq!(replay_after_rotation.outcome_code, "exact_replay");
    assert_eq!(
        replay_after_rotation.safe_turn_projection.as_deref(),
        Some(projection_one.as_slice())
    );

    let mut stale_scope = scope.clone();
    stale_scope.authority_revision = 1;
    stale_scope.authority_digest = authority_one_digest;
    let generation_three = commit_input(
        "third",
        br#"{"generation":3,"state":"discussion","text":"third"}"#,
    );
    let stale_commit = commit(&pool, &role, &stale_scope, 2, &generation_three).await;
    assert_eq!(stale_commit.outcome_code, "authority_conflict");
    assert_eq!(stale_commit.current_generation, Some(2));
    assert!(stale_commit.committed_generation.is_none());

    let rolled_back = commit_then_rollback(&pool, &role, &scope, 2, &generation_three).await;
    assert_eq!(rolled_back.outcome_code, "committed");
    assert_eq!(rolled_back.current_generation, Some(3));
    assert_eq!(rolled_back.committed_generation, Some(3));
    let after_rollback = check(&pool, &role, &scope, 2, &generation_three).await;
    assert_eq!(after_rollback.outcome_code, "proceed");
    assert_eq!(after_rollback.current_generation, Some(2));
    assert!(after_rollback.matched_generation.is_none());
    let third = commit(&pool, &role, &scope, 2, &generation_three).await;
    assert_eq!(third.outcome_code, "committed");
    assert_eq!(third.current_generation, Some(3));
    assert_eq!(third.committed_generation, Some(3));
    let third_replay = commit(&pool, &role, &scope, 2, &generation_three).await;
    assert_eq!(third_replay.outcome_code, "exact_replay");
    assert_eq!(third_replay.current_generation, Some(3));
    assert_eq!(third_replay.committed_generation, Some(3));
    assert_eq!(
        third_replay.safe_turn_projection.as_deref(),
        Some(generation_three.projection.as_slice())
    );

    let coverage = key_coverage(
        &pool,
        &role,
        vec!["snapshot-v1".to_string()],
        vec!["writer-v1".to_string()],
        vec![generation_one.writer_key_fingerprint.clone()],
    )
    .await;
    assert!(coverage);
    let incomplete_coverage = key_coverage(
        &pool,
        &role,
        vec!["snapshot-v1".to_string()],
        vec!["writer-v1".to_string()],
        vec![digest("wrong-key-fingerprint")],
    )
    .await;
    assert!(!incomplete_coverage);

    let tamper_error = sqlx::query(
        "UPDATE public.authoring_session_generations \
         SET safe_turn_projection = decode('00', 'hex') \
         WHERE session_id = $1 AND generation = 1",
    )
    .bind(&scope.session_id)
    .execute(&pool)
    .await
    .expect_err("committed generation metadata must be immutable");
    assert_eq!(
        tamper_error
            .as_database_error()
            .and_then(|error| error.code()),
        Some(std::borrow::Cow::Borrowed("23514"))
    );

    let generation_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.authoring_session_generations \
         WHERE tenant_id = $1 AND installation_id = $2 AND session_id = $3",
    )
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.session_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(generation_count, 3);

    cleanup(administrator, pool, &database_name, &role).await;
}
