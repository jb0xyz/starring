use std::collections::VecDeque;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::{Arc, Mutex};

use authoring_promotion::{
    plan_pending_activation_v1, plan_start_promotion_v1, ApprovalPolicyV1,
    AuthenticatedPromotionContext, AuthoringSessionId, AutomationInstallationId, BindingRevision,
    IdempotencyKey, PendingActivationDispositionV1, PendingActivationReceiptV1,
    PendingActivationTransitionV1, PolicyRevision, PreparedPromotionPlanV1, PrincipalId,
    PromotionRecordV1, SessionGeneration, StartPromotionV1, TenantId,
};
use automation_ruleset_activation::ActivationRequest;
use chrono::{DateTime, TimeDelta, Utc};
use design_harness::{
    BurstOutcome, DesignSession, LlmClient, LlmError, LlmResponse, Message, PreviewReadyArtifactV1,
    ResourceBindingMap, ToolCall, ToolDefinition,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, Permissions, UserId};
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::Postgres;
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
    decode_product_promotion_approval_environment_v1, decode_product_promotion_publication_v1,
    ProductPromotionAdmittedStageV1, ProductPromotionApprovalEnvironmentRowV1,
    ProductPromotionPrepareStageV1, ProductPromotionPublicationRowV1,
    ProductPromotionReplayStageV1,
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
const DRIFT_SESSION_DIGEST: [u8; 32] = [0x99; 32];

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
    promotion_context_at_generation(1)
}

