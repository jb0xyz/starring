use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use authoring_application_postgres::{
    PostgresProductActionRetention, PostgresProductActionRetentionConfig,
    ProductActionRetentionError, ProductDatabaseFailureV1, MIGRATOR,
};
use chrono::{DateTime, TimeDelta, Utc};
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor, Postgres};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct PurgeOutcome {
    deleted_receipts: i32,
    deleted_aliases: i32,
    backlog_remaining: bool,
}

struct TestDatabase {
    name: String,
    administrator: PgConnection,
    pool: PgPool,
}

struct AuthorityFixture {
    token: String,
    tenant_id: String,
    installation_id: String,
    principal_id: String,
    binding_fingerprint: String,
}

struct ReceiptFixture {
    receipt_id: String,
    event_id: String,
    idempotency_digest: String,
    key_id: String,
    key_fingerprint: String,
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

fn fixture_token(label: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{label}_{timestamp:x}_{sequence:x}")
}

fn digest(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

fn bytes_digest(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}

fn numeric_id(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(bytes).max(1).to_string()
}

async fn create_database(label: &str, migrate: bool) -> TestDatabase {
    let database_name = format!("starring_receipt_{label}_test_{}", fixture_token("db"));
    assert_test_database_name(&database_name);
    let base_options = database_url().parse::<PgConnectOptions>().unwrap();
    let mut administrator = PgConnection::connect_with(&base_options.clone().database("postgres"))
        .await
        .expect("connect to PostgreSQL administrator database");
    sqlx::query(&format!("CREATE DATABASE {database_name}"))
        .execute(&mut administrator)
        .await
        .expect("create isolated receipt retention test database");
    let pool = match PgPoolOptions::new()
        .max_connections(8)
        .connect_with(base_options.database(&database_name))
        .await
    {
        Ok(pool) => pool,
        Err(error) => {
            sqlx::query(&format!("DROP DATABASE {database_name} WITH (FORCE)"))
                .execute(&mut administrator)
                .await
                .expect("drop failed isolated receipt retention test database");
            panic!("connect to isolated receipt retention test database: {error}");
        }
    };
    if migrate {
        MIGRATOR
            .run(&pool)
            .await
            .expect("run fresh receipt retention migrations");
    }
    TestDatabase {
        name: database_name,
        administrator,
        pool,
    }
}

async fn drop_database(database: TestDatabase) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .expect("drop isolated receipt retention test database");
}

async fn seed_authority(pool: &PgPool, label: &str) -> AuthorityFixture {
    let token = fixture_token(label);
    let tenant_id = format!("tenant:{token}");
    let installation_id = format!("installation:{token}");
    let principal_id = format!("principal:{token}");
    let resource_bindings = ResourceBindingMap::default();
    let binding_fingerprint = resource_binding_fingerprint_v2(&resource_bindings).into_string();
    let mut transaction = pool.begin().await.expect("begin authority fixture");
    sqlx::query(
        "INSERT INTO public.product_principals \
         (principal_id, discord_user_id, display_profile) \
         VALUES ($1, $2, '{}'::JSONB)",
    )
    .bind(&principal_id)
    .bind(numeric_id(&format!("user:{token}")))
    .execute(&mut *transaction)
    .await
    .expect("insert receipt retention principal");
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) \
         VALUES ($1, 'active', $2)",
    )
    .bind(&tenant_id)
    .bind(format!("Tenant {label}"))
    .execute(&mut *transaction)
    .await
    .expect("insert receipt retention tenant");
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, \
          ruleset_key, lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(numeric_id(&format!("application:{token}")))
    .bind(numeric_id(&format!("guild:{token}")))
    .bind(format!("ruleset_{}", &digest(&token)[..24]))
    .execute(&mut *transaction)
    .await
    .expect("insert receipt retention installation");
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, \
          activation_ttl_seconds, authority_payload_digest, created_by_principal_id, \
          created_by_request_digest) \
         VALUES ($1, 1, $2, 1, \
          pg_catalog.jsonb_build_object(\
           'role_bindings', '{}'::JSONB, 'channel_bindings', '{}'::JSONB), \
          $3, 1, 1, 3600, $4, $5, $6)",
    )
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(&binding_fingerprint)
    .bind(digest(&format!("authority:{token}")))
    .bind(&principal_id)
    .bind(digest(&format!("authority-request:{token}")))
    .execute(&mut *transaction)
    .await
    .expect("insert receipt retention authority");
    transaction
        .commit()
        .await
        .expect("commit receipt retention authority fixture");
    AuthorityFixture {
        token,
        tenant_id,
        installation_id,
        principal_id,
        binding_fingerprint,
    }
}

