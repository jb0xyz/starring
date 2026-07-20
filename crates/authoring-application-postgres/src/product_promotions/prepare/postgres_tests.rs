use std::collections::VecDeque;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::{Arc, Mutex};

use authoring_promotion::{
    plan_start_promotion_v1, ApprovalPolicyV1, AuthenticatedPromotionContext, AuthoringSessionId,
    AutomationInstallationId, BindingRevision, IdempotencyKey, PolicyRevision,
    PreparedPromotionPlanV1, PrincipalId, PromotionRecordV1, SessionGeneration, StartPromotionV1,
    TenantId,
};
use chrono::{DateTime, TimeDelta, Utc};
use design_harness::{
    BurstOutcome, DesignSession, LlmClient, LlmError, LlmResponse, Message, PreviewReadyArtifactV1,
    ResourceBindingMap, ToolCall, ToolDefinition,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, Permissions, UserId};
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use super::SerializedProductPromotionPrepareV1;
use crate::product_action_digest::{
    keyed_digest, product_action_keyring_coverage_identity_v1,
    product_action_session_subject_digest_v1, unkeyed_digest, ProductActionDigestKeyV1,
    ProductActionDigestKeyringV1,
};
use crate::product_promotions::admission::{
    prepare_product_promotion_admission_v1, PreparedProductPromotionAdmissionV1,
    ProductPromotionAdmissionContextV1, ProductPromotionAdmissionEvidenceV1,
};
use crate::product_promotions::authorization::ProductPromotionAccessArgsV1;
use crate::product_promotions::digest::{promotion_action_ids_v1, ProductPromotionDigestsV1};
use crate::product_promotions::row::{
    ProductPromotionAdmittedStageV1, ProductPromotionPrepareStageV1, ProductPromotionReplayStageV1,
};
use crate::product_promotions::store::PostgresProductPromotions;
use crate::MIGRATOR;

const IDEMPOTENCY_DOMAIN: &[u8] = b"starring.product.promotion.idempotency.v1";
const SEMANTIC_REQUEST_DOMAIN: &[u8] = b"starring.product.promotion.request.v1";
const SESSION_SUBJECT_DOMAIN: &[u8] = b"starring.product.session.subject.v1";
const KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.promotion.digest-key-fingerprint.v1";
const SESSION_DIGEST: [u8; 32] = [0x11; 32];
const SHORT_SESSION_DIGEST: [u8; 32] = [0x55; 32];

#[derive(Clone)]
struct ScriptedClient {
    responses: Arc<Mutex<VecDeque<LlmResponse>>>,
}

impl ScriptedClient {
    fn validated_preview() -> Self {
        let response = LlmResponse::ToolCalls(vec![ToolCall {
            id: "interpret".to_string(),
            name: "interpret_intent_core".to_string(),
            arguments: json!({
                "expected_revision": 0,
                "request_mode": "build",
                "automation_kind": "managed_private_study_room",
                "requested_outcome": "validated_preview",
                "hub_channel": "community_hub",
                "language": "en",
                "close_policy": "disabled",
                "other_unmapped_required_capabilities": [],
                "response": ""
            })
            .to_string(),
        }]);
        Self {
            responses: Arc::new(Mutex::new(VecDeque::from([response]))),
        }
    }
}

impl LlmClient for ScriptedClient {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| LlmError::Client("unexpected model call".to_string()))
    }
}

async fn preview_ready_artifact() -> PreviewReadyArtifactV1 {
    let mut bindings = ResourceBindingMap::default();
    bindings
        .channel_bindings
        .insert(ResourceKey("community_hub".to_string()), ChannelId(700));
    let mut session =
        DesignSession::with_intent_recipe(ScriptedClient::validated_preview(), bindings);
    assert!(matches!(
        session
            .run_burst(
                "Create private study rooms in community_hub and prepare a validated preview"
            )
            .await,
        BurstOutcome::Ready { .. }
    ));
    session.export_preview_ready_artifact().unwrap()
}

