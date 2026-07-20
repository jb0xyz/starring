use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use authoring_application::{
    ApplyProductPromotionV1, ApprovalPayloadDigestV1, ApproveProductPromotionV1,
    AuthenticatedActorV1, AuthenticationError, AuthenticationPort, AuthorizedApprovalPreviewV1,
    AuthorizedDeploymentStatusV1, AuthorizedProductStatusV1, CapabilityV1, DeploymentStatusPort,
    DeploymentStatusPortError, DeploymentStatusProjectionV1, DeploymentStatusV1,
    ExactDeploymentSelectorV1, InstallationSelectorV1, MutationAuthenticationPort,
    ProductApplicationError,
    ProductApprovalPreviewV1, ProductControlApplication, ProductControlPortError,
    ProductCandidateErrorCodeV1, ProductDecisionPhaseV1, ProductDecisionProjectionV1,
    ProductDecisionQueryPort, ProductIdempotencyKeyV1, ProductRequestIdV1, ProductRevisionV1,
    ProductStatusQueryV1, ProductStatusV1, PromotionSelectorV1,
};
use authoring_application_discord::{
    AuthorityClock, DiscordApplicationIdV1, DiscordAuthorityClientError, DiscordAuthorityConfigV1,
    DiscordAuthoritySourceError, DiscordBotUserIdV1, DiscordGuildApplyAuthoritySnapshotV1,
    DiscordGuildAuthorityAdapter, DiscordGuildAuthorityClient, DiscordGuildAuthoritySnapshotV1,
    DiscordRoleSnapshotV1, FreshDiscordAuthorityEvidenceV1, InstallationAuthorityRecordV1,
    InstallationAuthoritySource,
};
use authoring_application_postgres::{
    digest_opaque_session_credential_v1, AuthenticationReadinessErrorV1,
    InstallationAuthorityReadinessErrorV1, PostgresAuthentication,
    PostgresAuthenticationConfig, PostgresInstallationAuthoritySource,
    PostgresInstallationAuthoritySourceConfig, PostgresProductDecisions,
    PostgresProductDeploymentStatuses, ProductDecisionDatabasePoolsV1,
    ProductDecisionDigestKeyV1, ProductDecisionDigestKeyringV1,
    ProductDecisionReadinessErrorV1, MIGRATOR,
};
use authoring_promotion::{
    approval_payload_digest_v1, ApprovalPolicyV1, AuthenticatedPromotionContext,
    AuthoringEvidenceV1, AuthoringHash, AuthoringPreviewSummaryV1, AuthoringPreviewV1,
    AuthoringSessionId, AutomationInstallationId, BindingRevision, IdempotencyScopeDigest,
    PolicyRevision, PrincipalId, ProductApprovalPayloadV1, PromotionId, PromotionIntentV1,
    PromotionRecordV1, PromotionRequestDigest, PublicationDispositionV1, PublicationRecordV1,
    SessionGeneration, TenantId,
};
use automation_ruleset::{
    content_hash, RuleSetKey, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, ControllerId,
    DeploymentId, DeploymentRevision, DrainAttestationV1, FencingToken, GatewayReadyAttestationV1,
    GatewayReadyKindV1, InstallationId as RuntimeInstallationId, PanelCertificateId,
    PanelCertificateV1, PreflightAttestationV1, ProcessInstanceId, RuntimeFailureId,
    RuntimeFailureKindV1, RuntimeGeneration, TenantId as RuntimeTenantId,
};
use automation_runtime_convergence_postgres::{
    ClaimDeploymentV1, DeploymentMutationV1, GatewayShardIdV1, LiveMetadataV1,
    MarkServingDisconnectedV1, PanelReportDigestV1, PostgresRuntimeConvergence,
    PostgresRuntimeConvergenceConfigV1, RecoverStaleLiveV1, RuntimeBuildRevisionV1,
    RuntimeDeploymentScopeV1, SubmitDeploymentMutationV1, SubmitLiveAttestationV1,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, TimeDelta, Utc};
