use super::*;

const TRUSTED_WRITER_MIGRATION: i64 = 202_607_300_001;
const TRUSTED_WRITER_FUNCTIONS: [&str; 6] = [
    "public.starring_authoring_session_writer_database_identity_v1()",
    "public.starring_authoring_session_writer_check_v1(text,text,text,text,bigint,text[],text[],text[],text[])",
    "public.starring_authoring_session_writer_load_v1(text,text,text,text,bigint)",
    "public.starring_authoring_session_writer_commit_v1(text,text,text,text,bigint,text[],text[],text[],text[],text,text,text,text,bigint,bytea,bytea,text,text,smallint,text,jsonb,text,bigint,text,jsonb,text,bigint,text,bytea,text,bigint)",
    "public.starring_authoring_session_writer_key_coverage_v1(text[],text[],text[])",
    "public.starring_product_authorized_snapshot_read_v2(text,text,bytea,text,text)",
];
const TRUSTED_WRITER_COLUMNS: [&str; 5] = [
    "writer_semantic_request_digest",
    "writer_digest_key_id",
    "writer_digest_key_fingerprint",
    "safe_turn_projection",
    "safe_turn_projection_digest",
];
const TRUSTED_WRITER_CONSTRAINTS: [&str; 6] = [
    "authoring_generations_writer_metadata_presence_valid",
    "authoring_generations_writer_semantic_digest_valid",
    "authoring_generations_writer_key_identity_valid",
    "authoring_generations_safe_projection_valid",
    "authoring_generations_trusted_stage_valid",
    "authoring_generations_trusted_candidate_valid",
];

fn trusted_writer_migration() -> &'static sqlx::migrate::Migration {
    MIGRATOR
        .iter()
        .find(|migration| migration.version == TRUSTED_WRITER_MIGRATION)
        .expect("trusted authoring writer migration must exist")
}

async fn apply_pre_trusted_writer_migrations(pool: &PgPool) {
    for migration in MIGRATOR
        .iter()
        .filter(|migration| migration.version < TRUSTED_WRITER_MIGRATION)
    {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
    }
}

async fn apply_trusted_writer_migration(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::raw_sql(trusted_writer_migration().sql.as_ref())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await
}

async fn apply_trusted_writer_migration_expect_failure(pool: &PgPool) -> sqlx::Error {
    let mut transaction = pool.begin().await.unwrap();
    let error = sqlx::raw_sql(trusted_writer_migration().sql.as_ref())
        .execute(&mut *transaction)
        .await
        .expect_err("trusted writer migration must fail");
    transaction.rollback().await.unwrap();
    error
}

async fn drop_migration_database(
    mut administrator: PgConnection,
    pool: PgPool,
    database_name: &str,
) {
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database_name} WITH (FORCE)"))
        .execute(&mut administrator)
        .await
        .unwrap();
}

async fn seed_legacy_generation(pool: &PgPool, suffix: &str) -> Scope {
    let scope = seed_scope(pool, suffix).await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.authoring_sessions \
         (session_id, tenant_id, installation_id, owner_principal_id, current_generation, \
          lifecycle_state) \
         VALUES ($1, $2, $3, $4, 1, 'active')",
    )
    .bind(&scope.session_id)
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(&scope.principal_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.authoring_session_generations \
         (session_id, generation, tenant_id, installation_id, snapshot_schema_version, \
          snapshot_ciphertext, snapshot_nonce, encryption_key_id, encryption_suite, \
          encryption_suite_version, authenticated_metadata_digest, resource_bindings, \
          binding_fingerprint, installation_authority_revision, summary, stage, \
          candidate_revision, candidate_hash, writer_request_digest, harness_contract_revision) \
         VALUES ($1, 1, $2, $3, 1, $4, $5, 'legacy-snapshot-v1', \
          'xchacha20_poly1305', 1, $6, $7, $8, 1, $9, 'discussion', NULL, NULL, $10, 1)",
    )
    .bind(&scope.session_id)
    .bind(&scope.tenant_id)
    .bind(&scope.installation_id)
    .bind(vec![0x41_u8; 48])
    .bind(vec![0x42_u8; 24])
    .bind(digest(format!("legacy-metadata:{suffix}")))
    .bind(Json(&scope.bindings))
    .bind(&scope.binding_fingerprint)
    .bind(Json(&json!({"legacy": true})))
    .bind(digest(format!("legacy-request:{suffix}")))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    scope
}