fn promotion_context() -> AuthenticatedPromotionContext {
    AuthenticatedPromotionContext {
        tenant_id: TenantId::parse("tenant").unwrap(),
        principal_id: PrincipalId::parse("principal").unwrap(),
        session_owner_id: PrincipalId::parse("principal").unwrap(),
        session_id: AuthoringSessionId::parse("authoring").unwrap(),
        session_generation: SessionGeneration::new(1).unwrap(),
        guild_id: GuildId(3001),
        installation_id: AutomationInstallationId::parse("installation").unwrap(),
        ruleset_key: "ruleset".parse().unwrap(),
        requester: UserId(1001),
        binding_revision: BindingRevision::new(1).unwrap(),
        policy: ApprovalPolicyV1 {
            revision: PolicyRevision::new(1).unwrap(),
            required_approvals: NonZeroU32::new(1).unwrap(),
            ttl_seconds: NonZeroU64::new(3600).unwrap(),
        },
    }
}

fn promotion_plan(secret: &str, artifact: PreviewReadyArtifactV1) -> PreparedPromotionPlanV1 {
    plan_start_promotion_v1(StartPromotionV1 {
        idempotency_key: IdempotencyKey::parse(secret).unwrap(),
        context: promotion_context(),
        artifact,
    })
    .unwrap()
}

fn digest_key(id: &str, seed: u8) -> ProductActionDigestKeyV1 {
    ProductActionDigestKeyV1::from_bytes(
        id,
        std::array::from_fn(|index| seed.wrapping_add(index as u8)),
    )
    .unwrap()
}

fn keyring() -> ProductActionDigestKeyringV1 {
    ProductActionDigestKeyringV1::new(digest_key("active-v2", 7), [digest_key("retired-v1", 113)])
        .unwrap()
}

fn access_args(
    database_now: DateTime<Utc>,
    session_digest: &[u8; 32],
) -> ProductPromotionAccessArgsV1 {
    ProductPromotionAccessArgsV1 {
        expected_tenant_id: "tenant".to_string(),
        expected_installation_id: "installation".to_string(),
        expected_principal_id: "principal".to_string(),
        expected_product_session_digest: session_digest.to_vec(),
        expected_acting_user_id: "1001".to_string(),
        expected_discord_application_id: "2001".to_string(),
        expected_guild_id: "3001".to_string(),
        expected_capability: "promote".to_string(),
        observed_current_authority_revision: 1,
        observed_current_authority_payload_digest: "5".repeat(64),
        authority_observation_digest: "7".repeat(64),
        authority_observed_at: database_now - TimeDelta::milliseconds(100),
        authority_expires_at: database_now + TimeDelta::seconds(4),
        effective_permission_bits: Permissions::MANAGE_GUILD.bits().to_string(),
        guild_owner: false,
    }
}

fn admission_context(request_id: &str) -> ProductPromotionAdmissionContextV1 {
    ProductPromotionAdmissionContextV1 {
        product_request_id: request_id.to_string(),
        authoring_session_id: AuthoringSessionId::parse("authoring").unwrap(),
        generation: SessionGeneration::new(1).unwrap(),
    }
}

fn promotion_digests(
    keyring: &ProductActionDigestKeyringV1,
    plan: &PreparedPromotionPlanV1,
    secret: &str,
    session_digest: &[u8; 32],
) -> ProductPromotionDigestsV1 {
    let idempotency_fields = [
        b"tenant".as_slice(),
        b"installation".as_slice(),
        b"principal".as_slice(),
        b"product_promote_v1".as_slice(),
        secret.as_bytes(),
    ];
    let idempotency_candidates = keyring
        .keys()
        .iter()
        .map(|key| keyed_digest(key, IDEMPOTENCY_DOMAIN, &idempotency_fields))
        .collect::<Vec<_>>();
    let active_idempotency = idempotency_candidates[0].clone();
    let coverage =
        product_action_keyring_coverage_identity_v1(keyring, KEY_MATERIAL_FINGERPRINT_DOMAIN);
    let semantic_request = unkeyed_digest(
        SEMANTIC_REQUEST_DOMAIN,
        &[
            b"tenant".as_slice(),
            b"installation".as_slice(),
            b"principal".as_slice(),
            b"authoring".as_slice(),
            b"1".as_slice(),
            plan.promotion_id.as_str().as_bytes(),
        ],
    );
    let action_ids = promotion_action_ids_v1(
        keyring.active(),
        "tenant",
        "installation",
        "principal",
        plan.promotion_id.as_str(),
        &active_idempotency,
        &semantic_request,
    );
    ProductPromotionDigestsV1 {
        promotion_id: plan.promotion_id.clone(),
        active_idempotency,
        idempotency_candidates,
        idempotency_candidate_key_ids: coverage.key_ids.clone(),
        idempotency_candidate_key_fingerprints: coverage.key_fingerprints.clone(),
        active_key_id: coverage.key_ids[0].clone(),
        active_key_fingerprint: coverage.key_fingerprints[0].clone(),
        semantic_request,
        receipt_id: action_ids.receipt_id,
        audit_event_id: action_ids.audit_event_id,
        session_subject: product_action_session_subject_digest_v1(
            SESSION_SUBJECT_DOMAIN,
            b"tenant",
            b"principal",
            session_digest,
        ),
    }
}

