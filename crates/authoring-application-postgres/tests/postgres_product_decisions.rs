use std::num::NonZeroU64;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use authoring_application_postgres::MIGRATOR;
use chrono::{DateTime, TimeDelta, Utc};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId};
use resource_resolution::{
    approval_binding_fingerprint_v1, resource_binding_fingerprint_v2, ResolvedApprovalBinding,
    ResourceBindingMap,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use sqlx::types::Json;

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
        .max_connections(8)
        .connect(&database_url())
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
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
struct ActorFixture {
    principal_id: String,
    user_id: String,
    observation_digest: String,
    session_digest: Vec<u8>,
    session_subject: Vec<u8>,
    csrf_digest: Vec<u8>,
    oauth_state: Vec<u8>,
    oauth_nonce: Vec<u8>,
}

impl ActorFixture {
    fn new(suffix: &str, label: &str, user_id: String) -> Self {
        Self {
            principal_id: format!("{label}-{suffix}"),
            user_id,
            observation_digest: digest(&format!("observation:{label}:{suffix}")),
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
    requester: ActorFixture,
    first_approver: ActorFixture,
    second_approver: ActorFixture,
    application_id: String,
    guild_id: String,
    ruleset_key: String,
    payload_digest: String,
    authority_digest: String,
    authority_binding_fingerprint: String,
    approval_binding_fingerprint: String,
    policy_digest: String,
    required_approvals: i32,
}

async fn seed_fixture(pool: &PgPool, required_approvals: i32) -> Fixture {
    let suffix = suffix();
    let tail = suffix[suffix.len().saturating_sub(9)..]
        .parse::<u64>()
        .unwrap();
    let tenant_id = format!("tenant-{suffix}");
    let installation_id = format!("installation-{suffix}");
    let promotion_id = digest(&format!("promotion:{suffix}"));
    let activation_id = format!("activation_{suffix}");
    let requester = ActorFixture::new(&suffix, "requester", (1_000_000_000 + tail).to_string());
    let first_approver = ActorFixture::new(
        &suffix,
        "approver-first",
        (2_000_000_000 + tail).to_string(),
    );
    let second_approver = ActorFixture::new(
        &suffix,
        "approver-second",
        (2_500_000_000 + tail).to_string(),
    );
    let application_id = (3_000_000_000 + tail).to_string();
    let guild_id = (4_000_000_000 + tail).to_string();
    let ruleset_key = format!("ruleset_{}", &suffix[suffix.len().saturating_sub(20)..]);
    let request_digest = digest(&format!("request:{suffix}"));
    let payload_digest = digest(&format!("payload:{suffix}"));
    let target_content_hash = digest(&format!("content:{suffix}"));
    let context_digest = digest(&format!("context:{suffix}"));
    let guild = GuildId(guild_id.parse::<u64>().unwrap());
    let channel_id = ChannelId(guild.0 + 1);
    let binding_key = ResourceKey("community_hub".to_string());
    let mut resource_bindings = ResourceBindingMap::default();
    resource_bindings
        .channel_bindings
        .insert(binding_key.clone(), channel_id);
    let authority_binding_fingerprint = resource_binding_fingerprint_v2(&resource_bindings);
    let required_bindings = vec![ResolvedApprovalBinding::Channel {
        key: binding_key,
        id: channel_id,
    }];
    let approval_binding_fingerprint =
        approval_binding_fingerprint_v1(guild, NonZeroU64::new(1).unwrap(), &required_bindings)
            .unwrap();
    assert_ne!(
        authority_binding_fingerprint.as_str(),
        approval_binding_fingerprint.as_str()
    );
    let stored_resource_bindings = json!({
        "role_bindings": {},
        "channel_bindings": {"community_hub": channel_id.to_string()}
    });
    let policy_digest = digest(&format!("policy:{suffix}"));
    let authority_digest = digest(&format!("authority:{suffix}"));
    let approval_context = json!({
        "authority": "product_authoring",
        "context": {
            "promotion_id": promotion_id,
            "promotion_request_digest": request_digest,
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
                "required_approvals": required_approvals,
                "ttl_seconds": 3600,
                "digest": policy_digest
            }
        }
    });
    let database_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(pool)
            .await
            .unwrap();
    let created_at = database_now - TimeDelta::minutes(1);
    let expires_at = database_now + TimeDelta::minutes(59);
    let linked_at = database_now - TimeDelta::seconds(30);
    let promotion_record = json!({
        "id": promotion_id,
        "revision": 3,
        "request_digest": request_digest,
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
                "required_approvals": required_approvals,
                "created_at": created_at,
                "expires_at": expires_at,
                "request_state_at_journal": "pending",
                "approval_context": approval_context["context"]
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
         VALUES ($1, $2, '{}'::JSONB), ($3, $4, '{}'::JSONB), \
         ($5, $6, '{}'::JSONB)",
    )
    .bind(&requester.principal_id)
    .bind(&requester.user_id)
    .bind(&first_approver.principal_id)
    .bind(&first_approver.user_id)
    .bind(&second_approver.principal_id)
    .bind(&second_approver.user_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    for actor in [&requester, &first_approver, &second_approver] {
        sqlx::query(
            "INSERT INTO public.product_oauth_flows \
             (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, \
              expires_at, consumed_at, terminal_result_code) \
             VALUES ($1, $2, 'https://starring.example/oauth/discord/callback', '/', \
              CURRENT_TIMESTAMP - INTERVAL '1 minute', CURRENT_TIMESTAMP + INTERVAL '5 minutes', \
              CURRENT_TIMESTAMP - INTERVAL '1 second', 'callback_claimed')",
        )
        .bind(&actor.oauth_state)
        .bind(&actor.oauth_nonce)
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
        .bind(&actor.session_digest)
        .bind(&actor.principal_id)
        .bind(&actor.csrf_digest)
        .bind(&actor.oauth_state)
        .execute(&mut *transaction)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) VALUES ($1, 'active', $2)",
    )
    .bind(&tenant_id)
    .bind(format!("Tenant {suffix}"))
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
         VALUES ($1, 1, $2, 1, $3, $4, 1, $5, 3600, $6, $7, $8)",
    )
    .bind(&installation_id)
    .bind(&tenant_id)
    .bind(Json(&stored_resource_bindings))
    .bind(authority_binding_fingerprint.as_str())
    .bind(required_approvals)
    .bind(&authority_digest)
    .bind(&requester.principal_id)
    .bind(digest(&format!("authority-request:{suffix}")))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, principal_id, \
          record) VALUES ($1, 1, 3, 'activation_pending', $2, $3, $4, $5)",
    )
    .bind(&promotion_id)
    .bind(&request_digest)
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
         VALUES ($1, $2, $3, 1, $4, $5, $6, 'pending', $7, $8, \
          'product_authoring', 'linked', $9, $10, \
          $11, $12, $13, $14, $15)",
    )
    .bind(&activation_id)
    .bind(&guild_id)
    .bind(&ruleset_key)
    .bind(&target_content_hash)
    .bind(&requester.user_id)
    .bind(required_approvals)
    .bind(created_at)
    .bind(expires_at)
    .bind(Json(&approval_context))
    .bind(Json(json!({"state": "linked", "linked_at": linked_at})))
    .bind(&promotion_id)
    .bind(&request_digest)
    .bind(&payload_digest)
    .bind(&context_digest)
    .bind(linked_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    Fixture {
        tenant_id,
        installation_id,
        promotion_id,
        activation_id,
        requester,
        first_approver,
        second_approver,
        application_id,
        guild_id,
        ruleset_key,
        payload_digest,
        authority_digest,
        authority_binding_fingerprint: authority_binding_fingerprint.into_string(),
        approval_binding_fingerprint: approval_binding_fingerprint.to_string(),
        policy_digest,
        required_approvals,
    }
}

