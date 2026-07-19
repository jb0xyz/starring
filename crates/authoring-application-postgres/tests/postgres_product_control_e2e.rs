use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use authoring_application::{
    ApplyProductPromotionV1, ApprovalPayloadDigestV1, ApproveProductPromotionV1,
    AuthenticatedActorV1, AuthenticationError, AuthorizedApprovalPreviewV1,
    AuthorizedDeploymentStatusV1, AuthorizedProductStatusV1, CapabilityV1, DeploymentStatusPort,
    DeploymentStatusPortError, DeploymentStatusProjectionV1, DeploymentStatusV1,
    ExactDeploymentSelectorV1, InstallationSelectorV1, ProductApplicationError,
    ProductApprovalPreviewV1, ProductControlApplication, ProductControlPortError,
    ProductDecisionPhaseV1, ProductDecisionProjectionV1, ProductDecisionQueryPort,
    ProductIdempotencyKeyV1, ProductRequestIdV1, ProductRevisionV1, ProductStatusQueryV1,
    ProductStatusV1, PromotionSelectorV1,
};
use authoring_application_discord::{
    DiscordApplicationIdV1, DiscordAuthorityClientError, DiscordAuthorityConfigV1,
    DiscordAuthoritySourceError, DiscordBotUserIdV1, DiscordGuildApplyAuthoritySnapshotV1,
    DiscordGuildAuthorityAdapter, DiscordGuildAuthorityClient, DiscordGuildAuthoritySnapshotV1,
    DiscordRoleSnapshotV1, FreshDiscordAuthorityEvidenceV1, InstallationAuthorityRecordV1,
    InstallationAuthoritySource,
};
use authoring_application_postgres::{
    digest_opaque_session_credential_v1, PostgresAuthentication, PostgresProductDecisions,
    PostgresProductDeploymentStatuses, ProductDecisionDigestKeyV1, ProductDecisionDigestKeyringV1,
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
use hmac::{Hmac, Mac};
use resource_resolution::{
    approval_binding_fingerprint_v1, resource_binding_fingerprint_v2, ResolvedApprovalBinding,
    ResourceBindingMap,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use sqlx::types::Json;

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

fn suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string()
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
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, \
          installation_id, principal_id, record) \
         VALUES ($1, 1, 3, 'activation_pending', $2, $3, $4, $5, $6)",
    )
    .bind(promotion_id.as_str())
    .bind(promotion_request_digest.as_str())
    .bind(tenant_id.as_str())
    .bind(installation_id.as_str())
    .bind(requester_principal.as_str())
    .bind(Json(&record))
    .execute(&mut *transaction)
    .await
    .unwrap();
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