struct PreparedCase {
    plan: PreparedPromotionPlanV1,
    access: ProductPromotionAccessArgsV1,
    context: ProductPromotionAdmissionContextV1,
    digests: ProductPromotionDigestsV1,
    admission: PreparedProductPromotionAdmissionV1,
    serialized: SerializedProductPromotionPrepareV1,
}

impl PreparedCase {
    fn new(
        keyring: &ProductActionDigestKeyringV1,
        plan: PreparedPromotionPlanV1,
        secret: &str,
        request_id: &str,
        database_now: DateTime<Utc>,
        session_digest: &[u8; 32],
    ) -> Self {
        let access = access_args(database_now, session_digest);
        let context = admission_context(request_id);
        let digests = promotion_digests(keyring, &plan, secret, session_digest);
        let admission =
            prepare_product_promotion_admission_v1(keyring, &context, &access, &plan, &digests)
                .unwrap();
        let serialized = SerializedProductPromotionPrepareV1::new(&plan, &admission).unwrap();
        Self {
            plan,
            access,
            context,
            digests,
            admission,
            serialized,
        }
    }
}

pub(in crate::product_promotions) async fn prepared_decoder_stage(
    database_now: DateTime<Utc>,
) -> ProductPromotionAdmittedStageV1 {
    let keyring = keyring();
    let secret = "decoder-adversarial-key";
    let plan = promotion_plan(secret, preview_ready_artifact().await);
    let case = PreparedCase::new(
        &keyring,
        plan,
        secret,
        "decoder-adversarial-request",
        database_now,
        &SESSION_DIGEST,
    );
    let record = PromotionRecordV1::prepared(case.plan.materialize(database_now).unwrap()).unwrap();
    ProductPromotionAdmittedStageV1 {
        record,
        admission: ProductPromotionAdmissionEvidenceV1 {
            format_version: 1,
            payload: case.admission.payload,
            admitted_at: database_now,
        },
        admission_digest: case.admission.digest,
        database_now,
    }
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
    administrator
        .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
        .await
        .unwrap();
    administrator
        .execute(format!("CREATE DATABASE {name}").as_str())
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options.database(name))
        .await
        .unwrap();
    (administrator, pool)
}

async fn drop_temporary_database(mut administrator: PgConnection, pool: PgPool, name: &str) {
    pool.close().await;
    administrator
        .execute(format!("DROP DATABASE {name} WITH (FORCE)").as_str())
        .await
        .unwrap();
}