async fn seed_additional_target(pool: &PgPool, fixture: &Fixture, label: &str) -> Fixture {
    let suffix = suffix();
    let promotion_id = digest(&format!("promotion:{label}:{suffix}"));
    let activation_id = format!("activation_{label}_{suffix}");
    let request_digest = digest(&format!("request:{label}:{suffix}"));
    let payload_digest = digest(&format!("payload:{label}:{suffix}"));
    let target_content_hash = digest(&format!("content:{label}:{suffix}"));
    let context_digest = digest(&format!("context:{label}:{suffix}"));
    let database_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(pool)
            .await
            .unwrap();
    let created_at = database_now - TimeDelta::minutes(1);
    let expires_at = database_now + TimeDelta::minutes(59);
    let linked_at = database_now - TimeDelta::seconds(30);
    let approval_context = json!({
        "authority": "product_authoring",
        "context": {
            "promotion_id": promotion_id,
            "promotion_request_digest": request_digest,
            "approval_payload_digest": payload_digest,
            "approval_context_digest": context_digest,
            "binding": {
                "revision": 1,
                "fingerprint": fixture.approval_binding_fingerprint,
                "required_bindings": [{
                    "kind": "channel",
                    "key": "community_hub",
                    "id": (fixture.guild_id.parse::<u64>().unwrap() + 1).to_string()
                }]
            },
            "baseline": {"state": "absent"},
            "policy": {
                "revision": 1,
                "required_approvals": fixture.required_approvals,
                "ttl_seconds": 3600,
                "digest": fixture.policy_digest
            }
        }
    });
    let promotion_record = json!({
        "id": promotion_id,
        "revision": 3,
        "request_digest": request_digest,
        "intent": {
            "authority": {
                "tenant_id": fixture.tenant_id,
                "principal_id": fixture.requester.principal_id,
                "installation_id": fixture.installation_id,
                "guild_id": fixture.guild_id,
                "ruleset_key": fixture.ruleset_key,
                "binding_revision": 1
            },
            "evidence": {
                "context_fingerprint": fixture.authority_binding_fingerprint
            }
        },
        "stage": {
            "state": "activation_pending",
            "activation": {
                "request_id": activation_id,
                "target": {
                    "guild_id": fixture.guild_id,
                    "ruleset_key": fixture.ruleset_key,
                    "version": 1,
                    "content_hash": target_content_hash
                },
                "requester": fixture.requester.user_id,
                "required_approvals": fixture.required_approvals,
                "created_at": created_at,
                "expires_at": expires_at,
                "request_state_at_journal": "pending",
                "approval_context": approval_context["context"]
            }
        }
    });
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, principal_id, \
          record) VALUES ($1, 1, 3, 'activation_pending', $2, $3, $4, $5)",
    )
    .bind(&promotion_id)
    .bind(&request_digest)
    .bind(&fixture.tenant_id)
    .bind(&fixture.requester.principal_id)
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
         VALUES ($1, $2, $3, 1, $4, $5, $6, 'pending', $7, $8, \
          'product_authoring', 'linked', $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(&activation_id)
    .bind(&fixture.guild_id)
    .bind(&fixture.ruleset_key)
    .bind(&target_content_hash)
    .bind(&fixture.requester.user_id)
    .bind(fixture.required_approvals)
    .bind(created_at)
    .bind(expires_at)
    .bind(Json(&approval_context))
    .bind(Json(json!({"state": "linked", "linked_at": linked_at})))
    .bind(&promotion_id)
    .bind(&request_digest)
    .bind(&payload_digest)
    .bind(&context_digest)
    .bind(linked_at)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let mut additional = fixture.clone();
    additional.promotion_id = promotion_id;
    additional.activation_id = activation_id;
    additional.payload_digest = payload_digest;
    additional
}

#[derive(Debug, sqlx::FromRow)]
struct ApprovalOutcome {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
}

fn assert_serialization_failure(error: &sqlx::Error) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref(),
        Some("40001")
    );
}

#[derive(Clone)]
struct ApprovalInvocation {
    actor: ActorFixture,
    expected_revision: i64,
    payload_digest: String,
    authority_revision: i64,
    authority_digest: String,
    observed_offset_seconds: i64,
    expires_offset_seconds: i64,
    permission_bits: String,
    guild_owner: bool,
    request_id: Option<String>,
    active_idempotency: String,
    idempotency_candidates: Vec<String>,
    idempotency_candidate_key_ids: Vec<String>,
    idempotency_candidate_key_fingerprints: Vec<String>,
    digest_key_id: String,
    semantic_request: String,
    receipt_id: String,
    audit_id: String,
}

