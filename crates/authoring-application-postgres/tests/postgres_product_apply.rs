use std::num::NonZeroU64;
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application_postgres::MIGRATOR;
use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, DeploymentId, InstallationId, PromotionId,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeGeneration,
    RuntimeProcessIdentityV1, TenantId,
};
use automation_runtime_convergence_postgres::{
    prepare_requested_deployment_v1, EnqueueDeploymentV1, PostgresRuntimeConvergence,
    PreparedRequestedDeploymentV1, RuntimeDeploymentScopeV1,
};
use chrono::{DateTime, TimeDelta, Utc};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, RoleId};
use resource_resolution::{
    approval_binding_fingerprint_v1, resource_binding_fingerprint_v2, ResolvedApprovalBinding,
    ResourceBindingFingerprint, ResourceBindingMap,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::types::Json;
use sqlx::{Connection, Postgres, Transaction};

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

async fn pool() -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&database_url())
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

struct TestDatabase {
    name: String,
    administrator: PgConnection,
    pool: PgPool,
}

async fn isolated_database(label: &str) -> TestDatabase {
    let name = format!("starring_apply_{label}_test_{}", suffix());
    assert!(
        name.starts_with("starring_")
            && name.split('_').any(|segment| segment == "test")
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
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

async fn drop_isolated_database(database: TestDatabase) {
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
}

fn suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string()
}

fn digest(seed: &str) -> String {
    let bytes = Sha256::digest(seed.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

#[derive(Clone)]
struct Actor {
    principal_id: String,
    user_id: String,
    session_digest: Vec<u8>,
    session_subject: Vec<u8>,
    csrf_digest: Vec<u8>,
    oauth_state: Vec<u8>,
    oauth_nonce: Vec<u8>,
}

impl Actor {
    fn new(suffix: &str, label: &str, user_id: String) -> Self {
        Self {
            principal_id: format!("{label}-{suffix}"),
            user_id,
            session_digest: Sha256::digest(format!("session:{label}:{suffix}")).to_vec(),
            session_subject: Sha256::digest(format!("subject:{label}:{suffix}")).to_vec(),
            csrf_digest: Sha256::digest(format!("csrf:{label}:{suffix}")).to_vec(),
            oauth_state: Sha256::digest(format!("oauth:{label}:{suffix}")).to_vec(),
            oauth_nonce: Sha256::digest(format!("nonce:{label}:{suffix}")).to_vec(),
        }
    }
}

#[derive(Clone)]
struct Fixture {
    tenant_id: String,
    installation_id: String,
    promotion_id: String,
    activation_id: String,
    actor: Actor,
    application_id: String,
    guild_id: String,
    ruleset_key: String,
    payload_digest: String,
    authority_digest: String,
    observation_digest: String,
}

#[derive(Clone)]
struct Operation {
    request_id: String,
    idempotency_digest: String,
    key_id: String,
    key_fingerprint: String,
    semantic_digest: String,
    receipt_id: String,
    audit_event_id: String,
    apply_attempt_id: String,
    deployment_id: String,
}

impl Operation {
    fn new(label: &str) -> Self {
        let suffix = suffix();
        Self {
            request_id: format!("apply-request-{label}-{suffix}"),
            idempotency_digest: digest(&format!("idempotency:{label}:{suffix}")),
            key_id: "apply-key-v1".to_string(),
            key_fingerprint: digest("apply-key-v1-material"),
            semantic_digest: digest(&format!("semantic:{label}:{suffix}")),
            receipt_id: digest(&format!("receipt:{label}:{suffix}")),
            audit_event_id: digest(&format!("audit:{label}:{suffix}")),
            apply_attempt_id: format!("apply_attempt_{suffix}"),
            deployment_id: format!("apply-deployment-{suffix}"),
        }
    }
}

#[derive(Clone)]
struct ApplyLockContext {
    expected_payload_digest: String,
    expected_authority_revision: i64,
    expected_authority_digest: String,
    active_idempotency_digest: String,
    idempotency_candidates: Vec<String>,
    candidate_key_ids: Vec<String>,
    candidate_key_fingerprints: Vec<String>,
    active_key_id: String,
}

impl ApplyLockContext {
    fn single(fixture: &Fixture, operation: &Operation) -> Self {
        Self {
            expected_payload_digest: fixture.payload_digest.clone(),
            expected_authority_revision: 1,
            expected_authority_digest: fixture.authority_digest.clone(),
            active_idempotency_digest: operation.idempotency_digest.clone(),
            idempotency_candidates: vec![operation.idempotency_digest.clone()],
            candidate_key_ids: vec![operation.key_id.clone()],
            candidate_key_fingerprints: vec![operation.key_fingerprint.clone()],
            active_key_id: operation.key_id.clone(),
        }
    }
}

#[derive(Clone)]
struct Call {
    expected_revision: i64,
    capability: String,
    session_digest: Vec<u8>,
    observed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    effective_permissions: String,
    guild_owner: bool,
}

impl Call {
    fn valid(fixture: &Fixture) -> Self {
        let observed_at = Utc::now() - TimeDelta::milliseconds(50);
        Self {
            expected_revision: 2,
            capability: "apply".to_string(),
            session_digest: fixture.actor.session_digest.clone(),
            observed_at,
            expires_at: observed_at + TimeDelta::seconds(5),
            effective_permissions: "32".to_string(),
            guild_owner: false,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct LockRow {
    outcome: String,
    exact_replay: bool,
    requires_commit: bool,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    deployment_id: Option<String>,
    desired_target_digest: Option<String>,
    locked_projection: Option<Json<Value>>,
}

#[derive(Debug, sqlx::FromRow)]
struct FinalizeRow {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
    guild_id: Option<String>,
    deployment_id: Option<String>,
    desired_target_digest: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct TerminalPersistenceRow {
    state: String,
    product_revision: i64,
    apply_attempt_no: i64,
    apply_attempt_id: Option<String>,
    apply_lease_until: Option<DateTime<Utc>>,
    termination: Option<Json<Value>>,
    receipt_resulting_revision: Option<i64>,
    receipt_resulting_state: Option<String>,
    receipt_result_code: Option<String>,
    receipt_http_disposition_class: Option<i16>,
    receipt_completed_at: Option<DateTime<Utc>>,
    audit_authority_revision: Option<i64>,
    audit_binding_fingerprint: Option<String>,
    audit_policy_revision: Option<i64>,
    audit_baseline_version: Option<i64>,
    audit_baseline_hash: Option<String>,
    audit_occurred_at: Option<DateTime<Utc>>,
    runtime_count: i64,
    alias_count: i64,
    audit_count: i64,
    evidence_count: i64,
}

async fn begin_serializable(pool: &PgPool) -> Transaction<'_, Postgres> {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SET LOCAL statement_timeout = '10s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction
}

async fn terminal_persistence(
    pool: &PgPool,
    fixture: &Fixture,
    operation: &Operation,
) -> TerminalPersistenceRow {
    sqlx::query_as::<_, TerminalPersistenceRow>(
        "SELECT activation.state, activation.product_revision, activation.apply_attempt_no, \
          activation.apply_attempt_id, activation.apply_lease_until, activation.termination, \
          receipt.resulting_revision AS receipt_resulting_revision, \
          receipt.resulting_state AS receipt_resulting_state, \
          receipt.result_code AS receipt_result_code, \
          receipt.http_disposition_class AS receipt_http_disposition_class, \
          receipt.completed_at AS receipt_completed_at, \
          audit.installation_authority_revision AS audit_authority_revision, \
          audit.binding_fingerprint AS audit_binding_fingerprint, \
          audit.policy_revision AS audit_policy_revision, \
          audit.active_baseline_version AS audit_baseline_version, \
          audit.active_baseline_hash AS audit_baseline_hash, \
          audit.occurred_at AS audit_occurred_at, \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
           WHERE deployment.activation_request_id = activation.id) AS runtime_count, \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_idempotency_aliases AS alias \
           WHERE alias.receipt_id = $2) AS alias_count, \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events AS event \
           WHERE event.receipt_id = $2) AS audit_count, \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_audit_evidence AS evidence \
           WHERE evidence.receipt_id = $2) AS evidence_count \
         FROM public.activation_requests AS activation \
         LEFT JOIN public.product_action_receipts AS receipt ON receipt.receipt_id = $2 \
         LEFT JOIN public.product_audit_events AS audit ON audit.receipt_id = $2 \
         WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn assert_terminal_persistence(row: &TerminalPersistenceRow, result_code: &str) {
    assert_eq!(row.state, "superseded");
    assert_eq!(row.product_revision, 4);
    assert_eq!(row.apply_attempt_no, 1);
    assert!(row.apply_attempt_id.is_none());
    assert!(row.apply_lease_until.is_none());
    assert_eq!(row.receipt_resulting_revision, Some(4));
    assert_eq!(row.receipt_resulting_state.as_deref(), Some("superseded"));
    assert_eq!(row.receipt_result_code.as_deref(), Some(result_code));
    assert_eq!(row.receipt_http_disposition_class, Some(4));
    assert_eq!(row.receipt_completed_at, row.audit_occurred_at);
    let termination_at =
        DateTime::parse_from_rfc3339(row.termination.as_ref().unwrap().0["at"].as_str().unwrap())
            .unwrap()
            .with_timezone(&Utc);
    assert_eq!(Some(termination_at), row.receipt_completed_at);
    assert_eq!(row.runtime_count, 0);
    assert_eq!(row.alias_count, 1);
    assert_eq!(row.audit_count, 1);
    assert_eq!(row.evidence_count, 1);
}

async fn seed_fixture(pool: &PgPool) -> Fixture {
    let suffix = suffix();
    let tail = suffix[suffix.len().saturating_sub(9)..]
        .parse::<u64>()
        .unwrap();
    let tenant_id = format!("apply-tenant-{suffix}");
    let installation_id = format!("apply-installation-{suffix}");
    let promotion_id = digest(&format!("apply-promotion:{suffix}"));
    let activation_id = format!("apply_activation_{suffix}");
    let requester = Actor::new(
        &suffix,
        "apply-requester",
        (1_000_000_000 + tail).to_string(),
    );
    let actor = Actor::new(&suffix, "apply-actor", (2_000_000_000 + tail).to_string());
    let application_id = (3_000_000_000 + tail).to_string();
    let guild_id = (4_000_000_000 + tail).to_string();
    let ruleset_key = format!(
        "apply_ruleset_{}",
        &suffix[suffix.len().saturating_sub(20)..]
    );
    let promotion_request_digest = digest(&format!("apply-promotion-request:{suffix}"));
    let payload_digest = digest(&format!("apply-payload:{suffix}"));
    let target_content_hash = digest(&format!("apply-target:{suffix}"));
    let context_digest = digest(&format!("apply-context:{suffix}"));
    let guild = GuildId(guild_id.parse().unwrap());
    let required_channel_key = ResourceKey("community_hub".to_string());
    let auxiliary_role_key = ResourceKey("automation_operator".to_string());
    let required_channel_id = ChannelId(5_000_000_000 + tail);
    let auxiliary_role_id = RoleId(6_000_000_000 + tail);
    let mut resource_bindings = ResourceBindingMap::default();
    resource_bindings
        .channel_bindings
        .insert(required_channel_key.clone(), required_channel_id);
    resource_bindings
        .role_bindings
        .insert(auxiliary_role_key, auxiliary_role_id);
    let authority_binding_fingerprint = resource_binding_fingerprint_v2(&resource_bindings);
    let required_bindings = vec![ResolvedApprovalBinding::Channel {
        key: required_channel_key,
        id: required_channel_id,
    }];
    let approval_binding_fingerprint =
        approval_binding_fingerprint_v1(guild, NonZeroU64::new(1).unwrap(), &required_bindings)
            .unwrap();
    assert_ne!(
        authority_binding_fingerprint.as_str(),
        approval_binding_fingerprint.as_str()
    );
    let stored_resource_bindings = json!({
        "role_bindings": &resource_bindings.role_bindings,
        "channel_bindings": &resource_bindings.channel_bindings
    });
    let policy_digest = digest(&format!("apply-policy:{suffix}"));
    let authority_digest = digest(&format!("apply-authority:{suffix}"));
    let observation_digest = digest(&format!("apply-observation:{suffix}"));
    let database_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(pool)
            .await
            .unwrap();
    let created_at = database_now - TimeDelta::minutes(1);
    let expires_at = database_now + TimeDelta::minutes(59);
    let linked_at = database_now - TimeDelta::seconds(30);
    let context = json!({
        "promotion_id": promotion_id,
        "promotion_request_digest": promotion_request_digest,
        "approval_payload_digest": payload_digest,
        "approval_context_digest": context_digest,
        "binding": {
            "revision": 1,
            "fingerprint": approval_binding_fingerprint,
            "required_bindings": required_bindings
        },
        "baseline": {"state": "absent"},
        "policy": {
            "revision": 1,
            "required_approvals": 1,
            "ttl_seconds": 3600,
            "digest": policy_digest
        }
    });
    let approval_context = json!({
        "authority": "product_authoring",
        "context": context
    });
    let promotion_record = json!({
        "id": promotion_id,
        "revision": 3,
        "request_digest": promotion_request_digest,
        "intent": {
            "authority": {
                "tenant_id": tenant_id,
                "principal_id": requester.principal_id,
                "installation_id": installation_id,
                "guild_id": guild_id,
                "ruleset_key": ruleset_key,
                "binding_revision": 1
            },
            "evidence": {
                "context_fingerprint": authority_binding_fingerprint
            }
        },
        "stage": {
            "state": "activation_pending",
            "activation": {
                "request_id": activation_id,
                "target": {
                    "guild_id": guild_id,
                    "ruleset_key": ruleset_key,
                    "version": 1,
                    "content_hash": target_content_hash
                },
                "requester": requester.user_id,
                "required_approvals": 1,
                "created_at": created_at,
                "expires_at": expires_at,
                "request_state_at_journal": "pending",
                "approval_context": context
            }
        }
    });
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals \
         (principal_id, discord_user_id, display_profile) \
         VALUES ($1, $2, '{}'::JSONB), ($3, $4, '{}'::JSONB)",
    )
    .bind(&requester.principal_id)
    .bind(&requester.user_id)
    .bind(&actor.principal_id)
    .bind(&actor.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    for identity in [&requester, &actor] {
        sqlx::query(
            "INSERT INTO public.product_oauth_flows \
             (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, \
              expires_at, consumed_at, terminal_result_code) \
             VALUES ($1, $2, 'https://starring.example/oauth/discord/callback', '/', \
              CURRENT_TIMESTAMP - INTERVAL '1 minute', CURRENT_TIMESTAMP + INTERVAL '5 minutes', \
              CURRENT_TIMESTAMP - INTERVAL '1 second', 'callback_claimed')",
        )
        .bind(&identity.oauth_state)
        .bind(&identity.oauth_nonce)
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO public.product_auth_sessions \
             (session_digest, principal_id, csrf_digest, oauth_state_digest, authenticated_at, \
              created_at, last_seen_at, idle_expires_at, absolute_expires_at) \
             SELECT $1, $2, $3, $4, captured_at, captured_at, captured_at, \
              captured_at + INTERVAL '20 minutes', captured_at + INTERVAL '1 hour' \
             FROM (SELECT pg_catalog.clock_timestamp() AS captured_at) AS clock",
        )
        .bind(&identity.session_digest)
        .bind(&identity.principal_id)
        .bind(&identity.csrf_digest)
        .bind(&identity.oauth_state)
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) VALUES ($1, 'active', $2)",
    )
    .bind(&tenant_id)
    .bind(format!("Apply Tenant {suffix}"))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, ruleset_key, \
          lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(&application_id)
    .bind(&guild_id)
    .bind(&ruleset_key)
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
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(Json(&stored_resource_bindings))
    .bind(authority_binding_fingerprint.as_str())
    .bind(&authority_digest)
    .bind(&requester.principal_id)
    .bind(digest(&format!("apply-authority-request:{suffix}")))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_heads \
         (guild_id, ruleset_key, next_version) VALUES ($1, $2, 2)",
    )
    .bind(&guild_id)
    .bind(&ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 1, 1, '{}'::JSONB, $3, $4)",
    )
    .bind(&guild_id)
    .bind(&ruleset_key)
    .bind(&target_content_hash)
    .bind(&requester.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, principal_id, \
          record) VALUES ($1, 1, 3, 'activation_pending', $2, $3, $4, $5)",
    )
    .bind(&promotion_id)
    .bind(&promotion_request_digest)
    .bind(&tenant_id)
    .bind(&requester.principal_id)
    .bind(Json(&promotion_record))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.activation_requests \
         (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, state, created_at, expires_at, authority_kind, link_state_name, \
          approval_context, link_state, promotion_id, promotion_request_digest, \
          approval_payload_digest, approval_context_digest, linked_at) \
         VALUES ($1, $2, $3, 1, $4, $5, 1, 'pending', $6, $7, \
          'product_authoring', 'linked', $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(&activation_id)
    .bind(&guild_id)
    .bind(&ruleset_key)
    .bind(&target_content_hash)
    .bind(&requester.user_id)
    .bind(created_at)
    .bind(expires_at)
    .bind(Json(&approval_context))
    .bind(Json(json!({"state": "linked", "linked_at": linked_at})))
    .bind(&promotion_id)
    .bind(&promotion_request_digest)
    .bind(&payload_digest)
    .bind(&context_digest)
    .bind(linked_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SELECT pg_catalog.set_config('starring.product_approval_gate', $1, TRUE)")
        .bind(&context_digest)
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.activation_request_approvals \
         (request_id, tenant_id, installation_id, approver_id, approved_at, \
          approval_payload_digest) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&activation_id)
    .bind(&tenant_id)
    .bind(&installation_id)
    .bind(&actor.user_id)
    .bind(linked_at + TimeDelta::seconds(1))
    .bind(&payload_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests SET state = 'approved', product_revision = 2 \
         WHERE id = $1 AND state = 'pending' AND product_revision = 1",
    )
    .bind(&activation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    Fixture {
        tenant_id,
        installation_id,
        promotion_id,
        activation_id,
        actor,
        application_id,
        guild_id,
        ruleset_key,
        payload_digest,
        authority_digest,
        observation_digest,
    }
}

#[derive(Clone)]
struct AuthorityHead {
    revision: i64,
    digest: String,
}

struct AuthorityAdvance<'a> {
    binding_revision: i64,
    resource_bindings: &'a Value,
    binding_fingerprint: &'a str,
    policy_revision: i64,
    required_approvals: i32,
    activation_ttl_seconds: i64,
}