async fn seed_control_plane(
    pool: &PgPool,
    plan: &PreparedPromotionPlanV1,
    access: &ProductPromotionAccessArgsV1,
) {
    let authority = &plan.intent.authority;
    let evidence = &plan.intent.evidence;
    let bindings = json!({
        "role_bindings": {},
        "channel_bindings": {"community_hub": "700"}
    });
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals (principal_id, discord_user_id) VALUES ($1, $2)",
    )
    .bind(authority.principal_id.as_str())
    .bind(authority.requester.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_oauth_flows \
         (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, expires_at, \
          consumed_at, terminal_result_code) \
         VALUES ($1, $2, 'https://starring.example/oauth/discord/callback', '/', \
          CURRENT_TIMESTAMP - INTERVAL '1 minute', CURRENT_TIMESTAMP + INTERVAL '5 minutes', \
          CURRENT_TIMESTAMP - INTERVAL '1 second', 'callback_claimed')",
    )
    .bind([0x33_u8; 32].as_slice())
    .bind([0x44_u8; 32].as_slice())
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
    .bind(SESSION_DIGEST.as_slice())
    .bind(authority.principal_id.as_str())
    .bind([0x22_u8; 32].as_slice())
    .bind([0x33_u8; 32].as_slice())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_tenants (tenant_id, lifecycle_state, display_name) \
         VALUES ($1, 'active', 'Promotion adapter test tenant')",
    )
    .bind(authority.tenant_id.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, ruleset_key, \
          lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(authority.installation_id.as_str())
    .bind(authority.tenant_id.as_str())
    .bind(&access.expected_discord_application_id)
    .bind(authority.guild_id.to_string())
    .bind(authority.ruleset_key.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(authority.installation_id.as_str())
    .bind(authority.tenant_id.as_str())
    .bind(i64::try_from(authority.binding_revision.get()).unwrap())
    .bind(sqlx::types::Json(&bindings))
    .bind(evidence.context_fingerprint.as_str())
    .bind(i64::try_from(authority.policy.revision.get()).unwrap())
    .bind(i32::try_from(authority.policy.required_approvals.get()).unwrap())
    .bind(i64::try_from(authority.policy.ttl_seconds.get()).unwrap())
    .bind(&access.observed_current_authority_payload_digest)
    .bind(authority.principal_id.as_str())
    .bind("4".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.authoring_sessions \
         (session_id, tenant_id, installation_id, owner_principal_id, current_generation, \
          lifecycle_state) VALUES ($1, $2, $3, $4, 1, 'active')",
    )
    .bind(authority.session_id.as_str())
    .bind(authority.tenant_id.as_str())
    .bind(authority.installation_id.as_str())
    .bind(authority.principal_id.as_str())
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
         VALUES ($1, 1, $2, $3, 1, $4, $5, 'keychain:promotion-test-v1', \
          'xchacha20_poly1305', 1, $6, $7, $8, 1, '{}'::JSONB, 'preview_ready', $9, $10, $11, 1)",
    )
    .bind(authority.session_id.as_str())
    .bind(authority.tenant_id.as_str())
    .bind(authority.installation_id.as_str())
    .bind([0x10_u8; 16].as_slice())
    .bind([0x20_u8; 24].as_slice())
    .bind("3".repeat(64))
    .bind(sqlx::types::Json(&bindings))
    .bind(evidence.context_fingerprint.as_str())
    .bind(i64::try_from(evidence.candidate_revision).unwrap())
    .bind(evidence.candidate_ruleset_hash.as_str())
    .bind("2".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn seed_short_lived_session(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.product_oauth_flows \
         (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, expires_at, \
          consumed_at, terminal_result_code) \
         VALUES ($1, $2, 'https://starring.example/oauth/discord/callback', '/', \
          CURRENT_TIMESTAMP - INTERVAL '1 minute', CURRENT_TIMESTAMP + INTERVAL '5 minutes', \
          CURRENT_TIMESTAMP - INTERVAL '1 second', 'callback_claimed')",
    )
    .bind([0x66_u8; 32].as_slice())
    .bind([0x77_u8; 32].as_slice())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_auth_sessions \
         (session_digest, principal_id, csrf_digest, oauth_state_digest, authenticated_at, \
          created_at, last_seen_at, idle_expires_at, absolute_expires_at) \
         SELECT $1, 'principal', $2, $3, captured_at, captured_at, captured_at, \
          captured_at + INTERVAL '100 milliseconds', captured_at + INTERVAL '1 hour' \
         FROM (SELECT pg_catalog.clock_timestamp() AS captured_at) AS clock",
    )
    .bind(SHORT_SESSION_DIGEST.as_slice())
    .bind([0x88_u8; 32].as_slice())
    .bind([0x66_u8; 32].as_slice())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn advance_authoring_head(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
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
         SELECT session_id, 2, tenant_id, installation_id, snapshot_schema_version, \
          snapshot_ciphertext, snapshot_nonce, encryption_key_id, encryption_suite, \
          encryption_suite_version, authenticated_metadata_digest, resource_bindings, \
          binding_fingerprint, installation_authority_revision, summary, stage, \
          candidate_revision, candidate_hash, $1, harness_contract_revision \
         FROM public.authoring_session_generations \
         WHERE session_id = 'authoring' AND generation = 1",
    )
    .bind("9".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_sessions SET current_generation = 2, \
         updated_at = pg_catalog.clock_timestamp() WHERE session_id = 'authoring'",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn validate_admitted(admitted: &ProductPromotionAdmittedStageV1, case: &PreparedCase) {
    case.plan
        .validate_prepared_record(&admitted.record)
        .unwrap();
    assert_eq!(admitted.admission_digest, case.admission.digest);
    assert_eq!(
        admitted.admission.payload.idempotency_digest_key_id,
        "active-v2"
    );
    assert_eq!(
        admitted.admission.payload.idempotency_key_digest,
        case.digests.active_idempotency
    );
    assert!(admitted.database_now >= admitted.record.created_at);
}

fn validate_prepare_stage(
    stage: ProductPromotionPrepareStageV1,
    case: &PreparedCase,
) -> &'static str {
    match stage {
        ProductPromotionPrepareStageV1::Created(admitted) => {
            validate_admitted(&admitted, case);
            "created"
        }
        ProductPromotionPrepareStageV1::PartialExact(admitted) => {
            validate_admitted(&admitted, case);
            "partial_exact"
        }
        ProductPromotionPrepareStageV1::FinalReplayRequired(_)
        | ProductPromotionPrepareStageV1::FinalExact(_) => {
            panic!("prepared promotion unexpectedly reached a final state")
        }
    }
}