async fn seed_receipt(
    pool: &PgPool,
    authority: &AuthorityFixture,
    label: &str,
    completed_at: DateTime<Utc>,
    key_id: &str,
    key_fingerprint: &str,
) -> ReceiptFixture {
    let identity = format!("{}:{label}", authority.token);
    let receipt_id = digest(&format!("receipt:{identity}"));
    let event_id = digest(&format!("event:{identity}"));
    let target_id = digest(&format!("target:{identity}"));
    let idempotency_digest = digest(&format!("idempotency:{identity}"));
    let request_digest = digest(&format!("request:{identity}"));
    let mut transaction = pool.begin().await.expect("begin receipt fixture");
    sqlx::query(
        "INSERT INTO public.product_action_receipts \
         (receipt_id, tenant_id, installation_id, principal_id, endpoint_domain, \
          idempotency_key_digest, idempotency_digest_key_id, \
          idempotency_digest_key_fingerprint, request_digest, target_resource_type, \
          target_resource_id, resulting_revision, resulting_state, result_code, \
          http_disposition_class, completed_at) \
         VALUES ($1, $2, $3, $4, 'product_approve_v1', $5, $6, $7, $8, \
          'authoring_promotion', $9, 2, 'pending', 'approval_recorded', 2, $10)",
    )
    .bind(&receipt_id)
    .bind(&authority.tenant_id)
    .bind(&authority.installation_id)
    .bind(&authority.principal_id)
    .bind(&idempotency_digest)
    .bind(key_id)
    .bind(key_fingerprint)
    .bind(&request_digest)
    .bind(&target_id)
    .bind(completed_at)
    .execute(&mut *transaction)
    .await
    .expect("insert product action receipt fixture");
    sqlx::query(
        "INSERT INTO public.product_action_receipt_idempotency_aliases \
         (tenant_id, installation_id, principal_id, endpoint_domain, \
          idempotency_key_digest, idempotency_digest_key_id, \
          idempotency_digest_key_fingerprint, receipt_id, created_at) \
         VALUES ($1, $2, $3, 'product_approve_v1', $4, $5, $6, $7, $8)",
    )
    .bind(&authority.tenant_id)
    .bind(&authority.installation_id)
    .bind(&authority.principal_id)
    .bind(&idempotency_digest)
    .bind(key_id)
    .bind(key_fingerprint)
    .bind(&receipt_id)
    .bind(completed_at)
    .execute(&mut *transaction)
    .await
    .expect("insert product action receipt primary alias fixture");
    sqlx::query(
        "INSERT INTO public.product_audit_events \
         (event_id, tenant_id, installation_id, principal_id, session_subject_digest, \
          action, target_resource_type, target_resource_id, request_id, receipt_id, \
          authority_observation_digest, effective_permission_bits, authority_observed_at, \
          installation_authority_revision, payload_digest, binding_fingerprint, \
          policy_revision, resulting_state, result_code, dependency_latency_classes, \
          occurred_at) \
         VALUES ($1, $2, $3, $4, $5, 'promotion.approve', 'authoring_promotion', \
          $6, $7, $8, $9, 8::NUMERIC, $10, 1, $11, $12, 1, 'pending', \
          'approval_recorded', '{}'::JSONB, $13)",
    )
    .bind(&event_id)
    .bind(&authority.tenant_id)
    .bind(&authority.installation_id)
    .bind(&authority.principal_id)
    .bind(bytes_digest(&format!("session-subject:{identity}")))
    .bind(&target_id)
    .bind(format!("request:{identity}"))
    .bind(&receipt_id)
    .bind(digest(&format!("observation:{identity}")))
    .bind(completed_at - TimeDelta::seconds(1))
    .bind(digest(&format!("payload:{identity}")))
    .bind(&authority.binding_fingerprint)
    .bind(completed_at)
    .execute(&mut *transaction)
    .await
    .expect("insert product action audit fixture");
    transaction
        .commit()
        .await
        .expect("commit product action receipt fixture");
    ReceiptFixture {
        receipt_id,
        event_id,
        idempotency_digest,
        key_id: key_id.to_string(),
        key_fingerprint: key_fingerprint.to_string(),
    }
}