async fn assert_trusted_writer_residue_counts(
    pool: &PgPool,
    expected_column_count: i64,
    expected_constraint_count: i64,
) {
    let column_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_attribute AS attribute \
         WHERE attribute.attrelid = pg_catalog.to_regclass(\
             'public.authoring_session_generations'\
         ) \
         AND attribute.attnum > 0 \
         AND NOT attribute.attisdropped \
         AND attribute.attname = ANY($1)",
    )
    .bind(TRUSTED_WRITER_COLUMNS.as_slice())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(column_count, expected_column_count);

    let constraint_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM pg_catalog.pg_constraint AS constraint_row \
         WHERE constraint_row.conrelid = pg_catalog.to_regclass(\
             'public.authoring_session_generations'\
         ) \
         AND constraint_row.conname = ANY($1)",
    )
    .bind(TRUSTED_WRITER_CONSTRAINTS.as_slice())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(constraint_count, expected_constraint_count);

    for function in TRUSTED_WRITER_FUNCTIONS {
        let exists =
            sqlx::query_scalar::<_, bool>("SELECT pg_catalog.to_regprocedure($1) IS NOT NULL")
                .bind(function)
                .fetch_one(pool)
                .await
                .unwrap();
        assert!(!exists, "unexpected trusted writer function {function}");
    }
}

async fn assert_no_trusted_writer_residue(pool: &PgPool) {
    assert_trusted_writer_residue_counts(pool, 0, 0).await;
}