fn validate_partial_replay(
    replay: ProductPromotionReplayStageV1,
    case: &PreparedCase,
) -> PromotionRecordV1 {
    match replay {
        ProductPromotionReplayStageV1::PartialExact(admitted) => {
            validate_admitted(&admitted, case);
            admitted.record
        }
        ProductPromotionReplayStageV1::Missing
        | ProductPromotionReplayStageV1::FinalExact(_)
        | ProductPromotionReplayStageV1::LegacyRepairRequired(_) => {
            panic!("prepared promotion replay did not return the exact partial state")
        }
    }
}

async fn direct_prepare_with_candidates(
    pool: &PgPool,
    case: &PreparedCase,
    digest_candidates: &[String],
    key_id_candidates: &[String],
    fingerprint_candidates: &[String],
) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT outcome_code FROM public.starring_product_promotion_prepare_v1(\
         $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
         $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, \
         $31, $32, $33, $34, $35)",
    )
    .bind(&case.access.expected_tenant_id)
    .bind(&case.access.expected_installation_id)
    .bind(&case.access.expected_principal_id)
    .bind(&case.access.expected_product_session_digest)
    .bind(&case.access.expected_acting_user_id)
    .bind(&case.access.expected_discord_application_id)
    .bind(&case.access.expected_guild_id)
    .bind(&case.access.expected_capability)
    .bind(case.access.observed_current_authority_revision)
    .bind(&case.access.observed_current_authority_payload_digest)
    .bind(&case.access.authority_observation_digest)
    .bind(case.access.authority_observed_at)
    .bind(case.access.authority_expires_at)
    .bind(&case.access.effective_permission_bits)
    .bind(case.access.guild_owner)
    .bind(&case.context.product_request_id)
    .bind(&case.digests.session_subject)
    .bind(case.context.authoring_session_id.as_str())
    .bind(i64::try_from(case.context.generation.get()).unwrap())
    .bind(i64::try_from(case.plan.intent.evidence.candidate_revision).unwrap())
    .bind(case.plan.intent.evidence.candidate_ruleset_hash.as_str())
    .bind(case.plan.intent.evidence.context_fingerprint.as_str())
    .bind(case.plan.promotion_id.as_str())
    .bind(case.plan.request_digest.as_str())
    .bind(&case.serialized.intent)
    .bind(&case.serialized.admission_payload)
    .bind(&case.admission.digest)
    .bind(&case.digests.active_idempotency)
    .bind(digest_candidates)
    .bind(key_id_candidates)
    .bind(fingerprint_candidates)
    .bind(&case.digests.active_key_id)
    .bind(&case.digests.semantic_request)
    .bind(&case.digests.receipt_id)
    .bind(&case.digests.audit_event_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn install_malformed_prepare_projection(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        ALTER FUNCTION public.starring_product_promotion_prepare_v1(
            TEXT, TEXT, TEXT, BYTEA, TEXT, TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT,
            TIMESTAMPTZ, TIMESTAMPTZ, TEXT, BOOLEAN, TEXT, BYTEA, TEXT, BIGINT,
            BIGINT, TEXT, TEXT, TEXT, TEXT, JSONB, JSONB, TEXT, TEXT, TEXT[],
            TEXT[], TEXT[], TEXT, TEXT, TEXT, TEXT
        ) RENAME TO starring_product_promotion_prepare_valid_v1;

        CREATE FUNCTION public.starring_product_promotion_prepare_v1(
            expected_tenant_id TEXT,
            expected_installation_id TEXT,
            expected_principal_id TEXT,
            expected_product_session_digest BYTEA,
            expected_acting_user_id TEXT,
            expected_discord_application_id TEXT,
            expected_guild_id TEXT,
            expected_capability TEXT,
            observed_current_authority_revision BIGINT,
            observed_current_authority_payload_digest TEXT,
            authority_observation_digest TEXT,
            authority_observed_at TIMESTAMPTZ,
            authority_expires_at TIMESTAMPTZ,
            effective_permission_bits TEXT,
            guild_owner BOOLEAN,
            product_request_id TEXT,
            session_subject_digest BYTEA,
            expected_session_id TEXT,
            expected_generation BIGINT,
            expected_candidate_revision BIGINT,
            expected_candidate_hash TEXT,
            expected_binding_fingerprint TEXT,
            expected_promotion_id TEXT,
            expected_promotion_request_digest TEXT,
            prepared_promotion_intent JSONB,
            product_admission_payload JSONB,
            product_admission_digest TEXT,
            active_idempotency_key_digest TEXT,
            idempotency_key_digest_candidates TEXT[],
            idempotency_digest_key_id_candidates TEXT[],
            idempotency_digest_key_fingerprint_candidates TEXT[],
            idempotency_digest_key_id TEXT,
            semantic_request_digest TEXT,
            new_receipt_id TEXT,
            new_audit_event_id TEXT
        )
        RETURNS TABLE(
            outcome_code TEXT,
            promotion_record JSONB,
            admission_evidence JSONB,
            admission_digest TEXT,
            database_now TIMESTAMPTZ
        )
        LANGUAGE sql
        VOLATILE
        STRICT
        SET search_path = pg_catalog
        AS $function$
        SELECT result.outcome_code,
            result.promotion_record || pg_catalog.jsonb_build_object('unexpected', TRUE),
            result.admission_evidence,
            result.admission_digest,
            result.database_now
        FROM public.starring_product_promotion_prepare_valid_v1(
            expected_tenant_id,
            expected_installation_id,
            expected_principal_id,
            expected_product_session_digest,
            expected_acting_user_id,
            expected_discord_application_id,
            expected_guild_id,
            expected_capability,
            observed_current_authority_revision,
            observed_current_authority_payload_digest,
            authority_observation_digest,
            authority_observed_at,
            authority_expires_at,
            effective_permission_bits,
            guild_owner,
            product_request_id,
            session_subject_digest,
            expected_session_id,
            expected_generation,
            expected_candidate_revision,
            expected_candidate_hash,
            expected_binding_fingerprint,
            expected_promotion_id,
            expected_promotion_request_digest,
            prepared_promotion_intent,
            product_admission_payload,
            product_admission_digest,
            active_idempotency_key_digest,
            idempotency_key_digest_candidates,
            idempotency_digest_key_id_candidates,
            idempotency_digest_key_fingerprint_candidates,
            idempotency_digest_key_id,
            semantic_request_digest,
            new_receipt_id,
            new_audit_event_id
        ) AS result;
        $function$;
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn real_adapter_converges_and_rolls_back_malformed_decode() {
    let name = "starring_product_promotion_adapter_test";
    let (administrator, pool) = temporary_database(name).await;
    MIGRATOR.run(&pool).await.unwrap();
    let artifact = preview_ready_artifact().await;
    let first_plan = promotion_plan("adapter-concurrent-key", artifact.clone());
    let ring = keyring();
    let initial_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let first_case = PreparedCase::new(
        &ring,
        first_plan,
        "adapter-concurrent-key",
        "request-concurrent",
        initial_now,
        &SESSION_DIGEST,
    );
    seed_control_plane(&pool, &first_case.plan, &first_case.access).await;
    let adapter = PostgresProductPromotions::new(pool.clone(), ring.clone()).unwrap();
    let sql_candidate_hash = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.encode( \
             pg_catalog.sha256( \
                 pg_catalog.convert_to('starring.intent.candidate_ruleset.v1', 'UTF8') \
                 || pg_catalog.decode('00', 'hex') \
                 || pg_catalog.convert_to( \
                     public.starring_canonical_json_v1($1::JSONB), 'UTF8' \
                 ) \
             ), \
             'hex' \
         )",
    )
    .bind(sqlx::types::Json(&first_case.plan.intent.definition))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        sql_candidate_hash,
        first_case
            .plan
            .intent
            .evidence
            .candidate_ruleset_hash
            .as_str()
    );

    assert!(matches!(
        adapter
            .execute_replay_stage_v1(&first_case.access, &first_case.context, &first_case.digests)
            .await
            .unwrap(),
        ProductPromotionReplayStageV1::Missing
    ));

    let first = adapter.execute_prepare_stage_v1(
        &first_case.access,
        &first_case.context,
        &first_case.digests,
        &first_case.plan,
        &first_case.admission,
        &first_case.serialized,
    );
    let second = adapter.execute_prepare_stage_v1(
        &first_case.access,
        &first_case.context,
        &first_case.digests,
        &first_case.plan,
        &first_case.admission,
        &first_case.serialized,
    );
    let (first, second) = tokio::join!(first, second);
    let mut outcomes = vec![
        validate_prepare_stage(first.unwrap(), &first_case),
        validate_prepare_stage(second.unwrap(), &first_case),
    ];
    outcomes.sort();
    assert_eq!(outcomes, vec!["created", "partial_exact"]);

    let replayed = validate_partial_replay(
        adapter
            .execute_replay_stage_v1(&first_case.access, &first_case.context, &first_case.digests)
            .await
            .unwrap(),
        &first_case,
    );
    first_case.plan.validate_prepared_record(&replayed).unwrap();
    let repeated = adapter
        .execute_prepare_stage_v1(
            &first_case.access,
            &first_case.context,
            &first_case.digests,
            &first_case.plan,
            &first_case.admission,
            &first_case.serialized,
        )
        .await
        .unwrap();
    assert_eq!(
        validate_prepare_stage(repeated, &first_case),
        "partial_exact"
    );

    let persisted = sqlx::query_as::<_, (i64, String, String, bool)>(
        "SELECT pg_catalog.count(*) OVER (), product_admission_digest, \
         product_admission #>> '{payload,idempotency_digest_key_id}', \
         (record ->> 'created_at')::TIMESTAMPTZ \
             = (product_admission ->> 'admitted_at')::TIMESTAMPTZ \
         FROM public.authoring_promotions WHERE id = $1",
    )
    .bind(first_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, 1);
    assert_eq!(persisted.1, first_case.admission.digest);
    assert_eq!(persisted.2, "active-v2");
    assert!(persisted.3);

    let definition_drift_plan = promotion_plan("adapter-definition-drift-key", artifact.clone());
    let definition_drift_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut definition_drift_case = PreparedCase::new(
        &ring,
        definition_drift_plan,
        "adapter-definition-drift-key",
        "request-definition-drift",
        definition_drift_now,
        &SESSION_DIGEST,
    );
    definition_drift_case.serialized.intent.0["definition"]["version"] = json!(2);
    let definition_drift = direct_prepare_with_candidates(
        &pool,
        &definition_drift_case,
        &definition_drift_case.digests.idempotency_candidates,
        &definition_drift_case.digests.idempotency_candidate_key_ids,
        &definition_drift_case
            .digests
            .idempotency_candidate_key_fingerprints,
    )
    .await;
    assert_eq!(definition_drift, "invalid_candidate");
    let definition_drift_write_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.authoring_promotions WHERE id = $1",
    )
    .bind(definition_drift_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(definition_drift_write_count, 0);

    let hostile_intent_plan = promotion_plan("adapter-hostile-intent-key", artifact.clone());
    let hostile_intent_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut hostile_intent_case = PreparedCase::new(
        &ring,
        hostile_intent_plan,
        "adapter-hostile-intent-key",
        "request-hostile-intent",
        hostile_intent_now,
        &SESSION_DIGEST,
    );
    hostile_intent_case.serialized.intent.0["evidence"]
        .as_object_mut()
        .unwrap()
        .remove("compiler_input_hash");
    let hostile_intent = direct_prepare_with_candidates(
        &pool,
        &hostile_intent_case,
        &hostile_intent_case.digests.idempotency_candidates,
        &hostile_intent_case.digests.idempotency_candidate_key_ids,
        &hostile_intent_case
            .digests
            .idempotency_candidate_key_fingerprints,
    )
    .await;
    assert_eq!(hostile_intent, "invalid_candidate");
    let hostile_intent_write_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.authoring_promotions WHERE id = $1",
    )
    .bind(hostile_intent_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(hostile_intent_write_count, 0);

    let malformed_arrays_plan = promotion_plan("adapter-malformed-arrays-key", artifact.clone());
    let malformed_arrays_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let malformed_arrays_case = PreparedCase::new(
        &ring,
        malformed_arrays_plan,
        "adapter-malformed-arrays-key",
        "request-malformed-arrays",
        malformed_arrays_now,
        &SESSION_DIGEST,
    );
    let malformed_key_ids = vec![malformed_arrays_case.digests.active_key_id.clone()];
    let malformed_arrays = direct_prepare_with_candidates(
        &pool,
        &malformed_arrays_case,
        &malformed_arrays_case.digests.idempotency_candidates,
        &malformed_key_ids,
        &malformed_arrays_case
            .digests
            .idempotency_candidate_key_fingerprints,
    )
    .await;
    assert_eq!(malformed_arrays, "persistence_corrupt");
    let malformed_arrays_write_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.authoring_promotions WHERE id = $1",
    )
    .bind(malformed_arrays_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(malformed_arrays_write_count, 0);

    let active_second_plan = promotion_plan("adapter-active-second-key", artifact.clone());
    let active_second_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let active_second_case = PreparedCase::new(
        &ring,
        active_second_plan,
        "adapter-active-second-key",
        "request-active-second",
        active_second_now,
        &SESSION_DIGEST,
    );
    let mut reversed_digests = active_second_case.digests.idempotency_candidates.clone();
    let mut reversed_key_ids = active_second_case
        .digests
        .idempotency_candidate_key_ids
        .clone();
    let mut reversed_fingerprints = active_second_case
        .digests
        .idempotency_candidate_key_fingerprints
        .clone();
    reversed_digests.reverse();
    reversed_key_ids.reverse();
    reversed_fingerprints.reverse();
    let active_second = direct_prepare_with_candidates(
        &pool,
        &active_second_case,
        &reversed_digests,
        &reversed_key_ids,
        &reversed_fingerprints,
    )
    .await;
    assert_eq!(active_second, "invalid_candidate");
    let active_second_write_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.authoring_promotions WHERE id = $1",
    )
    .bind(active_second_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_second_write_count, 0);

    seed_short_lived_session(&pool).await;
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let expired_plan = promotion_plan("adapter-expired-session-key", artifact.clone());
    let expired_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let expired_case = PreparedCase::new(
        &ring,
        expired_plan,
        "adapter-expired-session-key",
        "request-expired-session",
        expired_now,
        &SHORT_SESSION_DIGEST,
    );
    let expired = adapter
        .execute_prepare_stage_v1(
            &expired_case.access,
            &expired_case.context,
            &expired_case.digests,
            &expired_case.plan,
            &expired_case.admission,
            &expired_case.serialized,
        )
        .await;
    assert_eq!(
        expired.unwrap_err(),
        authoring_application::AuthorizedPromotionSubmissionErrorV1::Forbidden
    );
    let expired_write_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.authoring_promotions WHERE id = $1",
    )
    .bind(expired_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(expired_write_count, 0);

    install_malformed_prepare_projection(&pool).await;
    let malformed_plan = promotion_plan("adapter-malformed-key", artifact.clone());
    let malformed_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let malformed_case = PreparedCase::new(
        &ring,
        malformed_plan,
        "adapter-malformed-key",
        "request-malformed",
        malformed_now,
        &SESSION_DIGEST,
    );
    let malformed = adapter
        .execute_prepare_stage_v1(
            &malformed_case.access,
            &malformed_case.context,
            &malformed_case.digests,
            &malformed_case.plan,
            &malformed_case.admission,
            &malformed_case.serialized,
        )
        .await;
    assert_eq!(
        malformed.unwrap_err(),
        authoring_application::AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
    );
    let rolled_back = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.authoring_promotions WHERE id = $1",
    )
    .bind(malformed_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rolled_back, 0);

    advance_authoring_head(&pool).await;
    let stale_head_plan = promotion_plan("adapter-stale-head-key", artifact);
    let stale_head_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let stale_head_case = PreparedCase::new(
        &ring,
        stale_head_plan,
        "adapter-stale-head-key",
        "request-stale-head",
        stale_head_now,
        &SESSION_DIGEST,
    );
    let stale_head = adapter
        .execute_prepare_stage_v1(
            &stale_head_case.access,
            &stale_head_case.context,
            &stale_head_case.digests,
            &stale_head_case.plan,
            &stale_head_case.admission,
            &stale_head_case.serialized,
        )
        .await;
    assert_eq!(
        stale_head.unwrap_err(),
        authoring_application::AuthorizedPromotionSubmissionErrorV1::GenerationMismatch
    );
    let stale_head_write_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.authoring_promotions WHERE id = $1",
    )
    .bind(stale_head_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stale_head_write_count, 0);

    drop_temporary_database(administrator, pool, name).await;
}