use design_harness::IntentRequestedOutcome;
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, Permissions, RoleId, UserId};
use futures::FutureExt;
use hmac::{Hmac, Mac};
use resource_resolution::{
    approval_binding_fingerprint_v1, resource_binding_fingerprint_v2, ResolvedApprovalBinding,
    ResourceBindingMap,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::types::Json;
use sqlx::Connection;

const IDEMPOTENCY_DOMAIN: &[u8] = b"starring.product.approval.idempotency.v1";
const SEMANTIC_REQUEST_DOMAIN: &[u8] = b"starring.product.approval.request.v1";
const RECEIPT_ID_DOMAIN: &[u8] = b"starring.product.approval.receipt.v1";
const AUDIT_EVENT_ID_DOMAIN: &[u8] = b"starring.product.approval.audit.v1";
const SESSION_SUBJECT_DOMAIN: &[u8] = b"starring.product.session.subject.v1";
const KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.approval.digest-key-fingerprint.v1";
const AUTHORITY_OBSERVATION_DOMAIN: &[u8] = b"starring.discord-authority.v1";
const IDEMPOTENCY_SCOPE_DOMAIN: &[u8] = b"starring.authoring_promotion.scope.v1\0";
const PROMOTION_REQUEST_DOMAIN: &[u8] = b"starring.authoring_promotion.request.v1\0";
const ACTIVATION_REQUEST_DOMAIN: &[u8] = b"starring.authoring_promotion.activation_request.v1\0";
const APPROVAL_POLICY_DOMAIN: &[u8] = b"starring.activation.approval_policy.v1\0";
const APPROVAL_CONTEXT_DOMAIN: &[u8] = b"starring.activation.approval_context.v1\0";

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

struct ProductControlTestDatabase {
    name: String,
    administrator: PgConnection,
    pool: PgPool,
}

async fn isolated_product_control_database(label: &str) -> ProductControlTestDatabase {
    assert!(
        !label.is_empty()
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let bounded_label = label.chars().take(16).collect::<String>();
    let name = format!("starring_control_{bounded_label}_test_{}", suffix());
    assert!(
        name.len() <= 63
            && name.starts_with("starring_")
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
    ProductControlTestDatabase {
        name,
        administrator,
        pool,
    }
}

async fn drop_isolated_product_control_database(database: ProductControlTestDatabase) {
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

#[derive(Clone, Copy)]
struct SubmicrosecondClock;

impl AuthorityClock for SubmicrosecondClock {
    fn now(&self) -> DateTime<Utc> {
        let now = Utc::now();
        DateTime::from_timestamp_micros(now.timestamp_micros()).unwrap()
            + TimeDelta::nanoseconds(123)
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}

fn sha256_hex(seed: &str) -> String {
    lower_hex(&Sha256::digest(seed.as_bytes()))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        value => value,
    }
}

fn update_sha256(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap().to_be_bytes());
    hasher.update(value);
}

fn update_hmac(hasher: &mut Hmac<Sha256>, value: &[u8]) {
    Mac::update(hasher, &u64::try_from(value.len()).unwrap().to_be_bytes());
    Mac::update(hasher, value);
}

fn canonical_digest(domain: &[u8], value: &impl Serialize) -> String {
    let value = serde_json::to_value(value).unwrap();
    let bytes = serde_json::to_vec(&canonicalize(value)).unwrap();
    let mut hasher = Sha256::new();
    update_sha256(&mut hasher, domain);
    update_sha256(&mut hasher, &bytes);
    lower_hex(&hasher.finalize())
}

fn unkeyed_bytes(domain: &[u8], fields: &[Vec<u8>]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    update_sha256(&mut hasher, domain);
    for field in fields {
        update_sha256(&mut hasher, field);
    }
    hasher.finalize().to_vec()
}

fn unkeyed_hex(domain: &[u8], fields: &[Vec<u8>]) -> String {
    lower_hex(&unkeyed_bytes(domain, fields))
}

fn keyed_hex(secret: &[u8; 32], domain: &[u8], fields: &[Vec<u8>]) -> String {
    let mut hasher = <Hmac<Sha256> as Mac>::new_from_slice(secret).unwrap();
    update_hmac(&mut hasher, domain);
    for field in fields {
        update_hmac(&mut hasher, field);
    }
    lower_hex(&hasher.finalize().into_bytes())
}

fn bytes(value: impl AsRef<[u8]>) -> Vec<u8> {
    value.as_ref().to_vec()
}

async fn insert_activation_pending_promotion(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &str,
    request_digest: &str,
    tenant_id: &str,
    installation_id: &str,
    principal_id: &str,
    record: &Value,
) {
    let mut prepared = record.clone();
    prepared["revision"] = json!(1);
    prepared["stage"] = json!({"state": "prepared"});
    prepared["updated_at"] = prepared["created_at"].clone();
    let mut published = record.clone();
    published["revision"] = json!(2);
    published["stage"] = json!({
        "state": "published",
        "publication": record["stage"]["publication"].clone()
    });
    published["updated_at"] = published["created_at"].clone();
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, \
          installation_id, principal_id, record) \
         VALUES ($1, 1, 1, 'prepared', $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(request_digest)
    .bind(tenant_id)
    .bind(installation_id)
    .bind(principal_id)
    .bind(Json(&prepared))
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 2, stage = 'published', record = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(Json(&published))
    .execute(&mut **transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 3, stage = 'activation_pending', record = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(Json(record))
    .execute(&mut **transaction)
    .await
    .unwrap();
}

#[derive(Clone)]
struct Fixture {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    promotion_id: PromotionId,
    activation_id: String,
    approver_principal: PrincipalId,
    approver_user: UserId,
    application_id: DiscordApplicationIdV1,
    guild_id: GuildId,
    manager_role_id: RoleId,
    authority_revision: NonZeroU64,
    authority_digest: String,
    authority_binding_fingerprint: String,
    payload_digest: String,
    payload: ProductApprovalPayloadV1,
    credential: String,
    csrf: String,
    session_digest: [u8; 32],
}

async fn seed_fixture(pool: &PgPool) -> Fixture {
    let suffix = suffix();
    let numeric_tail = suffix[suffix.len().saturating_sub(8)..]
        .parse::<u64>()
        .unwrap();
    let tenant_id = TenantId::parse(&format!("tenant-e2e-{suffix}")).unwrap();
    let installation_id =
        AutomationInstallationId::parse(&format!("installation-e2e-{suffix}")).unwrap();
    let requester_principal = PrincipalId::parse(&format!("requester-e2e-{suffix}")).unwrap();
    let approver_principal = PrincipalId::parse(&format!("approver-e2e-{suffix}")).unwrap();
    let requester_user = UserId(6_000_000_000 + numeric_tail);
    let approver_user = UserId(7_000_000_000 + numeric_tail);
    let application_id = DiscordApplicationIdV1::new(8_000_000_000 + numeric_tail).unwrap();
    let guild_id = GuildId(9_000_000_000 + numeric_tail);
    let manager_role_id = RoleId(guild_id.0 + 1);
    let channel_id = ChannelId(guild_id.0 + 2);
    let ruleset_key = RuleSetKey::parse(&format!(
        "e2e_{}",
        &suffix[suffix.len().saturating_sub(20)..]
    ))
    .unwrap();
    let session_id = AuthoringSessionId::parse(&format!("session-e2e-{suffix}")).unwrap();
    let credential_secret: [u8; 32] = Sha256::digest(format!("credential:{suffix}")).into();
    let csrf_secret: [u8; 32] = Sha256::digest(format!("csrf:{suffix}")).into();
    let credential = URL_SAFE_NO_PAD.encode(credential_secret);
    let csrf = URL_SAFE_NO_PAD.encode(csrf_secret);
    let session_digest = digest_opaque_session_credential_v1(&credential)
        .unwrap()
        .into_bytes();
    let csrf_digest = digest_opaque_session_credential_v1(&csrf)
        .unwrap()
        .into_bytes();
    let oauth_state = Sha256::digest(format!("oauth-state:{suffix}")).to_vec();
    let oauth_nonce = Sha256::digest(format!("oauth-nonce:{suffix}")).to_vec();

    let mut resource_bindings = ResourceBindingMap::default();
    resource_bindings
        .channel_bindings
        .insert(ResourceKey("community_hub".to_string()), channel_id);
    let authority_binding_fingerprint = resource_binding_fingerprint_v2(&resource_bindings);
    let stored_resource_bindings = json!({
        "role_bindings": {},
        "channel_bindings": {"community_hub": channel_id.to_string()}
    });
    let required_bindings = vec![ResolvedApprovalBinding::Channel {
        key: ResourceKey("community_hub".to_string()),
        id: channel_id,
    }];
    let binding_revision = NonZeroU64::new(1).unwrap();
    let approval_binding_fingerprint =
        approval_binding_fingerprint_v1(guild_id, binding_revision, &required_bindings).unwrap();
    assert_ne!(
        authority_binding_fingerprint.as_str(),
        approval_binding_fingerprint.as_str()
    );
    let required_approvals = NonZeroU32::new(1).unwrap();
    let ttl_seconds = NonZeroU64::new(3_600).unwrap();
    let policy_revision = NonZeroU64::new(1).unwrap();
    let definition = serde_json::from_value(json!({
        "version": 1,
        "panels": [],
        "modals": [],
        "rules": []
    }))
    .unwrap();
    let ruleset_content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap();
    let scope_digest = canonical_digest(
        IDEMPOTENCY_SCOPE_DOMAIN,
        &json!({
            "tenant_id": tenant_id.as_str(),
            "principal_id": requester_principal.as_str(),
            "idempotency_key": format!("promotion-e2e-{suffix}")
        }),
    );
    let promotion_id = PromotionId::parse(&scope_digest).unwrap();
    let idempotency_scope_digest = IdempotencyScopeDigest::parse(&scope_digest).unwrap();
    let authority = AuthenticatedPromotionContext {
        tenant_id: tenant_id.clone(),
        principal_id: requester_principal.clone(),
        session_owner_id: requester_principal.clone(),
        session_id: session_id.clone(),
        session_generation: SessionGeneration::new(1).unwrap(),
        guild_id,
        installation_id: installation_id.clone(),
        ruleset_key: ruleset_key.clone(),
        requester: requester_user,
        binding_revision: BindingRevision::new(1).unwrap(),
        policy: ApprovalPolicyV1 {
            revision: PolicyRevision::new(1).unwrap(),
            required_approvals,
            ttl_seconds,
        },
    };
    let candidate_ruleset_hash = sha256_hex(&format!("candidate-ruleset:{suffix}"));
    let evidence = AuthoringEvidenceV1 {
        artifact_version: 1,
        intent_protocol_version: 1,
        identity_revision: 1,
        extractor_revision: 1,
        normalizer_revision: 1,
        compiler_revision: 1,
        simulator_revision: 1,
        recipe_id: "starring.e2e".to_string(),
        recipe_version: 1,
        recipe_descriptor_digest: AuthoringHash::parse(&sha256_hex(&format!(
            "recipe-descriptor:{suffix}"
        )))
        .unwrap(),
        recipe_registry_digest: AuthoringHash::parse(&sha256_hex(&format!(
            "recipe-registry:{suffix}"
        )))
        .unwrap(),
        requested_outcome: IntentRequestedOutcome::ValidatedPreview,
        intent_revision: 1,
        candidate_revision: 1,
        request_evidence_hash: AuthoringHash::parse(&sha256_hex(&format!(
            "request-evidence:{suffix}"
        )))
        .unwrap(),
        request_evidence_entries: 1,
        compiler_input_hash: AuthoringHash::parse(&sha256_hex(&format!("compiler-input:{suffix}")))
            .unwrap(),
        semantic_intent_hash: AuthoringHash::parse(&sha256_hex(&format!(
            "semantic-intent:{suffix}"
        )))
        .unwrap(),
        compiled_plan_hash: AuthoringHash::parse(&sha256_hex(&format!("compiled-plan:{suffix}")))
            .unwrap(),
        candidate_ruleset_hash: AuthoringHash::parse(&candidate_ruleset_hash).unwrap(),
        candidate_draft_hash: AuthoringHash::parse(&sha256_hex(&format!(
            "candidate-draft:{suffix}"
        )))
        .unwrap(),
        compiled_operations: 1,
        context_fingerprint: authority_binding_fingerprint.clone(),
        external_channel_bindings: vec!["community_hub".to_string()],
        stage_binding_digest: AuthoringHash::parse(&sha256_hex(&format!("stage-binding:{suffix}")))
            .unwrap(),
    };
    let intent = PromotionIntentV1 {
        idempotency_scope_digest,
        authority,
        evidence,
        definition: definition.clone(),
        preview: AuthoringPreviewV1 {
            revision: 1,
            summary: AuthoringPreviewSummaryV1 {
                panels: 0,
                modals: 0,
                rules: 0,
                actions: 0,
                unresolved_references: Vec::new(),
            },
        },
        registry_schema_version: CURRENT_RULESET_SCHEMA_VERSION,
        expected_registry_content_hash: ruleset_content_hash,
    };
    let promotion_request_digest =
        PromotionRequestDigest::parse(&canonical_digest(PROMOTION_REQUEST_DOMAIN, &intent))
            .unwrap();
    let publication = PublicationRecordV1 {
        version: RuleSetVersionId::FIRST,
        schema_version: CURRENT_RULESET_SCHEMA_VERSION,
        content_hash: ruleset_content_hash,
        disposition: PublicationDispositionV1::Created,
        registry_created_by: requester_user,
    };
    let activation_id = canonical_digest(
        ACTIVATION_REQUEST_DOMAIN,
        &json!({
            "promotion_id": promotion_id,
            "promotion_request_digest": promotion_request_digest,
            "version": publication.version,
            "schema_version": publication.schema_version,
            "content_hash": publication.content_hash
        }),
    );
    let database_now =
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT pg_catalog.clock_timestamp()")
            .fetch_one(pool)
            .await
            .unwrap();
    let promotion_created_at = database_now - TimeDelta::minutes(2);
    let activation_created_at = database_now - TimeDelta::minutes(1);
    let promotion_updated_at = database_now - TimeDelta::seconds(30);
    let activation_expires_at = activation_created_at + TimeDelta::seconds(3_600);
    let linked_at = promotion_updated_at;
    let policy_digest = unkeyed_hex(
        APPROVAL_POLICY_DOMAIN,
        &[
            bytes(policy_revision.get().to_be_bytes()),
            bytes(required_approvals.get().to_be_bytes()),
            bytes(ttl_seconds.get().to_be_bytes()),
        ],
    );
    let approval_context = |payload_digest: &str, context_digest: &str| {
        json!({
            "promotion_id": promotion_id.as_str(),
            "promotion_request_digest": promotion_request_digest.as_str(),
            "approval_payload_digest": payload_digest,
            "approval_context_digest": context_digest,
            "binding": {
                "revision": binding_revision,
                "required_bindings": required_bindings,
                "fingerprint": approval_binding_fingerprint
            },
            "baseline": {"state": "absent"},
            "policy": {
                "revision": policy_revision,
                "required_approvals": required_approvals,
                "ttl_seconds": ttl_seconds,
                "digest": policy_digest
            }
        })
    };
    let record_value = |context: Value| {
        json!({
            "id": promotion_id,
            "revision": 3,
            "request_digest": promotion_request_digest,
            "intent": intent,
            "stage": {
                "state": "activation_pending",
                "publication": publication,
                "activation": {
                    "request_id": activation_id,
                    "target": {
                        "guild_id": guild_id,
                        "ruleset_key": ruleset_key,
                        "version": publication.version,
                        "content_hash": publication.content_hash
                    },
                    "requester": requester_user,
                    "required_approvals": required_approvals,
                    "observed_active": null,
                    "created_at": activation_created_at,
                    "expires_at": activation_expires_at,
                    "disposition": "created",
                    "request_state_at_journal": "pending",
                    "approval_context": context
                }
            },
            "created_at": promotion_created_at,
            "updated_at": promotion_updated_at
        })
    };
    let provisional_record: PromotionRecordV1 = serde_json::from_value(record_value(
        approval_context(&"0".repeat(64), &"0".repeat(64)),
    ))
    .unwrap();
    let payload_digest =
        approval_payload_digest_v1(&provisional_record.product_approval_payload().unwrap())
            .unwrap()
            .to_string();
    let context_digest = unkeyed_hex(
        APPROVAL_CONTEXT_DOMAIN,
        &[
            bytes(activation_id.as_bytes()),
            bytes(guild_id.to_string()),
            bytes(ruleset_key.as_str()),
            bytes(publication.version.get().to_be_bytes()),
            bytes(publication.content_hash.to_hex()),
            bytes(requester_user.to_string()),
            bytes(promotion_id.as_str()),
            bytes(promotion_request_digest.as_str()),
            bytes(payload_digest.as_bytes()),
            bytes(binding_revision.get().to_be_bytes()),
            bytes(approval_binding_fingerprint.as_str()),
            bytes("channel"),
            bytes("community_hub"),
            bytes(channel_id.to_string()),
            bytes("absent"),
            bytes(policy_revision.get().to_be_bytes()),
            bytes(required_approvals.get().to_be_bytes()),
            bytes(ttl_seconds.get().to_be_bytes()),
            bytes(policy_digest.as_bytes()),
        ],
    );
    let approval_context_value = approval_context(&payload_digest, &context_digest);
    let record: PromotionRecordV1 =
        serde_json::from_value(record_value(approval_context_value.clone())).unwrap();
    record.validate().unwrap();
    let payload = record.product_approval_payload().unwrap();
    assert_eq!(
        approval_payload_digest_v1(&payload).unwrap().as_str(),
        payload_digest
    );
    let authority_digest = sha256_hex(&format!("authority:{suffix}"));

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
    .bind(requester_principal.as_str())
    .bind(requester_user.to_string())
    .bind(approver_principal.as_str())
    .bind(approver_user.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_oauth_flows \
         (state_digest, browser_nonce_digest, redirect_uri, return_path, created_at, \
          expires_at, consumed_at, terminal_result_code) \
         VALUES ($1, $2, 'https://starring.example/oauth/discord/callback', '/', \
          CURRENT_TIMESTAMP - INTERVAL '1 minute', CURRENT_TIMESTAMP + INTERVAL '5 minutes', \
          CURRENT_TIMESTAMP - INTERVAL '1 second', 'callback_claimed')",
    )
    .bind(&oauth_state)
    .bind(&oauth_nonce)
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
    .bind(session_digest.as_slice())
    .bind(approver_principal.as_str())
    .bind(csrf_digest.as_slice())
    .bind(&oauth_state)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) VALUES ($1, 'active', $2)",
    )
    .bind(tenant_id.as_str())
    .bind(format!("E2E Tenant {suffix}"))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, ruleset_key, \
          lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(installation_id.as_str())
    .bind(tenant_id.as_str())
    .bind(application_id.to_string())
    .bind(guild_id.to_string())
    .bind(ruleset_key.as_str())
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
    .bind(installation_id.as_str())
    .bind(tenant_id.as_str())
    .bind(Json(&stored_resource_bindings))
    .bind(authority_binding_fingerprint.as_str())
    .bind(&authority_digest)
    .bind(requester_principal.as_str())
    .bind(sha256_hex(&format!("authority-request:{suffix}")))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.authoring_sessions \
         (session_id, tenant_id, installation_id, owner_principal_id, current_generation, \
          lifecycle_state) VALUES ($1, $2, $3, $4, 1, 'active')",
    )
    .bind(session_id.as_str())
    .bind(tenant_id.as_str())
    .bind(installation_id.as_str())
    .bind(requester_principal.as_str())
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
         VALUES ($1, 1, $2, $3, 1, $4, $5, $6, $7, 1, $8, $9, $10, 1, \
          '{}'::JSONB, 'preview_ready', 1, $11, $12, 1)",
    )
    .bind(session_id.as_str())
    .bind(tenant_id.as_str())
    .bind(installation_id.as_str())
    .bind(vec![11_u8; 16])
    .bind(vec![13_u8; 24])
    .bind("keychain:e2e-authoring-v1")
    .bind("xchacha20_poly1305")
    .bind(sha256_hex(&format!("authenticated-metadata:{suffix}")))
    .bind(Json(&stored_resource_bindings))
    .bind(authority_binding_fingerprint.as_str())
    .bind(&candidate_ruleset_hash)
    .bind(sha256_hex(&format!("generation-write:{suffix}")))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_heads \
         (guild_id, ruleset_key, next_version) VALUES ($1, $2, 2)",
    )
    .bind(guild_id.to_string())
    .bind(ruleset_key.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_ruleset_versions \
         (guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by) \
         VALUES ($1, $2, 1, $3, $4, $5, $6)",
    )
    .bind(guild_id.to_string())
    .bind(ruleset_key.as_str())
    .bind(i64::from(CURRENT_RULESET_SCHEMA_VERSION.get()))
    .bind(Json(&definition))
    .bind(ruleset_content_hash.to_hex())
    .bind(requester_user.to_string())
    .execute(&mut *transaction)
    .await
    .unwrap();
    let persisted_record = serde_json::to_value(&record).unwrap();
    insert_activation_pending_promotion(
        &mut transaction,
        promotion_id.as_str(),
        promotion_request_digest.as_str(),
        tenant_id.as_str(),
        installation_id.as_str(),
        requester_principal.as_str(),
        &persisted_record,
    )
    .await;
    sqlx::query(
        "INSERT INTO public.activation_requests \
         (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, state, created_at, expires_at, authority_kind, link_state_name, \
          approval_context, link_state, promotion_id, promotion_request_digest, \
          approval_payload_digest, approval_context_digest, linked_at, tenant_id, \
          installation_id, product_revision) \
         VALUES ($1, $2, $3, 1, $4, $5, 1, 'pending', $6, $7, \
          'product_authoring', 'linked', $8, $9, $10, $11, $12, $13, $14, $15, $16, 1)",
    )
    .bind(&activation_id)
    .bind(guild_id.to_string())
    .bind(ruleset_key.as_str())
    .bind(ruleset_content_hash.to_hex())
    .bind(requester_user.to_string())
    .bind(activation_created_at)
    .bind(activation_expires_at)
    .bind(Json(json!({
        "authority": "product_authoring",
        "context": approval_context_value
    })))
    .bind(Json(json!({"state": "linked", "linked_at": linked_at})))
    .bind(promotion_id.as_str())
    .bind(promotion_request_digest.as_str())
    .bind(&payload_digest)
    .bind(&context_digest)
    .bind(linked_at)
    .bind(tenant_id.as_str())
    .bind(installation_id.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let persisted_hashes = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        "SELECT session_digest, csrf_digest FROM public.product_auth_sessions \
         WHERE principal_id = $1",
    )
    .bind(approver_principal.as_str())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(persisted_hashes.0, session_digest);
    assert_eq!(persisted_hashes.1, csrf_digest);
    assert_ne!(persisted_hashes.0, credential.as_bytes());
    assert_ne!(persisted_hashes.1, csrf.as_bytes());

    Fixture {
        tenant_id,
        installation_id,
        promotion_id,
        activation_id,
        approver_principal,
        approver_user,
        application_id,
        guild_id,
        manager_role_id,
        authority_revision: NonZeroU64::new(1).unwrap(),
        authority_digest,
        authority_binding_fingerprint: authority_binding_fingerprint.into_string(),
        payload_digest,
        payload,
        credential,
        csrf,
        session_digest,
    }
}

