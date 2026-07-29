use std::fmt::{Debug, Formatter};
use std::time::Duration;

use authoring_application::{
    AuthoringCommitOutcomeV1, AuthoringExpectedGenerationV1, AuthoringSessionCommitPort,
    AuthoringSessionLoadError, AuthoringSessionLoadPort, AuthoringSessionLoadV1,
    AuthoringSessionObservationErrorV1, AuthoringSessionObservationV1, AuthoringSessionReadPort,
    AuthoringStoredGenerationV1, AuthoringStoredRequestIdentityV1, AuthoringTurnCheckV1,
    AuthorizedAuthoringCommitV1, AuthorizedConversationAccessV1,
    AuthorizedConversationReadAccessV1, FreshGuildAuthorityEvidence, SafeAuthoringTurnProjectionV1,
    SafeAuthoringTurnStateV1,
};
use authoring_promotion::SessionGeneration;
use design_harness::{
    DesignSession, LlmClient, LlmError, LlmResponse, Message, PreviewReadyArtifactV1,
    SessionConfig, SessionSnapshot, ToolDefinition,
};
use resource_resolution::{
    resource_binding_fingerprint_v2, ResourceBindingFingerprint, ResourceBindingMap,
};
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::types::Json;
use zeroize::Zeroizing;

use super::digest::{
    safe_projection_digest_v1, writer_digest_candidates_v1, WriterDigestCandidateV1,
    WriterDigestInputV1,
};
use super::row::{AuthoringWriterCheckRowV1, AuthoringWriterCommitRowV1, AuthoringWriterLoadRowV1};
use crate::bindings::{decode_resource_bindings, encode_resource_bindings};
use crate::envelope::{
    build_snapshot_authenticated_data_v1, build_writer_snapshot_authenticated_data_v1,
    EncryptedSnapshotEnvelopeV1, SnapshotAuthenticatedDataInputV1, SnapshotEnvelopeCipher,
    SnapshotEnvelopeEncryptionPort, WriterSnapshotAuthenticatedDataInputV1,
};
use crate::{ProductActionDigestKeyringV1, ProductDatabaseFailureV1};

const DEFAULT_STATEMENT_TIMEOUT_MILLIS: u64 = 2_000;
const MAX_STATEMENT_TIMEOUT_MILLIS: u64 = 60_000;
const DEFAULT_MAX_PLAINTEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_PLAINTEXT_BYTES: usize = 8 * 1024 * 1024;
const HARNESS_CONTRACT_REVISION_V1: i64 = 1;
const CHECK_QUERY: &str =
    "SELECT * FROM public.starring_authoring_session_writer_check_v1($1,$2,$3,$4,$5,$6,$7,$8,$9)";
const LOAD_QUERY: &str =
    "SELECT * FROM public.starring_authoring_session_writer_load_v1($1,$2,$3,$4,$5)";
const COMMIT_QUERY: &str = "SELECT * FROM public.starring_authoring_session_writer_commit_v1(\
     $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,\
     $21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31)";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringConversationStoreConfigErrorV1 {
    #[error("authoring writer statement timeout is outside the supported range")]
    InvalidStatementTimeout,
    #[error("authoring writer plaintext bound is outside the supported range")]
    InvalidPlaintextBound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostgresAuthoringConversationStoreConfigV1 {
    statement_timeout: Duration,
    max_plaintext_bytes: usize,
}

impl PostgresAuthoringConversationStoreConfigV1 {
    pub fn new(
        statement_timeout: Duration,
        max_plaintext_bytes: usize,
    ) -> Result<Self, AuthoringConversationStoreConfigErrorV1> {
        if statement_timeout.is_zero()
            || statement_timeout > Duration::from_millis(MAX_STATEMENT_TIMEOUT_MILLIS)
            || !statement_timeout.subsec_nanos().is_multiple_of(1_000_000)
        {
            return Err(AuthoringConversationStoreConfigErrorV1::InvalidStatementTimeout);
        }
        if max_plaintext_bytes == 0 || max_plaintext_bytes > MAX_PLAINTEXT_BYTES {
            return Err(AuthoringConversationStoreConfigErrorV1::InvalidPlaintextBound);
        }
        Ok(Self {
            statement_timeout,
            max_plaintext_bytes,
        })
    }

    pub(super) fn statement_timeout(self) -> String {
        format!("{}ms", self.statement_timeout.as_millis())
    }
}

impl Default for PostgresAuthoringConversationStoreConfigV1 {
    fn default() -> Self {
        Self::new(
            Duration::from_millis(DEFAULT_STATEMENT_TIMEOUT_MILLIS),
            DEFAULT_MAX_PLAINTEXT_BYTES,
        )
        .expect("default authoring writer configuration is valid")
    }
}

#[derive(Clone)]
pub struct PostgresAuthoringConversationStoreV1<C> {
    pub(super) pool: PgPool,
    pub(super) cipher: C,
    pub(super) digest_keyring: ProductActionDigestKeyringV1,
    pub(super) config: PostgresAuthoringConversationStoreConfigV1,
}

impl<C> PostgresAuthoringConversationStoreV1<C> {
    pub fn new(pool: PgPool, cipher: C, digest_keyring: ProductActionDigestKeyringV1) -> Self {
        Self {
            pool,
            cipher,
            digest_keyring,
            config: PostgresAuthoringConversationStoreConfigV1::default(),
        }
    }

    pub fn with_config(
        pool: PgPool,
        cipher: C,
        digest_keyring: ProductActionDigestKeyringV1,
        config: PostgresAuthoringConversationStoreConfigV1,
    ) -> Self {
        Self {
            pool,
            cipher,
            digest_keyring,
            config,
        }
    }
}

impl<C> Debug for PostgresAuthoringConversationStoreV1<C> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PostgresAuthoringConversationStoreV1(<redacted>)")
    }
}

