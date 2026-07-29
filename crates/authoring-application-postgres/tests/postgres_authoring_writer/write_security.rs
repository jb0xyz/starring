use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use authoring_application::{
    AuthenticatedActorV1, AuthenticatedSessionFingerprintV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, AuthoringAdmissionError,
    AuthoringConversationConfigV1, AuthoringConversationError, AuthoringExpectedGenerationV1,
    AuthoringHumanMessageV1, AuthoringMutationDispositionV1, AuthoringSessionLoadError,
    AuthoringTurnAdmissionPort, AuthoringTurnOutcomeV1, AuthorizedInstallationScopeV1,
    AuthorizedInstallationV1, CapabilityV1, ConversationApplication, FreshGuildAuthorityError,
    FreshGuildAuthorityEvidence, FreshGuildAuthorityPort, InstallationSelectorV1,
    LocalAuthoringRequestKeyV1, MutationAuthenticationPort, ProductIdempotencyKeyV1,
    StartOrAdvanceAuthoringTurnV1,
};
use authoring_application_postgres::{
    EncryptedSnapshotEnvelopeV1, PostgresAuthoringConversationStoreV1, ProductActionDigestKeyV1,
    ProductActionDigestKeyringV1, SnapshotEnvelopeCipher, SnapshotEnvelopeCipherError,
    SnapshotEnvelopeEncryptionPort, XChaCha20Poly1305SnapshotEnvelopeCipherV1,
};
use authoring_promotion::{AuthoringSessionId, AutomationInstallationId, PrincipalId, TenantId};
use design_harness::{LlmClient, LlmError, LlmResponse, Message, ToolCall, ToolDefinition};
use discord_model::{GuildId, UserId};
use zeroize::Zeroizing;

use super::read_security::{cipher, key_material, snapshot_key, writer_store_pool};
use super::*;

const OLD_SNAPSHOT_KEY_ID: &str = "snapshot-write-v1";
const ACTIVE_SNAPSHOT_KEY_ID: &str = "snapshot-write-v2";
const OLD_DIGEST_KEY_ID: &str = "writer-write-v1";
const ACTIVE_DIGEST_KEY_ID: &str = "writer-write-v2";

#[derive(Clone)]
struct WriteAuthentication {
    principal_id: PrincipalId,
}

impl AuthenticationPort for WriteAuthentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if credential != "writer-credential" {
            return Err(AuthenticationError::InvalidCredential);
        }
        Ok(AuthenticationClaimsV1::from_authentication(
            self.principal_id.clone(),
            AuthenticatedSessionFingerprintV1::from_sha256_digest([37; 32]),
        ))
    }
}

impl MutationAuthenticationPort for WriteAuthentication {
    type CsrfProof = str;

    async fn authenticate_mutation(
        &self,
        credential: &Self::Credential,
        csrf: &Self::CsrfProof,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if csrf != "writer-csrf" {
            return Err(AuthenticationError::InvalidCsrf);
        }
        self.authenticate(credential).await
    }
}

#[derive(Clone)]
struct WriteEvidence {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    authority_revision: NonZeroU64,
    authority_digest: String,
    guild_id: GuildId,
    acting_user_id: UserId,
}

impl FreshGuildAuthorityEvidence for WriteEvidence {
    fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }

    fn discord_application_id(&self) -> NonZeroU64 {
        NonZeroU64::new(9_000_000_000_000_000_002).unwrap()
    }

    fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    fn acting_user_id(&self) -> UserId {
        self.acting_user_id
    }

    fn capability(&self) -> CapabilityV1 {
        CapabilityV1::Author
    }

    fn guild_owner(&self) -> bool {
        true
    }

    fn effective_permissions_bits(&self) -> u64 {
        0
    }

    fn installation_authority_revision(&self) -> NonZeroU64 {
        self.authority_revision
    }

    fn installation_authority_digest(&self) -> &str {
        &self.authority_digest
    }

    fn observation_digest(&self) -> &str {
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
    }

    fn observed_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(100)
    }

    fn expires_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(105)
    }
}

#[derive(Clone)]
struct WriteAuthority {
    principal_id: PrincipalId,
    scope: AuthorizedInstallationScopeV1,
    evidence: WriteEvidence,
}

impl FreshGuildAuthorityPort for WriteAuthority {
    type Evidence = WriteEvidence;

