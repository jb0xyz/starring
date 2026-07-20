use std::collections::VecDeque;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::{Arc, Mutex};

use authoring_promotion::{
    plan_pending_activation_v1, plan_start_promotion_v1, ApprovalPolicyV1,
    AuthenticatedPromotionContext, AuthoringSessionId, AutomationInstallationId, BindingRevision,
    IdempotencyKey, PendingActivationProposalV1, PolicyRevision, PreparedPromotionPlanV1,
    PrincipalId, PromotionRecordV1, SessionGeneration, StartPromotionV1, TenantId,
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
    decode_product_promotion_activation_link_v1, decode_product_promotion_approval_environment_v1,
    decode_product_promotion_publication_v1, ProductPromotionActivationLinkRowV1,
    ProductPromotionActivationStageV1, ProductPromotionAdmittedStageV1,
    ProductPromotionApprovalEnvironmentDecodedV1, ProductPromotionApprovalEnvironmentRowV1,
    ProductPromotionApprovalEnvironmentStageV1, ProductPromotionPrepareStageV1,
    ProductPromotionPublicationRowV1, ProductPromotionReplayStageV1,
};
use crate::product_promotions::store::PostgresProductPromotions;
use crate::MIGRATOR;

mod orchestrator_e2e;

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
            ttl_seconds: NonZeroU64::new(3).unwrap(),
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

async fn direct_activation_link<'executor, ExecutorType>(
    executor: ExecutorType,
    access: &ProductPromotionAccessArgsV1,
    admitted: &ProductPromotionAdmittedStageV1,
    proposal: &PendingActivationProposalV1,
) -> ProductPromotionActivationLinkRowV1
where
    ExecutorType: Executor<'executor, Database = Postgres>,
{
    try_direct_activation_link(executor, access, admitted, proposal)
        .await
        .unwrap()
}

async fn try_direct_activation_link<'executor, ExecutorType>(
    executor: ExecutorType,
    access: &ProductPromotionAccessArgsV1,
    admitted: &ProductPromotionAdmittedStageV1,
    proposal: &PendingActivationProposalV1,
) -> Result<ProductPromotionActivationLinkRowV1, sqlx::Error>
where
    ExecutorType: Executor<'executor, Database = Postgres>,
{
    let envelope = json!({"format_version": 1, "proposal": proposal});
    sqlx::query_as::<_, ProductPromotionActivationLinkRowV1>(
        "SELECT outcome_code, promotion_record, admission_evidence, admission_digest, \
         activation_projection, receipt_projection, audit_evidence_projection, database_now \
         FROM public.starring_product_promotion_activation_link_v1(\
         $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
         $16, $17, $18, $19, $20)",
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
    .bind(sqlx::types::Json(envelope))
    .fetch_one(executor)
    .await
}

async fn insert_unlinked_activation(pool: &PgPool, request: &ActivationRequest) {
    let approval_context = serde_json::to_value(&request.approval_context).unwrap();
    let context = approval_context.get("context").unwrap();
    let observed_version = request
        .observed_active
        .as_ref()
        .map(|observed| i64::from(observed.version.get()));
    let observed_hash = request
        .observed_active
        .as_ref()
        .map(|observed| observed.content_hash.to_hex());
    sqlx::query(
        "INSERT INTO public.activation_requests (\
         id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
         required_approvals, state, created_at, expires_at, observed_active_version, \
         observed_active_hash, authority_kind, link_state_name, approval_context, \
         link_state, promotion_id, promotion_request_digest, approval_payload_digest, \
         approval_context_digest) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8, $9, $10, $11, \
         'product_authoring', 'unlinked', $12, $13, $14, $15, $16, $17)",
    )
    .bind(request.id.as_str())
    .bind(request.target.guild_id.to_string())
    .bind(request.target.ruleset_key.as_str())
    .bind(i64::from(request.target.version.get()))
    .bind(request.target.content_hash.to_hex())
    .bind(request.requester.to_string())
    .bind(i32::try_from(request.required_approvals).unwrap())
    .bind(request.created_at)
    .bind(request.expires_at)
    .bind(observed_version)
    .bind(observed_hash)
    .bind(sqlx::types::Json(&approval_context))
    .bind(sqlx::types::Json(
        serde_json::to_value(&request.link_state).unwrap(),
    ))
    .bind(context["promotion_id"].as_str().unwrap())
    .bind(context["promotion_request_digest"].as_str().unwrap())
    .bind(context["approval_payload_digest"].as_str().unwrap())
    .bind(context["approval_context_digest"].as_str().unwrap())
    .execute(pool)
    .await
    .unwrap();
}