fn authority_adapter(fixture: Fixture) -> DiscordGuildAuthorityAdapter<Source, Client> {
    DiscordGuildAuthorityAdapter::new(
        Source {
            fixture: fixture.clone(),
            calls: Arc::new(AtomicUsize::new(0)),
        },
        Client {
            fixture,
            calls: Arc::new(AtomicUsize::new(0)),
        },
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
        [],
    )
    .unwrap();
    PostgresProductDecisions::new(pool.clone(), keyring).unwrap()
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

async fn assert_drift_supersession(rotation: AuthorityRotation, expected_reason: &str) {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let rotated = rotate_authority(&pool, &fixture, rotation).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(rotated.clone());
    let deployments = PendingDeployments;
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let error = application
        .apply(
            &rotated.credential,
            &rotated.csrf,
            &ProductRequestIdV1::parse(&format!("apply.drift.{}", suffix())).unwrap(),
            &selector(&rotated),
            apply_command(&rotated, &format!("apply-drift-{}", suffix())),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProductApplicationError::Control(ProductControlPortError::Superseded)
    );
    assert_eq!(
        application
            .get_product_status(
                &rotated.credential,
                &selector(&rotated),
                status_query(&rotated)
            )
            .await
            .unwrap(),
        ProductStatusV1::Superseded
    );
    let persisted = sqlx::query_as::<_, (String, i64, String, i64)>(
        "SELECT activation.state, activation.product_revision, \
         activation.termination #>> '{reason,reason}', \
         (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
          WHERE deployment.activation_request_id = activation.id) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&rotated.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        ("superseded".to_string(), 4, expected_reason.to_string(), 0)
    );
    let stale_authority = authority_adapter(fixture.clone());
    let stale_application =
        ProductControlApplication::new(&authentication, &stale_authority, &decisions, &deployments);
    assert_eq!(
        stale_application
            .get_product_status(
                &fixture.credential,
                &selector(&fixture),
                status_query(&fixture)
            )
            .await
            .unwrap_err(),
        ProductApplicationError::Control(ProductControlPortError::InvalidState)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn binding_drift_is_terminal_and_readable_with_current_authority() {
    assert_drift_supersession(AuthorityRotation::Binding, "binding_drift").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn policy_drift_is_terminal_and_readable_with_current_authority() {
    assert_drift_supersession(AuthorityRotation::Policy, "policy_drift").await;
}

async fn assert_applied_history_survives_authority_rotation(rotation: AuthorityRotation) {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let original_authority = authority_adapter(fixture.clone());
    let deployments = PendingDeployments;
    let original = ProductControlApplication::new(
        &authentication,
        &original_authority,
        &decisions,
        &deployments,
    );
    let applied = original
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.history.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-history-{}", suffix())),
        )
        .await
        .unwrap();
    let rotated = rotate_authority(&pool, &fixture, rotation).await;
    let current_authority = authority_adapter(rotated.clone());
    let current = ProductControlApplication::new(
        &authentication,
        &current_authority,
        &decisions,
        &deployments,
    );
    let preview = current
        .get_approval_preview(
            &rotated.credential,
            &selector(&rotated),
            status_query(&rotated),
        )
        .await
        .unwrap();
    assert_eq!(
        preview.phase(),
        &ProductDecisionPhaseV1::Applied {
            exact_deployment: applied.exact_deployment().clone(),
        }
    );
    assert_eq!(
        original
            .get_approval_preview(
                &fixture.credential,
                &selector(&fixture),
                status_query(&fixture),
            )
            .await
            .unwrap_err(),
        ProductApplicationError::Control(ProductControlPortError::InvalidState)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn applied_history_survives_later_binding_rotation_for_current_readers() {
    assert_applied_history_survives_authority_rotation(AuthorityRotation::Binding).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn applied_history_survives_later_policy_rotation_for_current_readers() {
    assert_applied_history_survives_authority_rotation(AuthorityRotation::Policy).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn corrupted_historical_generation_hash_returns_redacted_integrity_failure() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let deployments = PendingDeployments;
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let installation = selector(&fixture);
    let query = status_query(&fixture);
    application
        .get_approval_preview(&fixture.credential, &installation, query.clone())
        .await
        .unwrap();

    let original_hash = sqlx::query_scalar::<_, String>(
        "SELECT generation.candidate_hash FROM public.authoring_session_generations AS generation \
         INNER JOIN public.authoring_promotions AS promotion \
           ON promotion.tenant_id = generation.tenant_id \
           AND promotion.installation_id = generation.installation_id \
           AND promotion.record #>> '{intent,authority,session_id}' = generation.session_id \
           AND (promotion.record #>> '{intent,authority,session_generation}')::BIGINT \
             = generation.generation \
         WHERE promotion.id = $1",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    let corrupted_hash = sha256_hex(&format!(
        "corrupted-historical-generation:{}",
        fixture.promotion_id.as_str()
    ));
    assert_ne!(corrupted_hash, original_hash);

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE public.authoring_session_generations \
         DISABLE TRIGGER authoring_generations_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    let update = sqlx::query(
        "UPDATE public.authoring_session_generations AS generation \
         SET candidate_hash = $1 \
         FROM public.authoring_promotions AS promotion \
         WHERE promotion.id = $2 \
           AND promotion.tenant_id = generation.tenant_id \
           AND promotion.installation_id = generation.installation_id \
           AND promotion.record #>> '{intent,authority,session_id}' = generation.session_id \
           AND (promotion.record #>> '{intent,authority,session_generation}')::BIGINT \
             = generation.generation",
    )
    .bind(&corrupted_hash)
    .bind(fixture.promotion_id.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(update.rows_affected(), 1);
    sqlx::query(
        "ALTER TABLE public.authoring_session_generations \
         ENABLE TRIGGER authoring_generations_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let expected = ProductApplicationError::Control(ProductControlPortError::Backend(
        "persisted product decision violates its integrity contract".to_string(),
    ));
    let preview_error = application
        .get_approval_preview(&fixture.credential, &installation, query.clone())
        .await
        .unwrap_err();
    assert_eq!(preview_error, expected);
    assert!(!format!("{preview_error:?}").contains(&corrupted_hash));

    let status_error = application
        .get_product_status(&fixture.credential, &installation, query)
        .await
        .unwrap_err();
    assert_eq!(status_error, expected);
    assert!(!format!("{status_error:?}").contains(&corrupted_hash));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn deployment_status_redacts_controller_failure_evidence() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let runtime = PostgresRuntimeConvergence::new(pool.clone());
    let deployments = PostgresProductDeploymentStatuses::new(runtime.clone());
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.failure.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-failure-{}", suffix())),
        )
        .await
        .unwrap();
    let runtime_scope = RuntimeDeploymentScopeV1 {
        tenant_id: RuntimeTenantId::parse(fixture.tenant_id.as_str()).unwrap(),
        installation_id: RuntimeInstallationId::parse(fixture.installation_id.as_str()).unwrap(),
        deployment_id: DeploymentId::parse(applied.exact_deployment().deployment_reference())
            .unwrap(),
    };
    let requested = runtime.status(&runtime_scope).await.unwrap();
    let controller = ControllerId::parse(format!("controller-{}", suffix())).unwrap();
    let claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: runtime_scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: controller.clone(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let ready_revision = advance_product_runtime_to_ready(&runtime, &runtime_scope, &claim).await;
    let private_code = sha256_hex(&format!("private-runtime-code:{}", suffix()));
    runtime
        .mutate(SubmitDeploymentMutationV1 {
            scope: runtime_scope,
            expected_revision: ready_revision,
            controller_id: controller,
            fencing_token: claim.fencing_token,
            runtime_generation: RuntimeGeneration::FIRST,
            mutation: DeploymentMutationV1::RecordRetryableFailure {
                failure_id: RuntimeFailureId::parse(format!("failure-{}", suffix())).unwrap(),
                kind: RuntimeFailureKindV1::GatewayStart,
                code: private_code.clone(),
                message: "private runtime diagnostic".to_string(),
                attempt: NonZeroU32::MIN,
                retry_after: Duration::from_secs(1),
            },
        })
        .await
        .unwrap();
    let status = application
        .get_deployment_status(
            &fixture.credential,
            &selector(&fixture),
            authoring_application::RuntimeDeploymentQueryV1 {
                promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        status,
        DeploymentStatusV1::Failed {
            retryable: true,
            failure_code: "gateway_start_failed".to_string(),
        }
    );
    assert!(!format!("{status:?}").contains(&private_code));
}

fn selector(fixture: &Fixture) -> InstallationSelectorV1 {
    InstallationSelectorV1::new(fixture.installation_id.clone())
}

fn status_query(fixture: &Fixture) -> ProductStatusQueryV1 {
    ProductStatusQueryV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
    }
}

fn approval_command(fixture: &Fixture, idempotency_key: &str) -> ApproveProductPromotionV1 {
    ApproveProductPromotionV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&fixture.payload_digest).unwrap(),
        expected_revision: ProductRevisionV1::new(1).unwrap(),
        idempotency_key: ProductIdempotencyKeyV1::parse(idempotency_key).unwrap(),
    }
}

fn apply_command(fixture: &Fixture, idempotency_key: &str) -> ApplyProductPromotionV1 {
    ApplyProductPromotionV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&fixture.payload_digest).unwrap(),
        expected_revision: ProductRevisionV1::new(2).unwrap(),
        idempotency_key: ProductIdempotencyKeyV1::parse(idempotency_key).unwrap(),
    }
}

#[derive(sqlx::FromRow)]
struct ReceiptRow {
    receipt_id: String,
    idempotency_key_digest: String,
    idempotency_digest_key_id: String,
    idempotency_digest_key_fingerprint: String,
    request_digest: String,
    resulting_revision: i64,
    resulting_state: String,
    result_code: String,
}

#[derive(sqlx::FromRow)]
struct AuditRow {
    event_id: String,
    session_subject_digest: Vec<u8>,
    request_id: String,
    authority_observation_digest: String,
    effective_permission_bits: String,
    authority_observed_at: DateTime<Utc>,
    payload_digest: String,
    binding_fingerprint: String,
    policy_revision: i64,
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_control_application_approves_and_replays_through_all_trust_boundaries() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let source_calls = Arc::new(AtomicUsize::new(0));
    let client_calls = Arc::new(AtomicUsize::new(0));
    let authority = DiscordGuildAuthorityAdapter::new(
        Source {
            fixture: fixture.clone(),
            calls: source_calls.clone(),
        },
        Client {
            fixture: fixture.clone(),
            calls: client_calls.clone(),
        },
        DiscordAuthorityConfigV1::new(
            Duration::from_secs(2),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap(),
    );
    let authentication = PostgresAuthentication::new(pool.clone());
    let key_material = std::array::from_fn(|index| 41_u8.wrapping_add(index as u8));
    let keyring = ProductDecisionDigestKeyringV1::new(
        ProductDecisionDigestKeyV1::from_bytes("product-e2e-v1", key_material).unwrap(),
        [],
    )
    .unwrap();
    let decisions = PostgresProductDecisions::new(pool.clone(), keyring).unwrap();
    decisions.verify_keyring_coverage().await.unwrap();
    let deployments =
        PostgresProductDeploymentStatuses::new(PostgresRuntimeConvergence::new(pool.clone()));
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let installation = selector(&fixture);

    let preview = application
        .get_approval_preview(&fixture.credential, &installation, status_query(&fixture))
        .await
        .unwrap();
    assert_eq!(preview.installation_id(), &fixture.installation_id);
    assert_eq!(preview.guild_id(), fixture.guild_id);
    assert_eq!(preview.payload(), &fixture.payload);
    assert_eq!(preview.payload_digest().as_str(), fixture.payload_digest);
    assert_eq!(preview.revision().get(), 1);
    assert_eq!(preview.phase(), &ProductDecisionPhaseV1::PendingApproval);
    assert_eq!(
        application
            .get_product_status(&fixture.credential, &installation, status_query(&fixture))
            .await
            .unwrap(),
        ProductStatusV1::PendingApproval
    );

    let idempotency_key = format!("approve-e2e-{}", suffix());
    let first_request = ProductRequestIdV1::parse(&format!("approve.first.{}", suffix())).unwrap();
    let wrong_csrf = URL_SAFE_NO_PAD.encode([201_u8; 32]);
    let calls_before_invalid_csrf = source_calls.load(Ordering::SeqCst);
    assert_eq!(
        application
            .approve(
                &fixture.credential,
                &wrong_csrf,
                &first_request,
                &installation,
                approval_command(&fixture, &idempotency_key)
            )
            .await
            .unwrap_err(),
        ProductApplicationError::Authentication(AuthenticationError::InvalidCsrf)
    );
    assert_eq!(
        source_calls.load(Ordering::SeqCst),
        calls_before_invalid_csrf
    );

    let first = application
        .approve(
            &fixture.credential,
            &fixture.csrf,
            &first_request,
            &installation,
            approval_command(&fixture, &idempotency_key),
        )
        .await
        .unwrap();
    assert!(!first.exact_replay());
    assert_eq!(first.projection().revision().get(), 2);
    assert_eq!(
        first.projection().phase(),
        &ProductDecisionPhaseV1::Approved
    );
    assert_eq!(
        application
            .get_product_status(&fixture.credential, &installation, status_query(&fixture))
            .await
            .unwrap(),
        ProductStatusV1::Approved
    );
    decisions.verify_keyring_coverage().await.unwrap();
    let next_key_material = std::array::from_fn(|index| 97_u8.wrapping_add(index as u8));
    let next_only = PostgresProductDecisions::new(
        pool.clone(),
        ProductDecisionDigestKeyringV1::new(
            ProductDecisionDigestKeyV1::from_bytes("product-e2e-v2", next_key_material).unwrap(),
            [],
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        next_only.verify_keyring_coverage().await.unwrap_err(),
        ProductDecisionReadinessErrorV1::IncompleteCoverage
    );
    let rolling = PostgresProductDecisions::new(
        pool.clone(),
        ProductDecisionDigestKeyringV1::new(
            ProductDecisionDigestKeyV1::from_bytes("product-e2e-v2", next_key_material).unwrap(),
            [ProductDecisionDigestKeyV1::from_bytes("product-e2e-v1", key_material).unwrap()],
        )
        .unwrap(),
    )
    .unwrap();
    rolling.verify_keyring_coverage().await.unwrap();

    let replay_request =
        ProductRequestIdV1::parse(&format!("approve.replay.{}", suffix())).unwrap();
    let replay = application
        .approve(
            &fixture.credential,
            &fixture.csrf,
            &replay_request,
            &installation,
            approval_command(&fixture, &idempotency_key),
        )
        .await
        .unwrap();
    assert!(replay.exact_replay());
    assert_eq!(replay.projection(), first.projection());

    let persisted = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
         (SELECT pg_catalog.count(*) FROM public.activation_request_approvals AS approval \
          WHERE approval.request_id = activation.id), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
          WHERE receipt.target_resource_id = activation.promotion_id), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
          WHERE audit.target_resource_id = activation.promotion_id) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted, ("approved".to_string(), 2, 1, 1, 1));

    let receipt = sqlx::query_as::<_, ReceiptRow>(
        "SELECT receipt_id, idempotency_key_digest, idempotency_digest_key_id, \
         idempotency_digest_key_fingerprint, request_digest, resulting_revision, \
         resulting_state, result_code FROM public.product_action_receipts \
         WHERE target_resource_id = $1",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    let idempotency_fields = vec![
        bytes(fixture.tenant_id.as_str()),
        bytes(fixture.installation_id.as_str()),
        bytes(fixture.approver_principal.as_str()),
        bytes("product_approve_v1"),
        bytes(idempotency_key.as_bytes()),
    ];
    let expected_idempotency = keyed_hex(&key_material, IDEMPOTENCY_DOMAIN, &idempotency_fields);
    let expected_semantic = unkeyed_hex(
        SEMANTIC_REQUEST_DOMAIN,
        &[
            bytes(fixture.tenant_id.as_str()),
            bytes(fixture.installation_id.as_str()),
            bytes(fixture.approver_principal.as_str()),
            bytes(fixture.promotion_id.as_str()),
            bytes("1"),
            bytes(fixture.payload_digest.as_bytes()),
        ],
    );
    let identity_fields = vec![
        bytes(fixture.tenant_id.as_str()),
        bytes(fixture.installation_id.as_str()),
        bytes(fixture.approver_principal.as_str()),
        bytes(expected_idempotency.as_bytes()),
        bytes(expected_semantic.as_bytes()),
    ];
    let expected_receipt = keyed_hex(&key_material, RECEIPT_ID_DOMAIN, &identity_fields);
    let expected_audit = keyed_hex(&key_material, AUDIT_EVENT_ID_DOMAIN, &identity_fields);
    let expected_key_fingerprint =
        unkeyed_hex(KEY_MATERIAL_FINGERPRINT_DOMAIN, &[bytes(key_material)]);
    assert_eq!(receipt.receipt_id, expected_receipt);
    assert_eq!(receipt.idempotency_key_digest, expected_idempotency);
    assert_eq!(receipt.idempotency_digest_key_id, "product-e2e-v1");
    assert_eq!(
        receipt.idempotency_digest_key_fingerprint,
        expected_key_fingerprint
    );
    assert_eq!(receipt.request_digest, expected_semantic);
    assert_eq!(receipt.resulting_revision, 2);
    assert_eq!(receipt.resulting_state, "approved");
    assert_eq!(receipt.result_code, "approval_quorum_reached");

    let alias = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT idempotency_key_digest, idempotency_digest_key_id, \
         idempotency_digest_key_fingerprint, receipt_id \
         FROM public.product_action_receipt_idempotency_aliases \
         WHERE receipt_id = $1",
    )
    .bind(&expected_receipt)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        alias,
        (
            expected_idempotency,
            "product-e2e-v1".to_string(),
            expected_key_fingerprint,
            expected_receipt
        )
    );

    let audit = sqlx::query_as::<_, AuditRow>(
        "SELECT event_id, session_subject_digest, request_id, authority_observation_digest, \
         effective_permission_bits::TEXT, authority_observed_at, payload_digest, \
         binding_fingerprint, policy_revision FROM public.product_audit_events \
         WHERE target_resource_id = $1",
    )
    .bind(fixture.promotion_id.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    let expected_session_subject = unkeyed_bytes(
        SESSION_SUBJECT_DOMAIN,
        &[
            bytes(fixture.tenant_id.as_str()),
            bytes(fixture.approver_principal.as_str()),
            bytes(fixture.session_digest),
        ],
    );
    let effective_permissions = Permissions::VIEW_CHANNEL | Permissions::MANAGE_GUILD;
    let authority_expiry = audit.authority_observed_at + TimeDelta::seconds(5);
    let expected_observation = unkeyed_hex(
        AUTHORITY_OBSERVATION_DOMAIN,
        &[
            bytes(fixture.tenant_id.as_str()),
            bytes(fixture.installation_id.as_str()),
            bytes(fixture.application_id.to_string()),
            bytes(fixture.guild_id.to_string()),
            bytes(fixture.approver_user.to_string()),
            bytes("approve"),
            bytes(effective_permissions.bits().to_string()),
            bytes("member"),
            bytes("1"),
            bytes(fixture.authority_digest.as_bytes()),
            bytes(fixture.guild_id.to_string()),
            bytes(Permissions::VIEW_CHANNEL.bits().to_string()),
            bytes(fixture.manager_role_id.to_string()),
            bytes(Permissions::MANAGE_GUILD.bits().to_string()),
            bytes(audit.authority_observed_at.timestamp_millis().to_string()),
            bytes(authority_expiry.timestamp_millis().to_string()),
        ],
    );
    assert_eq!(audit.event_id, expected_audit);
    assert_eq!(audit.session_subject_digest, expected_session_subject);
    assert_ne!(audit.session_subject_digest, fixture.session_digest);
    assert_eq!(audit.request_id, first_request.as_str());
    assert_eq!(audit.authority_observation_digest, expected_observation);
    assert_eq!(
        audit.effective_permission_bits,
        effective_permissions.bits().to_string()
    );
    assert_eq!(audit.payload_digest, fixture.payload_digest);
    assert_eq!(
        audit.binding_fingerprint,
        fixture.authority_binding_fingerprint
    );
    assert_eq!(audit.policy_revision, 1);
    assert_eq!(source_calls.load(Ordering::SeqCst), 5);
    assert_eq!(client_calls.load(Ordering::SeqCst), 5);

    let apply_key = format!("apply-e2e-{}", suffix());
    let apply_request = ProductRequestIdV1::parse(&format!("apply.first.{}", suffix())).unwrap();
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &apply_request,
            &installation,
            apply_command(&fixture, &apply_key),
        )
        .await
        .unwrap();
    assert_eq!(applied.status(), ProductStatusV1::RuntimePending);
    assert!(!applied.exact_replay());
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &installation,
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Pending
    );

    let replay_request = ProductRequestIdV1::parse(&format!("apply.replay.{}", suffix())).unwrap();
    let replay = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &replay_request,
            &installation,
            apply_command(&fixture, &apply_key),
        )
        .await
        .unwrap();
    assert_eq!(replay.status(), ProductStatusV1::RuntimePending);
    assert!(replay.exact_replay());
    assert_eq!(replay.exact_deployment(), applied.exact_deployment());

    let applied_state = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
        "SELECT activation.state, activation.product_revision, \
         (SELECT pg_catalog.count(*) FROM public.automation_ruleset_activations AS active \
          WHERE active.guild_id = activation.guild_id \
            AND active.ruleset_key = activation.ruleset_key \
            AND active.active_version = activation.target_version), \
         (SELECT pg_catalog.count(*) FROM public.runtime_deployments AS deployment \
          WHERE deployment.activation_request_id = activation.id \
            AND deployment.phase = 'requested'), \
         (SELECT pg_catalog.count(*) FROM public.product_action_receipts AS receipt \
          WHERE receipt.target_resource_id = activation.promotion_id \
            AND receipt.endpoint_domain = 'product_apply_v1'), \
         (SELECT pg_catalog.count(*) FROM public.product_audit_events AS audit \
          WHERE audit.target_resource_id = activation.promotion_id \
            AND audit.action = 'promotion.apply') \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(&fixture.activation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(applied_state, ("applied".to_string(), 4, 1, 1, 1, 1));

    let rotated = rotate_authority(&pool, &fixture, AuthorityRotation::Safe).await;
    let rotated_authority = authority_adapter(rotated.clone());
    let rotated_application = ProductControlApplication::new(
        &authentication,
        &rotated_authority,
        &decisions,
        &deployments,
    );
    assert_eq!(
        rotated_application
            .get_product_status(
                &rotated.credential,
                &selector(&rotated),
                status_query(&rotated)
            )
            .await
            .unwrap(),
        ProductStatusV1::RuntimePending
    );
    assert_eq!(
        application
            .get_product_status(&fixture.credential, &installation, status_query(&fixture))
            .await
            .unwrap_err(),
        ProductApplicationError::Control(ProductControlPortError::InvalidState)
    );
}

