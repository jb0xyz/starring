use std::num::NonZeroU64;
use std::time::{Duration, SystemTime};

use authoring_application::{
    AuthenticatedActorV1, AuthenticatedSessionFingerprintV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, AuthoringConversationConfigV1,
    AuthoringConversationError, AuthoringSessionObservationErrorV1, AuthoringSessionObservationV1,
    AuthorizedInstallationScopeV1, AuthorizedInstallationV1, CapabilityV1, ConversationApplication,
    FreshGuildAuthorityError, FreshGuildAuthorityEvidence, FreshGuildAuthorityPort,
    InstallationSelectorV1, ReadAuthoringSessionV1, SafeAuthoringTurnProjectionV1,
    SafeAuthoringTurnStateV1,
};
use authoring_application_postgres::{
    build_writer_snapshot_authenticated_data_v1, PostgresAuthoringConversationStoreV1,
    ProductActionDigestKeyV1, ProductActionDigestKeyringV1, SnapshotAuthenticatedDataInputV1,
    SnapshotEnvelopeCipher, SnapshotEnvelopeEncryptionPort, SnapshotEnvelopeKeyV1,
    SnapshotEnvelopeKeyringV1, WriterSnapshotAuthenticatedDataInputV1,
    XChaCha20Poly1305SnapshotEnvelopeCipherV1,
};
use authoring_promotion::{
    AuthoringSessionId, AutomationInstallationId, PrincipalId, SessionGeneration, TenantId,
};
use design_harness::DesignSession;
use discord_model::{GuildId, UserId};
use resource_resolution::ResourceBindingFingerprint;
use serde_json::json;
use sqlx::PgPool;
use zeroize::Zeroizing;

use super::*;

const ACTIVE_KEY_ID: &str = "snapshot-read-active-v2";
const RETIRED_KEY_ID: &str = "snapshot-read-retired-v1";

#[derive(Clone)]
struct ReadAuthentication {
    principal_id: PrincipalId,
}

impl AuthenticationPort for ReadAuthentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if credential != "read-credential" {
            return Err(AuthenticationError::InvalidCredential);
        }
        Ok(AuthenticationClaimsV1::from_authentication(
            self.principal_id.clone(),
            AuthenticatedSessionFingerprintV1::from_sha256_digest([29; 32]),
        ))
    }
}

#[derive(Clone)]
struct ReadEvidence {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    authority_digest: String,
    guild_id: GuildId,
    acting_user_id: UserId,
    guild_owner: bool,
}

impl FreshGuildAuthorityEvidence for ReadEvidence {
    fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }

    fn discord_application_id(&self) -> NonZeroU64 {
        NonZeroU64::new(9_000_000_000_000_000_101).unwrap()
    }

    fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    fn acting_user_id(&self) -> UserId {
        self.acting_user_id
    }

    fn capability(&self) -> CapabilityV1 {
        CapabilityV1::Read
    }

    fn guild_owner(&self) -> bool {
        self.guild_owner
    }

    fn effective_permissions_bits(&self) -> u64 {
        0
    }

    fn installation_authority_revision(&self) -> NonZeroU64 {
        NonZeroU64::new(1).unwrap()
    }

    fn installation_authority_digest(&self) -> &str {
        &self.authority_digest
    }

    fn observation_digest(&self) -> &str {
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    }

    fn observed_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(100)
    }

    fn expires_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(105)
    }
}

#[derive(Clone)]
struct ReadAuthority {
    scope: AuthorizedInstallationScopeV1,
    evidence: ReadEvidence,
}

impl FreshGuildAuthorityPort for ReadAuthority {
    type Evidence = ReadEvidence;