async fn insert_alias(
    connection: impl Executor<'_, Database = Postgres>,
    authority: &AuthorityFixture,
    receipt_id: &str,
    label: &str,
    key_id: &str,
    key_fingerprint: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO public.product_action_receipt_idempotency_aliases \
         (tenant_id, installation_id, principal_id, endpoint_domain, \
          idempotency_key_digest, idempotency_digest_key_id, \
          idempotency_digest_key_fingerprint, receipt_id) \
         VALUES ($1, $2, $3, 'product_approve_v1', $4, $5, $6, $7)",
    )
    .bind(&authority.tenant_id)
    .bind(&authority.installation_id)
    .bind(&authority.principal_id)
    .bind(digest(&format!("alias:{}:{label}", authority.token)))
    .bind(key_id)
    .bind(key_fingerprint)
    .bind(receipt_id)
    .execute(connection)
    .await?;
    Ok(())
}

async fn purge(pool: &PgPool, batch_limit: i32) -> Result<PurgeOutcome, sqlx::Error> {
    sqlx::query_as::<_, PurgeOutcome>(
        "SELECT deleted_receipts, deleted_aliases, backlog_remaining \
         FROM public.starring_purge_product_action_receipts_v1($1)",
    )
    .bind(batch_limit)
    .fetch_one(pool)
    .await
}

async fn coverage(pool: &PgPool, key_ids: &[String], fingerprints: &[String]) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT outcome FROM public.starring_product_approval_keyring_coverage_v1($1, $2)",
    )
    .bind(key_ids)
    .bind(fingerprints)
    .fetch_one(pool)
    .await
    .expect("read product approval keyring coverage")
}

async fn row_snapshot(pool: &PgPool, table: &str, field: &str, value: &str) -> String {
    assert!(matches!(
        table,
        "product_audit_events" | "product_action_receipt_audit_evidence"
    ));
    assert!(matches!(field, "event_id" | "receipt_id"));
    sqlx::query_scalar::<_, String>(&format!(
        "SELECT pg_catalog.to_jsonb(record)::TEXT FROM public.{table} AS record \
         WHERE record.{field} = $1"
    ))
    .bind(value)
    .fetch_one(pool)
    .await
    .expect("read immutable receipt evidence snapshot")
}

fn assert_database_error(error: sqlx::Error, state: &str, message: &str) {
    let sqlx::Error::Database(database) = error else {
        panic!("expected database error, received {error:?}");
    };
    assert_eq!(database.code().as_deref(), Some(state));
    assert_eq!(database.message(), message);
}