#[derive(Clone)]
struct Source {
    fixture: Fixture,
    calls: Arc<AtomicUsize>,
}

impl InstallationAuthoritySource for Source {
    async fn load_for_actor(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
    ) -> Result<InstallationAuthorityRecordV1, DiscordAuthoritySourceError> {
        assert_eq!(actor.principal_id(), &self.fixture.approver_principal);
        assert_eq!(
            actor.session_fingerprint().as_bytes(),
            &self.fixture.session_digest
        );
        assert_eq!(
            installation.installation_id(),
            &self.fixture.installation_id
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(InstallationAuthorityRecordV1 {
            tenant_id: self.fixture.tenant_id.clone(),
            installation_id: self.fixture.installation_id.clone(),
            application_id: self.fixture.application_id,
            guild_id: self.fixture.guild_id,
            acting_user_id: self.fixture.approver_user,
            authority_revision: self.fixture.authority_revision,
            authority_digest: self.fixture.authority_digest.clone(),
        })
    }
}

#[derive(Clone)]
struct Client {
    fixture: Fixture,
    calls: Arc<AtomicUsize>,
}

impl DiscordGuildAuthorityClient for Client {
    fn application_id(&self) -> DiscordApplicationIdV1 {
        self.fixture.application_id
    }

    fn bot_user_id(&self) -> Option<DiscordBotUserIdV1> {
        DiscordBotUserIdV1::new(self.fixture.approver_user.0 + 2).ok()
    }