    async fn authorize_installation(
        &self,
        _actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError> {
        assert_eq!(capability, CapabilityV1::Read);
        assert_eq!(installation.installation_id(), self.scope.installation_id());
        Ok(AuthorizedInstallationV1::from_fresh_authority(
            self.scope.clone(),
            self.evidence.clone(),
        ))
    }
}

pub(super) fn key_material(seed: u8) -> [u8; 32] {
    std::array::from_fn(|index| seed.wrapping_add((index as u8).wrapping_mul(17)))
}

pub(super) fn snapshot_key(key_id: &str, seed: u8) -> SnapshotEnvelopeKeyV1 {
    SnapshotEnvelopeKeyV1::new(key_id, Zeroizing::new(key_material(seed))).unwrap()
}

pub(super) fn cipher(
    active_key_id: &str,
    active_seed: u8,
    retired: impl IntoIterator<Item = SnapshotEnvelopeKeyV1>,
) -> XChaCha20Poly1305SnapshotEnvelopeCipherV1 {
    XChaCha20Poly1305SnapshotEnvelopeCipherV1::new(
        SnapshotEnvelopeKeyringV1::new(snapshot_key(active_key_id, active_seed), retired).unwrap(),
    )
}

fn digest_keyring() -> ProductActionDigestKeyringV1 {
    ProductActionDigestKeyringV1::new(
        ProductActionDigestKeyV1::from_bytes("writer-read-v1", key_material(181)).unwrap(),
        [],
    )
    .unwrap()
}

fn canonical_projection() -> Vec<u8> {
    SafeAuthoringTurnProjectionV1::from_canonical_json(
        br#"{"schema_version":1,"state":"discussion","assistant_message":"Ready","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#,
    )
    .unwrap()
    .to_canonical_json()
    .unwrap()
}

fn framed_digest(domain: &[u8], value: &[u8]) -> String {
    let mut framed = Vec::with_capacity(16 + domain.len() + value.len());
    framed.extend_from_slice(&u64::try_from(domain.len()).unwrap().to_be_bytes());
    framed.extend_from_slice(domain);
    framed.extend_from_slice(&u64::try_from(value.len()).unwrap().to_be_bytes());
    framed.extend_from_slice(value);
    digest(framed)
}

fn safe_projection_digest(projection: &[u8]) -> String {
    framed_digest(b"starring.authoring.safe_turn_projection.v1", projection)
}

fn writer_key_fingerprint() -> String {
    framed_digest(
        b"starring.authoring.writer_digest_key_fingerprint.v1",
        &key_material(181),
    )
}

fn encrypted_commit_input(
    scope: &Scope,
    seed: &str,
    cipher: &XChaCha20Poly1305SnapshotEnvelopeCipherV1,
) -> CommitInput {
    let snapshot = DesignSession::new(()).snapshot();
    let projection = canonical_projection();
    let projection_digest = safe_projection_digest(&projection);
    let request_digest = digest(format!("request:{seed}"));
    let semantic_digest = digest(format!("semantic:{seed}"));
    let writer_key_id = "writer-read-v1".to_string();
    let writer_key_fingerprint = writer_key_fingerprint();
    let binding_fingerprint =
        ResourceBindingFingerprint::parse(&scope.binding_fingerprint).unwrap();
    let tenant_id = TenantId::parse(&scope.tenant_id).unwrap();
    let installation_id = AutomationInstallationId::parse(&scope.installation_id).unwrap();
    let session_id = AuthoringSessionId::parse(&scope.session_id).unwrap();
    let authenticated_data =
        build_writer_snapshot_authenticated_data_v1(WriterSnapshotAuthenticatedDataInputV1 {
            snapshot: SnapshotAuthenticatedDataInputV1 {
                tenant_id: &tenant_id,
                installation_id: &installation_id,
                session_id: &session_id,
                generation: SessionGeneration::new(1).unwrap(),
                snapshot_schema_version: snapshot.schema_version,
                binding_fingerprint: &binding_fingerprint,
                encryption_key_id: cipher.active_encryption_key_id(),
                encryption_suite: cipher.encryption_suite(),
                encryption_suite_version: cipher.encryption_suite_version(),
            },
            installation_authority_revision: 1,
            installation_authority_digest: &scope.authority_digest,
            writer_request_digest: &request_digest,
            writer_semantic_request_digest: &semantic_digest,
            writer_digest_key_id: &writer_key_id,
            writer_digest_key_fingerprint: &writer_key_fingerprint,
            safe_turn_projection_digest: &projection_digest,
        })
        .unwrap();
    let plaintext = Zeroizing::new(serde_json::to_vec(&snapshot).unwrap());
    let envelope = cipher
        .encrypt(&plaintext, authenticated_data.as_bytes())
        .unwrap();
    CommitInput {
        request_digest,
        semantic_digest,
        writer_key_id,
        writer_key_fingerprint,
        snapshot_schema_version: i64::from(snapshot.schema_version),
        ciphertext: envelope.ciphertext().to_vec(),
        nonce: envelope.nonce().to_vec(),
        encryption_key_id: envelope.encryption_key_id().to_string(),
        metadata_digest: authenticated_data.digest_hex().to_string(),
        summary: json!({
            "panels": 0,
            "modals": 0,
            "rules": 0,
            "actions": 0,
            "unresolved_references": []
        }),
        stage: "discussion".to_string(),
        candidate_revision: None,
        candidate_hash: None,
        projection,
        projection_digest,
    }
}

async fn insert_principal(pool: &PgPool, principal_id: &str, discord_user_id: &str) {
    sqlx::query(
        "INSERT INTO public.product_principals \
         (principal_id, discord_user_id, display_profile) \
         VALUES ($1, $2, '{}'::JSONB)",
    )
    .bind(principal_id)
    .bind(discord_user_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_installation(
    pool: &PgPool,
    tenant_id: &str,
    installation_id: &str,
    principal_id: &str,
    suffix: &str,
    discord_application_id: &str,
    discord_guild_id: &str,
) -> String {
    let authority_digest = digest(format!("authority:{suffix}:1"));
    let binding_fingerprint =
        resource_binding_fingerprint_v2(&ResourceBindingMap::default()).into_string();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, ruleset_key, \
          lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1)",
    )
    .bind(installation_id)
    .bind(tenant_id)
    .bind(discord_application_id)
    .bind(discord_guild_id)
    .bind(format!("authoring_read_{suffix}"))
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
    .bind(installation_id)
    .bind(tenant_id)
    .bind(Json(json!({"role_bindings": {}, "channel_bindings": {}})))
    .bind(&binding_fingerprint)
    .bind(&authority_digest)
    .bind(principal_id)
    .bind(digest(format!("authority-request:{suffix}:1")))
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    authority_digest
}

async fn insert_other_tenant(
    pool: &PgPool,
    suffix: &str,
    principal_id: &str,
) -> (String, String, String) {
    let tenant_id = format!("tenant-other-{suffix}");
    let installation_id = format!("installation-other-{suffix}");
    insert_principal(pool, principal_id, "900000000000000011").await;
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name) \
         VALUES ($1, 'active', 'Authoring read integration')",
    )
    .bind(&tenant_id)
    .execute(pool)
    .await
    .unwrap();
    let authority_digest = insert_installation(
        pool,
        &tenant_id,
        &installation_id,
        principal_id,
        &format!("other-{suffix}"),
        "900000000000000012",
        "900000000000000013",
    )
    .await;
    (tenant_id, installation_id, authority_digest)
}