    async fn authorize_installation(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError> {
        assert_eq!(actor.principal_id(), &self.principal_id);
        assert_eq!(installation.installation_id(), self.scope.installation_id());
        assert_eq!(capability, CapabilityV1::Author);
        Ok(AuthorizedInstallationV1::from_fresh_authority(
            self.scope.clone(),
            self.evidence.clone(),
        ))
    }
}

struct OpenAdmission;

impl AuthoringTurnAdmissionPort for OpenAdmission {
    type KeyedPermit = ();
    type ModelPermit = ();

    async fn acquire_keyed(
        &self,
        _key: &LocalAuthoringRequestKeyV1,
    ) -> Result<Self::KeyedPermit, AuthoringAdmissionError> {
        Ok(())
    }

    async fn acquire_model_capacity(&self) -> Result<Self::ModelPermit, AuthoringAdmissionError> {
        Ok(())
    }
}

#[derive(Clone)]
struct ScriptedClient {
    responses: Arc<Mutex<VecDeque<LlmResponse>>>,
    calls: Arc<AtomicUsize>,
}

impl ScriptedClient {
    fn new(responses: impl IntoIterator<Item = LlmResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl LlmClient for ScriptedClient {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted authoring response"))
    }
}

#[derive(Clone)]
struct CorruptingEncryptionCipher {
    inner: XChaCha20Poly1305SnapshotEnvelopeCipherV1,
}

impl SnapshotEnvelopeCipher for CorruptingEncryptionCipher {
    fn configured_encryption_key_ids(&self) -> Option<Vec<&str>> {
        self.inner.configured_encryption_key_ids()
    }

    async fn decrypt(
        &self,
        envelope: &EncryptedSnapshotEnvelopeV1,
        authenticated_data: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, SnapshotEnvelopeCipherError> {
        self.inner.decrypt(envelope, authenticated_data).await
    }
}

impl SnapshotEnvelopeEncryptionPort for CorruptingEncryptionCipher {
    fn active_encryption_key_id(&self) -> &str {
        self.inner.active_encryption_key_id()
    }

    fn encryption_suite(&self) -> &str {
        self.inner.encryption_suite()
    }

    fn encryption_suite_version(&self) -> u16 {
        self.inner.encryption_suite_version()
    }