impl<C, E> AuthoringSessionLoadPort<E> for PostgresAuthoringConversationStoreV1<C>
where
    C: SnapshotEnvelopeCipher,
    E: FreshGuildAuthorityEvidence,
{
    async fn check_replay_or_head(
        &self,
        access: &AuthorizedConversationAccessV1<'_, E>,
    ) -> Result<AuthoringTurnCheckV1, AuthoringSessionLoadError> {
        let candidates = self.digest_candidates(access);
        let arrays = CandidateArraysV1::from_candidates(&candidates);
        let mut transaction = self.read_transaction().await?;
        let row = sqlx::query_as::<_, AuthoringWriterCheckRowV1>(CHECK_QUERY)
            .bind(access.scope().tenant_id().as_str())
            .bind(access.scope().installation_id().as_str())
            .bind(access.actor().principal_id().as_str())
            .bind(access.command().session_id().as_str())
            .bind(expected_generation_i64(
                access.command().expected_generation(),
            )?)
            .bind(arrays.request_digests)
            .bind(arrays.semantic_digests)
            .bind(arrays.key_ids)
            .bind(arrays.key_fingerprints)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        transaction.commit().await.map_err(map_database_error)?;
        match row.outcome_code.as_str() {
            "proceed" => {
                if row.matched_generation.is_some()
                    || row.safe_turn_projection.is_some()
                    || row.safe_turn_projection_digest.is_some()
                {
                    return Err(AuthoringSessionLoadError::InvalidState);
                }
                Ok(AuthoringTurnCheckV1::Proceed)
            }
            "exact_replay" => {
                let generation = parse_generation(row.matched_generation)?;
                let projection_bytes = row
                    .safe_turn_projection
                    .ok_or(AuthoringSessionLoadError::InvalidState)?;
                validate_projection_digest(
                    &projection_bytes,
                    row.safe_turn_projection_digest.as_deref(),
                )?;
                let stored = self
                    .load_replay_generation(access, generation, &projection_bytes)
                    .await?;
                Ok(AuthoringTurnCheckV1::ExactReplay(stored))
            }
            "idempotency_conflict" => Ok(AuthoringTurnCheckV1::IdempotencyConflict),
            "generation_conflict" => Ok(AuthoringTurnCheckV1::GenerationConflict {
                current_generation: optional_generation(row.current_generation)?,
            }),
            _ => Err(AuthoringSessionLoadError::InvalidState),
        }
    }

    async fn load_exact_generation(
        &self,
        access: &AuthorizedConversationAccessV1<'_, E>,
    ) -> Result<AuthoringSessionLoadV1, AuthoringSessionLoadError> {
        let expected = access.command().expected_generation();
        let row = self
            .load_row(access, expected_generation_i64(expected)?)
            .await?;
        match row.outcome_code.as_str() {
            "empty" if expected.get() == 0 => {
                let bindings = current_bindings(&row, access.evidence())?;
                ensure_generation_fields_absent(&row)?;
                AuthoringSessionLoadV1::from_storage(None, None, bindings)
            }
            "loaded" if expected.get() != 0 => {
                let loaded = self
                    .materialize_loaded_row(&row, access.evidence(), access.command().session_id())
                    .await?;
                ensure_generation_matches_current(&row)?;
                if loaded.generation.get() != expected.get() {
                    return Err(AuthoringSessionLoadError::InvalidState);
                }
                AuthoringSessionLoadV1::from_storage(
                    Some(loaded.generation),
                    Some(loaded.snapshot),
                    loaded.bindings,
                )
            }
            "not_found" | "generation_conflict" => Err(AuthoringSessionLoadError::InvalidState),
            _ => Err(AuthoringSessionLoadError::InvalidState),
        }
    }
}