async fn install_exact_applied_pointer(pool: &PgPool, promotion_id: &str) -> (i64, String) {
    let mut transaction = pool.begin().await.unwrap();
    let activation = sqlx::query_as::<_, (String, i64, String)>(
        "SELECT id, target_version, target_content_hash \
         FROM public.activation_requests WHERE promotion_id = $1",
    )
    .bind(promotion_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    let mutation_clock =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.activation_requests DISABLE TRIGGER USER; \
         ALTER TABLE public.runtime_deployments DISABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let applied = sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'applied', applied_at = $2, applied_by = requester_id, \
             completion_kind = 'activated', activation_notices = '[]'::JSONB \
         WHERE id = $1 AND state = 'pending' AND link_state_name = 'linked'",
    )
    .bind(&activation.0)
    .bind(mutation_clock)
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(applied.rows_affected(), 1);
    sqlx::query(
        "INSERT INTO public.runtime_deployments ( \
         deployment_id, tenant_id, installation_id, promotion_id, activation_request_id, \
         installation_authority_revision, guild_id, ruleset_key, target_version, \
         target_content_hash, binding_revision, binding_fingerprint, desired_target_digest, \
         runtime_generation, requested_at, snapshot_format_version, snapshot, revision, phase, \
         policy_revision, created_at, updated_at) \
         SELECT $1, 'tenant', 'installation', $2, $3, 1, '3001', 'ruleset', $4, $5, \
          1, authority.binding_fingerprint, pg_catalog.repeat('d', 64), 1, $6, 1, \
          pg_catalog.jsonb_build_object('fixture', pg_catalog.repeat('x', 64)), \
          1, 'requested', authority.policy_revision, $6, $6 \
         FROM public.automation_installation_authority_versions AS authority \
         WHERE authority.tenant_id = 'tenant' AND authority.installation_id = 'installation' \
           AND authority.revision = 1",
    )
    .bind(format!("race-deployment-{}", &promotion_id[..16]))
    .bind(promotion_id)
    .bind(&activation.0)
    .bind(activation.1)
    .bind(&activation.2)
    .bind(mutation_clock)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE public.runtime_deployments ENABLE TRIGGER USER; \
         ALTER TABLE public.activation_requests ENABLE TRIGGER USER",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_activations \
         (guild_id, ruleset_key, active_version) VALUES ('3001', 'ruleset', $1)",
    )
    .bind(activation.1)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let exact = sqlx::query_scalar::<_, bool>(
        "SELECT public.starring_product_ruleset_slot_exact_v1( \
         'tenant', 'installation', '3001', 'ruleset', $1) \
         AND version.content_hash = public.starring_ruleset_content_hash_v1( \
          version.schema_version, version.definition) \
         AND version.canonical_content_hash = version.content_hash \
         FROM public.automation_ruleset_versions AS version \
         WHERE version.guild_id = '3001' AND version.ruleset_key = 'ruleset' \
           AND version.version = $1",
    )
    .bind(activation.1)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(exact);
    (activation.1, activation.2)
}