async fn authority_binding_material(pool: &PgPool, fixture: &Fixture) -> (Value, String) {
    let (bindings, fingerprint) = sqlx::query_as::<_, (Json<Value>, String)>(
        "SELECT resource_bindings, binding_fingerprint \
         FROM public.automation_installation_authority_versions \
         WHERE tenant_id = $1 AND installation_id = $2 AND revision = 1",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .fetch_one(pool)
    .await
    .unwrap();
    (bindings.0, fingerprint)
}

async fn advance_authority(
    pool: &PgPool,
    fixture: &Fixture,
    advance: AuthorityAdvance<'_>,
) -> AuthorityHead {
    let AuthorityAdvance {
        binding_revision,
        resource_bindings,
        binding_fingerprint,
        policy_revision,
        required_approvals,
        activation_ttl_seconds,
    } = advance;
    let authority_digest = digest(&format!(
        "apply-authority-head:{}:{binding_revision}:{policy_revision}:{required_approvals}:{activation_ttl_seconds}",
        fixture.installation_id
    ));
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
         VALUES ($1, 2, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(&fixture.installation_id)
    .bind(&fixture.tenant_id)
    .bind(binding_revision)
    .bind(Json(resource_bindings))
    .bind(binding_fingerprint)
    .bind(policy_revision)
    .bind(required_approvals)
    .bind(activation_ttl_seconds)
    .bind(&authority_digest)
    .bind(&fixture.actor.principal_id)
    .bind(digest(&format!(
        "apply-authority-head-request:{}",
        fixture.installation_id
    )))
    .execute(&mut *transaction)
    .await
    .unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_installations \
         SET current_authority_revision = 2, updated_at = pg_catalog.clock_timestamp() \
         WHERE tenant_id = $1 AND installation_id = $2 AND current_authority_revision = 1",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    transaction.commit().await.unwrap();
    AuthorityHead {
        revision: 2,
        digest: authority_digest,
    }
}

fn apply_context_at_authority(
    fixture: &Fixture,
    operation: &Operation,
    authority: &AuthorityHead,
) -> ApplyLockContext {
    let mut context = ApplyLockContext::single(fixture, operation);
    context.expected_authority_revision = authority.revision;
    context.expected_authority_digest = authority.digest.clone();
    context
}

async fn set_competing_active_baseline(pool: &PgPool, fixture: &Fixture) -> String {
    let content_hash = digest(&format!(
        "apply-competing-baseline:{}",
        fixture.activation_id
    ));
    let mut transaction = pool.begin().await.unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_ruleset_heads SET next_version = 3 \
         WHERE guild_id = $1 AND ruleset_key = $2 AND next_version = 2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 2, 1, '{}'::JSONB, $3, $4)",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&content_hash)
    .bind(&fixture.actor.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) VALUES ($1, $2, 2)",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    content_hash
}

async fn lock_apply(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    operation: &Operation,
    call: &Call,
) -> Result<LockRow, sqlx::Error> {
    let context = ApplyLockContext::single(fixture, operation);
    lock_apply_with_context(transaction, fixture, operation, call, &context).await
}

async fn lock_apply_with_context(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    operation: &Operation,
    call: &Call,
    context: &ApplyLockContext,
) -> Result<LockRow, sqlx::Error> {
    sqlx::query_as::<_, LockRow>(
        "SELECT outcome, exact_replay, requires_commit, resulting_revision, resulting_state, \
         deployment_id, desired_target_digest, locked_projection \
         FROM public.starring_product_apply_lock_v1(\
          $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
          $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30)",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.promotion_id)
    .bind(call.expected_revision)
    .bind(&context.expected_payload_digest)
    .bind(&fixture.actor.principal_id)
    .bind(&call.session_digest)
    .bind(&fixture.actor.session_subject)
    .bind(&fixture.actor.user_id)
    .bind(&fixture.application_id)
    .bind(&fixture.guild_id)
    .bind(&call.capability)
    .bind(context.expected_authority_revision)
    .bind(&context.expected_authority_digest)
    .bind(&fixture.observation_digest)
    .bind(call.observed_at)
    .bind(call.expires_at)
    .bind(&call.effective_permissions)
    .bind(call.guild_owner)
    .bind(&operation.request_id)
    .bind(&context.active_idempotency_digest)
    .bind(&context.idempotency_candidates)
    .bind(&context.candidate_key_ids)
    .bind(&context.candidate_key_fingerprints)
    .bind(&context.active_key_id)
    .bind(&operation.semantic_digest)
    .bind(&operation.receipt_id)
    .bind(&operation.audit_event_id)
    .bind(&operation.apply_attempt_id)
    .bind(&operation.deployment_id)
    .fetch_one(&mut **transaction)
    .await
}

#[derive(Deserialize)]
struct LockedApplyProjectionV1 {
    requested_at: DateTime<Utc>,
    runtime_generation: RuntimeGeneration,
    previous_runtime: Option<RuntimeProcessIdentityV1>,
    operation: LockedApplyOperationV1,
    server: LockedApplyServerV1,
}

#[derive(Deserialize)]
struct LockedApplyOperationV1 {
    deployment_id: DeploymentId,
}

#[derive(Deserialize)]
struct LockedApplyServerV1 {
    scope: LockedApplyScopeV1,
    activation: LockedApplyActivationV1,
    authority: LockedApplyAuthorityV1,
    target: LockedApplyTargetV1,
}

#[derive(Deserialize)]
struct LockedApplyScopeV1 {
    tenant_id: TenantId,
    installation_id: InstallationId,
    promotion_id: PromotionId,
}

#[derive(Deserialize)]
struct LockedApplyActivationV1 {
    request_id: ActivationRequestId,
}

#[derive(Deserialize)]
struct LockedApplyAuthorityV1 {
    revision: u64,
    binding_revision: BindingRevision,
    binding_fingerprint: ResourceBindingFingerprint,
}

#[derive(Deserialize)]
struct LockedApplyTargetV1 {
    guild_id: GuildId,
    ruleset_key: RuleSetKey,
    version: RuleSetVersionId,
    content_hash: RuleSetContentHash,
}

fn prepare_requested_deployment(lock: &LockRow) -> PreparedRequestedDeploymentV1 {
    let projection: LockedApplyProjectionV1 = serde_json::from_value(
        lock.locked_projection
            .as_ref()
            .expect("fresh lock projection")
            .0
            .clone(),
    )
    .expect("locked apply projection must decode");
    prepare_requested_deployment_v1(
        EnqueueDeploymentV1 {
            identity: RuntimeDeploymentIdentityV1 {
                deployment_id: projection.operation.deployment_id,
                tenant_id: projection.server.scope.tenant_id,
                installation_id: projection.server.scope.installation_id,
                promotion_id: projection.server.scope.promotion_id,
                activation_request_id: projection.server.activation.request_id,
            },
            target: RuntimeDeploymentTargetV1 {
                guild_id: projection.server.target.guild_id,
                ruleset_key: projection.server.target.ruleset_key,
                version: projection.server.target.version,
                content_hash: projection.server.target.content_hash,
                binding_revision: projection.server.authority.binding_revision,
                binding_fingerprint: projection.server.authority.binding_fingerprint,
            },
            runtime_generation: projection.runtime_generation,
            previous_runtime: projection.previous_runtime,
            installation_authority_revision: projection.server.authority.revision,
        },
        projection.requested_at,
    )
    .expect("locked apply projection must prepare")
}

fn deployment_scope(prepared: &PreparedRequestedDeploymentV1) -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: prepared.snapshot().identity.tenant_id.clone(),
        installation_id: prepared.snapshot().identity.installation_id.clone(),
        deployment_id: prepared.snapshot().identity.deployment_id.clone(),
    }
}