impl<C, E> AuthoringSessionCommitPort<E> for PostgresAuthoringConversationStoreV1<C>
where
    C: SnapshotEnvelopeCipher + SnapshotEnvelopeEncryptionPort,
    E: FreshGuildAuthorityEvidence,
{
    async fn commit_authorized_generation(
        &self,
        request: AuthorizedAuthoringCommitV1<'_, E>,
    ) -> Result<AuthoringCommitOutcomeV1, AuthoringSessionLoadError> {
        let access = request.access();
        let candidates = self.digest_candidates(access);
        let active = candidates
            .first()
            .ok_or(AuthoringSessionLoadError::InvalidState)?;
        let projection_bytes = request
            .projection()
            .to_canonical_json()
            .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        let projection_digest = safe_projection_digest_v1(&projection_bytes);
        let binding_fingerprint = resource_binding_fingerprint_v2(request.resource_bindings());
        if &binding_fingerprint != request.binding_fingerprint() {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        let bindings = encode_resource_bindings(request.resource_bindings())
            .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        let plaintext = Zeroizing::new(
            serde_json::to_vec(request.snapshot())
                .map_err(|_| AuthoringSessionLoadError::InvalidState)?,
        );
        if plaintext.is_empty() || plaintext.len() > self.config.max_plaintext_bytes {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        let generation = successor_generation(access.command().expected_generation())?;
        let authority_revision = access.evidence().installation_authority_revision().get();
        let authenticated_data =
            build_writer_snapshot_authenticated_data_v1(WriterSnapshotAuthenticatedDataInputV1 {
                snapshot: SnapshotAuthenticatedDataInputV1 {
                    tenant_id: access.scope().tenant_id(),
                    installation_id: access.scope().installation_id(),
                    session_id: access.command().session_id(),
                    generation,
                    snapshot_schema_version: request.snapshot().schema_version,
                    binding_fingerprint: request.binding_fingerprint(),
                    encryption_key_id: self.cipher.active_encryption_key_id(),
                    encryption_suite: self.cipher.encryption_suite(),
                    encryption_suite_version: self.cipher.encryption_suite_version(),
                },
                installation_authority_revision: authority_revision,
                installation_authority_digest: access.evidence().installation_authority_digest(),
                writer_request_digest: active.request_digest(),
                writer_semantic_request_digest: active.semantic_digest(),
                writer_digest_key_id: active.key_id(),
                writer_digest_key_fingerprint: active.key_fingerprint(),
                safe_turn_projection_digest: &projection_digest,
            })
            .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        let envelope = self
            .cipher
            .encrypt(&plaintext, authenticated_data.as_bytes())
            .map_err(|_| AuthoringSessionLoadError::Unavailable)?;
        let arrays = CandidateArraysV1::from_candidates(&candidates);
        let stage = projection_stage(request.projection().state());
        let summary = serde_json::to_value(request.projection().draft())
            .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        let (candidate_revision, candidate_hash) = request
            .preview_ready_artifact()
            .map(|artifact| {
                Ok::<_, AuthoringSessionLoadError>((
                    Some(
                        i64::try_from(artifact.receipt().candidate_revision)
                            .map_err(|_| AuthoringSessionLoadError::InvalidState)?,
                    ),
                    Some(artifact.receipt().candidate_ruleset_hash.clone()),
                ))
            })
            .transpose()?
            .unwrap_or((None, None));
        let row = self
            .commit_row(
                &request,
                AuthoringWriterCommitInputV1 {
                    arrays,
                    active,
                    envelope: &envelope,
                    authenticated_metadata_digest: authenticated_data.digest_hex(),
                    bindings: Json(bindings),
                    authority_revision,
                    summary: Json(summary),
                    stage,
                    candidate_revision,
                    candidate_hash,
                    projection_bytes: projection_bytes.clone(),
                    projection_digest: projection_digest.clone(),
                },
            )
            .await?;
        match row.outcome_code.as_str() {
            "committed" => {
                let stored_generation = parse_generation(row.committed_generation)?;
                if stored_generation != generation {
                    return Err(AuthoringSessionLoadError::InvalidState);
                }
                validate_returned_projection(&row, &projection_bytes, &projection_digest)?;
                let identity = verified_identity(access);
                let stored = AuthoringStoredGenerationV1::from_storage(
                    identity,
                    stored_generation,
                    request.projection().clone(),
                    request.preview_ready_artifact(),
                )
                .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
                Ok(AuthoringCommitOutcomeV1::Created(stored))
            }
            "exact_replay" => {
                let generation = parse_generation(row.committed_generation)?;
                let bytes = row
                    .safe_turn_projection
                    .ok_or(AuthoringSessionLoadError::InvalidState)?;
                validate_projection_digest(&bytes, row.safe_turn_projection_digest.as_deref())?;
                let stored = self
                    .load_replay_generation(access, generation, &bytes)
                    .await?;
                Ok(AuthoringCommitOutcomeV1::ExactReplay(stored))
            }
            "idempotency_conflict" => Ok(AuthoringCommitOutcomeV1::IdempotencyConflict),
            "generation_conflict" => Ok(AuthoringCommitOutcomeV1::GenerationConflict {
                current_generation: optional_generation(row.current_generation)?,
            }),
            "authority_conflict" => Ok(AuthoringCommitOutcomeV1::AuthorityConflict),
            "binding_conflict" => Ok(AuthoringCommitOutcomeV1::BindingConflict),
            _ => Err(AuthoringSessionLoadError::InvalidState),
        }
    }
}

impl<C, E> AuthoringSessionReadPort<E> for PostgresAuthoringConversationStoreV1<C>
where
    C: SnapshotEnvelopeCipher,
    E: FreshGuildAuthorityEvidence,
{
    async fn read_authorized_session(
        &self,
        access: &AuthorizedConversationReadAccessV1<'_, E>,
    ) -> Result<AuthoringSessionObservationV1, AuthoringSessionObservationErrorV1> {
        let row = self
            .load_scoped_row(
                access.scope().tenant_id().as_str(),
                access.scope().installation_id().as_str(),
                access.actor().principal_id().as_str(),
                access.query().session_id().as_str(),
                0,
            )
            .await
            .map_err(map_observation_load_error)?;
        if row.outcome_code != "loaded" {
            return match row.outcome_code.as_str() {
                "not_found" | "empty" | "generation_conflict" => {
                    Err(AuthoringSessionObservationErrorV1::NotFound)
                }
                _ => Err(AuthoringSessionObservationErrorV1::InvalidState),
            };
        }
        let projection_bytes = row
            .safe_turn_projection
            .as_deref()
            .ok_or(AuthoringSessionObservationErrorV1::InvalidState)?;
        validate_projection_digest(projection_bytes, row.safe_turn_projection_digest.as_deref())
            .map_err(map_observation_load_error)?;
        let projection = SafeAuthoringTurnProjectionV1::from_canonical_json(projection_bytes)
            .map_err(|_| AuthoringSessionObservationErrorV1::InvalidState)?;
        let loaded = self
            .materialize_loaded_row(&row, access.evidence(), access.query().session_id())
            .await
            .map_err(map_observation_load_error)?;
        let artifact = if projection.state() == SafeAuthoringTurnStateV1::PreviewReady {
            Some(
                export_preview_artifact(
                    loaded.snapshot,
                    loaded.bindings,
                    &loaded.binding_fingerprint,
                )
                .map_err(map_observation_load_error)?,
            )
        } else {
            None
        };
        AuthoringSessionObservationV1::from_storage(
            access.query().session_id().clone(),
            loaded.generation,
            projection,
            artifact.as_ref(),
        )
        .map_err(|_| AuthoringSessionObservationErrorV1::InvalidState)
    }
}

impl<C> PostgresAuthoringConversationStoreV1<C>
where
    C: SnapshotEnvelopeCipher,
{
    fn digest_candidates<E: FreshGuildAuthorityEvidence>(
        &self,
        access: &AuthorizedConversationAccessV1<'_, E>,
    ) -> Vec<WriterDigestCandidateV1> {
        writer_digest_candidates_v1(
            &self.digest_keyring,
            WriterDigestInputV1 {
                tenant_id: access.scope().tenant_id(),
                installation_id: access.scope().installation_id(),
                principal_id: access.actor().principal_id(),
                session_id: access.command().session_id(),
                expected_generation: access.command().expected_generation(),
                idempotency_key: access.command().idempotency_key(),
                human_message: access.command().human_message(),
            },
        )
    }

    async fn read_transaction(
        &self,
    ) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, AuthoringSessionLoadError> {
        let mut transaction = self.pool.begin().await.map_err(map_database_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        set_transaction_timeouts(&mut transaction, self.config).await?;
        Ok(transaction)
    }

    async fn load_row<E: FreshGuildAuthorityEvidence>(
        &self,
        access: &AuthorizedConversationAccessV1<'_, E>,
        generation: i64,
    ) -> Result<AuthoringWriterLoadRowV1, AuthoringSessionLoadError> {
        self.load_scoped_row(
            access.scope().tenant_id().as_str(),
            access.scope().installation_id().as_str(),
            access.actor().principal_id().as_str(),
            access.command().session_id().as_str(),
            generation,
        )
        .await
    }

    async fn load_scoped_row(
        &self,
        tenant_id: &str,
        installation_id: &str,
        principal_id: &str,
        session_id: &str,
        generation: i64,
    ) -> Result<AuthoringWriterLoadRowV1, AuthoringSessionLoadError> {
        let mut transaction = self.read_transaction().await?;
        let row = sqlx::query_as::<_, AuthoringWriterLoadRowV1>(LOAD_QUERY)
            .bind(tenant_id)
            .bind(installation_id)
            .bind(principal_id)
            .bind(session_id)
            .bind(generation)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(row)
    }

    async fn load_replay_generation<E: FreshGuildAuthorityEvidence>(
        &self,
        access: &AuthorizedConversationAccessV1<'_, E>,
        generation: SessionGeneration,
        expected_projection_bytes: &[u8],
    ) -> Result<AuthoringStoredGenerationV1, AuthoringSessionLoadError> {
        let row = self
            .load_row(
                access,
                i64::try_from(generation.get())
                    .map_err(|_| AuthoringSessionLoadError::InvalidState)?,
            )
            .await?;
        if row.outcome_code != "loaded" {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        let projection_bytes = row
            .safe_turn_projection
            .as_deref()
            .ok_or(AuthoringSessionLoadError::InvalidState)?;
        if projection_bytes != expected_projection_bytes {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        validate_projection_digest(projection_bytes, row.safe_turn_projection_digest.as_deref())?;
        let projection = SafeAuthoringTurnProjectionV1::from_canonical_json(projection_bytes)
            .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        let loaded = self
            .materialize_loaded_row(&row, access.evidence(), access.command().session_id())
            .await?;
        if loaded.generation != generation {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        let artifact = if projection.state() == SafeAuthoringTurnStateV1::PreviewReady {
            Some(export_preview_artifact(
                loaded.snapshot,
                loaded.bindings,
                &loaded.binding_fingerprint,
            )?)
        } else {
            None
        };
        AuthoringStoredGenerationV1::from_storage(
            verified_identity(access),
            generation,
            projection,
            artifact.as_ref(),
        )
        .map_err(|_| AuthoringSessionLoadError::InvalidState)
    }

    async fn materialize_loaded_row<E: FreshGuildAuthorityEvidence>(
        &self,
        row: &AuthoringWriterLoadRowV1,
        evidence: &E,
        session_id: &authoring_promotion::AuthoringSessionId,
    ) -> Result<LoadedGenerationV1, AuthoringSessionLoadError> {
        let generation = parse_generation(row.head_generation)?;
        validate_current_authority(row, evidence)?;
        let bindings = historical_bindings(row)?;
        let binding_fingerprint = ResourceBindingFingerprint::parse(
            row.binding_fingerprint
                .as_deref()
                .ok_or(AuthoringSessionLoadError::InvalidState)?,
        )
        .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        if resource_binding_fingerprint_v2(&bindings) != binding_fingerprint {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        let snapshot_schema_version = row
            .snapshot_schema_version
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value != 0)
            .ok_or(AuthoringSessionLoadError::InvalidState)?;
        let encryption_suite_version = row
            .encryption_suite_version
            .and_then(|value| u16::try_from(value).ok())
            .ok_or(AuthoringSessionLoadError::InvalidState)?;
        let envelope = EncryptedSnapshotEnvelopeV1::from_persisted_parts(
            row.snapshot_ciphertext
                .clone()
                .ok_or(AuthoringSessionLoadError::InvalidState)?,
            row.snapshot_nonce
                .clone()
                .ok_or(AuthoringSessionLoadError::InvalidState)?,
            row.encryption_key_id
                .clone()
                .ok_or(AuthoringSessionLoadError::InvalidState)?,
            row.encryption_suite
                .clone()
                .ok_or(AuthoringSessionLoadError::InvalidState)?,
            encryption_suite_version,
        )
        .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        let tenant_id = evidence.tenant_id();
        let installation_id = evidence.installation_id();
        let snapshot_aad = SnapshotAuthenticatedDataInputV1 {
            tenant_id,
            installation_id,
            session_id,
            generation,
            snapshot_schema_version,
            binding_fingerprint: &binding_fingerprint,
            encryption_key_id: envelope.encryption_key_id(),
            encryption_suite: envelope.encryption_suite(),
            encryption_suite_version: envelope.encryption_suite_version(),
        };
        let authenticated_data = match trusted_metadata(row)? {
            Some(metadata) => build_writer_snapshot_authenticated_data_v1(
                WriterSnapshotAuthenticatedDataInputV1 {
                    snapshot: snapshot_aad,
                    installation_authority_revision: metadata.authority_revision,
                    installation_authority_digest: metadata.authority_digest,
                    writer_request_digest: metadata.writer_request_digest,
                    writer_semantic_request_digest: metadata.semantic_digest,
                    writer_digest_key_id: metadata.digest_key_id,
                    writer_digest_key_fingerprint: metadata.digest_key_fingerprint,
                    safe_turn_projection_digest: metadata.projection_digest,
                },
            ),
            None => build_snapshot_authenticated_data_v1(snapshot_aad),
        }
        .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        if row.authenticated_metadata_digest.as_deref() != Some(authenticated_data.digest_hex()) {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        let plaintext = self
            .cipher
            .decrypt(&envelope, authenticated_data.as_bytes())
            .await
            .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        if plaintext.is_empty() || plaintext.len() > self.config.max_plaintext_bytes {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        let snapshot = serde_json::from_slice::<SessionSnapshot>(&plaintext)
            .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        if snapshot.schema_version != snapshot_schema_version {
            return Err(AuthoringSessionLoadError::InvalidState);
        }
        snapshot
            .validate_durable_size()
            .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
        Ok(LoadedGenerationV1 {
            generation,
            snapshot,
            bindings,
            binding_fingerprint,
        })
    }

    async fn commit_row<E: FreshGuildAuthorityEvidence>(
        &self,
        request: &AuthorizedAuthoringCommitV1<'_, E>,
        input: AuthoringWriterCommitInputV1<'_>,
    ) -> Result<AuthoringWriterCommitRowV1, AuthoringSessionLoadError> {
        let access = request.access();
        let mut transaction = self.pool.begin().await.map_err(map_database_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        set_transaction_timeouts(&mut transaction, self.config).await?;
        let row = sqlx::query_as::<_, AuthoringWriterCommitRowV1>(COMMIT_QUERY)
            .bind(access.scope().tenant_id().as_str())
            .bind(access.scope().installation_id().as_str())
            .bind(access.actor().principal_id().as_str())
            .bind(access.command().session_id().as_str())
            .bind(expected_generation_i64(
                access.command().expected_generation(),
            )?)
            .bind(input.arrays.request_digests)
            .bind(input.arrays.semantic_digests)
            .bind(input.arrays.key_ids)
            .bind(input.arrays.key_fingerprints)
            .bind(input.active.request_digest())
            .bind(input.active.semantic_digest())
            .bind(input.active.key_id())
            .bind(input.active.key_fingerprint())
            .bind(i64::from(request.snapshot().schema_version))
            .bind(input.envelope.ciphertext())
            .bind(input.envelope.nonce())
            .bind(input.envelope.encryption_key_id())
            .bind(input.envelope.encryption_suite())
            .bind(
                i16::try_from(input.envelope.encryption_suite_version())
                    .map_err(|_| AuthoringSessionLoadError::InvalidState)?,
            )
            .bind(input.authenticated_metadata_digest)
            .bind(input.bindings)
            .bind(request.binding_fingerprint().as_str())
            .bind(
                i64::try_from(input.authority_revision)
                    .map_err(|_| AuthoringSessionLoadError::InvalidState)?,
            )
            .bind(access.evidence().installation_authority_digest())
            .bind(input.summary)
            .bind(input.stage)
            .bind(input.candidate_revision)
            .bind(input.candidate_hash)
            .bind(input.projection_bytes)
            .bind(input.projection_digest)
            .bind(HARNESS_CONTRACT_REVISION_V1)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(row)
    }
}

struct AuthoringWriterCommitInputV1<'a> {
    arrays: CandidateArraysV1,
    active: &'a WriterDigestCandidateV1,
    envelope: &'a EncryptedSnapshotEnvelopeV1,
    authenticated_metadata_digest: &'a str,
    bindings: Json<Value>,
    authority_revision: u64,
    summary: Json<Value>,
    stage: &'static str,
    candidate_revision: Option<i64>,
    candidate_hash: Option<String>,
    projection_bytes: Vec<u8>,
    projection_digest: String,
}

struct CandidateArraysV1 {
    request_digests: Vec<String>,
    semantic_digests: Vec<String>,
    key_ids: Vec<String>,
    key_fingerprints: Vec<String>,
}

impl CandidateArraysV1 {
    fn from_candidates(candidates: &[WriterDigestCandidateV1]) -> Self {
        Self {
            request_digests: candidates
                .iter()
                .map(|candidate| candidate.request_digest().to_string())
                .collect(),
            semantic_digests: candidates
                .iter()
                .map(|candidate| candidate.semantic_digest().to_string())
                .collect(),
            key_ids: candidates
                .iter()
                .map(|candidate| candidate.key_id().to_string())
                .collect(),
            key_fingerprints: candidates
                .iter()
                .map(|candidate| candidate.key_fingerprint().to_string())
                .collect(),
        }
    }
}

struct LoadedGenerationV1 {
    generation: SessionGeneration,
    snapshot: SessionSnapshot,
    bindings: ResourceBindingMap,
    binding_fingerprint: ResourceBindingFingerprint,
}

struct TrustedMetadataV1<'a> {
    authority_revision: u64,
    authority_digest: &'a str,
    writer_request_digest: &'a str,
    semantic_digest: &'a str,
    digest_key_id: &'a str,
    digest_key_fingerprint: &'a str,
    projection_digest: &'a str,
}

fn trusted_metadata(
    row: &AuthoringWriterLoadRowV1,
) -> Result<Option<TrustedMetadataV1<'_>>, AuthoringSessionLoadError> {
    let fields_present = [
        row.writer_semantic_request_digest.is_some(),
        row.writer_digest_key_id.is_some(),
        row.writer_digest_key_fingerprint.is_some(),
        row.safe_turn_projection.is_some(),
        row.safe_turn_projection_digest.is_some(),
    ];
    if fields_present.iter().all(|present| !present) {
        return Ok(None);
    }
    if !fields_present.iter().all(|present| *present) {
        return Err(AuthoringSessionLoadError::InvalidState);
    }
    let authority_revision = row
        .installation_authority_revision
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value != 0)
        .ok_or(AuthoringSessionLoadError::InvalidState)?;
    Ok(Some(TrustedMetadataV1 {
        authority_revision,
        authority_digest: row
            .authority_payload_digest
            .as_deref()
            .ok_or(AuthoringSessionLoadError::InvalidState)?,
        writer_request_digest: row
            .writer_request_digest
            .as_deref()
            .ok_or(AuthoringSessionLoadError::InvalidState)?,
        semantic_digest: row
            .writer_semantic_request_digest
            .as_deref()
            .ok_or(AuthoringSessionLoadError::InvalidState)?,
        digest_key_id: row
            .writer_digest_key_id
            .as_deref()
            .ok_or(AuthoringSessionLoadError::InvalidState)?,
        digest_key_fingerprint: row
            .writer_digest_key_fingerprint
            .as_deref()
            .ok_or(AuthoringSessionLoadError::InvalidState)?,
        projection_digest: row
            .safe_turn_projection_digest
            .as_deref()
            .ok_or(AuthoringSessionLoadError::InvalidState)?,
    }))
}

fn validate_current_authority<E: FreshGuildAuthorityEvidence>(
    row: &AuthoringWriterLoadRowV1,
    evidence: &E,
) -> Result<(), AuthoringSessionLoadError> {
    let revision = row
        .current_authority_revision
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value != 0)
        .ok_or(AuthoringSessionLoadError::InvalidState)?;
    if revision != evidence.installation_authority_revision().get()
        || row.current_authority_payload_digest.as_deref()
            != Some(evidence.installation_authority_digest())
    {
        return Err(AuthoringSessionLoadError::InvalidState);
    }
    Ok(())
}

fn current_bindings<E: FreshGuildAuthorityEvidence>(
    row: &AuthoringWriterLoadRowV1,
    evidence: &E,
) -> Result<ResourceBindingMap, AuthoringSessionLoadError> {
    validate_current_authority(row, evidence)?;
    let bindings = decode_resource_bindings(
        row.current_resource_bindings
            .as_ref()
            .ok_or(AuthoringSessionLoadError::InvalidState)?
            .0
            .clone(),
    )
    .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
    let fingerprint = resource_binding_fingerprint_v2(&bindings);
    if row.current_binding_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(AuthoringSessionLoadError::InvalidState);
    }
    Ok(bindings)
}

fn historical_bindings(
    row: &AuthoringWriterLoadRowV1,
) -> Result<ResourceBindingMap, AuthoringSessionLoadError> {
    let bindings = decode_resource_bindings(
        row.resource_bindings
            .as_ref()
            .ok_or(AuthoringSessionLoadError::InvalidState)?
            .0
            .clone(),
    )
    .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
    let fingerprint = resource_binding_fingerprint_v2(&bindings);
    if row.binding_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(AuthoringSessionLoadError::InvalidState);
    }
    Ok(bindings)
}

fn ensure_generation_matches_current(
    row: &AuthoringWriterLoadRowV1,
) -> Result<(), AuthoringSessionLoadError> {
    if row.installation_authority_revision != row.current_authority_revision
        || row.authority_payload_digest != row.current_authority_payload_digest
        || row.resource_bindings != row.current_resource_bindings
        || row.binding_fingerprint != row.current_binding_fingerprint
    {
        return Err(AuthoringSessionLoadError::InvalidState);
    }
    Ok(())
}

fn ensure_generation_fields_absent(
    row: &AuthoringWriterLoadRowV1,
) -> Result<(), AuthoringSessionLoadError> {
    if row.head_generation.is_some()
        || row.snapshot_schema_version.is_some()
        || row.snapshot_ciphertext.is_some()
        || row.snapshot_nonce.is_some()
        || row.encryption_key_id.is_some()
        || row.encryption_suite.is_some()
        || row.encryption_suite_version.is_some()
        || row.authenticated_metadata_digest.is_some()
        || row.resource_bindings.is_some()
        || row.binding_fingerprint.is_some()
        || row.installation_authority_revision.is_some()
        || row.authority_payload_digest.is_some()
        || row.writer_request_digest.is_some()
        || row.writer_semantic_request_digest.is_some()
        || row.writer_digest_key_id.is_some()
        || row.writer_digest_key_fingerprint.is_some()
        || row.safe_turn_projection.is_some()
        || row.safe_turn_projection_digest.is_some()
        || row.stage.is_some()
        || row.candidate_revision.is_some()
        || row.candidate_hash.is_some()
        || row.harness_contract_revision.is_some()
    {
        return Err(AuthoringSessionLoadError::InvalidState);
    }
    Ok(())
}

fn projection_stage(state: SafeAuthoringTurnStateV1) -> &'static str {
    match state {
        SafeAuthoringTurnStateV1::NeedsInput => "needs_input",
        SafeAuthoringTurnStateV1::Discussion => "discussion",
        SafeAuthoringTurnStateV1::CapabilityGap => "capability_gap",
        SafeAuthoringTurnStateV1::PreviewReady => "preview_ready",
        SafeAuthoringTurnStateV1::Unsupported | SafeAuthoringTurnStateV1::Rejected => {
            "invalid_non_durable"
        }
    }
}

fn validate_projection_digest(
    bytes: &[u8],
    digest: Option<&str>,
) -> Result<(), AuthoringSessionLoadError> {
    if bytes.is_empty()
        || digest != Some(safe_projection_digest_v1(bytes).as_str())
        || SafeAuthoringTurnProjectionV1::from_canonical_json(bytes).is_err()
    {
        return Err(AuthoringSessionLoadError::InvalidState);
    }
    Ok(())
}

fn validate_returned_projection(
    row: &AuthoringWriterCommitRowV1,
    expected_bytes: &[u8],
    expected_digest: &str,
) -> Result<(), AuthoringSessionLoadError> {
    if row.safe_turn_projection.as_deref() != Some(expected_bytes)
        || row.safe_turn_projection_digest.as_deref() != Some(expected_digest)
    {
        return Err(AuthoringSessionLoadError::InvalidState);
    }
    Ok(())
}

fn verified_identity<E: FreshGuildAuthorityEvidence>(
    access: &AuthorizedConversationAccessV1<'_, E>,
) -> AuthoringStoredRequestIdentityV1 {
    AuthoringStoredRequestIdentityV1::from_verified_storage_match(
        access.scope().clone(),
        access.actor().principal_id().clone(),
        access.command().session_id().clone(),
        access.command().expected_generation(),
        access.command().idempotency_key().clone(),
        access.command().human_message().clone(),
    )
}

fn export_preview_artifact(
    snapshot: SessionSnapshot,
    bindings: ResourceBindingMap,
    expected_fingerprint: &ResourceBindingFingerprint,
) -> Result<PreviewReadyArtifactV1, AuthoringSessionLoadError> {
    let restored = DesignSession::restore_intent_recipe(
        NoLlmClient,
        SessionConfig::default(),
        snapshot,
        bindings,
    )
    .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
    let artifact = restored
        .export_preview_ready_artifact()
        .map_err(|_| AuthoringSessionLoadError::InvalidState)?;
    if artifact.context_fingerprint() != expected_fingerprint {
        return Err(AuthoringSessionLoadError::InvalidState);
    }
    Ok(artifact)
}

async fn set_transaction_timeouts(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    config: PostgresAuthoringConversationStoreConfigV1,
) -> Result<(), AuthoringSessionLoadError> {
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, true), \
         pg_catalog.set_config('lock_timeout', $1, true), \
         pg_catalog.set_config('idle_in_transaction_session_timeout', $1, true)",
    )
    .bind(config.statement_timeout())
    .execute(&mut **transaction)
    .await
    .map_err(map_database_error)?;
    Ok(())
}