pub(super) async fn writer_store_pool(database_name: &str, role: &str) -> PgPool {
    let options = database_url().parse::<PgConnectOptions>().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.database(database_name))
        .await
        .unwrap();
    sqlx::query(&format!("SET ROLE {role}"))
        .execute(&pool)
        .await
        .unwrap();
    let current_user = sqlx::query_scalar::<_, String>("SELECT CURRENT_USER::TEXT")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(current_user, role);
    pool
}

fn read_authority(
    tenant_id: &str,
    installation_id: &str,
    authority_digest: &str,
    guild_owner: bool,
) -> ReadAuthority {
    let tenant_id = TenantId::parse(tenant_id).unwrap();
    let installation_id = AutomationInstallationId::parse(installation_id).unwrap();
    let guild_id = GuildId(9_000_000_000_000_000_102);
    let acting_user_id = UserId(9_000_000_000_000_000_103);
    ReadAuthority {
        scope: AuthorizedInstallationScopeV1::from_fresh_authority(
            tenant_id.clone(),
            installation_id.clone(),
            guild_id,
            acting_user_id,
        ),
        evidence: ReadEvidence {
            tenant_id,
            installation_id,
            authority_digest: authority_digest.to_string(),
            guild_id,
            acting_user_id,
            guild_owner,
        },
    }
}