    async fn fetch_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError> {
        assert_eq!(guild_id, self.fixture.guild_id);
        assert_eq!(user_id, self.fixture.approver_user);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(DiscordGuildAuthoritySnapshotV1 {
            guild_id,
            owner_id: UserId(self.fixture.approver_user.0 + 1),
            member_user_id: user_id,
            member_is_bot: false,
            member_is_system: false,
            member_pending: false,
            member_role_ids: vec![self.fixture.manager_role_id],
            roles: vec![
                DiscordRoleSnapshotV1 {
                    role_id: RoleId(guild_id.0),
                    permissions: Permissions::VIEW_CHANNEL,
                    position: 0,
                    managed: false,
                },
                DiscordRoleSnapshotV1 {
                    role_id: self.fixture.manager_role_id,
                    permissions: Permissions::MANAGE_GUILD,
                    position: 10,
                    managed: false,
                },
            ],
        })
    }

    async fn fetch_apply_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildApplyAuthoritySnapshotV1, DiscordAuthorityClientError> {
        assert_eq!(guild_id, self.fixture.guild_id);
        assert_eq!(user_id, self.fixture.approver_user);
        self.calls.fetch_add(1, Ordering::SeqCst);
        let bot_user = self.bot_user_id().unwrap().to_user_id();
        Ok(DiscordGuildApplyAuthoritySnapshotV1 {
            authority: DiscordGuildAuthoritySnapshotV1 {
                guild_id,
                owner_id: UserId(self.fixture.approver_user.0 + 1),
                member_user_id: user_id,
                member_is_bot: false,
                member_is_system: false,
                member_pending: false,
                member_role_ids: vec![self.fixture.manager_role_id],
                roles: vec![
                    DiscordRoleSnapshotV1 {
                        role_id: RoleId(guild_id.0),
                        permissions: Permissions::VIEW_CHANNEL,
                        position: 0,
                        managed: false,
                    },
                    DiscordRoleSnapshotV1 {
                        role_id: self.fixture.manager_role_id,
                        permissions: Permissions::MANAGE_GUILD,
                        position: 10,
                        managed: false,
                    },
                ],
            },
            bot_member_user_id: bot_user,
            bot_member_is_bot: true,
            bot_member_is_system: false,
            bot_member_pending: false,
            bot_member_role_ids: vec![self.fixture.manager_role_id],
        })
    }
}

struct PendingDeployments;

impl DeploymentStatusPort<FreshDiscordAuthorityEvidenceV1> for PendingDeployments {
    async fn load_exact_deployment_status(
        &self,
        _request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
        Ok(DeploymentStatusProjectionV1::Pending)
    }
}

#[derive(Clone, Copy)]
enum AuthorityRotation {
    Safe,
    Binding,
    Policy,
}

async fn rotate_authority(
    pool: &PgPool,
    fixture: &Fixture,
    rotation: AuthorityRotation,
) -> Fixture {
    let channel_id = match rotation {
        AuthorityRotation::Binding => ChannelId(fixture.guild_id.0 + 3),
        AuthorityRotation::Safe | AuthorityRotation::Policy => ChannelId(fixture.guild_id.0 + 2),
    };
    let mut bindings = ResourceBindingMap::default();
    bindings
        .channel_bindings
        .insert(ResourceKey("community_hub".to_string()), channel_id);
    let binding_fingerprint = resource_binding_fingerprint_v2(&bindings);
    let binding_revision = match rotation {
        AuthorityRotation::Binding => 2_i64,
        AuthorityRotation::Safe | AuthorityRotation::Policy => 1_i64,
    };
    let policy_revision = match rotation {
        AuthorityRotation::Policy => 2_i64,
        AuthorityRotation::Safe | AuthorityRotation::Binding => 1_i64,
    };
    let ttl_seconds = match rotation {
        AuthorityRotation::Policy => 7_200_i64,
        AuthorityRotation::Safe | AuthorityRotation::Binding => 3_600_i64,
    };
    let authority_digest = sha256_hex(&format!(
        "authority-rotation:{}:{binding_revision}:{policy_revision}:{ttl_seconds}",
        fixture.promotion_id.as_str()
    ));
    let request_digest = sha256_hex(&format!(
        "authority-rotation-request:{}:{binding_revision}:{policy_revision}:{ttl_seconds}",
        fixture.promotion_id.as_str()
    ));
    let stored_bindings = json!({
        "role_bindings": {},
        "channel_bindings": {"community_hub": channel_id.to_string()}
    });
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
         VALUES ($1, 2, $2, $3, $4, $5, $6, 1, $7, $8, $9, $10)",
    )
    .bind(fixture.installation_id.as_str())
    .bind(fixture.tenant_id.as_str())
    .bind(binding_revision)
    .bind(Json(&stored_bindings))
    .bind(binding_fingerprint.as_str())
    .bind(policy_revision)
    .bind(ttl_seconds)
    .bind(&authority_digest)
    .bind(fixture.approver_principal.as_str())
    .bind(&request_digest)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE public.automation_installations \
         SET current_authority_revision = 2, updated_at = pg_catalog.clock_timestamp() \
         WHERE tenant_id = $1 AND installation_id = $2",
    )
    .bind(fixture.tenant_id.as_str())
    .bind(fixture.installation_id.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let mut rotated = fixture.clone();
    rotated.authority_revision = NonZeroU64::new(2).unwrap();
    rotated.authority_digest = authority_digest;
    rotated.authority_binding_fingerprint = binding_fingerprint.into_string();
    rotated
}