#[derive(Clone)]
struct RecordingDeploymentStatuses {
    inner: PostgresProductDeploymentStatuses,
    authority_windows: Arc<Mutex<Vec<(CapabilityV1, i64)>>>,
}

impl RecordingDeploymentStatuses {
    fn new(inner: PostgresProductDeploymentStatuses) -> Self {
        Self {
            inner,
            authority_windows: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn authority_windows(&self) -> Vec<(CapabilityV1, i64)> {
        self.authority_windows.lock().unwrap().clone()
    }
}

impl DeploymentStatusPort<FreshDiscordAuthorityEvidenceV1> for RecordingDeploymentStatuses {
    async fn load_exact_deployment_status(
        &self,
        request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
        let evidence = request.evidence();
        self.authority_windows.lock().unwrap().push((
            evidence.capability(),
            (evidence.expires_at() - evidence.observed_at()).num_milliseconds(),
        ));
        self.inner.load_exact_deployment_status(request).await
    }
}

#[derive(Clone)]
struct ProjectedDecision {
    projection: ProductDecisionProjectionV1,
}

impl ProductDecisionQueryPort<FreshDiscordAuthorityEvidenceV1> for ProjectedDecision {
    async fn load_approval_preview(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        Err(ProductControlPortError::InvalidState)
    }

    async fn load_product_status(
        &self,
        _request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        Ok(self.projection.clone())
    }
}

fn product_runtime_scope(
    fixture: &Fixture,
    exact: &ExactDeploymentSelectorV1,
) -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: RuntimeTenantId::parse(fixture.tenant_id.as_str()).unwrap(),
        installation_id: RuntimeInstallationId::parse(fixture.installation_id.as_str()).unwrap(),
        deployment_id: DeploymentId::parse(exact.deployment_reference()).unwrap(),
    }
}