    fn encrypt(
        &self,
        plaintext: &Zeroizing<Vec<u8>>,
        authenticated_data: &[u8],
    ) -> Result<EncryptedSnapshotEnvelopeV1, SnapshotEnvelopeCipherError> {
        let envelope = self.inner.encrypt(plaintext, authenticated_data)?;
        let mut ciphertext = envelope.ciphertext().to_vec();
        let first = ciphertext
            .first_mut()
            .ok_or(SnapshotEnvelopeCipherError::Backend)?;
        *first ^= 1;
        EncryptedSnapshotEnvelopeV1::from_persisted_parts(
            ciphertext,
            envelope.nonce().to_vec(),
            envelope.encryption_key_id().to_string(),
            envelope.encryption_suite().to_string(),
            envelope.encryption_suite_version(),
        )
        .map_err(|_| SnapshotEnvelopeCipherError::Backend)
    }
}

fn writer_digest_key(key_id: &str, seed: u8) -> ProductActionDigestKeyV1 {
    ProductActionDigestKeyV1::from_bytes(key_id, key_material(seed)).unwrap()
}

fn writer_digest_keyring(
    active_key_id: &str,
    active_seed: u8,
    retired: impl IntoIterator<Item = ProductActionDigestKeyV1>,
) -> ProductActionDigestKeyringV1 {
    ProductActionDigestKeyringV1::new(writer_digest_key(active_key_id, active_seed), retired)
        .unwrap()
}

fn discussion_response(expected_revision: u64) -> LlmResponse {
    LlmResponse::ToolCalls(vec![ToolCall {
        id: format!("discussion-{expected_revision}"),
        name: "interpret_intent_core".to_string(),
        arguments: serde_json::json!({
            "expected_revision": expected_revision,
            "request_mode": "discussion",
            "automation_kind": "none",
            "requested_outcome": "discussion",
            "hub_channel": null,
            "language": "en",
            "close_policy": "disabled",
            "other_unmapped_required_capabilities": [],
            "response": "The design is ready for the next decision."
        })
        .to_string(),
    }])
}

fn write_authority(scope: &Scope) -> WriteAuthority {
    let tenant_id = TenantId::parse(&scope.tenant_id).unwrap();
    let installation_id = AutomationInstallationId::parse(&scope.installation_id).unwrap();
    let guild_id = GuildId(9_000_000_000_000_000_003);
    let acting_user_id = UserId(9_000_000_000_000_000_001);
    WriteAuthority {
        principal_id: PrincipalId::parse(&scope.principal_id).unwrap(),
        scope: AuthorizedInstallationScopeV1::from_fresh_authority(
            tenant_id.clone(),
            installation_id.clone(),
            guild_id,
            acting_user_id,
        ),
        evidence: WriteEvidence {
            tenant_id,
            installation_id,
            authority_revision: NonZeroU64::new(u64::try_from(scope.authority_revision).unwrap())
                .unwrap(),
            authority_digest: scope.authority_digest.clone(),
            guild_id,
            acting_user_id,
        },
    }
}

async fn run_turn<C>(
    store: &PostgresAuthoringConversationStoreV1<C>,
    client: &ScriptedClient,
    scope: &Scope,
    session_id: &str,
    expected_generation: u64,
    idempotency_key: &str,
    human_message: &str,
) -> Result<AuthoringTurnOutcomeV1, AuthoringConversationError>
where
    C: SnapshotEnvelopeCipher + SnapshotEnvelopeEncryptionPort,
{
    let authentication = WriteAuthentication {
        principal_id: PrincipalId::parse(&scope.principal_id).unwrap(),
    };
    let authority = write_authority(scope);
    let admission = OpenAdmission;
    let installation = InstallationSelectorV1::new(
        AutomationInstallationId::parse(&scope.installation_id).unwrap(),
    );
    let application = ConversationApplication::new(
        &authentication,
        &authority,
        store,
        &admission,
        client,
        AuthoringConversationConfigV1::default(),
    );
    application
        .start_or_advance_turn(
            "writer-credential",
            "writer-csrf",
            &installation,
            StartOrAdvanceAuthoringTurnV1::new(
                AuthoringSessionId::parse(session_id).unwrap(),
                AuthoringExpectedGenerationV1::new(expected_generation).unwrap(),
                ProductIdempotencyKeyV1::parse(idempotency_key).unwrap(),
                AuthoringHumanMessageV1::parse(human_message).unwrap(),
            ),
        )
        .await
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn postgres_store_commits_loads_replays_rotated_keys_and_rejects_tampering() {
    let suffix = unique_suffix();
    let tail = &suffix[suffix.len().saturating_sub(14)..];
    let database_name = format!("starring_authoring_store_test_{tail}");
    let role = format!("starring_authoring_store_test_{tail}");
    let (mut administrator, migration_pool) = temporary_database(&database_name).await;
    apply_fresh_migrations(&migration_pool).await;
    migration_pool.close().await;
    let pool = application_pool(&database_name).await;
    grant_writer_capability(&pool, &mut administrator, &role).await;
    let writer_pool = writer_store_pool(&database_name, &role).await;
    let scope = seed_scope(&pool, tail).await;

    let old_cipher = cipher(OLD_SNAPSHOT_KEY_ID, 53, []);
    let old_store = PostgresAuthoringConversationStoreV1::new(
        writer_pool.clone(),
        old_cipher,
        writer_digest_keyring(OLD_DIGEST_KEY_ID, 79, []),
    );
    let first_client = ScriptedClient::new([discussion_response(0)]);
    let first = run_turn(
        &old_store,
        &first_client,
        &scope,
        &scope.session_id,
        0,
        "writer-e2e-first",
        "Discuss the initial automation shape",
    )
    .await
    .unwrap();
    assert_eq!(
        first.disposition(),
        Some(AuthoringMutationDispositionV1::Created)
    );
    assert_eq!(first.generation().map(|value| value.get()), Some(1));
    assert_eq!(first_client.calls(), 1);
    let first_projection = first.projection().to_canonical_json().unwrap();

    let rotated_cipher = cipher(
        ACTIVE_SNAPSHOT_KEY_ID,
        107,
        [snapshot_key(OLD_SNAPSHOT_KEY_ID, 53)],
    );
    let rotated_digest_keyring = writer_digest_keyring(
        ACTIVE_DIGEST_KEY_ID,
        131,
        [writer_digest_key(OLD_DIGEST_KEY_ID, 79)],
    );
    let rotated_store = PostgresAuthoringConversationStoreV1::new(
        writer_pool.clone(),
        rotated_cipher.clone(),
        rotated_digest_keyring.clone(),
    );
    let second_client = ScriptedClient::new([discussion_response(1)]);
    let second = run_turn(
        &rotated_store,
        &second_client,
        &scope,
        &scope.session_id,
        1,
        "writer-e2e-second",
        "Continue with the next design decision",
    )
    .await
    .unwrap();
    assert_eq!(
        second.disposition(),
        Some(AuthoringMutationDispositionV1::Created)
    );
    assert_eq!(second.generation().map(|value| value.get()), Some(2));
    assert_eq!(second_client.calls(), 1);
    let second_projection = second.projection().to_canonical_json().unwrap();

    let generation_one = load(&pool, &role, &scope, 1).await;
    assert_eq!(
        generation_one.encryption_key_id.as_deref(),
        Some(OLD_SNAPSHOT_KEY_ID)
    );
    assert_eq!(
        generation_one.writer_digest_key_id.as_deref(),
        Some(OLD_DIGEST_KEY_ID)
    );
    assert_eq!(
        generation_one.safe_turn_projection.as_deref(),
        Some(first_projection.as_slice())
    );
    assert_eq!(
        generation_one.snapshot_nonce.as_ref().map(Vec::len),
        Some(24)
    );
    let generation_two = load(&pool, &role, &scope, 2).await;
    assert_eq!(
        generation_two.encryption_key_id.as_deref(),
        Some(ACTIVE_SNAPSHOT_KEY_ID)
    );
    assert_eq!(
        generation_two.writer_digest_key_id.as_deref(),
        Some(ACTIVE_DIGEST_KEY_ID)
    );
    assert_eq!(
        generation_two.safe_turn_projection.as_deref(),
        Some(second_projection.as_slice())
    );
    assert_eq!(
        generation_two.snapshot_nonce.as_ref().map(Vec::len),
        Some(24)
    );
    assert_ne!(generation_one.snapshot_nonce, generation_two.snapshot_nonce);
    assert_ne!(
        generation_one.snapshot_ciphertext,
        generation_two.snapshot_ciphertext
    );

    let replay_client = ScriptedClient::new([]);
    let first_replay = run_turn(
        &rotated_store,
        &replay_client,
        &scope,
        &scope.session_id,
        0,
        "writer-e2e-first",
        "Discuss the initial automation shape",
    )
    .await
    .unwrap();
    assert_eq!(
        first_replay.disposition(),
        Some(AuthoringMutationDispositionV1::ExactReplay)
    );
    assert_eq!(first_replay.generation().map(|value| value.get()), Some(1));
    assert_eq!(
        first_replay.projection().to_canonical_json().unwrap(),
        first_projection
    );
    let second_replay = run_turn(
        &rotated_store,
        &replay_client,
        &scope,
        &scope.session_id,
        1,
        "writer-e2e-second",
        "Continue with the next design decision",
    )
    .await
    .unwrap();
    assert_eq!(
        second_replay.disposition(),
        Some(AuthoringMutationDispositionV1::ExactReplay)
    );
    assert_eq!(second_replay.generation().map(|value| value.get()), Some(2));
    assert_eq!(
        second_replay.projection().to_canonical_json().unwrap(),
        second_projection
    );
    assert_eq!(replay_client.calls(), 0);

    let corrupt_session_id = format!("session-corrupt-{tail}");
    let corrupt_store = PostgresAuthoringConversationStoreV1::new(
        writer_pool.clone(),
        CorruptingEncryptionCipher {
            inner: rotated_cipher,
        },
        rotated_digest_keyring.clone(),
    );
    let corrupt_client = ScriptedClient::new([discussion_response(0)]);
    let corrupt_created = run_turn(
        &corrupt_store,
        &corrupt_client,
        &scope,
        &corrupt_session_id,
        0,
        "writer-e2e-corrupt",
        "Discuss a separate automation shape",
    )
    .await
    .unwrap();
    assert_eq!(
        corrupt_created.disposition(),
        Some(AuthoringMutationDispositionV1::Created)
    );
    let normal_store = PostgresAuthoringConversationStoreV1::new(
        writer_pool.clone(),
        cipher(
            ACTIVE_SNAPSHOT_KEY_ID,
            107,
            [snapshot_key(OLD_SNAPSHOT_KEY_ID, 53)],
        ),
        rotated_digest_keyring,
    );
    let corrupt_replay_client = ScriptedClient::new([]);
    let corrupt_replay = run_turn(
        &normal_store,
        &corrupt_replay_client,
        &scope,
        &corrupt_session_id,
        0,
        "writer-e2e-corrupt",
        "Discuss a separate automation shape",
    )
    .await;
    assert!(matches!(
        corrupt_replay,
        Err(AuthoringConversationError::Store(
            AuthoringSessionLoadError::InvalidState
        ))
    ));
    assert_eq!(corrupt_replay_client.calls(), 0);

    writer_pool.close().await;
    cleanup(administrator, pool, &database_name, &role).await;
}