fn promotion_context_at_generation(generation: u64) -> AuthenticatedPromotionContext {
    AuthenticatedPromotionContext {
        tenant_id: TenantId::parse("tenant").unwrap(),
        principal_id: PrincipalId::parse("principal").unwrap(),
        session_owner_id: PrincipalId::parse("principal").unwrap(),
        session_id: AuthoringSessionId::parse("authoring").unwrap(),
        session_generation: SessionGeneration::new(generation).unwrap(),
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
    promotion_plan_at_generation(secret, artifact, 1)
}

fn promotion_plan_at_generation(
    secret: &str,
    artifact: PreviewReadyArtifactV1,
    generation: u64,
) -> PreparedPromotionPlanV1 {
    plan_start_promotion_v1(StartPromotionV1 {
        idempotency_key: IdempotencyKey::parse(secret).unwrap(),
        context: promotion_context_at_generation(generation),
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

fn admission_context(
    request_id: &str,
    generation: SessionGeneration,
) -> ProductPromotionAdmissionContextV1 {
    ProductPromotionAdmissionContextV1 {
        product_request_id: request_id.to_string(),
        authoring_session_id: AuthoringSessionId::parse("authoring").unwrap(),
        generation,
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
            plan.intent
                .authority
                .session_generation
                .get()
                .to_string()
                .as_bytes(),
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
        let context = admission_context(request_id, plan.intent.authority.session_generation);
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

async fn seed_drift_session(pool: &PgPool) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO public.product_oauth_flows \
         (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, expires_at, \
          consumed_at, terminal_result_code) \
         VALUES ($1, $2, 'https://starring.example/oauth/discord/callback', '/', \
          CURRENT_TIMESTAMP - INTERVAL '1 minute', CURRENT_TIMESTAMP + INTERVAL '5 minutes', \
          CURRENT_TIMESTAMP - INTERVAL '1 second', 'callback_claimed')",
    )
    .bind([0xaa_u8; 32].as_slice())
    .bind([0xbb_u8; 32].as_slice())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_auth_sessions \
         (session_digest, principal_id, csrf_digest, oauth_state_digest, authenticated_at, \
          created_at, last_seen_at, idle_expires_at, absolute_expires_at) \
         SELECT $1, 'principal', $2, $3, captured_at, captured_at, captured_at, \
          captured_at + INTERVAL '20 minutes', captured_at + INTERVAL '1 hour' \
         FROM (SELECT pg_catalog.clock_timestamp() AS captured_at) AS clock",
    )
    .bind(DRIFT_SESSION_DIGEST.as_slice())
    .bind([0xcc_u8; 32].as_slice())
    .bind([0xaa_u8; 32].as_slice())
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

fn admitted_prepare_stage(
    stage: ProductPromotionPrepareStageV1,
    case: &PreparedCase,
) -> ProductPromotionAdmittedStageV1 {
    match stage {
        ProductPromotionPrepareStageV1::Created(admitted)
        | ProductPromotionPrepareStageV1::PartialExact(admitted) => {
            validate_admitted(&admitted, case);
            *admitted
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

async fn direct_publish<'executor, ExecutorType>(
    executor: ExecutorType,
    access: &ProductPromotionAccessArgsV1,
    admitted: &ProductPromotionAdmittedStageV1,
) -> ProductPromotionPublicationRowV1
where
    ExecutorType: Executor<'executor, Database = Postgres>,
{
    sqlx::query_as::<_, ProductPromotionPublicationRowV1>(
        "SELECT outcome_code, publication_projection, promotion_record, database_now \
         FROM public.starring_product_promotion_publish_v1(\
         $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
         $16, $17, $18, $19)",
    )
    .bind(&access.expected_tenant_id)
    .bind(&access.expected_installation_id)
    .bind(&access.expected_principal_id)
    .bind(&access.expected_product_session_digest)
    .bind(&access.expected_acting_user_id)
    .bind(&access.expected_discord_application_id)
    .bind(&access.expected_guild_id)
    .bind(&access.expected_capability)
    .bind(access.observed_current_authority_revision)
    .bind(&access.observed_current_authority_payload_digest)
    .bind(&access.authority_observation_digest)
    .bind(access.authority_observed_at)
    .bind(access.authority_expires_at)
    .bind(&access.effective_permission_bits)
    .bind(access.guild_owner)
    .bind(admitted.record.id.as_str())
    .bind(i64::try_from(admitted.record.revision.get()).unwrap())
    .bind(admitted.record.request_digest.as_str())
    .bind(&admitted.admission_digest)
    .fetch_one(executor)
    .await
    .unwrap()
}

async fn direct_approval_environment<'executor, ExecutorType>(
    executor: ExecutorType,
    access: &ProductPromotionAccessArgsV1,
    admitted: &ProductPromotionAdmittedStageV1,
) -> ProductPromotionApprovalEnvironmentRowV1
where
    ExecutorType: Executor<'executor, Database = Postgres>,
{
    sqlx::query_as::<_, ProductPromotionApprovalEnvironmentRowV1>(
        "SELECT outcome_code, promotion_record, historical_binding_revision, historical_resource_bindings, \
         historical_binding_fingerprint, active_version, active_content_hash, \
         target_artifact_projection, database_now \
         FROM public.starring_product_promotion_approval_environment_v1(\
         $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
         $16, $17, $18, $19)",
    )
    .bind(&access.expected_tenant_id)
    .bind(&access.expected_installation_id)
    .bind(&access.expected_principal_id)
    .bind(&access.expected_product_session_digest)
    .bind(&access.expected_acting_user_id)
    .bind(&access.expected_discord_application_id)
    .bind(&access.expected_guild_id)
    .bind(&access.expected_capability)
    .bind(access.observed_current_authority_revision)
    .bind(&access.observed_current_authority_payload_digest)
    .bind(&access.authority_observation_digest)
    .bind(access.authority_observed_at)
    .bind(access.authority_expires_at)
    .bind(&access.effective_permission_bits)
    .bind(access.guild_owner)
    .bind(admitted.record.id.as_str())
    .bind(i64::try_from(admitted.record.revision.get()).unwrap())
    .bind(admitted.record.request_digest.as_str())
    .bind(&admitted.admission_digest)
    .fetch_one(executor)
    .await
    .unwrap()
}

async fn await_publish_lock_wait(pool: &PgPool) {
    for _ in 0..200 {
        let waiting = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS ( \
             SELECT 1 FROM pg_catalog.pg_stat_activity \
             WHERE datname = pg_catalog.current_database() \
               AND pid <> pg_catalog.pg_backend_pid() \
               AND state = 'active' \
               AND wait_event_type = 'Lock' \
               AND query LIKE '%starring_product_promotion_publish_v1%' \
             )",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("publish did not enter a lock wait");
}

async fn install_ruleset_head_insert_delay(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        CREATE FUNCTION public.starring_test_delay_ruleset_head_insert()
        RETURNS TRIGGER
        LANGUAGE plpgsql
        AS $function$
        BEGIN
            PERFORM pg_catalog.pg_sleep(0.35);
            RETURN NEW;
        END;
        $function$;

        CREATE TRIGGER automation_ruleset_heads_test_insert_delay
        BEFORE INSERT ON public.automation_ruleset_heads
        FOR EACH ROW
        EXECUTE FUNCTION public.starring_test_delay_ruleset_head_insert();
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn remove_ruleset_head_insert_delay(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TRIGGER automation_ruleset_heads_test_insert_delay
        ON public.automation_ruleset_heads;
        DROP FUNCTION public.starring_test_delay_ruleset_head_insert();
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
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

    let publication_plan = promotion_plan("adapter-publication-key", artifact.clone());
    let publication_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let publication_case = PreparedCase::new(
        &ring,
        publication_plan,
        "adapter-publication-key",
        "request-publication",
        publication_now,
        &SESSION_DIGEST,
    );
    let first_prepared = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &publication_case.access,
                &publication_case.context,
                &publication_case.digests,
                &publication_case.plan,
                &publication_case.admission,
                &publication_case.serialized,
            )
            .await
            .unwrap(),
        &publication_case,
    );

    install_ruleset_head_insert_delay(&pool).await;
    let expiry_probe_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut expiry_probe_access = access_args(expiry_probe_now, &SESSION_DIGEST);
    expiry_probe_access.authority_expires_at = expiry_probe_now + TimeDelta::milliseconds(200);
    let expiry_probe = direct_publish(&pool, &expiry_probe_access, &first_prepared).await;
    assert_eq!(expiry_probe.outcome_code, "access_denied");
    assert!(expiry_probe.publication_projection.is_none());
    assert!(expiry_probe.promotion_record.is_none());
    remove_ruleset_head_insert_delay(&pool).await;
    let denied_write_state = sqlx::query_as::<_, (i64, i64, String, i64)>(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_heads), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_versions), \
         (SELECT stage FROM public.authoring_promotions WHERE id = $1), \
         (SELECT revision FROM public.authoring_promotions WHERE id = $1)",
    )
    .bind(publication_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(denied_write_state, (0, 0, "prepared".to_string(), 1));

    let invalid_digest_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let invalid_digest_access = access_args(invalid_digest_access_now, &SESSION_DIGEST);
    let invalid_digest_stage = ProductPromotionAdmittedStageV1 {
        record: first_prepared.record.clone(),
        admission: first_prepared.admission.clone(),
        admission_digest: "0".repeat(64),
        database_now: first_prepared.database_now,
    };
    let invalid_digest = direct_publish(&pool, &invalid_digest_access, &invalid_digest_stage).await;
    assert_eq!(invalid_digest.outcome_code, "persistence_corrupt");
    assert!(invalid_digest.publication_projection.is_none());
    assert!(invalid_digest.promotion_record.is_none());
    let premature_environment =
        direct_approval_environment(&pool, &invalid_digest_access, &first_prepared).await;
    assert_eq!(premature_environment.outcome_code, "persistence_corrupt");
    assert!(premature_environment.promotion_record.is_none());
    assert!(premature_environment.historical_binding_revision.is_none());
    assert!(premature_environment.historical_resource_bindings.is_none());
    assert!(premature_environment
        .historical_binding_fingerprint
        .is_none());
    assert!(premature_environment.active_version.is_none());
    assert!(premature_environment.active_content_hash.is_none());
    assert!(premature_environment.target_artifact_projection.is_none());
    let rejected_write_state = sqlx::query_as::<_, (i64, i64, String, i64)>(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_heads), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_versions), \
         (SELECT stage FROM public.authoring_promotions WHERE id = $1), \
         (SELECT revision FROM public.authoring_promotions WHERE id = $1)",
    )
    .bind(publication_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rejected_write_state, (0, 0, "prepared".to_string(), 1));

    let mut collision_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_versions \
         DROP CONSTRAINT arv_content_integrity",
    )
    .execute(&mut *collision_transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_heads \
         (guild_id, ruleset_key, next_version) VALUES ('3001', 'ruleset', 2)",
    )
    .execute(&mut *collision_transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ('3001', 'ruleset', 1, 1, \
          pg_catalog.jsonb_build_object('version', 1, 'panels', '[]'::JSONB, \
           'modals', '[]'::JSONB, 'rules', '[]'::JSONB), $1, '1001')",
    )
    .bind(
        publication_case
            .plan
            .intent
            .expected_registry_content_hash
            .to_string(),
    )
    .execute(&mut *collision_transaction)
    .await
    .unwrap();
    let collision_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *collision_transaction)
            .await
            .unwrap();
    let collision_access = access_args(collision_access_now, &SESSION_DIGEST);
    let collision = direct_publish(
        &mut *collision_transaction,
        &collision_access,
        &first_prepared,
    )
    .await;
    assert_eq!(collision.outcome_code, "persistence_corrupt");
    assert!(collision.publication_projection.is_none());
    assert!(collision.promotion_record.is_none());
    let collision_state = sqlx::query_as::<_, (i64, i64, String, i64)>(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_heads), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_versions), \
         (SELECT stage FROM public.authoring_promotions WHERE id = $1), \
         (SELECT revision FROM public.authoring_promotions WHERE id = $1)",
    )
    .bind(publication_case.plan.promotion_id.as_str())
    .fetch_one(&mut *collision_transaction)
    .await
    .unwrap();
    assert_eq!(collision_state, (1, 1, "prepared".to_string(), 1));
    collision_transaction.rollback().await.unwrap();
    let collision_rollback_state = sqlx::query_as::<_, (i64, i64, String, i64)>(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_heads), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_versions), \
         (SELECT stage FROM public.authoring_promotions WHERE id = $1), \
         (SELECT revision FROM public.authoring_promotions WHERE id = $1)",
    )
    .bind(publication_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(collision_rollback_state, (0, 0, "prepared".to_string(), 1));

    seed_drift_session(&pool).await;
    let drift_plan = promotion_plan("adapter-session-drift-key", artifact.clone());
    let drift_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let drift_case = PreparedCase::new(
        &ring,
        drift_plan,
        "adapter-session-drift-key",
        "request-session-drift",
        drift_now,
        &DRIFT_SESSION_DIGEST,
    );
    let drift_prepared = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &drift_case.access,
                &drift_case.context,
                &drift_case.digests,
                &drift_case.plan,
                &drift_case.admission,
                &drift_case.serialized,
            )
            .await
            .unwrap(),
        &drift_case,
    );
    let mut session_drift_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE public.product_auth_sessions \
         SET revoked_at = GREATEST(pg_catalog.clock_timestamp(), last_seen_at), \
             revocation_reason = 'security_revocation' \
         WHERE session_digest = $1",
    )
    .bind(DRIFT_SESSION_DIGEST.as_slice())
    .execute(&mut *session_drift_transaction)
    .await
    .unwrap();
    let drift_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let drift_access = access_args(drift_access_now, &DRIFT_SESSION_DIGEST);
    let drift_publish = direct_publish(&pool, &drift_access, &drift_prepared);
    let release_drift = async {
        await_publish_lock_wait(&pool).await;
        session_drift_transaction.commit().await.unwrap();
    };
    let (drift_publish, ()) = tokio::join!(drift_publish, release_drift);
    assert_eq!(drift_publish.outcome_code, "access_denied");
    assert!(drift_publish.publication_projection.is_none());
    assert!(drift_publish.promotion_record.is_none());
    let drift_write_state = sqlx::query_as::<_, (i64, i64, String, i64)>(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_heads), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_versions), \
         (SELECT stage FROM public.authoring_promotions WHERE id = $1), \
         (SELECT revision FROM public.authoring_promotions WHERE id = $1)",
    )
    .bind(drift_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(drift_write_state, (0, 0, "prepared".to_string(), 1));

    let second_prepared = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &publication_case.access,
                &publication_case.context,
                &publication_case.digests,
                &publication_case.plan,
                &publication_case.admission,
                &publication_case.serialized,
            )
            .await
            .unwrap(),
        &publication_case,
    );
    let publish_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let publish_access = access_args(publish_access_now, &SESSION_DIGEST);
    let first_publish = direct_publish(&pool, &publish_access, &first_prepared);
    let second_publish = direct_publish(&pool, &publish_access, &second_prepared);
    let (first_publish, second_publish) = tokio::join!(first_publish, second_publish);
    let mut publication_outcomes = vec![
        first_publish.outcome_code.clone(),
        second_publish.outcome_code.clone(),
    ];
    publication_outcomes.sort();
    assert_eq!(publication_outcomes, vec!["created", "published_exact"]);
    let first_published =
        decode_product_promotion_publication_v1(first_publish, &first_prepared).unwrap();
    let second_published =
        decode_product_promotion_publication_v1(second_publish, &second_prepared).unwrap();
    assert_eq!(first_published.record, second_published.record);
    assert!(!first_published.final_replay_required);
    let published = ProductPromotionAdmittedStageV1 {
        record: first_published.record,
        admission: first_prepared.admission,
        admission_digest: first_prepared.admission_digest,
        database_now: first_published.database_now,
    };
    let publication_registry_state = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_heads), \
         (SELECT next_version FROM public.automation_ruleset_heads \
          WHERE guild_id = '3001' AND ruleset_key = 'ruleset'), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_versions), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(publication_registry_state, (1, 2, 1, 0));

    let exact_publish_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let exact_publish_access = access_args(exact_publish_access_now, &SESSION_DIGEST);
    let exact_publish = direct_publish(&pool, &exact_publish_access, &published).await;
    assert_eq!(exact_publish.outcome_code, "published_exact");
    decode_product_promotion_publication_v1(exact_publish, &published).unwrap();

    let environment_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let environment_access = access_args(environment_access_now, &SESSION_DIGEST);
    let absent_environment =
        direct_approval_environment(&pool, &environment_access, &published).await;
    assert_eq!(absent_environment.outcome_code, "resolved");
    assert!(absent_environment.active_version.is_none());
    assert!(absent_environment.active_content_hash.is_none());
    let absent_environment =
        decode_product_promotion_approval_environment_v1(absent_environment, &published).unwrap();
    assert_eq!(
        absent_environment.target_artifact.content_hash,
        publication_case.plan.intent.expected_registry_content_hash
    );

    let mut corrupt_pointer_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_activations \
         DISABLE TRIGGER automation_ruleset_activations_assert_product_slot",
    )
    .execute(&mut *corrupt_pointer_transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) VALUES ('3001', 'ruleset', 1)",
    )
    .execute(&mut *corrupt_pointer_transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.automation_ruleset_activations \
         ENABLE TRIGGER automation_ruleset_activations_assert_product_slot",
    )
    .execute(&mut *corrupt_pointer_transaction)
    .await
    .unwrap();
    let corrupt_pointer_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *corrupt_pointer_transaction)
            .await
            .unwrap();
    let corrupt_pointer_access = access_args(corrupt_pointer_access_now, &SESSION_DIGEST);
    let corrupt_pointer = direct_approval_environment(
        &mut *corrupt_pointer_transaction,
        &corrupt_pointer_access,
        &published,
    )
    .await;
    assert_eq!(corrupt_pointer.outcome_code, "persistence_corrupt");
    assert!(corrupt_pointer.promotion_record.is_none());
    assert!(corrupt_pointer.historical_binding_revision.is_none());
    assert!(corrupt_pointer.historical_resource_bindings.is_none());
    assert!(corrupt_pointer.historical_binding_fingerprint.is_none());
    assert!(corrupt_pointer.active_version.is_none());
    assert!(corrupt_pointer.active_content_hash.is_none());
    assert!(corrupt_pointer.target_artifact_projection.is_none());
    corrupt_pointer_transaction.rollback().await.unwrap();

    let pending_plan =
        plan_pending_activation_v1(&published.record, absent_environment.resolved.clone()).unwrap();
    let activation = ActivationRequest::create_product(
        pending_plan.request().create,
        absent_environment.database_now,
    )
    .unwrap();
    let final_record = match pending_plan
        .complete(
            &published.record,
            &PendingActivationReceiptV1 {
                request: activation,
                disposition: PendingActivationDispositionV1::Created,
            },
            absent_environment.database_now,
        )
        .unwrap()
    {
        PendingActivationTransitionV1::ActivationPending {
            expected_record, ..
        } => expected_record,
        PendingActivationTransitionV1::Expired { .. }
        | PendingActivationTransitionV1::RefreshJournal => {
            panic!("expected activation-pending transition")
        }
    };
    let final_record_json = serde_json::to_value(&final_record).unwrap();
    let finalized_count = sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 3, stage = 'activation_pending', \
             record = record || pg_catalog.jsonb_build_object( \
                 'revision', 3, 'stage', $2::JSONB, 'updated_at', $3::TEXT \
             ) \
         WHERE id = $1 AND revision = 2 AND stage = 'published'",
    )
    .bind(publication_case.plan.promotion_id.as_str())
    .bind(sqlx::types::Json(&final_record_json["stage"]))
    .bind(final_record_json["updated_at"].as_str().unwrap())
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(finalized_count, 1);
    let final_publish_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let final_publish_access = access_args(final_publish_access_now, &SESSION_DIGEST);
    let final_publish = direct_publish(&pool, &final_publish_access, &published).await;
    assert_eq!(final_publish.outcome_code, "final_exact");
    assert!(final_publish.promotion_record.is_some());
    let final_publish = decode_product_promotion_publication_v1(final_publish, &published).unwrap();
    assert!(final_publish.final_replay_required);
    assert_eq!(final_publish.record, final_record);

    let reused_plan = promotion_plan("adapter-publication-reused-key", artifact.clone());
    let reused_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let reused_case = PreparedCase::new(
        &ring,
        reused_plan,
        "adapter-publication-reused-key",
        "request-publication-reused",
        reused_now,
        &SESSION_DIGEST,
    );
    let reused_prepared = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &reused_case.access,
                &reused_case.context,
                &reused_case.digests,
                &reused_case.plan,
                &reused_case.admission,
                &reused_case.serialized,
            )
            .await
            .unwrap(),
        &reused_case,
    );
    let reused_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let reused_access = access_args(reused_access_now, &SESSION_DIGEST);
    let reused = direct_publish(&pool, &reused_access, &reused_prepared).await;
    assert_eq!(reused.outcome_code, "reused");
    decode_product_promotion_publication_v1(reused, &reused_prepared).unwrap();
    let reused_registry_state = sqlx::query_as::<_, (i64, i64)>(
        "SELECT \
         (SELECT next_version FROM public.automation_ruleset_heads \
          WHERE guild_id = '3001' AND ruleset_key = 'ruleset'), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_versions)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reused_registry_state, (2, 1));

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