async fn receipt_exists(pool: &PgPool, receipt_id: &str) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM public.product_action_receipts \
         WHERE receipt_id = $1)",
    )
    .bind(receipt_id)
    .fetch_one(pool)
    .await
    .expect("check product action receipt existence")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn fresh_retention_preserves_audit_and_enforces_the_exact_replay_boundary() {
    let database = create_database("fresh", true).await;
    let outcome = async {
        let authority = seed_authority(&database.pool, "fresh").await;
        let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&database.pool)
            .await?;
        let old_fingerprint = digest("retention-old-key-material");
        let new_fingerprint = digest("retention-new-key-material");
        let expired = seed_receipt(
            &database.pool,
            &authority,
            "expired",
            now - TimeDelta::days(7),
            "old-key",
            &old_fingerprint,
        )
        .await;
        let retained = seed_receipt(
            &database.pool,
            &authority,
            "retained",
            now - TimeDelta::days(7) + TimeDelta::minutes(1),
            "new-key",
            &new_fingerprint,
        )
        .await;
        assert_eq!(
            coverage(
                &database.pool,
                &["new-key".to_string()],
                std::slice::from_ref(&new_fingerprint),
            )
            .await,
            "idempotency_keyring_incomplete"
        );
        let audit_before = row_snapshot(
            &database.pool,
            "product_audit_events",
            "event_id",
            &expired.event_id,
        )
        .await;
        let evidence_before = row_snapshot(
            &database.pool,
            "product_action_receipt_audit_evidence",
            "receipt_id",
            &expired.receipt_id,
        )
        .await;
        let direct_delete =
            sqlx::query("DELETE FROM public.product_action_receipts WHERE receipt_id = $1")
                .bind(&expired.receipt_id)
                .execute(&database.pool)
                .await
                .expect_err("direct product action receipt deletion must fail");
        assert_database_error(
            direct_delete,
            "23514",
            "immutable product records cannot be updated or deleted",
        );
        let alias_update = sqlx::query(
            "UPDATE public.product_action_receipt_idempotency_aliases \
             SET created_at = created_at WHERE receipt_id = $1",
        )
        .bind(&expired.receipt_id)
        .execute(&database.pool)
        .await
        .expect_err("direct product action receipt alias update must fail");
        assert_database_error(
            alias_update,
            "23514",
            "immutable product records cannot be updated or deleted",
        );
        let evidence_update = sqlx::query(
            "UPDATE public.product_action_receipt_audit_evidence \
             SET result_code = result_code WHERE receipt_id = $1",
        )
        .bind(&expired.receipt_id)
        .execute(&database.pool)
        .await
        .expect_err("product action receipt evidence update must fail");
        assert_database_error(
            evidence_update,
            "23514",
            "immutable product records cannot be updated or deleted",
        );
        let audit_delete =
            sqlx::query("DELETE FROM public.product_audit_events WHERE event_id = $1")
                .bind(&expired.event_id)
                .execute(&database.pool)
                .await
                .expect_err("direct product audit deletion must fail");
        assert_database_error(
            audit_delete,
            "23514",
            "immutable product records cannot be updated or deleted",
        );
        let mut forged_gate = database.pool.begin().await?;
        sqlx::query(
            "SELECT pg_catalog.set_config(\
             'starring.product_action_receipt_retention_gate', \
             'starring.product.action.receipt.retention.v1', TRUE)",
        )
        .execute(&mut *forged_gate)
        .await?;
        let forged_delete =
            sqlx::query("DELETE FROM public.product_action_receipts WHERE receipt_id = $1")
                .bind(&retained.receipt_id)
                .execute(&mut *forged_gate)
                .await
                .expect_err("forged retention gate must not delete a live receipt");
        assert_database_error(
            forged_delete,
            "23514",
            "product action receipt is not retention eligible",
        );
        forged_gate.rollback().await?;
        for invalid in [0, 1001] {
            let error = purge(&database.pool, invalid)
                .await
                .expect_err("invalid receipt purge batch must fail");
            assert_database_error(
                error,
                "22023",
                "product action receipt purge batch limit is invalid",
            );
        }
        let null_error = sqlx::query_as::<_, PurgeOutcome>(
            "SELECT deleted_receipts, deleted_aliases, backlog_remaining \
             FROM public.starring_purge_product_action_receipts_v1(NULL::INTEGER)",
        )
        .fetch_one(&database.pool)
        .await
        .expect_err("null receipt purge batch must fail");
        assert_database_error(
            null_error,
            "22023",
            "product action receipt purge batch limit is invalid",
        );
        let purged = purge(&database.pool, 1).await?;
        assert_eq!(
            purged,
            PurgeOutcome {
                deleted_receipts: 1,
                deleted_aliases: 1,
                backlog_remaining: false,
            }
        );
        assert!(!receipt_exists(&database.pool, &expired.receipt_id).await);
        assert!(receipt_exists(&database.pool, &retained.receipt_id).await);
        assert_eq!(
            audit_before,
            row_snapshot(
                &database.pool,
                "product_audit_events",
                "event_id",
                &expired.event_id,
            )
            .await
        );
        assert_eq!(
            evidence_before,
            row_snapshot(
                &database.pool,
                "product_action_receipt_audit_evidence",
                "receipt_id",
                &expired.receipt_id,
            )
            .await
        );
        assert_eq!(
            coverage(
                &database.pool,
                &["new-key".to_string()],
                std::slice::from_ref(&new_fingerprint),
            )
            .await,
            "ok"
        );
        let schema_contract = sqlx::query_as::<_, (bool, bool, bool, bool, bool)>(
            "SELECT \
             pg_catalog.to_regclass('public.product_action_receipts_approval_retention_index') \
                 IS NOT NULL, \
             pg_catalog.to_regclass(\
              'public.product_action_aliases_receipt_retention_index') \
                 IS NOT NULL, \
             EXISTS (SELECT 1 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'product_audit_events_receipt_evidence_fk'), \
             EXISTS (SELECT 1 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'product_audit_events_principal_fk' AND convalidated), \
             NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'product_audit_events_receipt_fk')",
        )
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(schema_contract, (true, true, true, true, true));
        let mut plan_transaction = database.pool.begin().await?;
        sqlx::query("SET LOCAL enable_seqscan = off")
            .execute(&mut *plan_transaction)
            .await?;
        let alias_delete_plan = sqlx::query_scalar::<_, serde_json::Value>(
            "EXPLAIN (FORMAT JSON) \
             DELETE FROM public.product_action_receipt_idempotency_aliases AS alias \
             WHERE alias.endpoint_domain = 'product_approve_v1' \
              AND alias.receipt_id = ANY($1)",
        )
        .bind(vec![retained.receipt_id.clone()])
        .fetch_one(&mut *plan_transaction)
        .await?;
        assert!(alias_delete_plan
            .to_string()
            .contains("product_action_aliases_receipt_retention_index"));
        plan_transaction.rollback().await?;
        let forged_principal = sqlx::query(
            "INSERT INTO public.product_audit_events \
             (event_id, tenant_id, installation_id, principal_id, session_subject_digest, \
              action, target_resource_type, target_resource_id, request_id, receipt_id, \
              authority_observation_digest, effective_permission_bits, authority_observed_at, \
              installation_authority_revision, resulting_state, result_code, \
              dependency_latency_classes, occurred_at) \
             SELECT $1, tenant_id, installation_id, $2, session_subject_digest, action, \
              target_resource_type, target_resource_id, $3, $4, authority_observation_digest, \
              effective_permission_bits, authority_observed_at, \
              installation_authority_revision, resulting_state, result_code, \
              dependency_latency_classes, occurred_at \
             FROM public.product_audit_events WHERE event_id = $5",
        )
        .bind(digest("forged-principal-event"))
        .bind("principal:does-not-exist")
        .bind("forged-principal-request")
        .bind(digest("forged-principal-receipt"))
        .bind(&expired.event_id)
        .execute(&database.pool)
        .await;
        assert!(forged_principal.is_err());
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn key_rotation_requires_coverage_and_alias_fanout_remains_bounded() {
    let database = create_database("rotation", true).await;
    let outcome = async {
        let authority = seed_authority(&database.pool, "rotation").await;
        let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&database.pool)
            .await?;
        let old_fingerprint = digest("rotation-old-key-material");
        let new_fingerprint = digest("rotation-new-key-material");
        let receipt = seed_receipt(
            &database.pool,
            &authority,
            "live",
            now,
            "rotation-old",
            &old_fingerprint,
        )
        .await;
        assert_eq!(
            coverage(
                &database.pool,
                &["rotation-new".to_string()],
                std::slice::from_ref(&new_fingerprint),
            )
            .await,
            "idempotency_keyring_incomplete"
        );
        assert_eq!(coverage(&database.pool, &[], &[]).await, "invalid_input");
        assert_eq!(
            coverage(
                &database.pool,
                &["rotation-new".to_string(), "rotation-new".to_string()],
                &[new_fingerprint.clone(), digest("rotation-another-material")],
            )
            .await,
            "invalid_input"
        );
        insert_alias(
            &database.pool,
            &authority,
            &receipt.receipt_id,
            "rotation-new",
            "rotation-new",
            &new_fingerprint,
        )
        .await?;
        assert_eq!(
            coverage(
                &database.pool,
                &["rotation-new".to_string()],
                std::slice::from_ref(&new_fingerprint),
            )
            .await,
            "ok"
        );
        assert_eq!(
            coverage(
                &database.pool,
                &["rotation-old".to_string()],
                &[digest("rotation-old-key-material-reused")],
            )
            .await,
            "idempotency_keyring_incomplete"
        );
        for index in 0..30 {
            insert_alias(
                &database.pool,
                &authority,
                &receipt.receipt_id,
                &format!("capacity-{index}"),
                &format!("capacity-{index}"),
                &digest(&format!("capacity-key-material-{index}")),
            )
            .await?;
        }
        let alias_count = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) \
             FROM public.product_action_receipt_idempotency_aliases \
             WHERE receipt_id = $1",
        )
        .bind(&receipt.receipt_id)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(alias_count, 32);
        let capacity_error = insert_alias(
            &database.pool,
            &authority,
            &receipt.receipt_id,
            "capacity-overflow",
            "capacity-overflow",
            &digest("capacity-overflow-material"),
        )
        .await
        .expect_err("thirty-third receipt alias must fail");
        let sqlx::Error::Database(capacity_database_error) = capacity_error else {
            panic!("expected alias capacity database error");
        };
        assert_eq!(
            capacity_database_error.constraint(),
            Some("product_action_receipt_alias_capacity_valid")
        );
        let duplicate = sqlx::query(
            "INSERT INTO public.product_action_receipt_idempotency_aliases \
             (tenant_id, installation_id, principal_id, endpoint_domain, \
              idempotency_key_digest, idempotency_digest_key_id, \
              idempotency_digest_key_fingerprint, receipt_id) \
             VALUES ($1, $2, $3, 'product_approve_v1', $4, $5, $6, $7) \
             ON CONFLICT (tenant_id, installation_id, principal_id, endpoint_domain, \
              idempotency_key_digest) DO NOTHING",
        )
        .bind(&authority.tenant_id)
        .bind(&authority.installation_id)
        .bind(&authority.principal_id)
        .bind(&receipt.idempotency_digest)
        .bind(&receipt.key_id)
        .bind(&receipt.key_fingerprint)
        .bind(&receipt.receipt_id)
        .execute(&database.pool)
        .await?;
        assert_eq!(duplicate.rows_affected(), 0);
        assert_eq!(
            purge(&database.pool, 1000).await?,
            PurgeOutcome {
                deleted_receipts: 0,
                deleted_aliases: 0,
                backlog_remaining: false,
            }
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_purge_skips_locked_receipts_without_duplicate_work() {
    let database = create_database("concurrency", true).await;
    let outcome = async {
        let authority = seed_authority(&database.pool, "concurrency").await;
        let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&database.pool)
            .await?;
        let fingerprint = digest("concurrency-key-material");
        let locked = seed_receipt(
            &database.pool,
            &authority,
            "locked",
            now - TimeDelta::days(9),
            "concurrency-key",
            &fingerprint,
        )
        .await;
        let available = seed_receipt(
            &database.pool,
            &authority,
            "available",
            now - TimeDelta::days(8),
            "concurrency-key",
            &fingerprint,
        )
        .await;
        let mut blocker = database.pool.begin().await?;
        sqlx::query("SELECT receipt_id FROM public.product_action_receipts WHERE receipt_id = $1 FOR UPDATE")
            .bind(&locked.receipt_id)
            .fetch_one(&mut *blocker)
            .await?;
        let mut worker = database.pool.acquire().await?;
        sqlx::query("SET statement_timeout = '1s'")
            .execute(&mut *worker)
            .await?;
        let skipped = sqlx::query_as::<_, PurgeOutcome>(
            "SELECT deleted_receipts, deleted_aliases, backlog_remaining \
             FROM public.starring_purge_product_action_receipts_v1(1)",
        )
        .fetch_one(&mut *worker)
        .await?;
        assert_eq!(skipped.deleted_receipts, 1);
        assert_eq!(skipped.deleted_aliases, 1);
        assert!(skipped.backlog_remaining);
        assert!(receipt_exists(&database.pool, &locked.receipt_id).await);
        assert!(!receipt_exists(&database.pool, &available.receipt_id).await);
        blocker.rollback().await?;
        let remaining = purge(&database.pool, 1).await?;
        assert_eq!(remaining.deleted_receipts, 1);
        assert_eq!(remaining.deleted_aliases, 1);
        assert!(!remaining.backlog_remaining);
        let retained_audit = sqlx::query_scalar::<_, i64>(
            "SELECT pg_catalog.count(*) FROM public.product_audit_events \
             WHERE receipt_id = ANY($1)",
        )
        .bind(vec![locked.receipt_id, available.receipt_id])
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(retained_audit, 2);
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn upgrade_backfills_receipt_evidence_without_rewriting_audit_history() {
    let database = create_database("upgrade", false).await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_006)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        let authority = seed_authority(&database.pool, "upgrade").await;
        let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&database.pool)
            .await?;
        let receipt = seed_receipt(
            &database.pool,
            &authority,
            "legacy",
            now - TimeDelta::days(8),
            "upgrade-key",
            &digest("upgrade-key-material"),
        )
        .await;
        let audit_before = row_snapshot(
            &database.pool,
            "product_audit_events",
            "event_id",
            &receipt.event_id,
        )
        .await;
        let evidence_before = sqlx::query_scalar::<_, bool>(
            "SELECT pg_catalog.to_regclass(\
             'public.product_action_receipt_audit_evidence') IS NOT NULL",
        )
        .fetch_one(&database.pool)
        .await?;
        assert!(!evidence_before);
        let retention_migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_007)
            .expect("receipt retention migration must exist");
        sqlx::raw_sql(retention_migration.sql.as_ref())
            .execute(&database.pool)
            .await?;
        assert_eq!(
            audit_before,
            row_snapshot(
                &database.pool,
                "product_audit_events",
                "event_id",
                &receipt.event_id,
            )
            .await
        );
        let evidence = row_snapshot(
            &database.pool,
            "product_action_receipt_audit_evidence",
            "receipt_id",
            &receipt.receipt_id,
        )
        .await;
        assert!(evidence.contains("product_approve_v1"));
        assert!(evidence.contains("replay_guaranteed_until"));
        let approval_signature = sqlx::query_as::<_, (i16, bool)>(
            "SELECT procedure.pronargs, procedure.prosecdef \
             FROM pg_catalog.pg_proc AS procedure \
             WHERE procedure.oid = pg_catalog.to_regprocedure(\
              'public.starring_product_approve_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text)')",
        )
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(approval_signature, (28, true));
        let purged = purge(&database.pool, 1).await?;
        assert_eq!(purged.deleted_receipts, 1);
        assert_eq!(purged.deleted_aliases, 1);
        assert_eq!(
            audit_before,
            row_snapshot(
                &database.pool,
                "product_audit_events",
                "event_id",
                &receipt.event_id,
            )
            .await
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn action_retention_adapter_commits_bounded_work_and_times_out_on_database_locks() {
    let database = create_database("adapter", true).await;
    let authority = seed_authority(&database.pool, "adapter").await;
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    seed_receipt(
        &database.pool,
        &authority,
        "committed",
        now - TimeDelta::days(8),
        "adapter-key",
        &digest("adapter-key-material"),
    )
    .await;
    let retention = PostgresProductActionRetention::with_config(
        database.pool.clone(),
        PostgresProductActionRetentionConfig::new(
            Duration::from_secs(1),
            Duration::from_millis(50),
        )
        .unwrap(),
    );
    assert_eq!(
        retention.purge(0).await.unwrap_err(),
        ProductActionRetentionError::InvalidBatchLimit
    );
    let report = retention.purge(1).await.unwrap();
    assert_eq!(report.deleted_receipts(), 1);
    assert_eq!(report.deleted_aliases(), 1);
    assert!(!report.backlog_remaining());

    seed_receipt(
        &database.pool,
        &authority,
        "locked",
        now - TimeDelta::days(8),
        "adapter-key",
        &digest("adapter-key-material"),
    )
    .await;
    let mut locker = database.pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE public.product_action_receipts IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *locker)
        .await
        .unwrap();
    let bounded = tokio::time::timeout(Duration::from_secs(2), retention.purge(1))
        .await
        .expect("product action retention exceeded its outer test deadline")
        .unwrap_err();
    assert_eq!(
        bounded,
        ProductActionRetentionError::Database(ProductDatabaseFailureV1::Timeout)
    );
    locker.rollback().await.unwrap();
    let final_report = retention.purge(1).await.unwrap();
    assert_eq!(final_report.deleted_receipts(), 1);
    assert_eq!(final_report.deleted_aliases(), 1);
    drop(retention);
    drop_database(database).await;
}