fn one_bit_wrong_digest(value: &str) -> String {
    let mut bytes = value.as_bytes().to_vec();
    let index = bytes.len() - 1;
    bytes[index] = match bytes[index] {
        b'0' => b'1',
        b'1' => b'0',
        b'2' => b'3',
        b'3' => b'2',
        b'4' => b'5',
        b'5' => b'4',
        b'6' => b'7',
        b'7' => b'6',
        b'8' => b'9',
        b'9' => b'8',
        b'a' => b'b',
        b'b' => b'a',
        b'c' => b'd',
        b'd' => b'c',
        b'e' => b'f',
        b'f' => b'e',
        _ => panic!("digest must be lowercase hexadecimal"),
    };
    String::from_utf8(bytes).unwrap()
}

async fn assert_runtime_mutation_clock_cleared(transaction: &mut Transaction<'_, Postgres>) {
    let configured = sqlx::query_scalar::<_, Option<String>>(
        "SELECT NULLIF(\
         pg_catalog.current_setting('starring.runtime_mutation_clock', TRUE), '')",
    )
    .fetch_one(&mut **transaction)
    .await
    .unwrap();
    assert!(configured.is_none());
    sqlx::query("SAVEPOINT assert_runtime_mutation_clock_cleared")
        .execute(&mut **transaction)
        .await
        .unwrap();
    let error = sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT public.starring_runtime_current_mutation_clock()",
    )
    .fetch_one(&mut **transaction)
    .await
    .expect_err("cleared runtime mutation clock cannot be reused");
    assert!(matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("55000")
    ));
    sqlx::query("ROLLBACK TO SAVEPOINT assert_runtime_mutation_clock_cleared")
        .execute(&mut **transaction)
        .await
        .unwrap();
    sqlx::query("RELEASE SAVEPOINT assert_runtime_mutation_clock_cleared")
        .execute(&mut **transaction)
        .await
        .unwrap();
}

struct FinalizeProjection<'a> {
    desired_target_digest: &'a str,
    previous_runtime: Option<&'a Value>,
    snapshot: &'a Value,
    notices: &'a Value,
}

fn finalize_projection<'a>(
    desired_target_digest: &'a str,
    previous_runtime: Option<&'a Value>,
    snapshot: &'a Value,
) -> FinalizeProjection<'a> {
    static EMPTY_NOTICES: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| json!([]));
    FinalizeProjection {
        desired_target_digest,
        previous_runtime,
        snapshot,
        notices: &EMPTY_NOTICES,
    }
}

async fn finalize_apply(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    operation: &Operation,
    call: &Call,
    lock: &LockRow,
    projection_input: FinalizeProjection<'_>,
) -> Result<FinalizeRow, sqlx::Error> {
    let FinalizeProjection {
        desired_target_digest,
        previous_runtime,
        snapshot,
        notices,
    } = projection_input;
    let projection = lock
        .locked_projection
        .as_ref()
        .expect("fresh lock projection");
    let previous_runtime = previous_runtime.cloned().unwrap_or(Value::Null);
    sqlx::query_as::<_, FinalizeRow>(
        "SELECT outcome, resulting_revision, resulting_state, exact_replay, guild_id, \
         deployment_id, desired_target_digest \
         FROM public.starring_product_apply_finalize_v1(\
          $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
          $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, \
          $31, $32, $33, $34, $35)",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.promotion_id)
    .bind(call.expected_revision)
    .bind(&fixture.payload_digest)
    .bind(&fixture.actor.principal_id)
    .bind(&call.session_digest)
    .bind(&fixture.actor.session_subject)
    .bind(&fixture.actor.user_id)
    .bind(&fixture.application_id)
    .bind(&fixture.guild_id)
    .bind(&call.capability)
    .bind(1_i64)
    .bind(&fixture.authority_digest)
    .bind(&fixture.observation_digest)
    .bind(call.observed_at)
    .bind(call.expires_at)
    .bind(&call.effective_permissions)
    .bind(call.guild_owner)
    .bind(&operation.request_id)
    .bind(&operation.idempotency_digest)
    .bind(vec![operation.idempotency_digest.clone()])
    .bind(vec![operation.key_id.clone()])
    .bind(vec![operation.key_fingerprint.clone()])
    .bind(&operation.key_id)
    .bind(&operation.semantic_digest)
    .bind(&operation.receipt_id)
    .bind(&operation.audit_event_id)
    .bind(&operation.apply_attempt_id)
    .bind(&operation.deployment_id)
    .bind(projection)
    .bind(desired_target_digest)
    .bind(Json(previous_runtime))
    .bind(Json(snapshot))
    .bind(Json(notices))
    .fetch_one(&mut **transaction)
    .await
}

async fn assert_apply_unmutated(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: &Fixture,
    operation: &Operation,
) {
    let unchanged = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
          (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations \
           WHERE guild_id = $2 AND ruleset_key = $3), \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $4), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $4), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
           WHERE receipt_id = $4) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&operation.receipt_id)
    .fetch_one(&mut **transaction)
    .await
    .unwrap();
    assert_eq!(unchanged, ("approved".to_string(), 2, 0, 0, 0, 0, 0));
}

