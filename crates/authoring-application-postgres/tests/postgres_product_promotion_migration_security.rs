use authoring_application_postgres::MIGRATOR;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Connection;

const PROMOTION_MIGRATION: i64 = 202_607_200_002;

fn suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
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
        .max_connections(4)
        .connect_with(options.database(name))
        .await
        .unwrap();
    (administrator, pool)
}

async fn drop_temporary_database(mut administrator: PgConnection, pool: PgPool, name: &str) {
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&mut administrator)
        .await
        .unwrap();
}

#[test]
fn promotion_migration_is_fail_closed_and_scoped() {
    let migration =
        include_str!("../../../migrations/202607200002_scope_product_promotion_execution.sql");
    assert_eq!(
        migration
            .matches("CREATE FUNCTION public.starring_product_promotion_")
            .count(),
        10
    );
    assert_eq!(
        migration
            .matches("RETURNS TABLE(\n    outcome_code TEXT")
            .count(),
        7
    );
    for required in [
        "product_admission_format_version SMALLINT",
        "product_admission_digest TEXT",
        "product_admission JSONB",
        "pg_catalog.octet_length(product_admission::TEXT) <= 32768",
        "product_admission_payload JSONB",
        "product_admission_digest TEXT",
        "authoring_promotions_enforce_product_admission",
        "authoring_promotions_enforce_product_transition",
        "starring.product_promotion_legacy_repair_gate",
        ") = NEW.product_admission_digest",
        "OLD.stage = 'prepared'",
        "OLD.stage = 'published'",
        "OLD.stage = 'activation_pending'",
        "WHEN 'product_promote_v1' THEN 'promotion.promote'",
        "product_action_receipts_promotion_retention_index",
        "product_action_aliases_promotion_receipt_retention_index",
        "promotion.stage IN ('prepared', 'published')",
        "REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE",
    ] {
        assert!(migration.contains(required), "missing guard: {required}");
    }
    assert!(!migration.contains("CREATE ROLE"));
    assert!(!migration.contains("GRANT EXECUTE"));
    assert!(!migration.contains(
        "NEW.product_admission ->> 'admitted_at'\n            IS DISTINCT FROM NEW.record ->> 'created_at'"
    ));
    assert!(!migration.contains("--"));
    assert!(!migration.contains("/*"));
    let publish_body = migration
        .split("CREATE FUNCTION public.starring_product_promotion_publish_v1(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    for required in [
        "starring_product_promotion_authorize_current_v1",
        "FOR UPDATE",
        "INSERT INTO public.automation_ruleset_versions",
        "UPDATE public.automation_ruleset_heads",
        "UPDATE public.authoring_promotions",
        "starring_ruleset_content_hash_v1",
        "'published_exact'",
        "'final_exact'",
    ] {
        assert!(
            publish_body.contains(required),
            "missing publish guard: {required}"
        );
    }
    assert!(!publish_body.contains("automation_ruleset_activations.active_version"));

    let environment_body = migration
        .split("CREATE FUNCTION public.starring_product_promotion_approval_environment_v1(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    for required in [
        "starring_product_promotion_authorize_current_v1",
        "promotion_record JSONB",
        "historical_resource_bindings",
        "historical_binding_fingerprint",
        "automation_ruleset_activations",
        "starring_product_ruleset_slot_exact_v1",
        "target_artifact_projection",
        "RETURN QUERY SELECT 'resolved'",
    ] {
        assert!(
            environment_body.contains(required),
            "missing approval-environment guard: {required}"
        );
    }
    assert!(!environment_body.contains("INSERT INTO"));
    assert!(!environment_body.contains("UPDATE public."));
    assert!(!environment_body.contains("DELETE FROM"));

    let activation_body = migration
        .split("CREATE FUNCTION public.starring_product_promotion_activation_link_v1(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    for required in [
        "starring_product_promotion_authorize_current_v1",
        "FOR UPDATE",
        "INSERT INTO public.activation_requests",
        "UPDATE public.authoring_promotions",
        "starring_product_promotion_finalize_receipt_v1",
        "'final_replay_required'",
        "calculated_active_content_hash",
        "'approval_environment_changed'",
    ] {
        assert!(
            activation_body.contains(required),
            "missing activation guard: {required}"
        );
    }
    assert!(!activation_body.contains("UPDATE public.automation_ruleset_activations"));
    assert!(!activation_body.contains("DELETE FROM"));

    let repair_body = migration
        .split("CREATE FUNCTION public.starring_product_promotion_repair_link_v1(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    for required in [
        "starring_product_promotion_authorize_current_v1",
        "FOR UPDATE",
        "starring.product_promotion_legacy_repair_gate",
        "UPDATE public.authoring_promotions",
        "UPDATE public.activation_requests",
        "starring_product_promotion_finalize_receipt_v1",
        "'recovered'",
        "'final_replay_required'",
    ] {
        assert!(
            repair_body.contains(required),
            "missing repair guard: {required}"
        );
    }
    assert!(!repair_body.contains("INSERT INTO public.automation_ruleset_versions"));
    assert!(!repair_body.contains("INSERT INTO public.activation_requests"));
    assert!(!repair_body.contains("UPDATE public.automation_ruleset_activations"));
    assert!(!repair_body.contains("DELETE FROM"));

    for helper in [
        "public.enforce_authoring_promotion_scope()",
        "public.enforce_authoring_promotion_product_admission()",
        "public.enforce_authoring_promotion_product_transition()",
        "public.reject_ruleset_artifact_mutation()",
        "public.enforce_product_activation_journal_link()",
        "public.enforce_product_activation_scope()",
        "public.guard_legacy_activation_product_slot()",
        "public.guard_product_ruleset_artifact_transition()",
        "public.assert_product_approval_receipt_alias()",
        "public.assert_product_approval_receipt_audit()",
        "public.enforce_product_action_receipt_retention()",
        "public.enforce_product_action_receipt_alias_capacity()",
        "public.enforce_product_action_receipt_alias_retention()",
        "public.capture_product_action_receipt_audit_evidence()",
        "public.reject_immutable_product_approval_row()",
    ] {
        assert!(migration.contains(&format!("'{helper}'")));
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn promotion_migration_applies_fresh_and_collision_rolls_back_without_residue() {
    let name = "starring_promotion_migration_test";
    let (mut administrator, pool) = temporary_database(name).await;
    let hostile_role = format!("starring_promotion_hostile_{}", suffix());
    sqlx::query(&format!(
        "CREATE ROLE {hostile_role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOINHERIT NOREPLICATION NOBYPASSRLS"
    ))
    .execute(&mut administrator)
    .await
    .unwrap();
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version < PROMOTION_MIGRATION)
        {
            let mut transaction = pool.begin().await?;
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
        }
        sqlx::query(
            "CREATE FUNCTION public.starring_product_promotion_replay_v1(TEXT) \
             RETURNS TEXT LANGUAGE sql AS 'SELECT $1'",
        )
        .execute(&pool)
        .await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == PROMOTION_MIGRATION)
            .unwrap();
        let mut transaction = pool.begin().await?;
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .expect_err("collision must fail the migration");
        transaction.rollback().await?;
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
        ));
        let admission_columns = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'authoring_promotions' \
               AND column_name LIKE 'product_admission%'",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(admission_columns, 0);
        let created_functions = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.pg_proc AS function_row \
             INNER JOIN pg_catalog.pg_namespace AS namespace \
               ON namespace.oid = function_row.pronamespace \
             WHERE namespace.nspname = 'public' \
               AND function_row.proname LIKE 'starring_product_promotion_%' \
               AND function_row.pronargs <> 1",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(created_functions, 0);

        sqlx::query("DROP FUNCTION public.starring_product_promotion_replay_v1(TEXT)")
            .execute(&pool)
            .await?;
        sqlx::query(&format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             GRANT EXECUTE ON FUNCTIONS TO {hostile_role}"
        ))
        .execute(&pool)
        .await?;
        let mut transaction = pool.begin().await?;
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await
            .expect_err("hostile default privileges must fail the migration preflight");
        transaction.rollback().await?;
        assert!(matches!(
            error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
        ));
        let preflight_residue = sqlx::query_as::<_, (i64, i64)>(
            "SELECT \
             (SELECT pg_catalog.count(*) FROM information_schema.columns \
              WHERE table_schema = 'public' AND table_name = 'authoring_promotions' \
                AND column_name LIKE 'product_admission%'), \
             (SELECT pg_catalog.count(*) FROM pg_catalog.pg_proc AS function_row \
              INNER JOIN pg_catalog.pg_namespace AS namespace \
                ON namespace.oid = function_row.pronamespace \
              WHERE namespace.nspname = 'public' \
                AND function_row.proname LIKE 'starring_product_promotion_%')",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(preflight_residue, (0, 0));
        sqlx::query(&format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             REVOKE EXECUTE ON FUNCTIONS FROM {hostile_role}"
        ))
        .execute(&pool)
        .await?;
        let trigger_helpers = [
            "public.enforce_authoring_promotion_scope()",
            "public.reject_ruleset_artifact_mutation()",
            "public.enforce_product_activation_journal_link()",
            "public.enforce_product_activation_scope()",
            "public.guard_legacy_activation_product_slot()",
            "public.guard_product_ruleset_artifact_transition()",
            "public.assert_product_approval_receipt_alias()",
            "public.assert_product_approval_receipt_audit()",
            "public.enforce_product_action_receipt_retention()",
            "public.enforce_product_action_receipt_alias_capacity()",
            "public.enforce_product_action_receipt_alias_retention()",
            "public.capture_product_action_receipt_audit_evidence()",
            "public.reject_immutable_product_approval_row()",
            "public.starring_canonical_json_v1(jsonb)",
            "public.starring_ruleset_content_hash_v1(bigint,jsonb)",
            "public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)",
        ];
        for function in trigger_helpers {
            sqlx::query(&format!(
                "GRANT EXECUTE ON FUNCTION {function} TO {hostile_role} WITH GRANT OPTION"
            ))
            .execute(&pool)
            .await?;
        }
        let mut transaction = pool.begin().await?;
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        let protected_trigger_helpers = vec![
            "public.enforce_authoring_promotion_scope()".to_string(),
            "public.enforce_authoring_promotion_product_admission()".to_string(),
            "public.enforce_authoring_promotion_product_transition()".to_string(),
            "public.reject_ruleset_artifact_mutation()".to_string(),
            "public.enforce_product_activation_journal_link()".to_string(),
            "public.enforce_product_activation_scope()".to_string(),
            "public.guard_legacy_activation_product_slot()".to_string(),
            "public.guard_product_ruleset_artifact_transition()".to_string(),
            "public.assert_product_approval_receipt_alias()".to_string(),
            "public.assert_product_approval_receipt_audit()".to_string(),
            "public.enforce_product_action_receipt_retention()".to_string(),
            "public.enforce_product_action_receipt_alias_capacity()".to_string(),
            "public.enforce_product_action_receipt_alias_retention()".to_string(),
            "public.capture_product_action_receipt_audit_evidence()".to_string(),
            "public.reject_immutable_product_approval_row()".to_string(),
            "public.starring_canonical_json_v1(jsonb)".to_string(),
            "public.starring_ruleset_content_hash_v1(bigint,jsonb)".to_string(),
            "public.starring_product_ruleset_slot_exact_v1(text,text,text,text,bigint)".to_string(),
        ];
        let helper_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM pg_catalog.unnest($1::TEXT[]) AS expected(identity) \
             WHERE pg_catalog.to_regprocedure(expected.identity) IS NOT NULL",
        )
        .bind(&protected_trigger_helpers)
        .fetch_one(&pool)
        .await?;
        assert_eq!(helper_count, 18);
        let leaked_acl_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.unnest($1::TEXT[]) AS expected(identity) \
             INNER JOIN pg_catalog.pg_proc AS function_row \
               ON function_row.oid = pg_catalog.to_regprocedure(expected.identity) \
             CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE( \
              function_row.proacl, pg_catalog.acldefault('f', function_row.proowner) \
             )) AS privilege \
             WHERE privilege.grantee <> function_row.proowner",
        )
        .bind(&protected_trigger_helpers)
        .fetch_one(&pool)
        .await?;
        assert_eq!(leaked_acl_count, 0);
        let hostile_execute_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM pg_catalog.unnest($1::TEXT[]) AS expected(identity) \
             WHERE pg_catalog.has_function_privilege( \
              $2, pg_catalog.to_regprocedure(expected.identity), 'EXECUTE')",
        )
        .bind(&protected_trigger_helpers)
        .bind(&hostile_role)
        .fetch_one(&pool)
        .await?;
        assert_eq!(hostile_execute_count, 0);

        let hostile_outcome = sqlx::query_scalar::<_, String>(
            "SELECT outcome_code \
             FROM public.starring_product_promotion_authorize_current_v1( \
              'tenant', 'installation', 'principal', \
              pg_catalog.decode(pg_catalog.repeat('00', 32), 'hex'), \
              'not-numeric', 'not-numeric', 'not-numeric', 'promote', 1, \
              pg_catalog.repeat('0', 64), pg_catalog.repeat('0', 64), \
              pg_catalog.clock_timestamp(), \
              pg_catalog.clock_timestamp() + INTERVAL '1 second', \
              'not-numeric', FALSE)",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(hostile_outcome, "access_denied");

        let mut probe_connection = pool.acquire().await?;
        sqlx::raw_sql(
            "CREATE TEMP TABLE transition_probe \
             (LIKE public.authoring_promotions INCLUDING ALL); \
             CREATE TRIGGER transition_probe_guard \
             BEFORE INSERT OR UPDATE ON transition_probe \
             FOR EACH ROW EXECUTE FUNCTION \
             public.enforce_authoring_promotion_product_transition()",
        )
        .execute(&mut *probe_connection)
        .await?;
        let null_error = sqlx::query(
            "INSERT INTO transition_probe \
             (id, record_format_version, revision, stage, request_digest, tenant_id, \
              installation_id, principal_id, record, product_admission_format_version, \
              product_admission_digest, product_admission) \
             VALUES (pg_catalog.repeat('1', 64), 1, 1, 'prepared', \
              pg_catalog.repeat('2', 64), 'tenant', 'installation', 'principal', \
              pg_catalog.jsonb_build_object( \
               'id', pg_catalog.repeat('1', 64), 'revision', 1, \
               'request_digest', pg_catalog.repeat('2', 64), \
               'intent', pg_catalog.jsonb_build_object('authority', \
                pg_catalog.jsonb_build_object('tenant_id', 'tenant', \
                 'installation_id', 'installation', 'principal_id', 'principal')), \
               'stage', pg_catalog.jsonb_build_object('state', 'prepared'), \
               'created_at', '2026-07-20T00:00:00Z', 'updated_at', 'null'::JSONB), \
              1, pg_catalog.repeat('3', 64), \
              pg_catalog.jsonb_build_object('format_version', 1, 'payload', '{}'::JSONB, \
               'admitted_at', '2026-07-20T00:00:00Z'))",
        )
        .execute(&mut *probe_connection)
        .await
        .expect_err("JSON null timestamp must fail closed");
        assert!(
            matches!(
                &null_error,
                sqlx::Error::Database(database) if database.code().as_deref() == Some("23514")
            ),
            "{null_error:?}"
        );
        let unknown_stage_error = sqlx::query(
            "INSERT INTO transition_probe \
             (id, record_format_version, revision, stage, request_digest, tenant_id, \
              installation_id, principal_id, record, product_admission_format_version, \
              product_admission_digest, product_admission) \
             VALUES (pg_catalog.repeat('4', 64), 1, 1, 'prepared', \
              pg_catalog.repeat('5', 64), 'tenant', 'installation', 'principal', \
              pg_catalog.jsonb_build_object( \
               'id', pg_catalog.repeat('4', 64), 'revision', 1, \
               'request_digest', pg_catalog.repeat('5', 64), \
               'intent', pg_catalog.jsonb_build_object('authority', \
                pg_catalog.jsonb_build_object('tenant_id', 'tenant', \
                 'installation_id', 'installation', 'principal_id', 'principal')), \
               'stage', pg_catalog.jsonb_build_object('state', 'prepared', 'unknown', TRUE), \
               'created_at', '2026-07-20T00:00:00Z', \
               'updated_at', '2026-07-20T00:00:00Z'), \
              1, pg_catalog.repeat('6', 64), \
              pg_catalog.jsonb_build_object('format_version', 1, 'payload', '{}'::JSONB, \
               'admitted_at', '2026-07-20T00:00:00Z'))",
        )
        .execute(&mut *probe_connection)
        .await
        .expect_err("unknown stage key must fail closed");
        assert!(
            matches!(
                &unknown_stage_error,
                sqlx::Error::Database(database) if database.code().as_deref() == Some("23514")
            ),
            "{unknown_stage_error:?}"
        );

        sqlx::raw_sql(
            "CREATE TEMP TABLE admission_probe \
             (LIKE public.authoring_promotions INCLUDING ALL); \
             CREATE TRIGGER admission_probe_guard \
             BEFORE INSERT OR UPDATE ON admission_probe \
             FOR EACH ROW EXECUTE FUNCTION \
             public.enforce_authoring_promotion_product_admission()",
        )
        .execute(&mut *probe_connection)
        .await?;
        let unknown_admission_error = sqlx::query(
            "INSERT INTO admission_probe \
             (id, record_format_version, revision, stage, request_digest, tenant_id, \
              installation_id, principal_id, record, product_admission_format_version, \
              product_admission_digest, product_admission) \
             VALUES (pg_catalog.repeat('7', 64), 1, 1, 'prepared', \
              pg_catalog.repeat('8', 64), 'tenant', 'installation', 'principal', \
              pg_catalog.jsonb_build_object( \
               'id', pg_catalog.repeat('7', 64), 'revision', 1, \
               'request_digest', pg_catalog.repeat('8', 64), \
               'intent', pg_catalog.jsonb_build_object('authority', \
                pg_catalog.jsonb_build_object('tenant_id', 'tenant', \
                 'installation_id', 'installation', 'principal_id', 'principal')), \
               'stage', pg_catalog.jsonb_build_object('state', 'prepared'), \
               'created_at', '2026-07-20T00:00:00Z', \
               'updated_at', '2026-07-20T00:00:00Z'), \
              1, pg_catalog.repeat('9', 64), \
              pg_catalog.jsonb_build_object('format_version', 1, \
               'payload', pg_catalog.jsonb_build_object('unknown', TRUE), \
               'admitted_at', '2026-07-20T00:00:00Z'))",
        )
        .execute(&mut *probe_connection)
        .await
        .expect_err("unknown admission key must fail closed");
        assert!(
            matches!(
                &unknown_admission_error,
                sqlx::Error::Database(database) if database.code().as_deref() == Some("23514")
            ),
            "{unknown_admission_error:?}"
        );

        sqlx::query(
            r#"INSERT INTO admission_probe
             (id, record_format_version, revision, stage, request_digest, tenant_id,
              installation_id, principal_id, record, product_admission_format_version,
              product_admission_digest, product_admission)
             VALUES (pg_catalog.repeat('a', 64), 1, 1, 'prepared',
              pg_catalog.repeat('b', 64), 'tenant', 'installation', 'principal',
              pg_catalog.jsonb_build_object(
               'id', pg_catalog.repeat('a', 64), 'revision', 1,
               'request_digest', pg_catalog.repeat('b', 64),
               'intent', pg_catalog.jsonb_build_object(
                'authority', pg_catalog.jsonb_build_object(
                 'tenant_id', 'tenant', 'installation_id', 'installation',
                 'principal_id', 'principal', 'session_id', 'session',
                 'session_generation', 1, 'guild_id', '1', 'requester', '2',
                 'binding_revision', 1, 'policy', pg_catalog.jsonb_build_object(
                  'revision', 1)),
                'evidence', pg_catalog.jsonb_build_object(
                 'candidate_revision', 1, 'candidate_ruleset_hash', pg_catalog.repeat('c', 64),
                 'context_fingerprint', pg_catalog.repeat('d', 64))),
               'stage', pg_catalog.jsonb_build_object('state', 'prepared'),
               'created_at', '2026-07-20T00:00:00Z',
               'updated_at', '2026-07-20T00:00:00Z'),
              1, pg_catalog.repeat('e', 64),
              pg_catalog.jsonb_build_object(
               'format_version', 1,
               'payload', pg_catalog.jsonb_build_object(
                'endpoint_domain', 'product_promote_v1',
                'product_request_id', 'request', 'tenant_id', 'tenant',
                'installation_id', 'installation', 'principal_id', 'principal',
                'authoring_session_id', 'session', 'generation', '1',
                'candidate_revision', '1', 'candidate_hash', pg_catalog.repeat('c', 64),
                'promotion_id', pg_catalog.repeat('a', 64),
                'promotion_request_digest', pg_catalog.repeat('b', 64),
                'session_subject_digest', pg_catalog.repeat('0', 64),
                'idempotency_key_digest', pg_catalog.repeat('1', 64),
                'idempotency_digest_key_id', 'active',
                'idempotency_digest_key_fingerprint', pg_catalog.repeat('2', 64),
                'semantic_request_digest', pg_catalog.repeat('3', 64),
                'receipt_id', pg_catalog.repeat('4', 64),
                'audit_event_id', pg_catalog.repeat('5', 64),
                'discord_application_id', '3', 'guild_id', '1',
                'acting_user_id', '2', 'capability', 'promote',
                'authority_revision', '1',
                'authority_payload_digest', pg_catalog.repeat('6', 64),
                'authority_observation_digest', pg_catalog.repeat('7', 64),
                'authority_observed_at', '2026-07-20T00:00:00Z',
                'authority_expires_at', '2026-07-20T00:00:01Z',
                'effective_permission_bits', '8', 'guild_owner', FALSE,
                'binding_fingerprint', pg_catalog.repeat('d', 64),
                'policy_revision', '1'),
               'admitted_at', '2026-07-20T00:00:00Z'))"#,
        )
        .execute(&mut *probe_connection)
        .await?;
        let drift_update = sqlx::query(
            "UPDATE admission_probe \
             SET record = pg_catalog.jsonb_set( \
              record, '{updated_at}', '\"2026-07-20T00:00:01Z\"'::JSONB) \
             WHERE id = pg_catalog.repeat('a', 64)",
        )
        .execute(&mut *probe_connection)
        .await?;
        assert_eq!(drift_update.rows_affected(), 1);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    let cleanup = sqlx::query(&format!("DROP OWNED BY {hostile_role}"))
        .execute(&pool)
        .await;
    let role_cleanup = sqlx::query(&format!("DROP ROLE {hostile_role}"))
        .execute(&mut administrator)
        .await;
    drop_temporary_database(administrator, pool, name).await;
    cleanup.unwrap();
    role_cleanup.unwrap();
    outcome.unwrap();
}