impl ApprovalInvocation {
    fn new(fixture: &Fixture, actor: &ActorFixture, expected_revision: i64, seed: &str) -> Self {
        let namespace = format!("{}:{seed}", fixture.promotion_id);
        let active_idempotency = digest(&format!("{namespace}:idempotency"));
        Self {
            actor: actor.clone(),
            expected_revision,
            payload_digest: fixture.payload_digest.clone(),
            authority_revision: 1,
            authority_digest: fixture.authority_digest.clone(),
            observed_offset_seconds: -1,
            expires_offset_seconds: 3,
            permission_bits: "0".to_string(),
            guild_owner: true,
            request_id: Some(format!("approval.{seed}")),
            idempotency_candidates: vec![active_idempotency.clone()],
            active_idempotency,
            idempotency_candidate_key_ids: vec!["test-key-v1".to_string()],
            idempotency_candidate_key_fingerprints: vec![digest("test-key-v1-material")],
            digest_key_id: "test-key-v1".to_string(),
            semantic_request: digest(&format!("{namespace}:semantic")),
            receipt_id: digest(&format!("{namespace}:receipt")),
            audit_id: digest(&format!("{namespace}:audit")),
        }
    }
}

async fn approve(
    pool: &PgPool,
    fixture: &Fixture,
    invocation: &ApprovalInvocation,
) -> Result<ApprovalOutcome, sqlx::Error> {
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(pool)
        .await
        .unwrap();
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await?;
    let outcome = sqlx::query_as::<_, ApprovalOutcome>(
        "SELECT outcome, resulting_revision, resulting_state, exact_replay \
         FROM public.starring_product_approve_v1(\
         $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'approve', $12, $13, $14, \
         $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27)",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.promotion_id)
    .bind(invocation.expected_revision)
    .bind(&invocation.payload_digest)
    .bind(&invocation.actor.principal_id)
    .bind(&invocation.actor.session_digest)
    .bind(&invocation.actor.session_subject)
    .bind(&invocation.actor.user_id)
    .bind(&fixture.application_id)
    .bind(&fixture.guild_id)
    .bind(invocation.authority_revision)
    .bind(&invocation.authority_digest)
    .bind(&invocation.actor.observation_digest)
    .bind(now + TimeDelta::seconds(invocation.observed_offset_seconds))
    .bind(now + TimeDelta::seconds(invocation.expires_offset_seconds))
    .bind(&invocation.permission_bits)
    .bind(invocation.guild_owner)
    .bind(invocation.request_id.as_deref())
    .bind(&invocation.active_idempotency)
    .bind(&invocation.idempotency_candidates)
    .bind(&invocation.idempotency_candidate_key_ids)
    .bind(&invocation.idempotency_candidate_key_fingerprints)
    .bind(&invocation.digest_key_id)
    .bind(&invocation.semantic_request)
    .bind(&invocation.receipt_id)
    .bind(&invocation.audit_id)
    .fetch_one(&mut *transaction)
    .await;
    match outcome {
        Ok(outcome) if outcome.outcome == "ok" => {
            transaction.commit().await?;
            Ok(outcome)
        }
        Ok(outcome) => {
            transaction.rollback().await?;
            Ok(outcome)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn persisted_counts(pool: &PgPool, fixture: &Fixture) -> (i64, i64, i64, i64, i64, String) {
    sqlx::query_as(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.activation_request_approvals \
          WHERE request_id = $1), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE target_resource_id = $2), \
         (SELECT pg_catalog.count(*) \
          FROM public.product_action_receipt_idempotency_aliases AS alias \
          INNER JOIN public.product_action_receipts AS receipt \
            ON receipt.receipt_id = alias.receipt_id \
          WHERE receipt.target_resource_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events \
          WHERE target_resource_id = $2), \
         product_revision, state \
         FROM public.activation_requests WHERE id = $1",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.promotion_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn alias_count(pool: &PgPool, fixture: &Fixture, idempotency_digest: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT pg_catalog.count(*) \
         FROM public.product_action_receipt_idempotency_aliases \
         WHERE tenant_id = $1 AND installation_id = $2 AND principal_id = $3 \
          AND endpoint_domain = 'product_approve_v1' AND idempotency_key_digest = $4",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.first_approver.principal_id)
    .bind(idempotency_digest)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[derive(Clone)]
struct AuthorityRevisionFixture {
    revision: i64,
    payload_digest: String,
    binding_fingerprint: String,
}

async fn advance_authority(pool: &PgPool, fixture: &Fixture) -> AuthorityRevisionFixture {
    let revision = 2;
    let binding_revision = 2;
    let channel_id = ChannelId(fixture.guild_id.parse::<u64>().unwrap() + 2);
    let binding_key = ResourceKey("community_hub".to_string());
    let mut resource_bindings = ResourceBindingMap::default();
    resource_bindings
        .channel_bindings
        .insert(binding_key, channel_id);
    let binding_fingerprint = resource_binding_fingerprint_v2(&resource_bindings).into_string();
    let stored_resource_bindings = json!({
        "role_bindings": {},
        "channel_bindings": {"community_hub": channel_id.to_string()}
    });
    let payload_digest = digest(&format!("{}:authority:v2", fixture.promotion_id));
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
         VALUES ($1, $2, $3, $4, $5, $6, 1, $7, 3600, $8, $9, $10)",
    )
    .bind(&fixture.installation_id)
    .bind(revision)
    .bind(&fixture.tenant_id)
    .bind(binding_revision)
    .bind(Json(&stored_resource_bindings))
    .bind(&binding_fingerprint)
    .bind(fixture.required_approvals)
    .bind(&payload_digest)
    .bind(&fixture.requester.principal_id)
    .bind(digest(&format!(
        "{}:authority-request:v2",
        fixture.promotion_id
    )))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.automation_installations \
         SET current_authority_revision = $1, updated_at = pg_catalog.clock_timestamp() \
         WHERE tenant_id = $2 AND installation_id = $3",
    )
    .bind(revision)
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    AuthorityRevisionFixture {
        revision,
        payload_digest,
        binding_fingerprint,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn exact_replay_uses_current_authorization_and_preserves_historical_authority() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool, 2).await;
    let first_invocation =
        ApprovalInvocation::new(&fixture, &fixture.first_approver, 1, "authority-v1");
    let first = approve(&pool, &fixture, &first_invocation).await.unwrap();
    assert_eq!(first.outcome, "ok");
    assert_eq!(first.resulting_revision, Some(2));
    assert_eq!(first.resulting_state.as_deref(), Some("pending"));
    assert!(!first.exact_replay);
    let receipt_before = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT pg_catalog.to_jsonb(receipt) \
         FROM public.product_action_receipts AS receipt WHERE receipt_id = $1",
    )
    .bind(&first_invocation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let audit_before = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT pg_catalog.to_jsonb(audit) \
         FROM public.product_audit_events AS audit WHERE receipt_id = $1",
    )
    .bind(&first_invocation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let current_authority = advance_authority(&pool, &fixture).await;
    assert_ne!(
        current_authority.binding_fingerprint,
        fixture.authority_binding_fingerprint
    );
    let mut replay = first_invocation.clone();
    replay.authority_revision = current_authority.revision;
    replay.authority_digest = current_authority.payload_digest.clone();
    replay.actor.observation_digest = digest(&format!(
        "{}:authority-v2-observation",
        fixture.promotion_id
    ));
    replay.request_id = Some("approval.authority-v2-retry".to_string());
    let replayed = approve(&pool, &fixture, &replay).await.unwrap();
    assert_eq!(replayed.outcome, "ok");
    assert_eq!(replayed.resulting_revision, first.resulting_revision);
    assert_eq!(replayed.resulting_state, first.resulting_state);
    assert!(replayed.exact_replay);
    let receipt_after = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT pg_catalog.to_jsonb(receipt) \
         FROM public.product_action_receipts AS receipt WHERE receipt_id = $1",
    )
    .bind(&first_invocation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let audit_after = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT pg_catalog.to_jsonb(audit) \
         FROM public.product_audit_events AS audit WHERE receipt_id = $1",
    )
    .bind(&first_invocation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(receipt_after, receipt_before);
    assert_eq!(audit_after, audit_before);
    let historical_authority = sqlx::query_as::<_, (i64, String)>(
        "SELECT installation_authority_revision, binding_fingerprint \
         FROM public.product_audit_events WHERE receipt_id = $1",
    )
    .bind(&first_invocation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(historical_authority.0, 1);
    assert_eq!(
        historical_authority.1,
        fixture.authority_binding_fingerprint
    );
    assert_ne!(
        historical_authority.1,
        current_authority.binding_fingerprint
    );
    let stale_mutation =
        ApprovalInvocation::new(&fixture, &fixture.second_approver, 2, "stale-authority-v1");
    let rejected = approve(&pool, &fixture, &stale_mutation).await.unwrap();
    assert_eq!(rejected.outcome, "authorization_stale");
    assert_eq!(
        persisted_counts(&pool, &fixture).await,
        (1, 1, 1, 1, 2, "pending".to_string())
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn approval_rechecks_expiring_authority_after_activation_lock_wait() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool, 2).await;
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.activation_requests WHERE id = $1 FOR UPDATE",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    let mut invocation =
        ApprovalInvocation::new(&fixture, &fixture.first_approver, 1, "expiring-authority");
    invocation.expires_offset_seconds = 1;
    let approval_pool = pool.clone();
    let approval_fixture = fixture.clone();
    let approval =
        tokio::spawn(async move { approve(&approval_pool, &approval_fixture, &invocation).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let waiting = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (\
                 SELECT 1 FROM pg_catalog.pg_stat_activity \
                 WHERE datname = pg_catalog.current_database() \
                  AND pid <> pg_catalog.pg_backend_pid() \
                  AND wait_event_type = 'Lock' \
                  AND query LIKE '%starring_product_approve_v1%')",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    blocker.rollback().await.unwrap();
    let rejected = tokio::time::timeout(Duration::from_secs(2), approval)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(rejected.outcome, "authorization_stale");
    assert_eq!(
        persisted_counts(&pool, &fixture).await,
        (0, 0, 0, 0, 1, "pending".to_string())
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn guarded_approval_is_atomic_payload_bound_quorum_aware_and_replayable() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool, 2).await;
    let mut first_invocation =
        ApprovalInvocation::new(&fixture, &fixture.first_approver, 1, "first");
    first_invocation.guild_owner = false;
    first_invocation.permission_bits = "8".to_string();
    let first = approve(&pool, &fixture, &first_invocation).await.unwrap();
    assert_eq!(first.outcome, "ok");
    assert_eq!(first.resulting_revision, Some(2));
    assert_eq!(first.resulting_state.as_deref(), Some("pending"));
    assert!(!first.exact_replay);
    let audit_binding_fingerprint = sqlx::query_scalar::<_, String>(
        "SELECT binding_fingerprint FROM public.product_audit_events WHERE receipt_id = $1",
    )
    .bind(&first_invocation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        audit_binding_fingerprint,
        fixture.authority_binding_fingerprint
    );
    assert_ne!(
        audit_binding_fingerprint,
        fixture.approval_binding_fingerprint
    );
    let retired_idempotency = first_invocation.active_idempotency.clone();
    let mut replay_invocation = first_invocation.clone();
    replay_invocation.request_id = Some("approval.first.retry".to_string());
    replay_invocation.active_idempotency = digest(&format!(
        "{}:first:rotated-idempotency",
        fixture.promotion_id
    ));
    replay_invocation.idempotency_candidates = vec![
        replay_invocation.active_idempotency.clone(),
        retired_idempotency,
    ];
    replay_invocation.idempotency_candidate_key_ids =
        vec!["test-key-v2".to_string(), "test-key-v1".to_string()];
    replay_invocation.idempotency_candidate_key_fingerprints = vec![
        digest("test-key-v2-material"),
        digest("test-key-v1-material"),
    ];
    replay_invocation.digest_key_id = "test-key-v2".to_string();
    replay_invocation.permission_bits = "18446744073709551615".to_string();
    replay_invocation.receipt_id =
        digest(&format!("{}:first:rotated-receipt", fixture.promotion_id));
    replay_invocation.audit_id = digest(&format!("{}:first:rotated-audit", fixture.promotion_id));
    let replay = approve(&pool, &fixture, &replay_invocation).await.unwrap();
    assert_eq!(replay.outcome, "ok");
    assert_eq!(replay.resulting_revision, Some(2));
    assert_eq!(replay.resulting_state.as_deref(), Some("pending"));
    assert!(replay.exact_replay);
    let mut retired_removed = replay_invocation.clone();
    retired_removed.request_id = Some("approval.first.new-key-only".to_string());
    retired_removed.idempotency_candidates = vec![retired_removed.active_idempotency.clone()];
    retired_removed.idempotency_candidate_key_ids = vec!["test-key-v2".to_string()];
    retired_removed.idempotency_candidate_key_fingerprints = vec![digest("test-key-v2-material")];
    let new_key_only_replay = approve(&pool, &fixture, &retired_removed).await.unwrap();
    assert_eq!(new_key_only_replay.outcome, "ok");
    assert!(new_key_only_replay.exact_replay);
    let mut conflict_invocation = replay_invocation.clone();
    conflict_invocation.request_id = Some("approval.first.conflict".to_string());
    conflict_invocation.payload_digest =
        digest(&format!("{}:first:different-payload", fixture.promotion_id));
    conflict_invocation.semantic_request = digest(&format!(
        "{}:first:different-semantic",
        fixture.promotion_id
    ));
    let conflict = approve(&pool, &fixture, &conflict_invocation)
        .await
        .unwrap();
    assert_eq!(conflict.outcome, "idempotency_conflict");
    assert_eq!(
        persisted_counts(&pool, &fixture).await,
        (1, 1, 2, 1, 2, "pending".to_string())
    );
    let mut rolled_back =
        ApprovalInvocation::new(&fixture, &fixture.second_approver, 2, "second-rollback");
    rolled_back.guild_owner = false;
    rolled_back.permission_bits = "32".to_string();
    rolled_back.audit_id = first_invocation.audit_id.clone();
    assert!(approve(&pool, &fixture, &rolled_back).await.is_err());
    assert_eq!(
        persisted_counts(&pool, &fixture).await,
        (1, 1, 2, 1, 2, "pending".to_string())
    );
    let mut second =
        ApprovalInvocation::new(&fixture, &fixture.second_approver, 2, "second-success");
    second.guild_owner = false;
    second.permission_bits = "32".to_string();
    let approved = approve(&pool, &fixture, &second).await.unwrap();
    assert_eq!(approved.outcome, "ok");
    assert_eq!(approved.resulting_revision, Some(3));
    assert_eq!(approved.resulting_state.as_deref(), Some("approved"));
    assert!(!approved.exact_replay);
    assert_eq!(
        persisted_counts(&pool, &fixture).await,
        (2, 2, 3, 2, 3, "approved".to_string())
    );
    let promotion_delete = sqlx::query("DELETE FROM public.authoring_promotions WHERE id = $1")
        .bind(&fixture.promotion_id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert_eq!(
        promotion_delete
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("activation_requests_product_promotion_scope_fk")
    );
    let direct = sqlx::query(
        "INSERT INTO public.activation_request_approvals \
         (request_id, approver_id, approved_at, approval_payload_digest) \
         VALUES ($1, $2, CURRENT_TIMESTAMP, $3)",
    )
    .bind(&fixture.activation_id)
    .bind(&fixture.requester.user_id)
    .bind(&fixture.payload_digest)
    .execute(&pool)
    .await;
    assert!(direct.is_err());
    let update = sqlx::query(
        "UPDATE public.activation_request_approvals SET approved_at = CURRENT_TIMESTAMP \
         WHERE request_id = $1",
    )
    .bind(&fixture.activation_id)
    .execute(&pool)
    .await;
    assert!(update.is_err());
    let alias_update = sqlx::query(
        "UPDATE public.product_action_receipt_idempotency_aliases \
         SET created_at = CURRENT_TIMESTAMP WHERE receipt_id = $1",
    )
    .bind(&first_invocation.receipt_id)
    .execute(&pool)
    .await;
    assert!(alias_update.is_err());
    let missing_audit_receipt = digest(&format!("{}:missing-audit-receipt", fixture.promotion_id));
    let missing_audit_idempotency = digest(&format!(
        "{}:missing-audit-idempotency",
        fixture.promotion_id
    ));
    let missing_audit_request = digest(&format!("{}:missing-audit-request", fixture.promotion_id));
    let mut missing_audit = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.product_action_receipts \
         (receipt_id, tenant_id, installation_id, principal_id, endpoint_domain, \
          idempotency_key_digest, idempotency_digest_key_id, \
          idempotency_digest_key_fingerprint, request_digest, target_resource_type, \
          target_resource_id, resulting_revision, resulting_state, result_code, \
          http_disposition_class) \
         VALUES ($1, $2, $3, $4, 'product_approve_v1', $5, 'test-missing-audit', \
          $6, $7, 'authoring_promotion', $8, 2, 'pending', 'approval_recorded', 2)",
    )
    .bind(&missing_audit_receipt)
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.first_approver.principal_id)
    .bind(&missing_audit_idempotency)
    .bind(digest("test-missing-audit-material"))
    .bind(&missing_audit_request)
    .bind(&fixture.promotion_id)
    .execute(&mut *missing_audit)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_action_receipt_idempotency_aliases \
         (tenant_id, installation_id, principal_id, endpoint_domain, \
          idempotency_key_digest, idempotency_digest_key_id, \
          idempotency_digest_key_fingerprint, receipt_id) \
         VALUES ($1, $2, $3, 'product_approve_v1', $4, 'test-missing-audit', $5, $6)",
    )
    .bind(&fixture.tenant_id)
    .bind(&fixture.installation_id)
    .bind(&fixture.first_approver.principal_id)
    .bind(&missing_audit_idempotency)
    .bind(digest("test-missing-audit-material"))
    .bind(&missing_audit_receipt)
    .execute(&mut *missing_audit)
    .await
    .unwrap();
    let missing_audit_error = missing_audit.commit().await.unwrap_err();
    assert_eq!(
        missing_audit_error
            .as_database_error()
            .map(|error| error.message()),
        Some("product approval receipt is missing its audit event")
    );
    let persisted_subject = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT session_subject_digest FROM public.product_audit_events \
         WHERE receipt_id = $1",
    )
    .bind(&first_invocation.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted_subject, fixture.first_approver.session_subject);
    assert_ne!(persisted_subject, fixture.first_approver.session_digest);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_same_idempotency_performs_exactly_one_mutation() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool, 2).await;
    let mut invocation =
        ApprovalInvocation::new(&fixture, &fixture.first_approver, 1, "concurrent");
    invocation.guild_owner = false;
    invocation.permission_bits = "8".to_string();
    let (left, right) = tokio::join!(
        approve(&pool, &fixture, &invocation),
        approve(&pool, &fixture, &invocation)
    );
    let results = [left, right];
    let mutations = results
        .iter()
        .filter(|result| {
            matches!(result, Ok(outcome) if outcome.outcome == "ok" && !outcome.exact_replay)
        })
        .count();
    assert_eq!(mutations, 1);
    for result in &results {
        match result {
            Ok(outcome) => {
                assert_eq!(outcome.outcome, "ok");
                assert_eq!(outcome.resulting_revision, Some(2));
            }
            Err(error) => assert_serialization_failure(error),
        }
    }
    let retry = approve(&pool, &fixture, &invocation).await.unwrap();
    assert_eq!(retry.outcome, "ok");
    assert!(retry.exact_replay);
    assert_eq!(
        persisted_counts(&pool, &fixture).await,
        (1, 1, 1, 1, 2, "pending".to_string())
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn rolling_keyrings_conflict_across_targets_in_both_orders_and_concurrently() {
    let pool = pool().await;
    let first = seed_fixture(&pool, 2).await;
    let second = seed_additional_target(&pool, &first, "old-first-second").await;
    let old_digest = digest(&format!("{}:rotation-one:old", first.tenant_id));
    let new_digest = digest(&format!("{}:rotation-one:new", first.tenant_id));
    let mut old_first = ApprovalInvocation::new(&first, &first.first_approver, 1, "old-first");
    old_first.active_idempotency = old_digest.clone();
    old_first.idempotency_candidates = vec![old_digest.clone()];
    old_first.idempotency_candidate_key_ids = vec!["rotation-one-old".to_string()];
    old_first.idempotency_candidate_key_fingerprints = vec![digest("rotation-one-old-material")];
    old_first.digest_key_id = "rotation-one-old".to_string();
    assert_eq!(
        approve(&pool, &first, &old_first).await.unwrap().outcome,
        "ok"
    );
    let reused_key_id = seed_additional_target(&pool, &first, "reused-key-id").await;
    let mut reused_key_id_invocation = ApprovalInvocation::new(
        &reused_key_id,
        &reused_key_id.first_approver,
        1,
        "reused-key-id",
    );
    reused_key_id_invocation.active_idempotency =
        digest(&format!("{}:reused-key-id:new-material", first.tenant_id));
    reused_key_id_invocation.idempotency_candidates =
        vec![reused_key_id_invocation.active_idempotency.clone()];
    reused_key_id_invocation.idempotency_candidate_key_ids = vec!["rotation-one-old".to_string()];
    reused_key_id_invocation.idempotency_candidate_key_fingerprints =
        vec![digest("rotation-one-old-reused-material")];
    reused_key_id_invocation.digest_key_id = "rotation-one-old".to_string();
    assert_eq!(
        approve(&pool, &reused_key_id, &reused_key_id_invocation)
            .await
            .unwrap()
            .outcome,
        "idempotency_keyring_incomplete"
    );
    assert_eq!(
        persisted_counts(&pool, &reused_key_id).await,
        (0, 0, 0, 0, 1, "pending".to_string())
    );
    let mut new_second =
        ApprovalInvocation::new(&second, &second.first_approver, 1, "old-first-new-second");
    new_second.active_idempotency = new_digest.clone();
    new_second.idempotency_candidates = vec![new_digest, old_digest];
    new_second.idempotency_candidate_key_ids = vec![
        "rotation-one-new".to_string(),
        "rotation-one-old".to_string(),
    ];
    new_second.idempotency_candidate_key_fingerprints = vec![
        digest("rotation-one-new-material"),
        digest("rotation-one-old-material"),
    ];
    new_second.digest_key_id = "rotation-one-new".to_string();
    assert_eq!(
        approve(&pool, &second, &new_second).await.unwrap().outcome,
        "idempotency_conflict"
    );
    assert_eq!(
        alias_count(
            &pool,
            &first,
            &digest(&format!("{}:rotation-one:new", first.tenant_id))
        )
        .await,
        0
    );
    let uncovered = seed_additional_target(&pool, &first, "uncovered-retired-key").await;
    let mut new_key_only =
        ApprovalInvocation::new(&uncovered, &uncovered.first_approver, 1, "new-key-only");
    new_key_only.active_idempotency = digest(&format!("{}:rotation-one:new", first.tenant_id));
    new_key_only.idempotency_candidates = vec![new_key_only.active_idempotency.clone()];
    new_key_only.idempotency_candidate_key_ids = vec!["rotation-one-new".to_string()];
    new_key_only.idempotency_candidate_key_fingerprints = vec![digest("rotation-one-new-material")];
    new_key_only.digest_key_id = "rotation-one-new".to_string();
    assert_eq!(
        approve(&pool, &uncovered, &new_key_only)
            .await
            .unwrap()
            .outcome,
        "idempotency_keyring_incomplete"
    );
    assert_eq!(
        persisted_counts(&pool, &uncovered).await,
        (0, 0, 0, 0, 1, "pending".to_string())
    );
    assert_eq!(
        persisted_counts(&pool, &second).await,
        (0, 0, 0, 0, 1, "pending".to_string())
    );

    let third = seed_fixture(&pool, 2).await;
    let fourth = seed_additional_target(&pool, &third, "new-first-fourth").await;
    let old_digest = digest(&format!("{}:rotation-two:old", third.tenant_id));
    let new_digest = digest(&format!("{}:rotation-two:new", third.tenant_id));
    let mut new_first = ApprovalInvocation::new(&third, &third.first_approver, 1, "new-first");
    new_first.active_idempotency = old_digest.clone();
    new_first.idempotency_candidates = vec![old_digest.clone(), new_digest.clone()];
    new_first.idempotency_candidate_key_ids = vec![
        "rotation-two-old".to_string(),
        "rotation-two-new".to_string(),
    ];
    new_first.idempotency_candidate_key_fingerprints = vec![
        digest("rotation-two-old-material"),
        digest("rotation-two-new-material"),
    ];
    new_first.digest_key_id = "rotation-two-old".to_string();
    assert_eq!(
        approve(&pool, &third, &new_first).await.unwrap().outcome,
        "ok"
    );
    let mut old_second =
        ApprovalInvocation::new(&fourth, &fourth.first_approver, 1, "new-first-old-second");
    old_second.active_idempotency = new_digest.clone();
    old_second.idempotency_candidates = vec![new_digest, old_digest];
    old_second.idempotency_candidate_key_ids = vec![
        "rotation-two-new".to_string(),
        "rotation-two-old".to_string(),
    ];
    old_second.idempotency_candidate_key_fingerprints = vec![
        digest("rotation-two-new-material"),
        digest("rotation-two-old-material"),
    ];
    old_second.digest_key_id = "rotation-two-new".to_string();
    assert_eq!(
        approve(&pool, &fourth, &old_second).await.unwrap().outcome,
        "idempotency_conflict"
    );
    assert_eq!(
        persisted_counts(&pool, &fourth).await,
        (0, 0, 0, 0, 1, "pending".to_string())
    );

    let ambiguity_first = seed_fixture(&pool, 2).await;
    let ambiguity_second =
        seed_additional_target(&pool, &ambiguity_first, "ambiguity-second").await;
    let ambiguous = seed_additional_target(&pool, &ambiguity_first, "ambiguity-target").await;
    let first_key_one = digest(&format!(
        "{}:ambiguity:first:key-one",
        ambiguity_first.tenant_id
    ));
    let first_key_two = digest(&format!(
        "{}:ambiguity:first:key-two",
        ambiguity_first.tenant_id
    ));
    let mut ambiguity_first_invocation = ApprovalInvocation::new(
        &ambiguity_first,
        &ambiguity_first.first_approver,
        1,
        "ambiguity-first",
    );
    ambiguity_first_invocation.active_idempotency = first_key_one.clone();
    ambiguity_first_invocation.idempotency_candidates = vec![first_key_one.clone(), first_key_two];
    ambiguity_first_invocation.idempotency_candidate_key_ids = vec![
        "coverage-key-one".to_string(),
        "coverage-key-two".to_string(),
    ];
    ambiguity_first_invocation.idempotency_candidate_key_fingerprints = vec![
        digest("coverage-key-one-material"),
        digest("coverage-key-two-material"),
    ];
    ambiguity_first_invocation.digest_key_id = "coverage-key-one".to_string();
    assert_eq!(
        approve(&pool, &ambiguity_first, &ambiguity_first_invocation)
            .await
            .unwrap()
            .outcome,
        "ok"
    );
    let second_key_one = digest(&format!(
        "{}:ambiguity:second:key-one",
        ambiguity_first.tenant_id
    ));
    let second_key_two = digest(&format!(
        "{}:ambiguity:second:key-two",
        ambiguity_first.tenant_id
    ));
    let mut ambiguity_second_invocation = ApprovalInvocation::new(
        &ambiguity_second,
        &ambiguity_second.first_approver,
        1,
        "ambiguity-second",
    );
    ambiguity_second_invocation.active_idempotency = second_key_one;
    ambiguity_second_invocation.idempotency_candidates = vec![
        ambiguity_second_invocation.active_idempotency.clone(),
        second_key_two.clone(),
    ];
    ambiguity_second_invocation.idempotency_candidate_key_ids = vec![
        "coverage-key-one".to_string(),
        "coverage-key-two".to_string(),
    ];
    ambiguity_second_invocation.idempotency_candidate_key_fingerprints = vec![
        digest("coverage-key-one-material"),
        digest("coverage-key-two-material"),
    ];
    ambiguity_second_invocation.digest_key_id = "coverage-key-one".to_string();
    assert_eq!(
        approve(&pool, &ambiguity_second, &ambiguity_second_invocation)
            .await
            .unwrap()
            .outcome,
        "ok"
    );
    let mut ambiguous_invocation = ApprovalInvocation::new(
        &ambiguous,
        &ambiguous.first_approver,
        1,
        "ambiguous-aliases",
    );
    let first_receipt_alias = first_key_one;
    let second_receipt_alias = second_key_two;
    ambiguous_invocation.active_idempotency = first_receipt_alias.clone();
    ambiguous_invocation.idempotency_candidates = vec![first_receipt_alias, second_receipt_alias];
    ambiguous_invocation.idempotency_candidate_key_ids = vec![
        "coverage-key-one".to_string(),
        "coverage-key-two".to_string(),
    ];
    ambiguous_invocation.idempotency_candidate_key_fingerprints = vec![
        digest("coverage-key-one-material"),
        digest("coverage-key-two-material"),
    ];
    ambiguous_invocation.digest_key_id = "coverage-key-one".to_string();
    assert_eq!(
        approve(&pool, &ambiguous, &ambiguous_invocation)
            .await
            .unwrap()
            .outcome,
        "indeterminate"
    );
    assert_eq!(
        persisted_counts(&pool, &ambiguous).await,
        (0, 0, 0, 0, 1, "pending".to_string())
    );

    let fifth = seed_fixture(&pool, 2).await;
    let sixth = seed_additional_target(&pool, &fifth, "concurrent-new").await;
    let old_digest = digest(&format!("{}:rotation-three:old", fifth.tenant_id));
    let new_digest = digest(&format!("{}:rotation-three:new", fifth.tenant_id));
    let mut old = ApprovalInvocation::new(&fifth, &fifth.first_approver, 1, "concurrent-old");
    old.active_idempotency = old_digest.clone();
    old.idempotency_candidates = vec![old_digest.clone()];
    old.idempotency_candidate_key_ids = vec!["rotation-three-old".to_string()];
    old.idempotency_candidate_key_fingerprints = vec![digest("rotation-three-old-material")];
    old.digest_key_id = "rotation-three-old".to_string();
    let mut new = ApprovalInvocation::new(&sixth, &sixth.first_approver, 1, "concurrent-new");
    new.active_idempotency = new_digest.clone();
    new.idempotency_candidates = vec![new_digest, old_digest];
    new.idempotency_candidate_key_ids = vec![
        "rotation-three-new".to_string(),
        "rotation-three-old".to_string(),
    ];
    new.idempotency_candidate_key_fingerprints = vec![
        digest("rotation-three-new-material"),
        digest("rotation-three-old-material"),
    ];
    new.digest_key_id = "rotation-three-new".to_string();
    let (old_outcome, new_outcome) =
        tokio::join!(approve(&pool, &fifth, &old), approve(&pool, &sixth, &new));
    let results = [old_outcome, new_outcome];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(outcome) if outcome.outcome == "ok"))
            .count(),
        1
    );
    for result in &results {
        match result {
            Ok(outcome) => assert!(matches!(
                outcome.outcome.as_str(),
                "ok" | "idempotency_conflict"
            )),
            Err(error) => assert_serialization_failure(error),
        }
    }
    let fifth_counts = persisted_counts(&pool, &fifth).await;
    let sixth_counts = persisted_counts(&pool, &sixth).await;
    assert_eq!(fifth_counts.0 + sixth_counts.0, 1);
    assert_eq!(fifth_counts.1 + sixth_counts.1, 1);
    let retry = if fifth_counts.0 == 1 {
        approve(&pool, &sixth, &new).await.unwrap()
    } else {
        approve(&pool, &fifth, &old).await.unwrap()
    };
    assert_eq!(retry.outcome, "idempotency_conflict");
    let expected_aliases = if fifth_counts.0 == 1 { 1 } else { 2 };
    assert_eq!(fifth_counts.2 + sixth_counts.2, expected_aliases);
    assert_eq!(fifth_counts.3 + sixth_counts.3, 1);

    let disjoint_old = seed_fixture(&pool, 2).await;
    let disjoint_new = seed_additional_target(&pool, &disjoint_old, "disjoint-new").await;
    let mut old_only = ApprovalInvocation::new(
        &disjoint_old,
        &disjoint_old.first_approver,
        1,
        "disjoint-old",
    );
    old_only.active_idempotency = digest(&format!("{}:disjoint:old", disjoint_old.tenant_id));
    old_only.idempotency_candidates = vec![old_only.active_idempotency.clone()];
    old_only.idempotency_candidate_key_ids = vec!["disjoint-old".to_string()];
    old_only.idempotency_candidate_key_fingerprints = vec![digest("disjoint-old-material")];
    old_only.digest_key_id = "disjoint-old".to_string();
    let mut new_only = ApprovalInvocation::new(
        &disjoint_new,
        &disjoint_new.first_approver,
        1,
        "disjoint-new",
    );
    new_only.active_idempotency = digest(&format!("{}:disjoint:new", disjoint_old.tenant_id));
    new_only.idempotency_candidates = vec![new_only.active_idempotency.clone()];
    new_only.idempotency_candidate_key_ids = vec!["disjoint-new".to_string()];
    new_only.idempotency_candidate_key_fingerprints = vec![digest("disjoint-new-material")];
    new_only.digest_key_id = "disjoint-new".to_string();
    let (old_result, new_result) = tokio::join!(
        approve(&pool, &disjoint_old, &old_only),
        approve(&pool, &disjoint_new, &new_only)
    );
    let accepted = [&old_result, &new_result]
        .into_iter()
        .filter(|result| matches!(result, Ok(outcome) if outcome.outcome == "ok"))
        .count();
    assert_eq!(accepted, 1);
    for result in [&old_result, &new_result] {
        match result {
            Ok(outcome) => assert!(matches!(
                outcome.outcome.as_str(),
                "ok" | "idempotency_keyring_incomplete"
            )),
            Err(error) => assert_serialization_failure(error),
        }
    }
    let old_counts = persisted_counts(&pool, &disjoint_old).await;
    let new_counts = persisted_counts(&pool, &disjoint_new).await;
    assert_eq!(old_counts.0 + new_counts.0, 1);
    assert_eq!(old_counts.1 + new_counts.1, 1);
    assert_eq!(old_counts.2 + new_counts.2, 1);
    assert_eq!(old_counts.3 + new_counts.3, 1);
    let retry = if old_counts.0 == 1 {
        approve(&pool, &disjoint_new, &new_only).await.unwrap()
    } else {
        approve(&pool, &disjoint_old, &old_only).await.unwrap()
    };
    assert_eq!(retry.outcome, "idempotency_keyring_incomplete");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn rejected_approvals_leave_no_decision_receipt_or_audit() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool, 2).await;
    let mut payload =
        ApprovalInvocation::new(&fixture, &fixture.first_approver, 1, "wrong-payload");
    payload.payload_digest = digest(&format!("{}:wrong-payload", fixture.promotion_id));
    assert_eq!(
        approve(&pool, &fixture, &payload).await.unwrap().outcome,
        "payload_mismatch"
    );
    let revision = ApprovalInvocation::new(&fixture, &fixture.first_approver, 2, "wrong-revision");
    assert_eq!(
        approve(&pool, &fixture, &revision).await.unwrap().outcome,
        "revision_conflict"
    );
    let mut authority =
        ApprovalInvocation::new(&fixture, &fixture.first_approver, 1, "wrong-authority");
    authority.authority_digest = digest(&format!("{}:wrong-authority", fixture.promotion_id));
    assert_eq!(
        approve(&pool, &fixture, &authority).await.unwrap().outcome,
        "authority_mismatch"
    );
    let self_approval = ApprovalInvocation::new(&fixture, &fixture.requester, 1, "self-approval");
    assert_eq!(
        approve(&pool, &fixture, &self_approval)
            .await
            .unwrap()
            .outcome,
        "self_approval_forbidden"
    );
    let mut insufficient_permission =
        ApprovalInvocation::new(&fixture, &fixture.first_approver, 1, "permission-denied");
    insufficient_permission.guild_owner = false;
    assert_eq!(
        approve(&pool, &fixture, &insufficient_permission)
            .await
            .unwrap()
            .outcome,
        "invalid_input"
    );
    let mut null_input =
        ApprovalInvocation::new(&fixture, &fixture.first_approver, 1, "null-request-id");
    null_input.request_id = None;
    assert!(approve(&pool, &fixture, &null_input).await.is_err());
    let null_revision =
        sqlx::query("UPDATE public.activation_requests SET product_revision = NULL WHERE id = $1")
            .bind(&fixture.activation_id)
            .execute(&pool)
            .await;
    assert!(null_revision.is_err());
    sqlx::query(
        "UPDATE public.product_auth_sessions SET revoked_at = pg_catalog.clock_timestamp() \
         WHERE session_digest = $1",
    )
    .bind(&fixture.first_approver.session_digest)
    .execute(&pool)
    .await
    .unwrap();
    let stale_session =
        ApprovalInvocation::new(&fixture, &fixture.first_approver, 1, "stale-session");
    assert_eq!(
        approve(&pool, &fixture, &stale_session)
            .await
            .unwrap()
            .outcome,
        "authorization_stale"
    );
    assert_eq!(
        persisted_counts(&pool, &fixture).await,
        (0, 0, 0, 0, 1, "pending".to_string())
    );
}