fn expected_generation_i64(
    generation: AuthoringExpectedGenerationV1,
) -> Result<i64, AuthoringSessionLoadError> {
    i64::try_from(generation.get()).map_err(|_| AuthoringSessionLoadError::InvalidState)
}

fn successor_generation(
    generation: AuthoringExpectedGenerationV1,
) -> Result<SessionGeneration, AuthoringSessionLoadError> {
    generation
        .get()
        .checked_add(1)
        .and_then(|value| SessionGeneration::new(value).ok())
        .ok_or(AuthoringSessionLoadError::InvalidState)
}

fn parse_generation(value: Option<i64>) -> Result<SessionGeneration, AuthoringSessionLoadError> {
    value
        .and_then(|value| u64::try_from(value).ok())
        .and_then(|value| SessionGeneration::new(value).ok())
        .ok_or(AuthoringSessionLoadError::InvalidState)
}

fn optional_generation(
    value: Option<i64>,
) -> Result<Option<SessionGeneration>, AuthoringSessionLoadError> {
    value.map(|value| parse_generation(Some(value))).transpose()
}

fn map_database_error(error: sqlx::Error) -> AuthoringSessionLoadError {
    match ProductDatabaseFailureV1::classify(&error) {
        ProductDatabaseFailureV1::Timeout => AuthoringSessionLoadError::Timeout,
        ProductDatabaseFailureV1::Retryable => AuthoringSessionLoadError::Retryable,
        ProductDatabaseFailureV1::Unavailable => AuthoringSessionLoadError::Unavailable,
    }
}

fn map_observation_load_error(
    error: AuthoringSessionLoadError,
) -> AuthoringSessionObservationErrorV1 {
    match error {
        AuthoringSessionLoadError::Timeout => AuthoringSessionObservationErrorV1::Timeout,
        AuthoringSessionLoadError::Retryable => AuthoringSessionObservationErrorV1::Retryable,
        AuthoringSessionLoadError::Unavailable => AuthoringSessionObservationErrorV1::Unavailable,
        AuthoringSessionLoadError::InvalidState => AuthoringSessionObservationErrorV1::InvalidState,
    }
}

#[derive(Clone, Copy)]
struct NoLlmClient;

impl LlmClient for NoLlmClient {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        Err(LlmError::Client(
            "model access is unavailable while restoring a durable snapshot".to_string(),
        ))
    }
}
