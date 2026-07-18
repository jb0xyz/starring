use std::num::{NonZeroU32, NonZeroU64};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use authoring_application::{
    AuthenticatedActorV1, AuthenticatedIdentityV1, AuthenticationPort, AuthoringApplication,
    AuthorizedInstallationScopeV1, AuthorizedInstallationV1, CapabilityV1,
    FreshGuildAuthorityError, FreshGuildAuthorityEvidence, FreshGuildAuthorityPort,
    InstallationSelectorV1, PromoteOwnedSessionV1, PromotionSubmissionPort,
};
use authoring_application_postgres::{
    build_snapshot_authenticated_data_v1, digest_opaque_session_credential_v1,
    EncryptedSnapshotEnvelopeV1, PostgresAuthentication, PostgresAuthorizedPromotionSnapshots,
    SnapshotAuthenticatedDataInputV1, SnapshotEnvelopeCipher, SnapshotEnvelopeCipherError,
    MIGRATOR,
};
use authoring_promotion::{
    AuthoringSessionId, AutomationInstallationId, IdempotencyKey, PrincipalId, PromotionError,
    SessionGeneration, StartPromotionV1, TenantId,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, TimeDelta, Utc};
use design_harness::{
    BurstOutcome, DesignSession, LlmClient, LlmError, LlmResponse, Message, ResourceBindingMap,
    ToolCall, ToolDefinition,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, UserId};
use resource_resolution::resource_binding_fingerprint_v2;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::types::Json;
use zeroize::Zeroizing;

#[derive(Clone)]
struct ScriptedClient {
    response: Arc<Mutex<Option<LlmResponse>>>,
}

impl ScriptedClient {
    fn preview_ready() -> Self {
        Self {
            response: Arc::new(Mutex::new(Some(LlmResponse::ToolCalls(vec![ToolCall {
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
            }])))),
        }
    }
}

impl LlmClient for ScriptedClient {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        self.response
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| LlmError::Client("unexpected model call".to_string()))
    }
}

#[derive(Clone, Copy)]
struct PassthroughCipher;

impl SnapshotEnvelopeCipher for PassthroughCipher {
    async fn decrypt(
        &self,
        envelope: &EncryptedSnapshotEnvelopeV1,
        authenticated_data: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, SnapshotEnvelopeCipherError> {
        if authenticated_data.is_empty() {
            return Err(SnapshotEnvelopeCipherError::AuthenticationFailed);
        }
        Ok(Zeroizing::new(envelope.ciphertext().to_vec()))
    }
}

#[derive(Clone)]
struct Evidence {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    application_id: NonZeroU64,
    guild_id: GuildId,
    user_id: UserId,
    observed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl FreshGuildAuthorityEvidence for Evidence {
    fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }

    fn discord_application_id(&self) -> NonZeroU64 {
        self.application_id
    }

    fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    fn acting_user_id(&self) -> UserId {
        self.user_id
    }

    fn capability(&self) -> CapabilityV1 {
        CapabilityV1::Promote
    }

    fn guild_owner(&self) -> bool {
        true
    }

    fn effective_permissions_bits(&self) -> u64 {
        0
    }

    fn installation_authority_revision(&self) -> NonZeroU64 {
        NonZeroU64::new(1).unwrap()
    }

    fn installation_authority_digest(&self) -> &str {
        "1111111111111111111111111111111111111111111111111111111111111111"
    }

    fn observation_digest(&self) -> &str {
        "abababababababababababababababababababababababababababababababab"
    }

    fn observed_at(&self) -> SystemTime {
        self.observed_at.into()
    }

    fn expires_at(&self) -> SystemTime {
        self.expires_at.into()
    }
}

struct TestGuildAuthority {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    application_id: String,
    guild_id: GuildId,
    user_id: UserId,
}

impl FreshGuildAuthorityPort for TestGuildAuthority {
    type Evidence = Evidence;