fn assert_operational_error(error: &sqlx::Error, expected_message: &str) {
    let database_error = error
        .as_database_error()
        .expect("migration failure must be a PostgreSQL error");
    assert_eq!(database_error.code().as_deref(), Some("55000"));
    assert_eq!(database_error.message(), expected_message);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn trusted_writer_column_collision_fails_without_additional_residue() {
    let database_name = format!("starring_writer_column_test_{}", unique_suffix());
    let (administrator, pool) = temporary_database(&database_name).await;
    let outcome = async {
        apply_pre_trusted_writer_migrations(&pool).await;
        sqlx::query(
            "ALTER TABLE public.authoring_session_generations \
             ADD COLUMN writer_semantic_request_digest BIGINT NOT NULL DEFAULT 7",
        )
        .execute(&pool)
        .await?;
        let collision_before = sqlx::query_as::<_, (String, bool, Option<String>)>(
            "SELECT pg_catalog.format_type(attribute.atttypid, attribute.atttypmod), \
             attribute.attnotnull, \
             pg_catalog.pg_get_expr(default_row.adbin, default_row.adrelid) \
             FROM pg_catalog.pg_attribute AS attribute \
             LEFT JOIN pg_catalog.pg_attrdef AS default_row \
             ON default_row.adrelid = attribute.attrelid \
             AND default_row.adnum = attribute.attnum \
             WHERE attribute.attrelid = pg_catalog.to_regclass(\
                 'public.authoring_session_generations'\
             ) \
             AND attribute.attname = 'writer_semantic_request_digest' \
             AND attribute.attnum > 0 \
             AND NOT attribute.attisdropped",
        )
        .fetch_one(&pool)
        .await?;

        let error = apply_trusted_writer_migration_expect_failure(&pool).await;
        assert_operational_error(&error, "authoring writer generation column collision");
        assert_trusted_writer_residue_counts(&pool, 1, 0).await;

        let collision_after = sqlx::query_as::<_, (String, bool, Option<String>)>(
            "SELECT pg_catalog.format_type(attribute.atttypid, attribute.atttypmod), \
             attribute.attnotnull, \
             pg_catalog.pg_get_expr(default_row.adbin, default_row.adrelid) \
             FROM pg_catalog.pg_attribute AS attribute \
             LEFT JOIN pg_catalog.pg_attrdef AS default_row \
             ON default_row.adrelid = attribute.attrelid \
             AND default_row.adnum = attribute.attnum \
             WHERE attribute.attrelid = pg_catalog.to_regclass(\
                 'public.authoring_session_generations'\
             ) \
             AND attribute.attname = 'writer_semantic_request_digest' \
             AND attribute.attnum > 0 \
             AND NOT attribute.attisdropped",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(collision_before, collision_after);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_migration_database(administrator, pool, &database_name).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn trusted_writer_constraint_collision_fails_without_additional_residue() {
    let database_name = format!("starring_writer_constraint_test_{}", unique_suffix());
    let (administrator, pool) = temporary_database(&database_name).await;
    let outcome = async {
        apply_pre_trusted_writer_migrations(&pool).await;
        sqlx::query(
            "ALTER TABLE public.authoring_session_generations \
             ADD CONSTRAINT authoring_generations_writer_metadata_presence_valid \
             CHECK (generation > 0) NOT VALID",
        )
        .execute(&pool)
        .await?;
        let collision_before = sqlx::query_as::<_, (String, bool)>(
            "SELECT pg_catalog.pg_get_constraintdef(constraint_row.oid, TRUE), \
             constraint_row.convalidated \
             FROM pg_catalog.pg_constraint AS constraint_row \
             WHERE constraint_row.conrelid = pg_catalog.to_regclass(\
                 'public.authoring_session_generations'\
             ) \
             AND constraint_row.conname = \
                 'authoring_generations_writer_metadata_presence_valid'",
        )
        .fetch_one(&pool)
        .await?;

        let error = apply_trusted_writer_migration_expect_failure(&pool).await;
        assert_operational_error(&error, "authoring writer generation constraint collision");
        assert_trusted_writer_residue_counts(&pool, 0, 1).await;

        let collision_after = sqlx::query_as::<_, (String, bool)>(
            "SELECT pg_catalog.pg_get_constraintdef(constraint_row.oid, TRUE), \
             constraint_row.convalidated \
             FROM pg_catalog.pg_constraint AS constraint_row \
             WHERE constraint_row.conrelid = pg_catalog.to_regclass(\
                 'public.authoring_session_generations'\
             ) \
             AND constraint_row.conname = \
                 'authoring_generations_writer_metadata_presence_valid'",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(collision_before, collision_after);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_migration_database(administrator, pool, &database_name).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn trusted_writer_upgrade_preserves_legacy_generation_with_null_writer_metadata() {
    let database_name = format!("starring_writer_upgrade_test_{}", unique_suffix());
    let (administrator, pool) = temporary_database(&database_name).await;
    let outcome = async {
        apply_pre_trusted_writer_migrations(&pool).await;
        let scope = seed_legacy_generation(&pool, &unique_suffix()).await;
        let before = sqlx::query_scalar::<_, Json<Value>>(
            "SELECT pg_catalog.to_jsonb(generation_row) \
             FROM public.authoring_session_generations AS generation_row \
             WHERE generation_row.session_id = $1 AND generation_row.generation = 1",
        )
        .bind(&scope.session_id)
        .fetch_one(&pool)
        .await?;

        apply_trusted_writer_migration(&pool).await?;

        let after = sqlx::query_scalar::<_, Json<Value>>(
            "SELECT pg_catalog.to_jsonb(generation_row) - $2::TEXT[] \
             FROM public.authoring_session_generations AS generation_row \
             WHERE generation_row.session_id = $1 AND generation_row.generation = 1",
        )
        .bind(&scope.session_id)
        .bind(TRUSTED_WRITER_COLUMNS.as_slice())
        .fetch_one(&pool)
        .await?;
        assert_eq!(before.0, after.0);

        let metadata = sqlx::query_as::<
            _,
            (
                Option<String>,
                Option<String>,
                Option<String>,
                Option<Vec<u8>>,
                Option<String>,
            ),
        >(
            "SELECT writer_semantic_request_digest, writer_digest_key_id, \
             writer_digest_key_fingerprint, safe_turn_projection, safe_turn_projection_digest \
             FROM public.authoring_session_generations \
             WHERE session_id = $1 AND generation = 1",
        )
        .bind(&scope.session_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(metadata, (None, None, None, None, None));
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_migration_database(administrator, pool, &database_name).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn trusted_writer_function_collision_fails_without_partial_residue() {
    let database_name = format!("starring_writer_collision_test_{}", unique_suffix());
    let (administrator, pool) = temporary_database(&database_name).await;
    let outcome = async {
        apply_pre_trusted_writer_migrations(&pool).await;
        sqlx::query(
            "CREATE FUNCTION public.starring_authoring_session_writer_check_v1(TEXT) \
             RETURNS TEXT LANGUAGE sql IMMUTABLE AS 'SELECT $1'",
        )
        .execute(&pool)
        .await?;
        let stub_before = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(\
                'public.starring_authoring_session_writer_check_v1(text)'\
             ))",
        )
        .fetch_one(&pool)
        .await?;

        let error = apply_trusted_writer_migration_expect_failure(&pool).await;
        assert_operational_error(&error, "authoring writer function collision");
        assert_no_trusted_writer_residue(&pool).await;

        let stub_after = sqlx::query_scalar::<_, String>(
            "SELECT pg_catalog.pg_get_functiondef(pg_catalog.to_regprocedure(\
                'public.starring_authoring_session_writer_check_v1(text)'\
             ))",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(stub_before, stub_after);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_migration_database(administrator, pool, &database_name).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn trusted_writer_hostile_public_schema_acl_fails_without_partial_residue() {
    let database_name = format!("starring_writer_acl_test_{}", unique_suffix());
    let (administrator, pool) = temporary_database(&database_name).await;
    let outcome = async {
        apply_pre_trusted_writer_migrations(&pool).await;
        sqlx::query("GRANT CREATE ON SCHEMA public TO PUBLIC")
            .execute(&pool)
            .await?;

        let error = apply_trusted_writer_migration_expect_failure(&pool).await;
        assert_operational_error(&error, "authoring writer schema is not trusted");
        assert_no_trusted_writer_residue(&pool).await;

        let public_create_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_namespace AS namespace \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(\
                 namespace.nspacl, \
                 pg_catalog.acldefault('n', namespace.nspowner)\
             )) AS privilege \
             WHERE namespace.nspname = 'public' \
             AND privilege.grantee = 0 \
             AND privilege.privilege_type = 'CREATE'",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(public_create_count, 1);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_migration_database(administrator, pool, &database_name).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires superuser STARRING_TEST_DATABASE_URL"]
async fn trusted_writer_postflight_drift_aborts_the_whole_migration() {
    let database_name = format!("starring_writer_postflight_test_{}", unique_suffix());
    let (administrator, pool) = temporary_database(&database_name).await;
    let outcome = async {
        apply_pre_trusted_writer_migrations(&pool).await;
        let superuser = sqlx::query_scalar::<_, bool>(
            "SELECT role.rolsuper \
             FROM pg_catalog.pg_roles AS role \
             WHERE role.rolname = CURRENT_USER",
        )
        .fetch_one(&pool)
        .await?;
        assert!(superuser);
        sqlx::raw_sql(
            "CREATE TABLE public.authoring_writer_postflight_probe (injected BOOLEAN NOT NULL); \
             INSERT INTO public.authoring_writer_postflight_probe VALUES (FALSE); \
             CREATE FUNCTION public.inject_authoring_writer_postflight_drift() \
             RETURNS EVENT_TRIGGER \
             LANGUAGE plpgsql \
             SET search_path = pg_catalog, public \
             AS $function$ \
             BEGIN \
                 IF TG_TAG = 'CREATE FUNCTION' \
                     AND pg_catalog.to_regprocedure(\
                         'public.starring_authoring_session_writer_key_coverage_v1(text[],text[],text[])'\
                     ) IS NOT NULL \
                     AND NOT EXISTS (\
                         SELECT 1 \
                         FROM public.authoring_writer_postflight_probe \
                         WHERE injected\
                     ) \
                 THEN \
                     UPDATE public.authoring_writer_postflight_probe SET injected = TRUE; \
                     ALTER FUNCTION \
                         public.starring_authoring_session_writer_key_coverage_v1(\
                             TEXT[], TEXT[], TEXT[]\
                         ) \
                         IMMUTABLE; \
                 END IF; \
             END; \
             $function$; \
             CREATE EVENT TRIGGER inject_authoring_writer_postflight_drift \
             ON ddl_command_end \
             WHEN TAG IN ('CREATE FUNCTION') \
             EXECUTE FUNCTION public.inject_authoring_writer_postflight_drift();",
        )
        .execute(&pool)
        .await?;

        let error = apply_trusted_writer_migration_expect_failure(&pool).await;
        assert_operational_error(&error, "authoring writer function metadata is invalid");
        assert_no_trusted_writer_residue(&pool).await;

        let injected = sqlx::query_scalar::<_, bool>(
            "SELECT injected FROM public.authoring_writer_postflight_probe",
        )
        .fetch_one(&pool)
        .await?;
        assert!(!injected);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_migration_database(administrator, pool, &database_name).await;
    outcome.unwrap();
}