async fn mutate_product_runtime(
    runtime: &PostgresRuntimeConvergence,
    scope: &RuntimeDeploymentScopeV1,
    expected_revision: DeploymentRevision,
    controller_id: &ControllerId,
    fencing_token: FencingToken,
    runtime_generation: RuntimeGeneration,
    mutation: DeploymentMutationV1,
) -> DeploymentRevision {
    runtime
        .mutate(SubmitDeploymentMutationV1 {
            scope: scope.clone(),
            expected_revision,
            controller_id: controller_id.clone(),
            fencing_token,
            runtime_generation,
            mutation,
        })
        .await
        .unwrap()
        .snapshot
        .revision
}

async fn advance_product_runtime_to_ready(
    runtime: &PostgresRuntimeConvergence,
    scope: &RuntimeDeploymentScopeV1,
    claim: &automation_runtime_convergence_postgres::ClaimReceiptV1,
) -> DeploymentRevision {
    let target = claim.snapshot.target.clone();
    let generation = claim.snapshot.runtime_generation;
    let mut revision = mutate_product_runtime(
        runtime,
        scope,
        claim.snapshot.revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::AcceptPreflight(PreflightAttestationV1 {
            target: target.clone(),
            runtime_generation: generation,
            observed_runtime: None,
            checked_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::RequestDrain,
    )
    .await;
    revision = mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::AcceptDrain(DrainAttestationV1 {
            previous_runtime: None,
            target_runtime_generation: generation,
            drained_at: claim.acquired_at,
        }),
    )
    .await;
    revision = mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::BeginActivation,
    )
    .await;
    mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::AcceptActivation(ActivationAttestationV1 {
            activation_request_id: ActivationRequestId::parse(
                claim.snapshot.identity.activation_request_id.as_str(),
            )
            .unwrap(),
            target,
            runtime_generation: generation,
            kind: ActivationOutcomeKindV1::AlreadyActive,
            activated_at: claim.acquired_at,
        }),
    )
    .await
}