async fn read_session<C: SnapshotEnvelopeCipher>(
    store: &PostgresAuthoringConversationStoreV1<C>,
    principal_id: &str,
    tenant_id: &str,
    installation_id: &str,
    authority_digest: &str,
    session_id: &str,
    guild_owner: bool,
) -> Result<AuthoringSessionObservationV1, AuthoringConversationError> {
    let authentication = ReadAuthentication {
        principal_id: PrincipalId::parse(principal_id).unwrap(),
    };
    let authority = read_authority(tenant_id, installation_id, authority_digest, guild_owner);
    let application = ConversationApplication::new(
        &authentication,
        &authority,
        store,
        &(),
        &(),
        AuthoringConversationConfigV1::default(),
    );
    application
        .read_session(
            "read-credential",
            &InstallationSelectorV1::new(AutomationInstallationId::parse(installation_id).unwrap()),
            ReadAuthoringSessionV1::new(AuthoringSessionId::parse(session_id).unwrap()),
        )
        .await
}

fn assert_not_found(result: Result<AuthoringSessionObservationV1, AuthoringConversationError>) {
    assert!(matches!(
        result,
        Err(AuthoringConversationError::Observation(
            AuthoringSessionObservationErrorV1::NotFound
        ))
    ));
}

fn assert_invalid_state(result: Result<AuthoringSessionObservationV1, AuthoringConversationError>) {
    assert!(matches!(
        result,
        Err(AuthoringConversationError::Observation(
            AuthoringSessionObservationErrorV1::InvalidState
        ))
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STARRING_TEST_DATABASE_URL"]
async fn authorized_session_read_is_exactly_scoped_and_envelope_key_safe() {
    let suffix = unique_suffix();
    let tail = &suffix[suffix.len().saturating_sub(14)..];
    let database_name = format!("starring_authoring_read_test_{tail}");
    let role = format!("starring_authoring_read_test_{tail}");
    let (mut administrator, migration_pool) = temporary_database(&database_name).await;
    apply_fresh_migrations(&migration_pool).await;
    migration_pool.close().await;
    let pool = application_pool(&database_name).await;
    let database_major = sqlx::query_scalar::<_, i32>(
        "SELECT pg_catalog.current_setting('server_version_num')::INTEGER / 10000",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(database_major, 16);
    grant_writer_capability(&pool, &mut administrator, &role).await;
    let read_pool = writer_store_pool(&database_name, &role).await;
    let owner_scope = seed_scope(&pool, tail).await;
    let other_principal_id = format!("principal-other-{tail}");
    let (other_tenant_id, other_installation_id, other_authority_digest) =
        insert_other_tenant(&pool, tail, &other_principal_id).await;
    let same_tenant_installation_id = format!("installation-peer-{tail}");
    let same_tenant_authority_digest = insert_installation(
        &pool,
        &owner_scope.tenant_id,
        &same_tenant_installation_id,
        &owner_scope.principal_id,
        &format!("peer-{tail}"),
        "900000000000000014",
        "900000000000000015",
    )
    .await;

    let active_writer = cipher(ACTIVE_KEY_ID, 71, []);
    let retired_writer = cipher(RETIRED_KEY_ID, 43, []);
    let rotated_reader = cipher(ACTIVE_KEY_ID, 71, [snapshot_key(RETIRED_KEY_ID, 43)]);
    let active_input = encrypted_commit_input(&owner_scope, "active", &active_writer);
    let active_commit = commit(&pool, &role, &owner_scope, 0, &active_input).await;
    assert_eq!(active_commit.outcome_code, "committed");
    let active_row = load(&pool, &role, &owner_scope, 0).await;
    assert_eq!(active_row.outcome_code, "loaded");
    assert_eq!(active_row.head_generation, Some(1));
    assert_eq!(
        active_row.snapshot_ciphertext.as_deref(),
        Some(active_input.ciphertext.as_slice())
    );
    assert_eq!(
        active_row.snapshot_nonce.as_deref(),
        Some(active_input.nonce.as_slice())
    );
    assert_eq!(
        active_row.encryption_key_id.as_deref(),
        Some(active_input.encryption_key_id.as_str())
    );
    assert_eq!(
        active_row.authenticated_metadata_digest.as_deref(),
        Some(active_input.metadata_digest.as_str())
    );
    assert_eq!(
        active_row.resource_bindings.as_ref().map(|value| &value.0),
        Some(&owner_scope.bindings)
    );
    assert_eq!(
        active_row.binding_fingerprint.as_deref(),
        Some(owner_scope.binding_fingerprint.as_str())
    );
    assert_eq!(active_row.installation_authority_revision, Some(1));
    assert_eq!(
        active_row.authority_payload_digest.as_deref(),
        Some(owner_scope.authority_digest.as_str())
    );
    assert_eq!(
        active_row.safe_turn_projection.as_deref(),
        Some(active_input.projection.as_slice())
    );
    assert_eq!(
        active_row.safe_turn_projection_digest.as_deref(),
        Some(active_input.projection_digest.as_str())
    );
    assert_eq!(active_row.current_authority_revision, Some(1));
    assert_eq!(
        active_row.current_authority_payload_digest.as_deref(),
        Some(owner_scope.authority_digest.as_str())
    );
    assert_eq!(
        active_row
            .current_resource_bindings
            .as_ref()
            .map(|value| &value.0),
        Some(&owner_scope.bindings)
    );
    assert_eq!(
        active_row.current_binding_fingerprint.as_deref(),
        Some(owner_scope.binding_fingerprint.as_str())
    );

    let observation = read_session(
        &PostgresAuthoringConversationStoreV1::new(
            read_pool.clone(),
            rotated_reader.clone(),
            digest_keyring(),
        ),
        &owner_scope.principal_id,
        &owner_scope.tenant_id,
        &owner_scope.installation_id,
        &owner_scope.authority_digest,
        &owner_scope.session_id,
        true,
    )
    .await
    .unwrap();
    assert_eq!(observation.session_id().as_str(), owner_scope.session_id);
    assert_eq!(observation.generation().get(), 1);
    assert_eq!(
        observation.projection().state(),
        SafeAuthoringTurnStateV1::Discussion
    );

    assert_not_found(
        read_session(
            &PostgresAuthoringConversationStoreV1::new(
                read_pool.clone(),
                rotated_reader.clone(),
                digest_keyring(),
            ),
            &other_principal_id,
            &owner_scope.tenant_id,
            &owner_scope.installation_id,
            &owner_scope.authority_digest,
            &owner_scope.session_id,
            false,
        )
        .await,
    );
    assert_not_found(
        read_session(
            &PostgresAuthoringConversationStoreV1::new(
                read_pool.clone(),
                rotated_reader.clone(),
                digest_keyring(),
            ),
            &owner_scope.principal_id,
            &other_tenant_id,
            &other_installation_id,
            &other_authority_digest,
            &owner_scope.session_id,
            false,
        )
        .await,
    );
    assert_not_found(
        read_session(
            &PostgresAuthoringConversationStoreV1::new(
                read_pool.clone(),
                rotated_reader.clone(),
                digest_keyring(),
            ),
            &owner_scope.principal_id,
            &owner_scope.tenant_id,
            &same_tenant_installation_id,
            &same_tenant_authority_digest,
            &owner_scope.session_id,
            false,
        )
        .await,
    );

    let non_owner_scope = Scope {
        principal_id: other_principal_id.clone(),
        session_id: format!("session-other-owner-{tail}"),
        ..owner_scope.clone()
    };
    let non_owner_input = encrypted_commit_input(&non_owner_scope, "non-owner", &active_writer);
    let non_owner_commit = commit(&pool, &role, &non_owner_scope, 0, &non_owner_input).await;
    assert_eq!(non_owner_commit.outcome_code, "committed");
    assert_not_found(
        read_session(
            &PostgresAuthoringConversationStoreV1::new(
                read_pool.clone(),
                rotated_reader.clone(),
                digest_keyring(),
            ),
            &owner_scope.principal_id,
            &owner_scope.tenant_id,
            &owner_scope.installation_id,
            &owner_scope.authority_digest,
            &non_owner_scope.session_id,
            false,
        )
        .await,
    );

    let retired_scope = Scope {
        session_id: format!("session-retired-{tail}"),
        ..owner_scope.clone()
    };
    let retired_input = encrypted_commit_input(&retired_scope, "retired", &retired_writer);
    let retired_commit = commit(&pool, &role, &retired_scope, 0, &retired_input).await;
    assert_eq!(retired_commit.outcome_code, "committed");
    let retired_observation = read_session(
        &PostgresAuthoringConversationStoreV1::new(
            read_pool.clone(),
            rotated_reader.clone(),
            digest_keyring(),
        ),
        &retired_scope.principal_id,
        &retired_scope.tenant_id,
        &retired_scope.installation_id,
        &retired_scope.authority_digest,
        &retired_scope.session_id,
        true,
    )
    .await
    .unwrap();
    assert_eq!(retired_observation.generation().get(), 1);

    assert_invalid_state(
        read_session(
            &PostgresAuthoringConversationStoreV1::new(
                read_pool.clone(),
                cipher(ACTIVE_KEY_ID, 71, []),
                digest_keyring(),
            ),
            &retired_scope.principal_id,
            &retired_scope.tenant_id,
            &retired_scope.installation_id,
            &retired_scope.authority_digest,
            &retired_scope.session_id,
            true,
        )
        .await,
    );
    assert_invalid_state(
        read_session(
            &PostgresAuthoringConversationStoreV1::new(
                read_pool.clone(),
                cipher(ACTIVE_KEY_ID, 71, [snapshot_key(RETIRED_KEY_ID, 137)]),
                digest_keyring(),
            ),
            &retired_scope.principal_id,
            &retired_scope.tenant_id,
            &retired_scope.installation_id,
            &retired_scope.authority_digest,
            &retired_scope.session_id,
            true,
        )
        .await,
    );

    let corrupt_scope = Scope {
        session_id: format!("session-corrupt-{tail}"),
        ..owner_scope.clone()
    };
    let mut corrupt_input = encrypted_commit_input(&corrupt_scope, "corrupt", &active_writer);
    corrupt_input.ciphertext[0] ^= 1;
    let corrupt_commit = commit(&pool, &role, &corrupt_scope, 0, &corrupt_input).await;
    assert_eq!(corrupt_commit.outcome_code, "committed");
    assert_invalid_state(
        read_session(
            &PostgresAuthoringConversationStoreV1::new(
                read_pool.clone(),
                rotated_reader,
                digest_keyring(),
            ),
            &corrupt_scope.principal_id,
            &corrupt_scope.tenant_id,
            &corrupt_scope.installation_id,
            &corrupt_scope.authority_digest,
            &corrupt_scope.session_id,
            true,
        )
        .await,
    );

    read_pool.close().await;
    cleanup(administrator, pool, &database_name, &role).await;
}