    async fn authorize_installation(
        &self,
        _actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError> {
        if installation.installation_id() != &self.installation_id
            || capability != CapabilityV1::Promote
        {
            return Err(FreshGuildAuthorityError::Forbidden);
        }
        let observed_at = Utc::now();
        Ok(AuthorizedInstallationV1::from_fresh_authority(
            AuthorizedInstallationScopeV1::from_fresh_authority(
                self.tenant_id.clone(),
                self.installation_id.clone(),
                self.guild_id,
                self.user_id,
            ),
            Evidence {
                tenant_id: self.tenant_id.clone(),
                installation_id: self.installation_id.clone(),
                application_id: NonZeroU64::new(self.application_id.parse().unwrap()).unwrap(),
                guild_id: self.guild_id,
                user_id: self.user_id,
                observed_at,
                expires_at: observed_at + TimeDelta::seconds(5),
            },
        ))
    }
}

struct PromotionCapture;

impl PromotionSubmissionPort for PromotionCapture {
    type Output = (String, u64, String);

    async fn submit_verified_promotion(
        &self,
        input: StartPromotionV1,
    ) -> Result<Self::Output, PromotionError> {
        Ok((
            input.context.tenant_id.to_string(),
            input.artifact.receipt().candidate_revision,
            input.artifact.context_fingerprint().to_string(),
        ))
    }
}

fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
    assert!(
        url.contains("test"),
        "refusing to run against a database whose name does not contain test"
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

fn unique_suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string()
}

fn credential(seed: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(seed.as_bytes()))
}

async fn insert_authentication_session(
    pool: &PgPool,
    principal_id: &str,
    discord_user_id: UserId,
    credential: &str,
    last_seen_minutes_ago: i64,
) {
    sqlx::query(
        "INSERT INTO product_principals \
         (principal_id, discord_user_id, disabled, identity_revision, display_profile) \
         VALUES ($1, $2, FALSE, 1, '{}'::JSONB)",
    )
    .bind(principal_id)
    .bind(discord_user_id.to_string())
    .execute(pool)
    .await
    .unwrap();
    let session_digest = digest_opaque_session_credential_v1(credential).unwrap();
    let csrf_digest: [u8; 32] = Sha256::digest(format!("csrf:{principal_id}").as_bytes()).into();
    sqlx::query(
        "INSERT INTO product_auth_sessions \
         (session_digest, principal_id, csrf_digest, authenticated_at, created_at, \
          last_seen_at, idle_expires_at, absolute_expires_at) \
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP - INTERVAL '20 minutes', \
          CURRENT_TIMESTAMP - INTERVAL '20 minutes', \
          CURRENT_TIMESTAMP - make_interval(mins => $4), \
          CURRENT_TIMESTAMP + INTERVAL '20 minutes', CURRENT_TIMESTAMP + INTERVAL '12 hours')",
    )
    .bind(session_digest.as_bytes().as_slice())
    .bind(principal_id)
    .bind(csrf_digest.as_slice())
    .bind(i32::try_from(last_seen_minutes_ago).unwrap())
    .execute(pool)
    .await
    .unwrap();
}

async fn preview_ready_snapshot() -> (design_harness::SessionSnapshot, ResourceBindingMap) {
    let mut bindings = ResourceBindingMap::default();
    bindings
        .channel_bindings
        .insert(ResourceKey("community_hub".to_string()), ChannelId(700));
    let mut session =
        DesignSession::with_intent_recipe(ScriptedClient::preview_ready(), bindings.clone());
    assert!(matches!(
        session
            .run_burst(
                "Create private study rooms in community_hub and prepare a validated preview"
            )
            .await,
        BurstOutcome::Ready { .. }
    ));
    (session.snapshot(), bindings)
}

struct ProductFixture {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    session_id: AuthoringSessionId,
    principal_id: PrincipalId,
    application_id: String,
    guild_id: GuildId,
    user_id: UserId,
    credential: String,
    binding_fingerprint: String,
    candidate_revision: u64,
}

async fn insert_product_fixture(pool: &PgPool) -> ProductFixture {
    let suffix = unique_suffix();
    let tenant_id = TenantId::parse(&format!("tenant-{suffix}")).unwrap();
    let installation_id =
        AutomationInstallationId::parse(&format!("installation-{suffix}")).unwrap();
    let session_id = AuthoringSessionId::parse(&format!("session-{suffix}")).unwrap();
    let principal_id = PrincipalId::parse(&format!("principal-{suffix}")).unwrap();
    let numeric_suffix = suffix[suffix.len().saturating_sub(9)..]
        .parse::<u64>()
        .unwrap();
    let user_id = UserId(1_000_000_000 + numeric_suffix);
    let guild_id = GuildId(2_000_000_000 + numeric_suffix);
    let application_id = (3_000_000_000 + numeric_suffix).to_string();
    let credential = credential(&format!("snapshot:{suffix}"));
    insert_authentication_session(pool, principal_id.as_str(), user_id, &credential, 0).await;
    let (snapshot, bindings) = preview_ready_snapshot().await;
    let plaintext = serde_json::to_vec(&snapshot).unwrap();
    let binding_fingerprint = resource_binding_fingerprint_v2(&bindings);
    let stored_bindings = json!({
        "role_bindings": {},
        "channel_bindings": {"community_hub": "700"}
    });
    let authenticated_data =
        build_snapshot_authenticated_data_v1(SnapshotAuthenticatedDataInputV1 {
            tenant_id: &tenant_id,
            installation_id: &installation_id,
            session_id: &session_id,
            generation: SessionGeneration::new(1).unwrap(),
            snapshot_schema_version: snapshot.schema_version,
            binding_fingerprint: &binding_fingerprint,
            encryption_key_id: "keychain:test-authoring-v1",
            encryption_suite: "xchacha20_poly1305",
            encryption_suite_version: 1,
        })
        .unwrap();
    let receipt = DesignSession::restore_intent_recipe(
        ScriptedClient {
            response: Arc::new(Mutex::new(None)),
        },
        Default::default(),
        snapshot.clone(),
        bindings,
    )
    .unwrap()
    .export_preview_ready_artifact()
    .unwrap()
    .receipt()
    .clone();
    let ruleset_key = format!("studyroom_{}", &suffix[suffix.len().saturating_sub(24)..]);
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO product_tenants \
         (tenant_id, lifecycle_state, display_name, display_metadata) \
         VALUES ($1, 'active', $2, '{}'::JSONB)",
    )
    .bind(tenant_id.as_str())
    .bind(format!("Tenant {suffix}"))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, \
          ruleset_key, lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(installation_id.as_str())
    .bind(tenant_id.as_str())
    .bind(&application_id)
    .bind(guild_id.to_string())
    .bind(&ruleset_key)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         VALUES ($1, 1, $2, 1, $3, $4, 1, $5, $6, $7, $8, $9)",
    )
    .bind(installation_id.as_str())
    .bind(tenant_id.as_str())
    .bind(Json(&stored_bindings))
    .bind(binding_fingerprint.as_str())
    .bind(i32::try_from(NonZeroU32::new(1).unwrap().get()).unwrap())
    .bind(i64::try_from(NonZeroU64::new(3600).unwrap().get()).unwrap())
    .bind("1".repeat(64))
    .bind(principal_id.as_str())
    .bind("2".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO authoring_sessions \
         (session_id, tenant_id, installation_id, owner_principal_id, \
          current_generation, lifecycle_state) \
         VALUES ($1, $2, $3, $4, 1, 'active')",
    )
    .bind(session_id.as_str())
    .bind(tenant_id.as_str())
    .bind(installation_id.as_str())
    .bind(principal_id.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO authoring_session_generations \
         (session_id, generation, tenant_id, installation_id, snapshot_schema_version, \
          snapshot_ciphertext, snapshot_nonce, encryption_key_id, encryption_suite, \
          encryption_suite_version, authenticated_metadata_digest, resource_bindings, \
          binding_fingerprint, installation_authority_revision, summary, stage, \
          candidate_revision, candidate_hash, writer_request_digest, harness_contract_revision) \
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, 1, $9, $10, $11, 1, \
          '{}'::JSONB, 'preview_ready', $12, $13, $14, 1)",
    )
    .bind(session_id.as_str())
    .bind(tenant_id.as_str())
    .bind(installation_id.as_str())
    .bind(i64::from(snapshot.schema_version))
    .bind(plaintext)
    .bind(vec![7_u8; 24])
    .bind("keychain:test-authoring-v1")
    .bind("xchacha20_poly1305")
    .bind(authenticated_data.digest_hex())
    .bind(Json(&stored_bindings))
    .bind(binding_fingerprint.as_str())
    .bind(i64::try_from(receipt.candidate_revision).unwrap())
    .bind(&receipt.candidate_ruleset_hash)
    .bind("3".repeat(64))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    ProductFixture {
        tenant_id,
        installation_id,
        session_id,
        principal_id,
        application_id,
        guild_id,
        user_id,
        credential,
        binding_fingerprint: binding_fingerprint.into_string(),
        candidate_revision: receipt.candidate_revision,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn postgres_authentication_uses_database_time_touches_and_revokes() {
    let pool = pool().await;
    let suffix = unique_suffix();
    let principal_id = format!("authentication-{suffix}");
    let numeric_suffix = suffix[suffix.len().saturating_sub(9)..]
        .parse::<u64>()
        .unwrap();
    let user_id = UserId(4_000_000_000 + numeric_suffix);
    let credential = credential(&format!("authentication:{suffix}"));
    insert_authentication_session(&pool, &principal_id, user_id, &credential, 10).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let identity = authentication.authenticate(&credential).await.unwrap();
    assert!(format!("{identity:?}").contains(&principal_id));
    let session_digest = digest_opaque_session_credential_v1(&credential).unwrap();
    let persisted_digest = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT session_digest FROM product_auth_sessions WHERE session_digest = $1",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted_digest, session_digest.as_bytes());
    assert_ne!(persisted_digest, credential.as_bytes());
    let last_seen_age_seconds = sqlx::query_scalar::<_, f64>(
        "SELECT EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - last_seen_at))::DOUBLE PRECISION \
         FROM product_auth_sessions WHERE session_digest = $1",
    )
    .bind(session_digest.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(last_seen_age_seconds < 5.0);
    sqlx::query(
        "UPDATE product_auth_sessions \
         SET revoked_at = CURRENT_TIMESTAMP, revocation_reason = 'test_revocation' \
         WHERE session_digest = $1",
    )
    .bind(session_digest.as_bytes().as_slice())
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        authentication.authenticate(&credential).await,
        Err(authoring_application::AuthenticationError::Revoked)
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn postgres_atomic_snapshot_rehydrates_only_the_exact_authorized_generation() {
    let pool = pool().await;
    let fixture = insert_product_fixture(&pool).await;
    let authentication = PostgresAuthentication::new(pool.clone());
    let guild_authority = TestGuildAuthority {
        tenant_id: fixture.tenant_id.clone(),
        installation_id: fixture.installation_id.clone(),
        application_id: fixture.application_id.clone(),
        guild_id: fixture.guild_id,
        user_id: fixture.user_id,
    };
    let snapshots = PostgresAuthorizedPromotionSnapshots::new(pool, PassthroughCipher);
    let promotions = PromotionCapture;
    let application =
        AuthoringApplication::new(&authentication, &guild_authority, &snapshots, &promotions);
    let output = application
        .promote_owned_session(
            &fixture.credential,
            &InstallationSelectorV1::new(fixture.installation_id.clone()),
            PromoteOwnedSessionV1 {
                idempotency_key: IdempotencyKey::parse("postgres-atomic-snapshot").unwrap(),
                session_id: fixture.session_id.clone(),
                expected_generation: SessionGeneration::new(1).unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(output.0, fixture.tenant_id.as_str());
    assert_eq!(output.1, fixture.candidate_revision);
    assert_eq!(output.2, fixture.binding_fingerprint);
    assert!(fixture.principal_id.as_str().starts_with("principal-"));
    let stale = application
        .promote_owned_session(
            &fixture.credential,
            &InstallationSelectorV1::new(fixture.installation_id),
            PromoteOwnedSessionV1 {
                idempotency_key: IdempotencyKey::parse("postgres-stale-snapshot").unwrap(),
                session_id: fixture.session_id,
                expected_generation: SessionGeneration::new(2).unwrap(),
            },
        )
        .await;
    assert!(matches!(
        stale,
        Err(authoring_application::AuthoringApplicationError::Session(
            authoring_application::OwnedSessionLoadError::GenerationMismatch
        ))
    ));
}

#[test]
fn authenticated_identity_remains_issued_by_authentication_port() {
    let identity = AuthenticatedIdentityV1::from_authentication(
        PrincipalId::parse("adapter-contract").unwrap(),
    );
    assert!(format!("{identity:?}").contains("adapter-contract"));
}