async fn certify_product_runtime_live(
    runtime: &PostgresRuntimeConvergence,
    scope: &RuntimeDeploymentScopeV1,
    claim: &automation_runtime_convergence_postgres::ClaimReceiptV1,
    ready_revision: DeploymentRevision,
    serving_lease_for: Duration,
) -> (
    automation_runtime_convergence_postgres::MutationReceiptV1,
    automation_runtime_convergence_postgres::ServingLeaseReceiptV1,
) {
    let target = claim.snapshot.target.clone();
    let generation = claim.snapshot.runtime_generation;
    let process_instance_id =
        ProcessInstanceId::parse(format!("product-live-process-{}", suffix())).unwrap();
    let revision = mutate_product_runtime(
        runtime,
        scope,
        ready_revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::BeginPanelReconciliation,
    )
    .await;
    let revision = mutate_product_runtime(
        runtime,
        scope,
        revision,
        &claim.controller_id,
        claim.fencing_token,
        generation,
        DeploymentMutationV1::AcceptPanelCertificate(PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse(format!(
                "product-live-certificate-{}",
                suffix()
            ))
            .unwrap(),
            target: target.clone(),
            runtime_generation: generation,
            process_instance_id: process_instance_id.clone(),
            declared_count: 0,
            installed_count: 0,
            unchanged_count: 0,
            skipped_transient_count: 0,
            skipped_unresolved_channel_count: 0,
            failed_count: 0,
            ambiguous_outcome_count: 0,
            stale_message_cleanup_pending_count: 0,
            orphan_message_cleanup_pending_count: 0,
            reposted_old_message_cleanup_pending_count: 0,
            reconciled_at: claim.acquired_at,
        }),
    )
    .await;
    runtime
        .certify_live(SubmitLiveAttestationV1 {
            scope: scope.clone(),
            expected_revision: revision,
            controller_id: claim.controller_id.clone(),
            fencing_token: claim.fencing_token,
            runtime_generation: generation,
            gateway_ready: GatewayReadyAttestationV1 {
                target,
                runtime_generation: generation,
                process_instance_id,
                kind: GatewayReadyKindV1::DiscordReady,
                ready_at: claim.acquired_at,
            },
            metadata: LiveMetadataV1 {
                runtime_build_revision: RuntimeBuildRevisionV1::parse("product-status-e2e")
                    .unwrap(),
                panel_report_digest: PanelReportDigestV1::parse(sha256_hex(&format!(
                    "product-panel-report:{}",
                    suffix()
                )))
                .unwrap(),
                gateway_shard_id: GatewayShardIdV1::parse("shard:product-status").unwrap(),
            },
            serving_lease_for,
        })
        .await
        .unwrap()
}