async fn complete_apply(
    pool: &PgPool,
    fixture: &Fixture,
    operation: &Operation,
) -> PreparedRequestedDeploymentV1 {
    let call = Call::valid(fixture);
    let mut transaction = begin_serializable(pool).await;
    let lock = lock_apply(&mut transaction, fixture, operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    let finalized = finalize_apply(
        &mut transaction,
        fixture,
        operation,
        &call,
        &lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(finalized.outcome, "ok");
    assert_eq!(finalized.resulting_revision, Some(4));
    assert_eq!(finalized.resulting_state.as_deref(), Some("applied"));
    assert!(!finalized.exact_replay);
    transaction.commit().await.unwrap();
    prepared
}

fn is_serialization_failure(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("40001")
    )
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn fresh_apply_exact_replay_and_semantic_conflict_are_atomic() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("fresh");
    let mut transaction = begin_serializable(&pool).await;
    let call = Call::valid(&fixture);
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    assert!(!lock.exact_replay);
    assert!(!lock.requires_commit);
    assert_eq!(lock.resulting_revision, Some(2));
    assert_eq!(lock.resulting_state.as_deref(), Some("approved"));
    assert_eq!(
        lock.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert!(lock.desired_target_digest.is_none());
    let unchanged = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $2) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(unchanged, ("approved".to_string(), 2, 0, 0));
    let prepared = prepare_requested_deployment(&lock);
    let finalized = finalize_apply(
        &mut transaction,
        &fixture,
        &operation,
        &call,
        &lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(finalized.outcome, "ok");
    assert_eq!(finalized.resulting_revision, Some(4));
    assert_eq!(finalized.resulting_state.as_deref(), Some("applied"));
    assert!(!finalized.exact_replay);
    assert_eq!(finalized.guild_id.as_deref(), Some(&*fixture.guild_id));
    assert_eq!(
        finalized.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert_eq!(
        finalized.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    transaction.commit().await.unwrap();

    let persisted = sqlx::query_as::<_, (String, i64, String, i64, i16, String, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, deployment.phase, \
          deployment.policy_revision, deployment.desired_target_digest_version, \
          deployment.desired_target_digest, \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $2 AND endpoint_domain = 'product_apply_v1'), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $2 AND action = 'promotion.apply'), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
           WHERE receipt_id = $2 AND endpoint_domain = 'product_apply_v1') \
         FROM public.activation_requests AS activation \
         INNER JOIN public.runtime_deployments AS deployment \
          ON deployment.activation_request_id = activation.id \
         WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (
            "applied".to_string(),
            4,
            "requested".to_string(),
            1,
            1,
            prepared.desired_target_digest().to_string(),
            1,
            1,
            1
        )
    );

    let scope = deployment_scope(&prepared);
    let status = PostgresRuntimeConvergence::new(pool.clone())
        .status(&scope)
        .await
        .unwrap();
    assert_eq!(&status.snapshot, prepared.snapshot());

    let mut replay_transaction = begin_serializable(&pool).await;
    let replay_call = Call::valid(&fixture);
    let replay = lock_apply(&mut replay_transaction, &fixture, &operation, &replay_call)
        .await
        .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(replay.resulting_revision, Some(4));
    assert_eq!(replay.resulting_state.as_deref(), Some("applied"));
    assert_eq!(
        replay.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    assert!(replay.locked_projection.is_none());
    replay_transaction.commit().await.unwrap();

    let mut conflict_operation = operation.clone();
    conflict_operation.semantic_digest = digest("different-apply-semantics");
    let mut conflict_transaction = begin_serializable(&pool).await;
    let conflict = lock_apply(
        &mut conflict_transaction,
        &fixture,
        &conflict_operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(conflict.outcome, "idempotency_conflict");
    conflict_transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn successful_finalize_clears_runtime_mutation_clock() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("runtime-clock");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    let finalized = finalize_apply(
        &mut transaction,
        &fixture,
        &operation,
        &call,
        &lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(finalized.outcome, "ok");
    assert_runtime_mutation_clock_cleared(&mut transaction).await;
    transaction.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn exact_replay_survives_later_active_pointer_change() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("replay-after-pointer-change");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    let finalized = finalize_apply(
        &mut transaction,
        &fixture,
        &operation,
        &call,
        &lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(finalized.outcome, "ok");
    transaction.commit().await.unwrap();

    let next_content_hash = digest(&format!("next-active:{}", fixture.activation_id));
    let mut pointer_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 2, 1, '{}'::JSONB, $3, $4)",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&next_content_hash)
    .bind(&fixture.actor.user_id)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_ruleset_heads SET next_version = 3 \
         WHERE guild_id = $1 AND ruleset_key = $2 AND next_version = 2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    let changed = sqlx::query(
        "UPDATE public.automation_ruleset_activations SET active_version = 2 \
         WHERE guild_id = $1 AND ruleset_key = $2 AND active_version = 1",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&mut *pointer_transaction)
    .await
    .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    pointer_transaction.commit().await.unwrap();

    let mut replay_transaction = begin_serializable(&pool).await;
    let replay = lock_apply(
        &mut replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(replay.resulting_revision, Some(4));
    assert_eq!(replay.resulting_state.as_deref(), Some("applied"));
    assert_eq!(
        replay.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    assert!(replay.locked_projection.is_none());
    replay_transaction.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn one_bit_wrong_desired_digest_is_rejected_without_mutation() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("wrong-runtime-digest");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    let wrong_digest = one_bit_wrong_digest(prepared.desired_target_digest());
    assert_ne!(wrong_digest, prepared.desired_target_digest());
    let finalized = finalize_apply(
        &mut transaction,
        &fixture,
        &operation,
        &call,
        &lock,
        finalize_projection(
            &wrong_digest,
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(finalized.outcome, "invalid_runtime_projection");
    assert_eq!(finalized.resulting_revision, None);
    assert_eq!(finalized.resulting_state, None);
    assert!(!finalized.exact_replay);
    assert_eq!(finalized.guild_id, None);
    assert_eq!(finalized.deployment_id, None);
    assert_eq!(finalized.desired_target_digest, None);
    assert_apply_unmutated(&mut transaction, &fixture, &operation).await;
    transaction.commit().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn malformed_runtime_projection_shapes_are_stable_and_atomic() {
    let pool = pool().await;
    for case in [
        "top-scalar",
        "top-array",
        "identity-array",
        "missing-nullable",
        "revision-string",
        "version-string",
        "generation-string",
        "notices-object",
    ] {
        let fixture = seed_fixture(&pool).await;
        let operation = Operation::new(case);
        let call = Call::valid(&fixture);
        let mut transaction = begin_serializable(&pool).await;
        let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
            .await
            .unwrap();
        assert_eq!(lock.outcome, "ready");
        let prepared = prepare_requested_deployment(&lock);
        let mut snapshot = prepared.snapshot_json().clone();
        let mut notices = json!([]);
        match case {
            "top-scalar" => snapshot = json!(1),
            "top-array" => snapshot = json!([]),
            "identity-array" => snapshot["identity"] = json!([]),
            "missing-nullable" => {
                let object = snapshot.as_object_mut().unwrap();
                assert_eq!(object.remove("controller_lease"), Some(Value::Null));
                assert_eq!(object.insert("unknown".to_string(), Value::Null), None);
            }
            "revision-string" => snapshot["revision"] = json!("1"),
            "version-string" => snapshot["target"]["version"] = json!("1"),
            "generation-string" => snapshot["runtime_generation"] = json!("1"),
            "notices-object" => notices = json!({}),
            _ => unreachable!(),
        }
        let finalized = finalize_apply(
            &mut transaction,
            &fixture,
            &operation,
            &call,
            &lock,
            FinalizeProjection {
                desired_target_digest: prepared.desired_target_digest(),
                previous_runtime: prepared.previous_runtime_json(),
                snapshot: &snapshot,
                notices: &notices,
            },
        )
        .await
        .unwrap();
        assert_eq!(finalized.outcome, "invalid_runtime_projection", "{case}");
        assert_eq!(finalized.resulting_revision, None, "{case}");
        assert_eq!(finalized.resulting_state, None, "{case}");
        assert!(!finalized.exact_replay, "{case}");
        assert_eq!(finalized.guild_id, None, "{case}");
        assert_eq!(finalized.deployment_id, None, "{case}");
        assert_eq!(finalized.desired_target_digest, None, "{case}");
        assert_apply_unmutated(&mut transaction, &fixture, &operation).await;
        transaction.commit().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn exact_replay_with_wrong_payload_fails_closed() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("replay-payload");
    complete_apply(&pool, &fixture, &operation).await;
    let mut context = ApplyLockContext::single(&fixture, &operation);
    context.expected_payload_digest = digest(&format!("wrong:payload:{}", fixture.activation_id));
    let mut transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "payload_mismatch");
    assert!(!replay.exact_replay);
    assert!(!replay.requires_commit);
    assert_eq!(replay.resulting_revision, None);
    assert_eq!(replay.resulting_state, None);
    assert_eq!(replay.deployment_id, None);
    assert_eq!(replay.desired_target_digest, None);
    transaction.rollback().await.unwrap();
    let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $2), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
           WHERE receipt_id = $2), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $2)",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 1, 1));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn applied_replay_rejects_tampered_revision_and_disposition_evidence() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("applied-replay-forensic");
    complete_apply(&pool, &fixture, &operation).await;

    sqlx::query(
        "UPDATE public.activation_requests SET product_revision = 5 \
         WHERE id = $1 AND state = 'applied' AND product_revision = 4",
    )
    .bind(&fixture.activation_id)
    .execute(&pool)
    .await
    .unwrap();
    let mut revision_transaction = begin_serializable(&pool).await;
    let revision = lock_apply(
        &mut revision_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(revision.outcome, "indeterminate");
    assert!(!revision.exact_replay);
    revision_transaction.rollback().await.unwrap();

    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("applied-replay-disposition");
    complete_apply(&pool, &fixture, &operation).await;

    let mut corruption = pool.begin().await.unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.product_action_receipts \
         DISABLE TRIGGER product_action_receipts_reject_mutation; \
         ALTER TABLE public.product_action_receipt_audit_evidence \
         DISABLE TRIGGER product_action_receipt_audit_evidence_reject_mutation",
    )
    .execute(&mut *corruption)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.product_action_receipts SET http_disposition_class = 4 \
         WHERE receipt_id = $1",
    )
    .bind(&operation.receipt_id)
    .execute(&mut *corruption)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.product_action_receipt_audit_evidence \
         SET http_disposition_class = 4 WHERE receipt_id = $1",
    )
    .bind(&operation.receipt_id)
    .execute(&mut *corruption)
    .await
    .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.product_action_receipts \
         ENABLE TRIGGER product_action_receipts_reject_mutation; \
         ALTER TABLE public.product_action_receipt_audit_evidence \
         ENABLE TRIGGER product_action_receipt_audit_evidence_reject_mutation",
    )
    .execute(&mut *corruption)
    .await
    .unwrap();
    corruption.commit().await.unwrap();

    let mut disposition_transaction = begin_serializable(&pool).await;
    let disposition = lock_apply(
        &mut disposition_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(disposition.outcome, "indeterminate");
    assert!(!disposition.exact_replay);
    disposition_transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn authority_drift_preserves_recorded_replay_and_blocks_fresh_apply() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("authority-drift-replay");
    let prepared = complete_apply(&pool, &fixture, &operation).await;
    let guild_id = fixture.guild_id.parse::<u64>().unwrap();
    let mut next_resource_bindings = ResourceBindingMap::default();
    next_resource_bindings.channel_bindings.insert(
        ResourceKey("community_hub".to_string()),
        ChannelId(guild_id + 1_000_000_000),
    );
    next_resource_bindings.role_bindings.insert(
        ResourceKey("automation_operator".to_string()),
        RoleId(guild_id + 2_000_000_000),
    );
    let next_binding_fingerprint = resource_binding_fingerprint_v2(&next_resource_bindings);
    let stored_next_resource_bindings = json!({
        "role_bindings": &next_resource_bindings.role_bindings,
        "channel_bindings": &next_resource_bindings.channel_bindings
    });
    let next_authority_digest = digest(&format!("authority:v2:{}", fixture.installation_id));
    let mut authority_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 2, $2, 2, $3, $4, 2, 1, 3600, $5, $6, $7)",
    )
    .bind(&fixture.installation_id)
    .bind(&fixture.tenant_id)
    .bind(Json(&stored_next_resource_bindings))
    .bind(next_binding_fingerprint.as_str())
    .bind(&next_authority_digest)
    .bind(&fixture.actor.principal_id)
    .bind(digest(&format!(
        "authority-request:v2:{}",
        fixture.installation_id
    )))
    .execute(&mut *authority_transaction)
    .await
    .unwrap();
    let advanced = sqlx::query(
        "UPDATE public.automation_installations \
         SET current_authority_revision = 2, updated_at = pg_catalog.clock_timestamp() \
         WHERE installation_id = $1 AND tenant_id = $2 AND current_authority_revision = 1",
    )
    .bind(&fixture.installation_id)
    .bind(&fixture.tenant_id)
    .execute(&mut *authority_transaction)
    .await
    .unwrap();
    assert_eq!(advanced.rows_affected(), 1);
    authority_transaction.commit().await.unwrap();

    let mut stale_replay_transaction = begin_serializable(&pool).await;
    let stale_replay = lock_apply(
        &mut stale_replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(stale_replay.outcome, "authority_mismatch");
    assert!(!stale_replay.exact_replay);
    assert!(!stale_replay.requires_commit);
    stale_replay_transaction.rollback().await.unwrap();

    let current_authority = AuthorityHead {
        revision: 2,
        digest: next_authority_digest,
    };
    let current_context = apply_context_at_authority(&fixture, &operation, &current_authority);
    let mut replay_transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &current_context,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(
        replay.deployment_id.as_deref(),
        Some(&*operation.deployment_id)
    );
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    replay_transaction.commit().await.unwrap();

    let fresh_operation = Operation::new("authority-drift-fresh");
    let mut fresh_transaction = begin_serializable(&pool).await;
    let fresh = lock_apply(
        &mut fresh_transaction,
        &fixture,
        &fresh_operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(fresh.outcome, "authority_mismatch");
    assert!(!fresh.exact_replay);
    assert!(!fresh.requires_commit);
    assert!(fresh.locked_projection.is_none());
    fresh_transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn stale_applied_replay_cannot_commit_a_rotated_idempotency_alias() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("stale-replay-alias");
    complete_apply(&pool, &fixture, &operation).await;
    let (bindings, fingerprint) = authority_binding_material(&pool, &fixture).await;
    advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 1,
            resource_bindings: &bindings,
            binding_fingerprint: &fingerprint,
            policy_revision: 2,
            required_approvals: 1,
            activation_ttl_seconds: 3_600,
        },
    )
    .await;
    let rotated_digest = digest(&format!("rotated-alias:{}", operation.request_id));
    let rotated_key_id = "apply-key-v2".to_string();
    let rotated_key_fingerprint = digest("apply-key-v2-material");
    let mut stale_context = ApplyLockContext::single(&fixture, &operation);
    stale_context.active_idempotency_digest = rotated_digest.clone();
    stale_context.idempotency_candidates =
        vec![rotated_digest.clone(), operation.idempotency_digest.clone()];
    stale_context.candidate_key_ids = vec![rotated_key_id.clone(), operation.key_id.clone()];
    stale_context.candidate_key_fingerprints = vec![
        rotated_key_fingerprint.clone(),
        operation.key_fingerprint.clone(),
    ];
    stale_context.active_key_id = rotated_key_id;

    let mut transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &stale_context,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "authority_mismatch");
    assert!(!replay.exact_replay);
    assert!(!replay.requires_commit);
    transaction.commit().await.unwrap();

    let aliases = sqlx::query_as::<_, (i64, i64)>(
        "SELECT pg_catalog.count(*), \
          pg_catalog.count(*) FILTER (WHERE idempotency_key_digest = $2) \
         FROM public.product_action_receipt_idempotency_aliases \
         WHERE receipt_id = $1",
    )
    .bind(&operation.receipt_id)
    .bind(&rotated_digest)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(aliases, (1, 0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn replay_rechecks_discord_freshness_after_the_final_receipt_lock() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("replay-final-freshness");
    complete_apply(&pool, &fixture, &operation).await;
    let rotated_digest = digest(&format!("freshness-alias:{}", operation.request_id));
    let mut context = ApplyLockContext::single(&fixture, &operation);
    context.active_idempotency_digest = rotated_digest.clone();
    context.idempotency_candidates =
        vec![rotated_digest.clone(), operation.idempotency_digest.clone()];
    context.candidate_key_ids = vec!["apply-key-v2".to_string(), operation.key_id.clone()];
    context.candidate_key_fingerprints = vec![
        digest("apply-key-v2-material"),
        operation.key_fingerprint.clone(),
    ];
    context.active_key_id = "apply-key-v2".to_string();
    let observed_at = Utc::now() - TimeDelta::milliseconds(20);
    let call = Call {
        expected_revision: 2,
        capability: "apply".to_string(),
        session_digest: fixture.actor.session_digest.clone(),
        observed_at,
        expires_at: observed_at + TimeDelta::seconds(2),
        effective_permissions: "32".to_string(),
        guild_owner: false,
    };
    let expires_at = call.expires_at;
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT receipt_id FROM public.product_action_receipts \
         WHERE receipt_id = $1 FOR UPDATE",
    )
    .bind(&operation.receipt_id)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    let replay_pool = pool.clone();
    let replay_fixture = fixture.clone();
    let replay_operation = operation.clone();
    let replay = tokio::spawn(async move {
        let mut transaction = begin_serializable(&replay_pool).await;
        let locked = lock_apply_with_context(
            &mut transaction,
            &replay_fixture,
            &replay_operation,
            &call,
            &context,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        locked
    });
    let mut reached_final_lock = false;
    for _ in 0..100 {
        reached_final_lock = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (\
              SELECT 1 FROM pg_catalog.pg_stat_activity \
              WHERE datname = pg_catalog.current_database() \
               AND pid <> pg_catalog.pg_backend_pid() \
               AND wait_event_type = 'Lock' \
               AND query LIKE '%starring_product_apply_lock_v1%')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if reached_final_lock {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(reached_final_lock);
    let remaining = (expires_at - Utc::now())
        .to_std()
        .unwrap_or(std::time::Duration::ZERO);
    tokio::time::sleep(remaining + std::time::Duration::from_millis(100)).await;
    blocker.commit().await.unwrap();
    let replay = replay.await.unwrap();
    assert_eq!(replay.outcome, "authorization_stale");
    assert!(!replay.exact_replay);
    assert!(!replay.requires_commit);
    let aliases = sqlx::query_as::<_, (i64, i64)>(
        "SELECT pg_catalog.count(*), \
          pg_catalog.count(*) FILTER (WHERE idempotency_key_digest = $2) \
         FROM public.product_action_receipt_idempotency_aliases \
         WHERE receipt_id = $1",
    )
    .bind(&operation.receipt_id)
    .bind(&rotated_digest)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(aliases, (1, 0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn baseline_drift_is_durably_superseded_and_exactly_replayed() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let competing_hash = set_competing_active_baseline(&pool, &fixture).await;
    let operation = Operation::new("superseded-baseline");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(locked.outcome, "superseded");
    assert!(!locked.exact_replay);
    assert!(locked.requires_commit);
    assert_eq!(locked.resulting_revision, Some(4));
    assert_eq!(locked.resulting_state.as_deref(), Some("superseded"));
    assert!(locked.deployment_id.is_none());
    assert!(locked.desired_target_digest.is_none());
    assert!(locked.locked_projection.is_none());
    transaction.commit().await.unwrap();

    let persisted = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&persisted, "superseded_baseline_drift");
    let termination = &persisted.termination.as_ref().unwrap().0;
    assert_eq!(termination["kind"], "superseded");
    assert_eq!(termination["reason"]["reason"], "active_baseline_drift");
    assert_eq!(termination["reason"]["expected"]["state"], "absent");
    assert_eq!(termination["reason"]["observed"]["state"], "exact");
    assert_eq!(termination["reason"]["observed"]["version"], 2);
    assert_eq!(
        termination["reason"]["observed"]["content_hash"],
        competing_hash
    );
    assert_eq!(persisted.audit_authority_revision, Some(1));
    assert_eq!(persisted.audit_policy_revision, Some(1));
    assert_eq!(persisted.audit_baseline_version, Some(2));
    assert_eq!(
        persisted.audit_baseline_hash.as_deref(),
        Some(&*competing_hash)
    );

    let mut replay_transaction = begin_serializable(&pool).await;
    let replay = lock_apply(
        &mut replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "superseded");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(replay.resulting_revision, Some(4));
    assert_eq!(replay.resulting_state.as_deref(), Some("superseded"));
    assert!(replay.deployment_id.is_none());
    assert!(replay.desired_target_digest.is_none());
    assert!(replay.locked_projection.is_none());
    replay_transaction.commit().await.unwrap();

    let replayed = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&replayed, "superseded_baseline_drift");
    assert_eq!(replayed.termination.unwrap().0, termination.clone());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn terminal_replay_rejects_mismatched_baseline_and_clock_evidence() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    set_competing_active_baseline(&pool, &fixture).await;
    let operation = Operation::new("terminal-evidence-tamper");
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "superseded");
    transaction.commit().await.unwrap();
    let original = terminal_persistence(&pool, &fixture, &operation)
        .await
        .termination
        .unwrap()
        .0;

    sqlx::query(
        "UPDATE public.activation_requests \
         SET termination = pg_catalog.jsonb_set(\
          termination, '{reason,expected}', \
          pg_catalog.jsonb_build_object(\
           'state', 'exact', 'version', 1, 'content_hash', target_content_hash)) \
         WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .execute(&pool)
    .await
    .unwrap();
    let mut baseline_transaction = begin_serializable(&pool).await;
    let baseline = lock_apply(
        &mut baseline_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(baseline.outcome, "indeterminate");
    assert!(!baseline.exact_replay);
    baseline_transaction.rollback().await.unwrap();

    sqlx::query("UPDATE public.activation_requests SET termination = $2 WHERE id = $1")
        .bind(&fixture.activation_id)
        .bind(Json(&original))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests AS activation \
         SET termination = pg_catalog.jsonb_set(\
          activation.termination, '{at}', pg_catalog.to_jsonb(receipt.completed_at + INTERVAL '1 second')) \
         FROM public.product_action_receipts AS receipt \
         WHERE activation.id = $1 AND receipt.receipt_id = $2",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .execute(&pool)
    .await
    .unwrap();
    let mut clock_transaction = begin_serializable(&pool).await;
    let clock = lock_apply(
        &mut clock_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(clock.outcome, "indeterminate");
    assert!(!clock.exact_replay);
    clock_transaction.rollback().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn binding_drift_uses_current_authority_and_historical_replay_evidence() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let guild_id = fixture.guild_id.parse::<u64>().unwrap();
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        ResourceKey("community_hub".to_string()),
        ChannelId(guild_id + 1_000_000_000),
    );
    bindings.role_bindings.insert(
        ResourceKey("automation_operator".to_string()),
        RoleId(guild_id + 2_000_000_000),
    );
    let fingerprint = resource_binding_fingerprint_v2(&bindings);
    let stored_bindings = json!({
        "role_bindings": &bindings.role_bindings,
        "channel_bindings": &bindings.channel_bindings
    });
    let authority = advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 2,
            resource_bindings: &stored_bindings,
            binding_fingerprint: fingerprint.as_str(),
            policy_revision: 1,
            required_approvals: 1,
            activation_ttl_seconds: 3_600,
        },
    )
    .await;
    let operation = Operation::new("superseded-binding");
    let context = apply_context_at_authority(&fixture, &operation, &authority);
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "superseded");
    assert!(!locked.exact_replay);
    assert!(locked.requires_commit);
    assert_eq!(locked.resulting_revision, Some(4));
    assert_eq!(locked.resulting_state.as_deref(), Some("superseded"));
    assert!(locked.deployment_id.is_none());
    assert!(locked.desired_target_digest.is_none());
    assert!(locked.locked_projection.is_none());
    transaction.commit().await.unwrap();

    let persisted = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&persisted, "superseded_binding_drift");
    let termination = &persisted.termination.as_ref().unwrap().0;
    assert_eq!(termination["reason"]["reason"], "binding_drift");
    assert_eq!(termination["reason"]["expected_revision"], 1);
    assert_eq!(termination["reason"]["observed_revision"], 2);
    assert!(termination["reason"]["observed_fingerprint"].is_null());
    assert_eq!(persisted.audit_authority_revision, Some(2));
    assert_eq!(
        persisted.audit_binding_fingerprint.as_deref(),
        Some(fingerprint.as_str())
    );
    assert_eq!(persisted.audit_policy_revision, Some(1));
    assert!(persisted.audit_baseline_version.is_none());
    assert!(persisted.audit_baseline_hash.is_none());

    let mut stale_replay_transaction = begin_serializable(&pool).await;
    let stale_replay = lock_apply(
        &mut stale_replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(stale_replay.outcome, "authority_mismatch");
    assert!(!stale_replay.exact_replay);
    assert!(!stale_replay.requires_commit);
    stale_replay_transaction.rollback().await.unwrap();

    let mut replay_transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut replay_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "superseded");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(replay.resulting_revision, Some(4));
    assert_eq!(replay.resulting_state.as_deref(), Some("superseded"));
    assert!(replay.locked_projection.is_none());
    replay_transaction.commit().await.unwrap();

    let replayed = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&replayed, "superseded_binding_drift");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn policy_drift_persists_exact_expected_and_observed_policy() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let (bindings, fingerprint) = authority_binding_material(&pool, &fixture).await;
    let authority = advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 1,
            resource_bindings: &bindings,
            binding_fingerprint: &fingerprint,
            policy_revision: 2,
            required_approvals: 2,
            activation_ttl_seconds: 1_800,
        },
    )
    .await;
    let operation = Operation::new("superseded-policy");
    let context = apply_context_at_authority(&fixture, &operation, &authority);
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "superseded");
    assert!(!locked.exact_replay);
    assert!(locked.requires_commit);
    assert_eq!(locked.resulting_revision, Some(4));
    assert_eq!(locked.resulting_state.as_deref(), Some("superseded"));
    assert!(locked.deployment_id.is_none());
    assert!(locked.desired_target_digest.is_none());
    assert!(locked.locked_projection.is_none());
    transaction.commit().await.unwrap();

    let persisted = terminal_persistence(&pool, &fixture, &operation).await;
    assert_terminal_persistence(&persisted, "superseded_policy_drift");
    let termination = &persisted.termination.as_ref().unwrap().0;
    assert_eq!(termination["reason"]["reason"], "policy_drift");
    assert_eq!(termination["reason"]["expected_revision"], 1);
    assert_eq!(termination["reason"]["observed_revision"], 2);
    assert_eq!(termination["reason"]["expected_required_approvals"], 1);
    assert_eq!(termination["reason"]["observed_required_approvals"], 2);
    assert_eq!(termination["reason"]["expected_ttl_seconds"], 3_600);
    assert_eq!(termination["reason"]["observed_ttl_seconds"], 1_800);
    assert_eq!(persisted.audit_authority_revision, Some(2));
    assert_eq!(
        persisted.audit_binding_fingerprint.as_deref(),
        Some(&*fingerprint)
    );
    assert_eq!(persisted.audit_policy_revision, Some(2));
    assert!(persisted.audit_baseline_version.is_none());
    assert!(persisted.audit_baseline_hash.is_none());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn missing_target_with_policy_drift_remains_target_mismatch_without_supersession() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let (bindings, fingerprint) = authority_binding_material(&pool, &fixture).await;
    let authority = advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 1,
            resource_bindings: &bindings,
            binding_fingerprint: &fingerprint,
            policy_revision: 2,
            required_approvals: 1,
            activation_ttl_seconds: 3_600,
        },
    )
    .await;
    let deleted = sqlx::query(
        "DELETE FROM public.automation_ruleset_versions \
         WHERE guild_id = $1 AND ruleset_key = $2 AND version = 1",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(deleted.rows_affected(), 1);

    let operation = Operation::new("missing-target-policy-drift");
    let context = apply_context_at_authority(&fixture, &operation, &authority);
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply_with_context(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &context,
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "target_mismatch");
    assert!(!locked.exact_replay);
    assert!(!locked.requires_commit);
    assert!(locked.resulting_revision.is_none());
    assert!(locked.resulting_state.is_none());
    assert!(locked.deployment_id.is_none());
    assert!(locked.desired_target_digest.is_none());
    assert!(locked.locked_projection.is_none());
    transaction.commit().await.unwrap();

    let persisted = sqlx::query_as::<
        _,
        (
            String,
            i64,
            i64,
            Option<Json<Value>>,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
        ),
    >(
        "SELECT activation.state, activation.product_revision, activation.apply_attempt_no, \
          activation.termination, \
          (SELECT head.next_version FROM public.automation_ruleset_heads AS head \
           WHERE head.guild_id = $2 AND head.ruleset_key = $3), \
          (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations AS active \
           WHERE active.guild_id = $2 AND active.ruleset_key = $3), \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
           WHERE deployment.activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
           WHERE receipt.receipt_id = $4), \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_idempotency_aliases AS alias \
           WHERE alias.receipt_id = $4), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
           WHERE audit.receipt_id = $4), \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_audit_evidence AS evidence \
           WHERE evidence.receipt_id = $4) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, "approved");
    assert_eq!(persisted.1, 2);
    assert_eq!(persisted.2, 0);
    assert!(persisted.3.is_none());
    assert_eq!(persisted.4, 2);
    assert_eq!((persisted.5, persisted.6), (0, 0));
    assert_eq!(
        (persisted.7, persisted.8, persisted.9, persisted.10),
        (0, 0, 0, 0)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn existing_runtime_deployment_blocks_fresh_drift_supersession() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let applied_operation = Operation::new("malformed-runtime-seed");
    complete_apply(&pool, &fixture, &applied_operation).await;
    sqlx::query(
        "UPDATE public.activation_requests SET state = 'approved' \
         WHERE id = $1 AND state = 'applied' AND product_revision = 4",
    )
    .bind(&fixture.activation_id)
    .execute(&pool)
    .await
    .unwrap();
    let (bindings, fingerprint) = authority_binding_material(&pool, &fixture).await;
    let authority = advance_authority(
        &pool,
        &fixture,
        AuthorityAdvance {
            binding_revision: 1,
            resource_bindings: &bindings,
            binding_fingerprint: &fingerprint,
            policy_revision: 2,
            required_approvals: 1,
            activation_ttl_seconds: 3_600,
        },
    )
    .await;
    let operation = Operation::new("malformed-runtime-drift");
    let context = apply_context_at_authority(&fixture, &operation, &authority);
    let mut call = Call::valid(&fixture);
    call.expected_revision = 4;
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply_with_context(&mut transaction, &fixture, &operation, &call, &context)
        .await
        .unwrap();
    assert_eq!(locked.outcome, "indeterminate");
    assert!(!locked.exact_replay);
    assert!(!locked.requires_commit);
    transaction.commit().await.unwrap();

    let unchanged = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
           WHERE deployment.activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
           WHERE receipt.receipt_id = $2) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unchanged, ("approved".to_string(), 4, 1, 0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn supersession_rolls_back_as_one_atomic_unit() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    set_competing_active_baseline(&pool, &fixture).await;
    let operation = Operation::new("superseded-rollback");
    let mut transaction = begin_serializable(&pool).await;
    let locked = lock_apply(
        &mut transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
    )
    .await
    .unwrap();
    assert_eq!(locked.outcome, "superseded");
    assert!(!locked.exact_replay);
    assert!(locked.requires_commit);
    transaction.rollback().await.unwrap();

    let rolled_back = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, activation.apply_attempt_no, \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
           WHERE deployment.activation_request_id = activation.id), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
           WHERE receipt.receipt_id = $2), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
           WHERE audit.receipt_id = $2), \
          (SELECT pg_catalog.count(*) \
           FROM public.product_action_receipt_audit_evidence AS evidence \
           WHERE evidence.receipt_id = $2) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rolled_back, ("approved".to_string(), 2, 0, 0, 0, 0, 0));
    let active_version = sqlx::query_scalar::<_, i64>(
        "SELECT active_version FROM public.automation_ruleset_activations \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_version, 2);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn apply_key_rotation_requires_coverage_and_promotes_new_alias() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("key-rotation");
    let prepared = complete_apply(&pool, &fixture, &operation).await;
    let new_digest = digest(&format!("idempotency:v2:{}", operation.request_id));
    let new_key_id = "apply-key-v2".to_string();
    let new_key_fingerprint = digest("apply-key-v2-material");
    let mut new_only = ApplyLockContext::single(&fixture, &operation);
    new_only.active_idempotency_digest = new_digest.clone();
    new_only.idempotency_candidates = vec![new_digest.clone()];
    new_only.candidate_key_ids = vec![new_key_id.clone()];
    new_only.candidate_key_fingerprints = vec![new_key_fingerprint.clone()];
    new_only.active_key_id = new_key_id.clone();

    let mut incomplete_transaction = begin_serializable(&pool).await;
    let incomplete = lock_apply_with_context(
        &mut incomplete_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &new_only,
    )
    .await
    .unwrap();
    assert_eq!(incomplete.outcome, "idempotency_keyring_incomplete");
    assert!(!incomplete.exact_replay);
    assert!(!incomplete.requires_commit);
    incomplete_transaction.rollback().await.unwrap();

    let mut rotating = new_only.clone();
    rotating
        .idempotency_candidates
        .push(operation.idempotency_digest.clone());
    rotating.candidate_key_ids.push(operation.key_id.clone());
    rotating
        .candidate_key_fingerprints
        .push(operation.key_fingerprint.clone());
    let mut rotation_transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut rotation_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &rotating,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    rotation_transaction.commit().await.unwrap();

    let aliases = sqlx::query_as::<_, (i64, i64)>(
        "SELECT \
          pg_catalog.count(*), \
          pg_catalog.count(*) FILTER (WHERE idempotency_key_digest = $5 \
           AND idempotency_digest_key_id = $6 \
           AND idempotency_digest_key_fingerprint = $7) \
         FROM public.product_action_receipt_idempotency_aliases \
         WHERE tenant_id = $1 AND installation_id = $2 AND principal_id = $3 \
          AND endpoint_domain = 'product_apply_v1' AND receipt_id = $4",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.actor.principal_id)
    .bind(&operation.receipt_id)
    .bind(&new_digest)
    .bind(&new_key_id)
    .bind(&new_key_fingerprint)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(aliases, (2, 1));

    let mut new_only_transaction = begin_serializable(&pool).await;
    let replay = lock_apply_with_context(
        &mut new_only_transaction,
        &fixture,
        &operation,
        &Call::valid(&fixture),
        &new_only,
    )
    .await
    .unwrap();
    assert_eq!(replay.outcome, "ok");
    assert!(replay.exact_replay);
    assert!(replay.requires_commit);
    assert_eq!(
        replay.desired_target_digest.as_deref(),
        Some(prepared.desired_target_digest())
    );
    new_only_transaction.commit().await.unwrap();

    let retained = sqlx::query_as::<_, (i32, i32, bool)>(
        "SELECT deleted_receipts, deleted_aliases, backlog_remaining \
         FROM public.starring_purge_product_action_receipts_v1(1)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retained, (0, 0, false));
    let retained_counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
           WHERE receipt_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
           WHERE receipt_id = $1)",
    )
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retained_counts, (1, 2, 1, 1));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn finalize_failure_rolls_back_pointer_activation_and_runtime() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("rollback");
    let call = Call::valid(&fixture);
    let mut transaction = begin_serializable(&pool).await;
    let lock = lock_apply(&mut transaction, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&lock);
    sqlx::query(
        "INSERT INTO public.product_action_receipts \
         (receipt_id, tenant_id, installation_id, principal_id, endpoint_domain, \
          idempotency_key_digest, request_digest, target_resource_type, target_resource_id, \
          resulting_state, result_code, http_disposition_class) \
         VALUES ($1, $2, $3, $4, 'test_collision_v1', $5, $6, \
          'test_collision', $7, 'collision', 'collision', 4)",
    )
    .bind(&operation.receipt_id)
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.actor.principal_id)
    .bind(digest("rollback-collision-idempotency"))
    .bind(digest("rollback-collision-request"))
    .bind(&fixture.promotion_id)
    .execute(&pool)
    .await
    .unwrap();
    let error = finalize_apply(
        &mut transaction,
        &fixture,
        &operation,
        &call,
        &lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            prepared.snapshot_json(),
        ),
    )
    .await
    .expect_err("receipt collision must abort the atomic finalizer");
    assert!(
        is_serialization_failure(&error)
            || matches!(
                &error,
                sqlx::Error::Database(database)
                    if database.code().as_deref() == Some("23505")
            )
    );
    transaction.rollback().await.unwrap();
    let rolled_back = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
          (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations \
           WHERE guild_id = $2 AND ruleset_key = $3), \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = activation.id) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rolled_back, ("approved".to_string(), 2, 0, 0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn stale_revision_authority_session_and_capability_fail_closed() {
    let pool = pool().await;

    let revision_fixture = seed_fixture(&pool).await;
    let operation = Operation::new("stale-revision");
    let mut revision_call = Call::valid(&revision_fixture);
    revision_call.expected_revision = 1;
    let mut transaction = begin_serializable(&pool).await;
    let revision = lock_apply(
        &mut transaction,
        &revision_fixture,
        &operation,
        &revision_call,
    )
    .await
    .unwrap();
    assert_eq!(revision.outcome, "revision_conflict");
    transaction.rollback().await.unwrap();

    let capability_fixture = seed_fixture(&pool).await;
    let mut capability_call = Call::valid(&capability_fixture);
    capability_call.effective_permissions = "0".to_string();
    let mut transaction = begin_serializable(&pool).await;
    let capability = lock_apply(
        &mut transaction,
        &capability_fixture,
        &Operation::new("capability"),
        &capability_call,
    )
    .await
    .unwrap();
    assert_eq!(capability.outcome, "invalid_input");
    transaction.rollback().await.unwrap();

    let observation_fixture = seed_fixture(&pool).await;
    let mut observation_call = Call::valid(&observation_fixture);
    observation_call.observed_at = Utc::now() - TimeDelta::seconds(10);
    observation_call.expires_at = observation_call.observed_at + TimeDelta::seconds(5);
    let mut transaction = begin_serializable(&pool).await;
    let observation = lock_apply(
        &mut transaction,
        &observation_fixture,
        &Operation::new("observation"),
        &observation_call,
    )
    .await
    .unwrap();
    assert_eq!(observation.outcome, "authorization_stale");
    transaction.rollback().await.unwrap();

    let authority_fixture = seed_fixture(&pool).await;
    let mut wrong_authority = authority_fixture.clone();
    wrong_authority.authority_digest = digest("wrong-authority");
    let mut transaction = begin_serializable(&pool).await;
    let authority = lock_apply(
        &mut transaction,
        &wrong_authority,
        &Operation::new("authority"),
        &Call::valid(&wrong_authority),
    )
    .await
    .unwrap();
    assert_eq!(authority.outcome, "authority_mismatch");
    transaction.rollback().await.unwrap();

    let session_fixture = seed_fixture(&pool).await;
    sqlx::query(
        "UPDATE public.product_auth_sessions \
         SET revoked_at = pg_catalog.clock_timestamp(), revocation_reason = 'security_test' \
         WHERE session_digest = $1",
    )
    .bind(&session_fixture.actor.session_digest)
    .execute(&pool)
    .await
    .unwrap();
    let mut transaction = begin_serializable(&pool).await;
    let session = lock_apply(
        &mut transaction,
        &session_fixture,
        &Operation::new("session"),
        &Call::valid(&session_fixture),
    )
    .await
    .unwrap();
    assert_eq!(session.outcome, "authorization_stale");
    transaction.rollback().await.unwrap();
}

async fn apply_with_serializable_retry(
    pool: PgPool,
    fixture: Fixture,
    operation: Operation,
) -> Result<bool, sqlx::Error> {
    for _ in 0..4 {
        let mut transaction = begin_serializable(&pool).await;
        let call = Call::valid(&fixture);
        let lock = match lock_apply(&mut transaction, &fixture, &operation, &call).await {
            Ok(lock) => lock,
            Err(error) if is_serialization_failure(&error) => {
                transaction.rollback().await.ok();
                continue;
            }
            Err(error) => return Err(error),
        };
        if lock.exact_replay {
            match transaction.commit().await {
                Ok(()) => return Ok(false),
                Err(error) if is_serialization_failure(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        assert_eq!(lock.outcome, "ready");
        let prepared = prepare_requested_deployment(&lock);
        let finalized = match finalize_apply(
            &mut transaction,
            &fixture,
            &operation,
            &call,
            &lock,
            finalize_projection(
                prepared.desired_target_digest(),
                prepared.previous_runtime_json(),
                prepared.snapshot_json(),
            ),
        )
        .await
        {
            Ok(finalized) => finalized,
            Err(error) if is_serialization_failure(&error) => {
                transaction.rollback().await.ok();
                continue;
            }
            Err(error) => return Err(error),
        };
        assert_eq!(finalized.outcome, "ok");
        match transaction.commit().await {
            Ok(()) => return Ok(true),
            Err(error) if is_serialization_failure(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    panic!("serializable apply did not converge within its bounded retry budget")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn concurrent_same_apply_converges_to_one_deployment_and_one_receipt() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let operation = Operation::new("concurrent");
    let first = tokio::spawn(apply_with_serializable_retry(
        pool.clone(),
        fixture.clone(),
        operation.clone(),
    ));
    let second = tokio::spawn(apply_with_serializable_retry(
        pool.clone(),
        fixture.clone(),
        operation.clone(),
    ));
    let (first, second) = tokio::join!(first, second);
    let first = first.unwrap().unwrap();
    let second = second.unwrap().unwrap();
    assert_ne!(first, second);
    let counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
          (SELECT pg_catalog.count(*) FROM public.runtime_deployments \
           WHERE activation_request_id = $1), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
           WHERE receipt_id = $2 AND endpoint_domain = 'product_apply_v1'), \
          (SELECT pg_catalog.count(*) FROM public.product_audit_events \
           WHERE receipt_id = $2 AND action = 'promotion.apply'), \
          (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
           WHERE receipt_id = $2 AND action = 'promotion.apply')",
    )
    .bind(&fixture.activation_id)
    .bind(&operation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 1, 1));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn deferred_invariant_and_security_contract_reject_bypass() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let mut bypass = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *bypass)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'applied', applied_at = pg_catalog.clock_timestamp(), applied_by = $2, \
          completion_kind = 'already_active', activation_notices = '[]'::JSONB, \
          product_revision = product_revision + 1 \
         WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.actor.user_id)
    .execute(&mut *bypass)
    .await
    .unwrap();
    let bypass_error = bypass
        .commit()
        .await
        .expect_err("Applied without an exact runtime deployment must fail at commit");
    let sqlx::Error::Database(database) = bypass_error else {
        panic!("expected deferred invariant database error");
    };
    assert_eq!(
        database.constraint(),
        Some("atomic_product_apply_runtime_request_exact")
    );
    let state = sqlx::query_as::<_, (String, i64)>(
        "SELECT state, product_revision FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, ("approved".to_string(), 2));

    let function_security = sqlx::query_as::<_, (bool, bool, bool, bool)>(
        "SELECT \
          lock_function.prosecdef, \
          lock_function.proconfig @> ARRAY['search_path=pg_catalog']::TEXT[], \
          finalize_function.prosecdef, \
          finalize_function.proconfig @> ARRAY['search_path=pg_catalog']::TEXT[] \
         FROM pg_catalog.pg_proc AS lock_function \
         CROSS JOIN pg_catalog.pg_proc AS finalize_function \
         WHERE lock_function.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)') \
          AND finalize_function.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(function_security, (true, true, true, true));
    let public_execute = sqlx::query_as::<_, (bool, bool)>(
        "SELECT \
          EXISTS (SELECT 1 FROM pg_catalog.aclexplode(lock_function.proacl) AS privilege \
           WHERE privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE'), \
          EXISTS (SELECT 1 FROM pg_catalog.aclexplode(finalize_function.proacl) AS privilege \
           WHERE privilege.grantee = 0 AND privilege.privilege_type = 'EXECUTE') \
         FROM pg_catalog.pg_proc AS lock_function \
         CROSS JOIN pg_catalog.pg_proc AS finalize_function \
         WHERE lock_function.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)') \
          AND finalize_function.oid = pg_catalog.to_regprocedure(\
          'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(public_execute, (false, false));

    let operation = Operation::new("isolation");
    let call = Call::valid(&fixture);
    let mut read_committed = pool.begin().await.unwrap();
    let isolation = lock_apply(&mut read_committed, &fixture, &operation, &call)
        .await
        .unwrap();
    assert_eq!(isolation.outcome, "invalid_input");
    read_committed.rollback().await.unwrap();

    let oversized_operation = Operation::new("oversized");
    let oversized_call = Call::valid(&fixture);
    let mut oversized_transaction = begin_serializable(&pool).await;
    let oversized_lock = lock_apply(
        &mut oversized_transaction,
        &fixture,
        &oversized_operation,
        &oversized_call,
    )
    .await
    .unwrap();
    assert_eq!(oversized_lock.outcome, "ready");
    let prepared = prepare_requested_deployment(&oversized_lock);
    let mut oversized_snapshot = prepared.snapshot_json().clone();
    oversized_snapshot["oversized"] = Value::String("x".repeat(300_000));
    let oversized = finalize_apply(
        &mut oversized_transaction,
        &fixture,
        &oversized_operation,
        &oversized_call,
        &oversized_lock,
        finalize_projection(
            prepared.desired_target_digest(),
            prepared.previous_runtime_json(),
            &oversized_snapshot,
        ),
    )
    .await
    .unwrap();
    assert_eq!(oversized.outcome, "invalid_runtime_projection");
    oversized_transaction.commit().await.unwrap();
    let unchanged = sqlx::query_as::<_, (String, i64)>(
        "SELECT state, product_revision FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unchanged, ("approved".to_string(), 2));

    let null_rows = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) \
         FROM public.starring_product_apply_lock_v1(\
          NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::BIGINT, NULL::TEXT, NULL::TEXT, \
          NULL::BYTEA, NULL::BYTEA, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::TEXT, \
          NULL::BIGINT, NULL::TEXT, NULL::TEXT, NULL::TIMESTAMPTZ, NULL::TIMESTAMPTZ, \
          NULL::TEXT, NULL::BOOLEAN, NULL::TEXT, NULL::TEXT, NULL::TEXT[], NULL::TEXT[], \
          NULL::TEXT[], NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::TEXT, NULL::TEXT)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(null_rows, 0);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn migration_preflight_refuses_ambiguous_applied_history() {
    let database = isolated_database("upgrade").await;
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_007)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        let fixture = seed_fixture(&database.pool).await;
        sqlx::query(
            "UPDATE public.activation_requests \
             SET state = 'applied', applied_at = pg_catalog.clock_timestamp(), applied_by = $2, \
              completion_kind = 'already_active', activation_notices = '[]'::JSONB, \
              product_revision = product_revision + 1 \
             WHERE id = $1",
        )
        .bind(&fixture.activation_id)
        .bind(&fixture.actor.user_id)
        .execute(&database.pool)
        .await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_008)
            .unwrap();
        let error = sqlx::raw_sql(migration.sql.as_ref())
            .execute(&database.pool)
            .await
            .expect_err("migration must reject Applied history without a deployment");
        let sqlx::Error::Database(database_error) = error else {
            panic!("expected migration preflight database error");
        };
        assert_eq!(
            database_error.constraint(),
            Some("atomic_product_apply_upgrade_deployment_complete")
        );
        Ok::<_, sqlx::Error>(())
    }
    .await;
    drop_isolated_database(database).await;
    outcome.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn migration_moves_explicit_apply_execute_privileges_to_the_wrapper() {
    let database = isolated_database("acl").await;
    let migration_role = format!("starring_apply_migrator_{}", suffix());
    let owner = sqlx::query_scalar::<_, String>("SELECT current_user")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    let quoted_owner = sqlx::query_scalar::<_, String>("SELECT pg_catalog.quote_ident($1)")
        .bind(&owner)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE ROLE {migration_role} NOLOGIN"))
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query(&format!("GRANT {quoted_owner} TO {migration_role}"))
        .execute(&database.pool)
        .await
        .unwrap();
    let outcome = async {
        for migration in MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 202_607_190_009)
        {
            sqlx::raw_sql(migration.sql.as_ref())
                .execute(&database.pool)
                .await?;
        }
        sqlx::raw_sql(
            "GRANT EXECUTE ON FUNCTION public.starring_product_apply_lock_v1(\
             text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,\
             timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,\
             text,text,text) TO pg_read_all_data WITH GRANT OPTION",
        )
        .execute(&database.pool)
        .await?;
        let mut delegated_grant = database.pool.begin().await?;
        sqlx::query("SET LOCAL ROLE pg_read_all_data")
            .execute(&mut *delegated_grant)
            .await?;
        sqlx::raw_sql(
            "GRANT EXECUTE ON FUNCTION public.starring_product_apply_lock_v1(\
             text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,\
             timestamptz,timestamptz,text,boolean,text,text,text[],text[],text[],text,text,text,\
             text,text,text) TO pg_write_all_data",
        )
        .execute(&mut *delegated_grant)
        .await?;
        delegated_grant.commit().await?;
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 202_607_190_010)
            .unwrap();
        let mut migration_transaction = database.pool.begin().await?;
        sqlx::query(&format!("SET LOCAL ROLE {migration_role}"))
            .execute(&mut *migration_transaction)
            .await?;
        sqlx::raw_sql(migration.sql.as_ref())
            .execute(&mut *migration_transaction)
            .await?;
        migration_transaction.commit().await?;
        let privileges = sqlx::query_as::<_, (bool, bool, bool, bool, bool, bool, bool, bool)>(
            "SELECT \
              pg_catalog.has_function_privilege(\
               'pg_read_all_data', \
               'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                text,text,text[],text[],text[],text,text,text,text,text,text)', 'EXECUTE'), \
              pg_catalog.has_function_privilege(\
               'pg_read_all_data', \
               'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,\
                bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                text,text,text[],text[],text[],text,text,text,text,text,text)', 'EXECUTE'), \
              EXISTS (\
               SELECT 1 FROM pg_catalog.pg_proc AS function_row \
               CROSS JOIN LATERAL pg_catalog.aclexplode(function_row.proacl) AS privilege \
               INNER JOIN pg_catalog.pg_roles AS role ON role.oid = privilege.grantee \
               WHERE function_row.oid = pg_catalog.to_regprocedure(\
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                 bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                 text,text,text[],text[],text[],text,text,text,text,text,text)') \
                AND role.rolname = 'pg_read_all_data' \
                AND privilege.privilege_type = 'EXECUTE' \
                AND privilege.is_grantable), \
              EXISTS (\
               SELECT 1 FROM pg_catalog.pg_proc AS function_row \
               CROSS JOIN LATERAL pg_catalog.aclexplode(function_row.proacl) AS privilege \
               WHERE function_row.oid = pg_catalog.to_regprocedure(\
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                 bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                 text,text,text[],text[],text[],text,text,text,text,text,text)') \
                AND privilege.grantee = 0 \
                AND privilege.privilege_type = 'EXECUTE'), \
              EXISTS (\
               SELECT 1 FROM pg_catalog.pg_proc AS function_row \
               CROSS JOIN LATERAL pg_catalog.aclexplode(function_row.proacl) AS privilege \
               WHERE function_row.oid = pg_catalog.to_regprocedure(\
                'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,\
                 bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                 text,text,text[],text[],text[],text,text,text,text,text,text)') \
                AND privilege.grantee = 0 \
                AND privilege.privilege_type = 'EXECUTE'), \
              wrapper.proowner = core.proowner, \
              wrapper.proowner = finalizer.proowner, \
              wrapper.proowner <> migration_role.oid \
             FROM pg_catalog.pg_proc AS wrapper \
             CROSS JOIN pg_catalog.pg_proc AS core \
             CROSS JOIN pg_catalog.pg_proc AS finalizer \
             CROSS JOIN pg_catalog.pg_roles AS migration_role \
             WHERE wrapper.oid = pg_catalog.to_regprocedure(\
              'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
               bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
               text,text,text[],text[],text[],text,text,text,text,text,text)') \
              AND core.oid = pg_catalog.to_regprocedure(\
              'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,\
               bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
               text,text,text[],text[],text[],text,text,text,text,text,text)') \
              AND finalizer.oid = pg_catalog.to_regprocedure(\
              'public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,\
               bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
               text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)') \
              AND migration_role.rolname = $1",
        )
        .bind(&migration_role)
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(
            privileges,
            (true, false, true, false, false, true, true, true)
        );
        let delegated = sqlx::query_as::<_, (bool, bool, bool)>(
            "SELECT \
              pg_catalog.has_function_privilege(\
               'pg_write_all_data', \
               'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                text,text,text[],text[],text[],text,text,text,text,text,text)', 'EXECUTE'), \
              pg_catalog.has_function_privilege(\
               'pg_write_all_data', \
               'public.starring_product_apply_lock_core_v1(text,text,text,bigint,text,text,bytea,\
                bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                text,text,text[],text[],text[],text,text,text,text,text,text)', 'EXECUTE'), \
              EXISTS (\
               SELECT 1 FROM pg_catalog.pg_proc AS function_row \
               CROSS JOIN LATERAL pg_catalog.aclexplode(function_row.proacl) AS privilege \
               INNER JOIN pg_catalog.pg_roles AS role ON role.oid = privilege.grantee \
               WHERE function_row.oid = pg_catalog.to_regprocedure(\
                'public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,\
                 bytea,text,text,text,text,bigint,text,text,timestamptz,timestamptz,text,boolean,\
                 text,text,text[],text[],text[],text,text,text,text,text,text)') \
                AND role.rolname = 'pg_write_all_data' \
                AND privilege.privilege_type = 'EXECUTE' \
                AND privilege.is_grantable)",
        )
        .fetch_one(&database.pool)
        .await?;
        assert_eq!(delegated, (true, false, false));
        let fixture = seed_fixture(&database.pool).await;
        let operation = Operation::new("acl-nested-core");
        let mut transaction = begin_serializable(&database.pool).await;
        sqlx::query("SET LOCAL ROLE pg_read_all_data")
            .execute(&mut *transaction)
            .await?;
        let locked = lock_apply(
            &mut transaction,
            &fixture,
            &operation,
            &Call::valid(&fixture),
        )
        .await?;
        assert_eq!(locked.outcome, "ready");
        transaction.rollback().await?;
        Ok::<_, sqlx::Error>(())
    }
    .await;
    database.pool.close().await;
    let mut administrator = database.administrator;
    sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", database.name))
        .execute(&mut administrator)
        .await
        .unwrap();
    sqlx::query(&format!("REVOKE {quoted_owner} FROM {migration_role}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {migration_role}"))
        .execute(&mut administrator)
        .await
        .unwrap();
    outcome.unwrap();
}