fn authority_adapter(
    fixture: Fixture,
) -> DiscordGuildAuthorityAdapter<Source, Client, SubmicrosecondClock> {
    DiscordGuildAuthorityAdapter::with_clock(
        Source {
            fixture: fixture.clone(),
            calls: Arc::new(AtomicUsize::new(0)),
        },
        Client {
            fixture,
            calls: Arc::new(AtomicUsize::new(0)),
        },
        SubmicrosecondClock,
        DiscordAuthorityConfigV1::new(
            Duration::from_secs(2),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap(),
    )
}

fn product_decisions(pool: &PgPool) -> PostgresProductDecisions {
    let key_material = std::array::from_fn(|index| 41_u8.wrapping_add(index as u8));
    let keyring = ProductDecisionDigestKeyringV1::new(
        ProductDecisionDigestKeyV1::from_bytes("product-e2e-v1", key_material).unwrap(),
        [ProductDecisionDigestKeyV1::from_bytes(
            "product-e2e-v2",
            std::array::from_fn(|index| 97_u8.wrapping_add(index as u8)),
        )
        .unwrap()],
    )
    .unwrap();
    PostgresProductDecisions::new(product_decision_pools(pool), keyring).unwrap()
}

fn product_decision_pools(pool: &PgPool) -> ProductDecisionDatabasePoolsV1 {
    ProductDecisionDatabasePoolsV1::new(pool.clone(), pool.clone(), pool.clone())
}

async fn approve_fixture(pool: &PgPool, fixture: &Fixture, decisions: &PostgresProductDecisions) {
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let deployments = PendingDeployments;
    let application =
        ProductControlApplication::new(&authentication, &authority, decisions, &deployments);
    application
        .approve(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("approve.drift.{}", suffix())).unwrap(),
            &selector(fixture),
            approval_command(fixture, &format!("approve-drift-{}", suffix())),
        )
        .await
        .unwrap();
}