fn applied_projection(
    fixture: &Fixture,
    exact: ExactDeploymentSelectorV1,
) -> ProductDecisionProjectionV1 {
    ProductDecisionProjectionV1::from_server_projection(
        fixture.tenant_id.clone(),
        fixture.installation_id.clone(),
        fixture.guild_id,
        fixture.promotion_id.clone(),
        ProductRevisionV1::new(4).unwrap(),
        ProductDecisionPhaseV1::Applied {
            exact_deployment: exact,
        },
    )
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_status_requires_exact_attestation_and_connected_unexpired_serving() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let runtime = PostgresRuntimeConvergence::with_config(
        pool.clone(),
        PostgresRuntimeConvergenceConfigV1 {
            statement_timeout: Duration::from_millis(500),
            lock_timeout: Duration::from_millis(250),
            ..PostgresRuntimeConvergenceConfigV1::default()
        },
    )
    .unwrap();
    let deployments =
        RecordingDeploymentStatuses::new(PostgresProductDeploymentStatuses::new(runtime.clone()));
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let apply_key = format!("apply-live-{}", suffix());
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.live.first.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &apply_key),
        )
        .await
        .unwrap();
    assert_eq!(applied.status(), ProductStatusV1::RuntimePending);
    assert_eq!(
        application
            .apply(
                &fixture.credential,
                &fixture.csrf,
                &ProductRequestIdV1::parse(&format!("apply.live.replay.{}", suffix())).unwrap(),
                &selector(&fixture),
                apply_command(&fixture, &apply_key),
            )
            .await
            .unwrap()
            .status(),
        ProductStatusV1::RuntimePending
    );
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Pending
    );
    let scope = product_runtime_scope(&fixture, applied.exact_deployment());
    let requested = runtime.status(&scope).await.unwrap();
    let claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: ControllerId::parse(format!("product-live-controller-{}", suffix()))
                .unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let ready_revision = advance_product_runtime_to_ready(&runtime, &scope, &claim).await;
    let (live, serving) = certify_product_runtime_live(
        &runtime,
        &scope,
        &claim,
        ready_revision,
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Live {
            attestation_revision: NonZeroU64::new(live.snapshot.revision.get()).unwrap(),
        }
    );
    tokio::time::sleep(Duration::from_millis(2_200)).await;
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Pending
    );
    let recovered = runtime
        .recover_stale_live(RecoverStaleLiveV1 {
            identity: serving.identity,
            expected_deployment_revision: live.snapshot.revision,
        })
        .await
        .unwrap();
    let recovered_claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: recovered.snapshot.revision,
            controller_id: ControllerId::parse(format!(
                "product-live-recovery-controller-{}",
                suffix()
            ))
            .unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let (recovered_live, recovered_serving) = certify_product_runtime_live(
        &runtime,
        &scope,
        &recovered_claim,
        recovered_claim.snapshot.revision,
        Duration::from_secs(45),
    )
    .await;
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Live {
            attestation_revision: NonZeroU64::new(recovered_live.snapshot.revision.get()).unwrap(),
        }
    );
    runtime
        .mark_serving_disconnected(MarkServingDisconnectedV1 {
            identity: recovered_serving.identity,
        })
        .await
        .unwrap();
    assert_eq!(
        application
            .get_deployment_status(
                &fixture.credential,
                &selector(&fixture),
                authoring_application::RuntimeDeploymentQueryV1 {
                    promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
                },
            )
            .await
            .unwrap(),
        DeploymentStatusV1::Pending
    );
    let authority_windows = deployments.authority_windows();
    assert!(authority_windows.contains(&(CapabilityV1::Apply, 5_000)));
    assert!(authority_windows.contains(&(CapabilityV1::Read, 30_000)));
    assert!(authority_windows.iter().all(|(capability, lifetime)| {
        matches!(
            (capability, lifetime),
            (CapabilityV1::Apply, 5_000) | (CapabilityV1::Read, 30_000)
        )
    }));
    assert!(DiscordAuthorityConfigV1::new(
        Duration::from_secs(2),
        Duration::from_millis(5_001),
        Duration::from_secs(30),
    )
    .is_err());
    assert!(DiscordAuthorityConfigV1::new(
        Duration::from_secs(2),
        Duration::from_secs(5),
        Duration::from_millis(30_001),
    )
    .is_err());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_status_maps_blocked_failure_to_stable_public_code() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let runtime = PostgresRuntimeConvergence::new(pool.clone());
    let deployments = PostgresProductDeploymentStatuses::new(runtime.clone());
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.blocked.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-blocked-{}", suffix())),
        )
        .await
        .unwrap();
    let scope = product_runtime_scope(&fixture, applied.exact_deployment());
    let requested = runtime.status(&scope).await.unwrap();
    let claim = runtime
        .claim(ClaimDeploymentV1 {
            scope: scope.clone(),
            expected_revision: requested.snapshot.revision,
            controller_id: ControllerId::parse(format!("product-blocked-{}", suffix())).unwrap(),
            lease_for: Duration::from_secs(90),
        })
        .await
        .unwrap();
    let ready_revision = advance_product_runtime_to_ready(&runtime, &scope, &claim).await;
    let private_code = sha256_hex(&format!("private-blocked-code:{}", suffix()));
    mutate_product_runtime(
        &runtime,
        &scope,
        ready_revision,
        &claim.controller_id,
        claim.fencing_token,
        claim.snapshot.runtime_generation,
        DeploymentMutationV1::RecordBlockedFailure {
            failure_id: RuntimeFailureId::parse(format!("blocked-{}", suffix())).unwrap(),
            kind: RuntimeFailureKindV1::InvariantViolation,
            code: private_code.clone(),
            message: "private blocked diagnostic".to_string(),
        },
    )
    .await;
    let status = application
        .get_deployment_status(
            &fixture.credential,
            &selector(&fixture),
            authoring_application::RuntimeDeploymentQueryV1 {
                promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        status,
        DeploymentStatusV1::Failed {
            retryable: false,
            failure_code: "runtime_invariant_violation".to_string(),
        }
    );
    assert!(!format!("{status:?}").contains(&private_code));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_status_fails_closed_for_exact_identity_digest_and_scope_mismatch() {
    let pool = pool().await;
    let fixture = seed_fixture(&pool).await;
    let decisions = product_decisions(&pool);
    approve_fixture(&pool, &fixture, &decisions).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let authority = authority_adapter(fixture.clone());
    let runtime = PostgresRuntimeConvergence::new(pool.clone());
    let deployments = PostgresProductDeploymentStatuses::new(runtime);
    let application =
        ProductControlApplication::new(&authentication, &authority, &decisions, &deployments);
    let applied = application
        .apply(
            &fixture.credential,
            &fixture.csrf,
            &ProductRequestIdV1::parse(&format!("apply.mismatch.{}", suffix())).unwrap(),
            &selector(&fixture),
            apply_command(&fixture, &format!("apply-mismatch-{}", suffix())),
        )
        .await
        .unwrap();
    let exact = applied.exact_deployment().clone();
    let query = authoring_application::RuntimeDeploymentQueryV1 {
        promotion: PromotionSelectorV1::new(fixture.promotion_id.clone()),
    };

    let wrong_deployment = ExactDeploymentSelectorV1::from_server_projection(
        fixture.installation_id.clone(),
        fixture.promotion_id.clone(),
        format!("missing-deployment-{}", suffix()),
        exact.target_digest(),
    )
    .unwrap();
    let wrong_deployment_decision = ProjectedDecision {
        projection: applied_projection(&fixture, wrong_deployment),
    };
    let wrong_deployment_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &wrong_deployment_decision,
        &deployments,
    );
    assert_eq!(
        wrong_deployment_application
            .get_deployment_status(&fixture.credential, &selector(&fixture), query.clone())
            .await
            .unwrap_err(),
        ProductApplicationError::Deployment(DeploymentStatusPortError::NotFound)
    );

    let wrong_digest = ExactDeploymentSelectorV1::from_server_projection(
        fixture.installation_id.clone(),
        fixture.promotion_id.clone(),
        exact.deployment_reference(),
        if exact.target_digest() == "0".repeat(64) {
            "1".repeat(64)
        } else {
            "0".repeat(64)
        },
    )
    .unwrap();
    let wrong_digest_decision = ProjectedDecision {
        projection: applied_projection(&fixture, wrong_digest),
    };
    let wrong_digest_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &wrong_digest_decision,
        &deployments,
    );
    assert_eq!(
        wrong_digest_application
            .get_deployment_status(&fixture.credential, &selector(&fixture), query.clone())
            .await
            .unwrap_err(),
        ProductApplicationError::Deployment(DeploymentStatusPortError::Indeterminate(
            "runtime deployment status projection is inconsistent".to_string(),
        ))
    );

    let wrong_promotion = ExactDeploymentSelectorV1::from_server_projection(
        fixture.installation_id.clone(),
        PromotionId::parse(&sha256_hex(&format!("wrong-promotion:{}", suffix()))).unwrap(),
        exact.deployment_reference(),
        exact.target_digest(),
    )
    .unwrap();
    let wrong_promotion_decision = ProjectedDecision {
        projection: applied_projection(&fixture, wrong_promotion),
    };
    let wrong_promotion_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &wrong_promotion_decision,
        &deployments,
    );
    assert_eq!(
        wrong_promotion_application
            .get_deployment_status(&fixture.credential, &selector(&fixture), query.clone())
            .await
            .unwrap_err(),
        ProductApplicationError::InvalidProjection
    );

    let wrong_installation = ExactDeploymentSelectorV1::from_server_projection(
        AutomationInstallationId::parse(&format!("wrong-installation-{}", suffix())).unwrap(),
        fixture.promotion_id.clone(),
        exact.deployment_reference(),
        exact.target_digest(),
    )
    .unwrap();
    let wrong_installation_decision = ProjectedDecision {
        projection: applied_projection(&fixture, wrong_installation),
    };
    let wrong_installation_application = ProductControlApplication::new(
        &authentication,
        &authority,
        &wrong_installation_decision,
        &deployments,
    );
    assert_eq!(
        wrong_installation_application
            .get_deployment_status(&fixture.credential, &selector(&fixture), query)
            .await
            .unwrap_err(),
        ProductApplicationError::InvalidProjection
    );
}