async fn await_stage_lock_wait(pool: &PgPool, function_name: &str) {
    for _ in 0..200 {
        let waiting = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS ( \
             SELECT 1 FROM pg_catalog.pg_stat_activity \
             WHERE datname = pg_catalog.current_database() \
               AND pid <> pg_catalog.pg_backend_pid() \
               AND state = 'active' \
               AND wait_event_type = 'Lock' \
               AND query LIKE '%' || $1 || '%' \
             )",
        )
        .bind(function_name)
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("promotion stage did not enter a lock wait");
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
        await_stage_lock_wait(&pool, "starring_product_promotion_publish_v1").await;
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
    let (resolved, target_artifact, environment_database_now) = match absent_environment {
        ProductPromotionApprovalEnvironmentDecodedV1::Resolved {
            resolved,
            target_artifact,
            database_now,
        } => (resolved, target_artifact, database_now),
        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired { .. } => {
            panic!("expected resolved approval environment")
        }
    };
    assert_eq!(
        target_artifact.content_hash,
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

    let pending_plan = plan_pending_activation_v1(&published.record, resolved.clone()).unwrap();
    let activation_environment = ProductPromotionApprovalEnvironmentStageV1 {
        admitted: ProductPromotionAdmittedStageV1 {
            record: published.record.clone(),
            admission: published.admission.clone(),
            admission_digest: published.admission_digest.clone(),
            database_now: environment_database_now,
        },
        resolved,
        target_artifact: *target_artifact,
    };
    let activation_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let activation_access = access_args(activation_access_now, &SESSION_DIGEST);
    let created_activation =
        direct_activation_link(&pool, &activation_access, &published, &pending_plan).await;
    assert_eq!(created_activation.outcome_code, "created");
    let finalized = decode_product_promotion_activation_link_v1(
        created_activation,
        &ring,
        &publication_case.context,
        &activation_access,
        &publication_case.digests,
        &activation_environment,
        &pending_plan,
    )
    .unwrap();
    let final_record = match finalized {
        ProductPromotionActivationStageV1::Finalized(finalized) => finalized.admitted.record,
        ProductPromotionActivationStageV1::FinalReplayRequired(_) => {
            panic!("expected created activation finalization")
        }
        ProductPromotionActivationStageV1::ApprovalEnvironmentChanged => {
            panic!("expected stable created activation environment")
        }
    };
    let durable_activation_state = sqlx::query_as::<_, (String, String, i64, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.link_state_name, \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE endpoint_domain = 'product_promote_v1'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
          WHERE endpoint_domain = 'product_promote_v1'), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events \
          WHERE action = 'promotion.promote'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
          WHERE endpoint_domain = 'product_promote_v1'), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations) \
         FROM public.activation_requests AS activation \
         WHERE activation.promotion_id = $1",
    )
    .bind(publication_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        durable_activation_state,
        ("pending".to_string(), "linked".to_string(), 1, 1, 1, 1, 0)
    );
    let mut final_environment_lock = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM public.authoring_promotions WHERE id = $1 FOR UPDATE")
        .bind(publication_case.plan.promotion_id.as_str())
        .fetch_one(&mut *final_environment_lock)
        .await
        .unwrap();
    let final_expiry_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut final_expiry_access = access_args(final_expiry_now, &SESSION_DIGEST);
    final_expiry_access.authority_expires_at = final_expiry_now + TimeDelta::milliseconds(500);
    let final_expiry_probe = direct_approval_environment(&pool, &final_expiry_access, &published);
    let release_final_environment = async {
        await_stage_lock_wait(&pool, "starring_product_promotion_approval_environment_v1").await;
        tokio::time::sleep(std::time::Duration::from_millis(550)).await;
        final_environment_lock.commit().await.unwrap();
    };
    let (final_expiry_probe, ()) = tokio::join!(final_expiry_probe, release_final_environment);
    assert_eq!(final_expiry_probe.outcome_code, "access_denied");
    assert!(final_expiry_probe.promotion_record.is_none());
    assert!(final_expiry_probe.historical_binding_revision.is_none());
    assert!(final_expiry_probe.historical_resource_bindings.is_none());
    assert!(final_expiry_probe.historical_binding_fingerprint.is_none());
    assert!(final_expiry_probe.active_version.is_none());
    assert!(final_expiry_probe.active_content_hash.is_none());
    assert!(final_expiry_probe.target_artifact_projection.is_none());
    let final_expiry_write_state = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT promotion.stage, promotion.revision, \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events \
          WHERE receipt_id = $2) \
         FROM public.authoring_promotions AS promotion WHERE promotion.id = $1",
    )
    .bind(publication_case.plan.promotion_id.as_str())
    .bind(&publication_case.admission.payload.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        final_expiry_write_state,
        ("activation_pending".to_string(), 3, 1, 1)
    );
    let replay_signal_access_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let replay_signal_access = access_args(replay_signal_access_now, &SESSION_DIGEST);
    let replay_signal =
        direct_activation_link(&pool, &replay_signal_access, &published, &pending_plan).await;
    assert_eq!(replay_signal.outcome_code, "final_replay_required");
    assert!(replay_signal.promotion_record.is_some());
    assert!(replay_signal.admission_evidence.is_some());
    assert!(replay_signal.admission_digest.is_some());
    assert!(replay_signal.activation_projection.is_none());
    assert!(replay_signal.receipt_projection.is_none());
    assert!(replay_signal.audit_evidence_projection.is_none());
    assert!(matches!(
        decode_product_promotion_activation_link_v1(
            replay_signal,
            &ring,
            &publication_case.context,
            &replay_signal_access,
            &publication_case.digests,
            &activation_environment,
            &pending_plan,
        )
        .unwrap(),
        ProductPromotionActivationStageV1::FinalReplayRequired(_)
    ));
    let final_environment =
        direct_approval_environment(&pool, &replay_signal_access, &published).await;
    assert_eq!(final_environment.outcome_code, "final_replay_required");
    assert!(final_environment.promotion_record.is_some());
    assert!(final_environment.historical_binding_revision.is_none());
    assert!(final_environment.historical_resource_bindings.is_none());
    assert!(final_environment.historical_binding_fingerprint.is_none());
    assert!(final_environment.active_version.is_none());
    assert!(final_environment.active_content_hash.is_none());
    assert!(final_environment.target_artifact_projection.is_none());
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
    let reused_publication =
        decode_product_promotion_publication_v1(reused, &reused_prepared).unwrap();
    let reused_published = ProductPromotionAdmittedStageV1 {
        record: reused_publication.record,
        admission: reused_prepared.admission,
        admission_digest: reused_prepared.admission_digest,
        database_now: reused_publication.database_now,
    };
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

    let reused_environment_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let reused_environment_access = access_args(reused_environment_now, &SESSION_DIGEST);
    let reused_environment =
        direct_approval_environment(&pool, &reused_environment_access, &reused_published).await;
    let reused_environment =
        decode_product_promotion_approval_environment_v1(reused_environment, &reused_published)
            .unwrap();
    let (reused_resolved, reused_target, reused_database_now) = match reused_environment {
        ProductPromotionApprovalEnvironmentDecodedV1::Resolved {
            resolved,
            target_artifact,
            database_now,
        } => (resolved, target_artifact, database_now),
        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired { .. } => {
            panic!("expected reusable approval environment")
        }
    };
    let reused_pending =
        plan_pending_activation_v1(&reused_published.record, reused_resolved.clone()).unwrap();
    let reusable_request =
        ActivationRequest::create_product(reused_pending.request().create, reused_database_now)
            .unwrap();
    insert_unlinked_activation(&pool, &reusable_request).await;
    let reused_activation_environment = ProductPromotionApprovalEnvironmentStageV1 {
        admitted: ProductPromotionAdmittedStageV1 {
            record: reused_published.record.clone(),
            admission: reused_published.admission.clone(),
            admission_digest: reused_published.admission_digest.clone(),
            database_now: reused_database_now,
        },
        resolved: reused_resolved,
        target_artifact: *reused_target,
    };
    let reused_activation_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let reused_activation_access = access_args(reused_activation_now, &SESSION_DIGEST);
    let reused_activation = direct_activation_link(
        &pool,
        &reused_activation_access,
        &reused_published,
        &reused_pending,
    )
    .await;
    assert_eq!(reused_activation.outcome_code, "reused");
    assert!(matches!(
        decode_product_promotion_activation_link_v1(
            reused_activation,
            &ring,
            &reused_case.context,
            &reused_activation_access,
            &reused_case.digests,
            &reused_activation_environment,
            &reused_pending,
        )
        .unwrap(),
        ProductPromotionActivationStageV1::Finalized(_)
    ));
    let reused_final_state = sqlx::query_as::<_, (String, String, i64, i64, i64, i64, i64)>(
        "SELECT promotion.stage, activation.link_state_name, \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE endpoint_domain = 'product_promote_v1'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
          WHERE endpoint_domain = 'product_promote_v1'), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events \
          WHERE action = 'promotion.promote'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
          WHERE endpoint_domain = 'product_promote_v1'), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations) \
         FROM public.authoring_promotions AS promotion \
         INNER JOIN public.activation_requests AS activation \
          ON activation.promotion_id = promotion.id \
         WHERE promotion.id = $1",
    )
    .bind(reused_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        reused_final_state,
        (
            "activation_pending".to_string(),
            "linked".to_string(),
            2,
            2,
            2,
            2,
            0,
        )
    );

    let direct_expired_plan = promotion_plan("adapter-direct-expired-key", artifact.clone());
    let direct_expired_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let direct_expired_case = PreparedCase::new(
        &ring,
        direct_expired_plan,
        "adapter-direct-expired-key",
        "request-direct-expired",
        direct_expired_now,
        &SESSION_DIGEST,
    );
    let direct_expired_prepared = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &direct_expired_case.access,
                &direct_expired_case.context,
                &direct_expired_case.digests,
                &direct_expired_case.plan,
                &direct_expired_case.admission,
                &direct_expired_case.serialized,
            )
            .await
            .unwrap(),
        &direct_expired_case,
    );
    let direct_expired_publish_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let direct_expired_publish_access = access_args(direct_expired_publish_now, &SESSION_DIGEST);
    let direct_expired_publication = direct_publish(
        &pool,
        &direct_expired_publish_access,
        &direct_expired_prepared,
    )
    .await;
    assert_eq!(direct_expired_publication.outcome_code, "reused");
    let direct_expired_publication = decode_product_promotion_publication_v1(
        direct_expired_publication,
        &direct_expired_prepared,
    )
    .unwrap();
    let direct_expired_published = ProductPromotionAdmittedStageV1 {
        record: direct_expired_publication.record,
        admission: direct_expired_prepared.admission,
        admission_digest: direct_expired_prepared.admission_digest,
        database_now: direct_expired_publication.database_now,
    };
    let direct_expired_environment_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let direct_expired_environment_access =
        access_args(direct_expired_environment_now, &SESSION_DIGEST);
    let direct_expired_environment = direct_approval_environment(
        &pool,
        &direct_expired_environment_access,
        &direct_expired_published,
    )
    .await;
    let direct_expired_environment = decode_product_promotion_approval_environment_v1(
        direct_expired_environment,
        &direct_expired_published,
    )
    .unwrap();
    let (expired_resolved, expired_target, expired_database_now) = match direct_expired_environment
    {
        ProductPromotionApprovalEnvironmentDecodedV1::Resolved {
            resolved,
            target_artifact,
            database_now,
        } => (resolved, target_artifact, database_now),
        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired { .. } => {
            panic!("expected expirable approval environment")
        }
    };
    let direct_expired_pending =
        plan_pending_activation_v1(&direct_expired_published.record, expired_resolved.clone())
            .unwrap();
    let expirable_request = ActivationRequest::create_product(
        direct_expired_pending.request().create,
        expired_database_now,
    )
    .unwrap();
    insert_unlinked_activation(&pool, &expirable_request).await;
    tokio::time::sleep(std::time::Duration::from_millis(3_100)).await;
    let direct_expired_activation_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let direct_expired_activation_access =
        access_args(direct_expired_activation_now, &SESSION_DIGEST);
    let direct_expired_activation_environment = ProductPromotionApprovalEnvironmentStageV1 {
        admitted: ProductPromotionAdmittedStageV1 {
            record: direct_expired_published.record.clone(),
            admission: direct_expired_published.admission.clone(),
            admission_digest: direct_expired_published.admission_digest.clone(),
            database_now: expired_database_now,
        },
        resolved: expired_resolved,
        target_artifact: *expired_target,
    };
    let direct_expired_activation = direct_activation_link(
        &pool,
        &direct_expired_activation_access,
        &direct_expired_published,
        &direct_expired_pending,
    )
    .await;
    assert_eq!(direct_expired_activation.outcome_code, "reused");
    let direct_expired_final = decode_product_promotion_activation_link_v1(
        direct_expired_activation,
        &ring,
        &direct_expired_case.context,
        &direct_expired_activation_access,
        &direct_expired_case.digests,
        &direct_expired_activation_environment,
        &direct_expired_pending,
    )
    .unwrap();
    let direct_expired_record = match direct_expired_final {
        ProductPromotionActivationStageV1::Finalized(finalized) => finalized.admitted.record,
        ProductPromotionActivationStageV1::FinalReplayRequired(_) => {
            panic!("expected direct expired finalization")
        }
        ProductPromotionActivationStageV1::ApprovalEnvironmentChanged => {
            panic!("expected stable direct expired activation environment")
        }
    };
    assert!(matches!(
        direct_expired_record.stage,
        authoring_promotion::PromotionStageV1::Expired { .. }
    ));
    let direct_expired_state = sqlx::query_as::<_, (String, String, String, i64, i64)>(
        "SELECT promotion.stage, activation.state, activation.link_state_name, \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE endpoint_domain = 'product_promote_v1'), \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations) \
         FROM public.authoring_promotions AS promotion \
         INNER JOIN public.activation_requests AS activation \
          ON activation.promotion_id = promotion.id \
         WHERE promotion.id = $1",
    )
    .bind(direct_expired_case.plan.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        direct_expired_state,
        (
            "expired".to_string(),
            "expired".to_string(),
            "unlinked".to_string(),
            3,
            0,
        )
    );

    let collision_plan = promotion_plan("adapter-activation-collision-key", artifact.clone());
    let collision_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let collision_case = PreparedCase::new(
        &ring,
        collision_plan,
        "adapter-activation-collision-key",
        "request-activation-collision",
        collision_now,
        &SESSION_DIGEST,
    );
    let collision_prepared = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &collision_case.access,
                &collision_case.context,
                &collision_case.digests,
                &collision_case.plan,
                &collision_case.admission,
                &collision_case.serialized,
            )
            .await
            .unwrap(),
        &collision_case,
    );
    let collision_publish_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let collision_publish_access = access_args(collision_publish_now, &SESSION_DIGEST);
    let collision_publication =
        direct_publish(&pool, &collision_publish_access, &collision_prepared).await;
    let collision_publication =
        decode_product_promotion_publication_v1(collision_publication, &collision_prepared)
            .unwrap();
    let collision_published = ProductPromotionAdmittedStageV1 {
        record: collision_publication.record,
        admission: collision_prepared.admission,
        admission_digest: collision_prepared.admission_digest,
        database_now: collision_publication.database_now,
    };
    let collision_environment_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let collision_environment_access = access_args(collision_environment_now, &SESSION_DIGEST);
    let collision_environment =
        direct_approval_environment(&pool, &collision_environment_access, &collision_published)
            .await;
    let collision_environment = decode_product_promotion_approval_environment_v1(
        collision_environment,
        &collision_published,
    )
    .unwrap();
    let collision_resolved = match collision_environment {
        ProductPromotionApprovalEnvironmentDecodedV1::Resolved { resolved, .. } => resolved,
        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired { .. } => {
            panic!("expected collision approval environment")
        }
    };
    let collision_pending =
        plan_pending_activation_v1(&collision_published.record, collision_resolved).unwrap();
    let mut projection_transaction = pool.begin().await.unwrap();
    let projection_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&mut *projection_transaction)
            .await
            .unwrap();
    let projection_access = access_args(projection_now, &SESSION_DIGEST);
    let collision_projection = direct_activation_link(
        &mut *projection_transaction,
        &projection_access,
        &collision_published,
        &collision_pending,
    )
    .await;
    assert_eq!(collision_projection.outcome_code, "created");
    projection_transaction.rollback().await.unwrap();
    let promotion_projection = collision_projection.promotion_record.unwrap();
    let admission_projection = collision_projection.admission_evidence.unwrap();
    let activation_projection = collision_projection.activation_projection.unwrap();
    let receipt_projection = collision_projection.receipt_projection.unwrap();
    let audit_projection = collision_projection.audit_evidence_projection.unwrap();
    let collision_seed_outcome = sqlx::query_scalar::<_, String>(
        "SELECT outcome_code \
         FROM public.starring_product_promotion_finalize_receipt_v1($1, $2, $3, $4, $5)",
    )
    .bind(&admission_projection)
    .bind(&promotion_projection)
    .bind(&activation_projection)
    .bind(&receipt_projection)
    .bind(&audit_projection)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(collision_seed_outcome, "created");
    let collision_activation_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let collision_activation_access = access_args(collision_activation_now, &SESSION_DIGEST);
    let collision_error = match try_direct_activation_link(
        &pool,
        &collision_activation_access,
        &collision_published,
        &collision_pending,
    )
    .await
    {
        Ok(_) => panic!("receipt, alias, and audit collisions must fail closed"),
        Err(error) => error,
    };
    assert!(matches!(
        &collision_error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23514")
    ));
    let collision_state = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
        "SELECT promotion.stage, \
         (SELECT pg_catalog.count(*) FROM public.activation_requests \
          WHERE promotion_id = promotion.id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
          WHERE receipt_id = $2) \
         FROM public.authoring_promotions AS promotion WHERE promotion.id = $1",
    )
    .bind(collision_case.plan.promotion_id.as_str())
    .bind(&collision_case.admission.payload.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(collision_state, ("published".to_string(), 0, 1, 1, 1, 1));
    let collision_pointer_count = sqlx::query_scalar::<_, i64>(
        "SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(collision_pointer_count, 0);

    let concurrent_plan = promotion_plan("adapter-activation-concurrent-key", artifact.clone());
    let concurrent_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let concurrent_case = PreparedCase::new(
        &ring,
        concurrent_plan,
        "adapter-activation-concurrent-key",
        "request-activation-concurrent",
        concurrent_now,
        &SESSION_DIGEST,
    );
    let concurrent_prepared = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &concurrent_case.access,
                &concurrent_case.context,
                &concurrent_case.digests,
                &concurrent_case.plan,
                &concurrent_case.admission,
                &concurrent_case.serialized,
            )
            .await
            .unwrap(),
        &concurrent_case,
    );
    let concurrent_publish_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let concurrent_publish_access = access_args(concurrent_publish_now, &SESSION_DIGEST);
    let concurrent_publication =
        direct_publish(&pool, &concurrent_publish_access, &concurrent_prepared).await;
    let concurrent_publication =
        decode_product_promotion_publication_v1(concurrent_publication, &concurrent_prepared)
            .unwrap();
    let concurrent_published = ProductPromotionAdmittedStageV1 {
        record: concurrent_publication.record,
        admission: concurrent_prepared.admission,
        admission_digest: concurrent_prepared.admission_digest,
        database_now: concurrent_publication.database_now,
    };
    let concurrent_environment_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let concurrent_environment_access = access_args(concurrent_environment_now, &SESSION_DIGEST);
    let concurrent_environment =
        direct_approval_environment(&pool, &concurrent_environment_access, &concurrent_published)
            .await;
    let concurrent_environment = decode_product_promotion_approval_environment_v1(
        concurrent_environment,
        &concurrent_published,
    )
    .unwrap();
    let concurrent_resolved = match concurrent_environment {
        ProductPromotionApprovalEnvironmentDecodedV1::Resolved { resolved, .. } => resolved,
        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired { .. } => {
            panic!("expected concurrent approval environment")
        }
    };
    let concurrent_pending =
        plan_pending_activation_v1(&concurrent_published.record, concurrent_resolved).unwrap();
    let concurrent_activation_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let concurrent_activation_access = access_args(concurrent_activation_now, &SESSION_DIGEST);
    let (concurrent_first, concurrent_second) = tokio::join!(
        direct_activation_link(
            &pool,
            &concurrent_activation_access,
            &concurrent_published,
            &concurrent_pending,
        ),
        direct_activation_link(
            &pool,
            &concurrent_activation_access,
            &concurrent_published,
            &concurrent_pending,
        )
    );
    let mut concurrent_outcomes = [
        concurrent_first.outcome_code.as_str(),
        concurrent_second.outcome_code.as_str(),
    ];
    concurrent_outcomes.sort_unstable();
    assert_eq!(concurrent_outcomes, ["created", "final_replay_required"]);
    let concurrent_state = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64)>(
        "SELECT promotion.stage, \
             (SELECT pg_catalog.count(*) FROM public.activation_requests \
              WHERE promotion_id = promotion.id), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
              WHERE receipt_id = $2), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipt_idempotency_aliases \
              WHERE receipt_id = $2), \
             (SELECT pg_catalog.count(*) FROM public.product_audit_events \
              WHERE receipt_id = $2), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
              WHERE receipt_id = $2), \
             (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations) \
             FROM public.authoring_promotions AS promotion WHERE promotion.id = $1",
    )
    .bind(concurrent_case.plan.promotion_id.as_str())
    .bind(&concurrent_case.admission.payload.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        concurrent_state,
        ("activation_pending".to_string(), 1, 1, 1, 1, 1, 0)
    );

    let environment_race_plan =
        promotion_plan("adapter-approval-environment-race-key", artifact.clone());
    let environment_race_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let environment_race_case = PreparedCase::new(
        &ring,
        environment_race_plan,
        "adapter-approval-environment-race-key",
        "request-approval-environment-race",
        environment_race_now,
        &SESSION_DIGEST,
    );
    let environment_race_prepared = admitted_prepare_stage(
        adapter
            .execute_prepare_stage_v1(
                &environment_race_case.access,
                &environment_race_case.context,
                &environment_race_case.digests,
                &environment_race_case.plan,
                &environment_race_case.admission,
                &environment_race_case.serialized,
            )
            .await
            .unwrap(),
        &environment_race_case,
    );
    let environment_race_publish_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let environment_race_publish_access =
        access_args(environment_race_publish_now, &SESSION_DIGEST);
    let environment_race_publication = direct_publish(
        &pool,
        &environment_race_publish_access,
        &environment_race_prepared,
    )
    .await;
    let environment_race_publication = decode_product_promotion_publication_v1(
        environment_race_publication,
        &environment_race_prepared,
    )
    .unwrap();
    let environment_race_published = ProductPromotionAdmittedStageV1 {
        record: environment_race_publication.record,
        admission: environment_race_prepared.admission,
        admission_digest: environment_race_prepared.admission_digest,
        database_now: environment_race_publication.database_now,
    };
    let old_environment_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let old_environment_access = access_args(old_environment_now, &SESSION_DIGEST);
    let old_environment =
        direct_approval_environment(&pool, &old_environment_access, &environment_race_published)
            .await;
    assert!(old_environment.active_version.is_none());
    assert!(old_environment.active_content_hash.is_none());
    let old_environment = decode_product_promotion_approval_environment_v1(
        old_environment,
        &environment_race_published,
    )
    .unwrap();
    let (old_resolved, old_target, old_database_now) = match old_environment {
        ProductPromotionApprovalEnvironmentDecodedV1::Resolved {
            resolved,
            target_artifact,
            database_now,
        } => (resolved, target_artifact, database_now),
        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired { .. } => {
            panic!("expected initial race approval environment")
        }
    };
    let old_pending =
        plan_pending_activation_v1(&environment_race_published.record, old_resolved.clone())
            .unwrap();
    let old_activation_environment = ProductPromotionApprovalEnvironmentStageV1 {
        admitted: ProductPromotionAdmittedStageV1 {
            record: environment_race_published.record.clone(),
            admission: environment_race_published.admission.clone(),
            admission_digest: environment_race_published.admission_digest.clone(),
            database_now: old_database_now,
        },
        resolved: old_resolved,
        target_artifact: *old_target,
    };
    let (active_version, active_content_hash) =
        install_exact_applied_pointer(&pool, concurrent_case.plan.promotion_id.as_str()).await;
    let mut changed_environment_lock = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT active_version FROM public.automation_ruleset_activations \
         WHERE guild_id = '3001' AND ruleset_key = 'ruleset' FOR UPDATE",
    )
    .fetch_one(&mut *changed_environment_lock)
    .await
    .unwrap();
    let changed_expiry_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut changed_expiry_access = access_args(changed_expiry_now, &SESSION_DIGEST);
    changed_expiry_access.authority_expires_at = changed_expiry_now + TimeDelta::milliseconds(500);
    let changed_expiry_probe = direct_activation_link(
        &pool,
        &changed_expiry_access,
        &environment_race_published,
        &old_pending,
    );
    let release_changed_environment = async {
        await_stage_lock_wait(&pool, "starring_product_promotion_activation_link_v1").await;
        tokio::time::sleep(std::time::Duration::from_millis(550)).await;
        changed_environment_lock.commit().await.unwrap();
    };
    let (changed_expiry_probe, ()) =
        tokio::join!(changed_expiry_probe, release_changed_environment);
    assert_eq!(changed_expiry_probe.outcome_code, "access_denied");
    assert!(changed_expiry_probe.promotion_record.is_none());
    assert!(changed_expiry_probe.admission_evidence.is_none());
    assert!(changed_expiry_probe.admission_digest.is_none());
    assert!(changed_expiry_probe.activation_projection.is_none());
    assert!(changed_expiry_probe.receipt_projection.is_none());
    assert!(changed_expiry_probe.audit_evidence_projection.is_none());
    let changed_expiry_write_state = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        "SELECT promotion.stage, promotion.revision, \
         (SELECT pg_catalog.count(*) FROM public.activation_requests \
          WHERE promotion_id = promotion.id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events \
          WHERE receipt_id = $2) \
         FROM public.authoring_promotions AS promotion WHERE promotion.id = $1",
    )
    .bind(environment_race_case.plan.promotion_id.as_str())
    .bind(&environment_race_case.admission.payload.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        changed_expiry_write_state,
        ("published".to_string(), 2, 0, 0, 0)
    );
    let stale_environment_activation_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let stale_environment_activation_access =
        access_args(stale_environment_activation_now, &SESSION_DIGEST);
    let stale_environment_signal = direct_activation_link(
        &pool,
        &stale_environment_activation_access,
        &environment_race_published,
        &old_pending,
    )
    .await;
    assert_eq!(
        stale_environment_signal.outcome_code,
        "approval_environment_changed"
    );
    assert!(stale_environment_signal.promotion_record.is_none());
    assert!(stale_environment_signal.admission_evidence.is_none());
    assert!(stale_environment_signal.admission_digest.is_none());
    assert!(stale_environment_signal.activation_projection.is_none());
    assert!(stale_environment_signal.receipt_projection.is_none());
    assert!(stale_environment_signal.audit_evidence_projection.is_none());
    assert!(matches!(
        decode_product_promotion_activation_link_v1(
            stale_environment_signal,
            &ring,
            &environment_race_case.context,
            &stale_environment_activation_access,
            &environment_race_case.digests,
            &old_activation_environment,
            &old_pending,
        )
        .unwrap(),
        ProductPromotionActivationStageV1::ApprovalEnvironmentChanged
    ));
    let mut polluted_environment_signal = direct_activation_link(
        &pool,
        &stale_environment_activation_access,
        &environment_race_published,
        &old_pending,
    )
    .await;
    polluted_environment_signal.promotion_record =
        Some(sqlx::types::Json(json!({"unexpected": true})));
    assert_eq!(
        decode_product_promotion_activation_link_v1(
            polluted_environment_signal,
            &ring,
            &environment_race_case.context,
            &stale_environment_activation_access,
            &environment_race_case.digests,
            &old_activation_environment,
            &old_pending,
        )
        .unwrap_err(),
        authoring_application::AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
    );
    let stale_environment_state = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        "SELECT promotion.stage, \
         (SELECT pg_catalog.count(*) FROM public.activation_requests \
          WHERE promotion_id = promotion.id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events \
          WHERE receipt_id = $2), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
          WHERE receipt_id = $2) \
         FROM public.authoring_promotions AS promotion WHERE promotion.id = $1",
    )
    .bind(environment_race_case.plan.promotion_id.as_str())
    .bind(&environment_race_case.admission.payload.receipt_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stale_environment_state,
        ("published".to_string(), 0, 0, 0, 0)
    );
    let refreshed_environment_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let refreshed_environment_access = access_args(refreshed_environment_now, &SESSION_DIGEST);
    let refreshed_environment = direct_approval_environment(
        &pool,
        &refreshed_environment_access,
        &environment_race_published,
    )
    .await;
    assert_eq!(refreshed_environment.active_version, Some(active_version));
    assert_eq!(
        refreshed_environment.active_content_hash.as_deref(),
        Some(active_content_hash.as_str())
    );
    let refreshed_environment = decode_product_promotion_approval_environment_v1(
        refreshed_environment,
        &environment_race_published,
    )
    .unwrap();
    let (refreshed_resolved, refreshed_target, refreshed_database_now) = match refreshed_environment
    {
        ProductPromotionApprovalEnvironmentDecodedV1::Resolved {
            resolved,
            target_artifact,
            database_now,
        } => (resolved, target_artifact, database_now),
        ProductPromotionApprovalEnvironmentDecodedV1::FinalReplayRequired { .. } => {
            panic!("expected refreshed race approval environment")
        }
    };
    let refreshed_pending = plan_pending_activation_v1(
        &environment_race_published.record,
        refreshed_resolved.clone(),
    )
    .unwrap();
    let refreshed_activation_environment = ProductPromotionApprovalEnvironmentStageV1 {
        admitted: ProductPromotionAdmittedStageV1 {
            record: environment_race_published.record.clone(),
            admission: environment_race_published.admission.clone(),
            admission_digest: environment_race_published.admission_digest.clone(),
            database_now: refreshed_database_now,
        },
        resolved: refreshed_resolved,
        target_artifact: *refreshed_target,
    };
    let refreshed_activation_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(&pool)
            .await
            .unwrap();
    let refreshed_activation_access = access_args(refreshed_activation_now, &SESSION_DIGEST);
    let refreshed_activation = direct_activation_link(
        &pool,
        &refreshed_activation_access,
        &environment_race_published,
        &refreshed_pending,
    )
    .await;
    assert_eq!(refreshed_activation.outcome_code, "created");
    assert!(matches!(
        decode_product_promotion_activation_link_v1(
            refreshed_activation,
            &ring,
            &environment_race_case.context,
            &refreshed_activation_access,
            &environment_race_case.digests,
            &refreshed_activation_environment,
            &refreshed_pending,
        )
        .unwrap(),
        ProductPromotionActivationStageV1::Finalized(_)
    ));
    let refreshed_environment_state = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
        "SELECT promotion.stage, \
             (SELECT pg_catalog.count(*) FROM public.activation_requests \
              WHERE promotion_id = promotion.id), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipts \
              WHERE receipt_id = $2), \
             (SELECT pg_catalog.count(*) FROM public.product_audit_events \
              WHERE receipt_id = $2), \
             (SELECT pg_catalog.count(*) FROM public.product_action_receipt_audit_evidence \
              WHERE receipt_id = $2), \
             (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations \
              WHERE guild_id = '3001' AND ruleset_key = 'ruleset' \
                AND active_version = $3) \
             FROM public.authoring_promotions AS promotion WHERE promotion.id = $1",
    )
    .bind(environment_race_case.plan.promotion_id.as_str())
    .bind(&environment_race_case.admission.payload.receipt_id)
    .bind(active_version)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        refreshed_environment_state,
        ("activation_pending".to_string(), 1, 1, 1, 1, 1)
    );

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
