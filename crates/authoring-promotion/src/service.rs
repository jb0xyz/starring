use automation_ruleset::{content_hash, RuleSetStoreError, CURRENT_RULESET_SCHEMA_VERSION};
use automation_ruleset_activation::{
    ActivationRequestId, ActivationRequestState, ActivationTarget,
};
use chrono::Duration;
use design_harness::{IntentRequestedOutcome, PreviewReadyArtifactV1};

use crate::digest::{
    activation_request_hash_v1, idempotency_scope_digest_v1, promotion_request_digest_v1,
    DigestError,
};
use crate::id::{AuthoringHash, PromotionIdError};
use crate::{
    AuthenticatedPromotionContext, AuthoringEvidenceV1, AuthoringPreviewSummaryV1,
    AuthoringPreviewV1, CreatePromotionOutcomeV1, EnsurePendingActivationV1, IdempotencyKey,
    NewPromotionV1, PendingActivationLinkV1, PendingActivationPort, PendingActivationPortError,
    PendingActivationReceiptV1, PromotionClock, PromotionId, PromotionIntentV1, PromotionRecordV1,
    PromotionStageV1, PromotionStore, PromotionStoreError, PublicationDispositionV1,
    PublicationPortOutcomeV1, PublicationRecordV1, PublishAuthoringRuleSetV1,
    RuleSetPublicationPort,
};

pub struct StartPromotionV1 {
    pub idempotency_key: IdempotencyKey,
    pub context: AuthenticatedPromotionContext,
    pub artifact: PreviewReadyArtifactV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResumePromotionOutcomeV1 {
    Advanced(PromotionRecordV1),
    AlreadyActivationPending(PromotionRecordV1),
    TerminalExpired(PromotionRecordV1),
}

pub struct PromotionService<'a, S, P, A, C> {
    store: &'a S,
    publication: &'a P,
    activation: &'a A,
    clock: C,
}

impl<'a, S, P, A, C> PromotionService<'a, S, P, A, C> {
    pub fn new(store: &'a S, publication: &'a P, activation: &'a A, clock: C) -> Self {
        Self {
            store,
            publication,
            activation,
            clock,
        }
    }
}

impl<S, P, A, C> PromotionService<'_, S, P, A, C>
where
    S: PromotionStore,
    P: RuleSetPublicationPort,
    A: PendingActivationPort,
    C: PromotionClock,
{
    pub async fn start(
        &self,
        input: StartPromotionV1,
    ) -> Result<CreatePromotionOutcomeV1, PromotionError> {
        let promotion = prepare_promotion(input, self.clock.now())?;
        self.store
            .create_prepared(promotion)
            .await
            .map_err(PromotionError::Store)
    }

    pub async fn resume_to_activation_pending(
        &self,
        promotion_id: &PromotionId,
    ) -> Result<ResumePromotionOutcomeV1, PromotionError> {
        for _ in 0..4 {
            let record = self
                .store
                .get(promotion_id)
                .await?
                .ok_or(PromotionError::NotFound)?;
            record
                .validate()
                .map_err(PromotionStoreError::InvalidRecord)?;
            match &record.stage {
                PromotionStageV1::Prepared => {
                    let publication = self.publish(&record).await?;
                    match self
                        .store
                        .mark_published(&record.id, record.revision, publication, self.clock.now())
                        .await
                    {
                        Ok(_) | Err(PromotionStoreError::RevisionConflict { .. }) => continue,
                        Err(error) => return Err(PromotionError::Store(error)),
                    }
                }
                PromotionStageV1::Published { publication } => {
                    let request = self.request_pending(&record, publication).await?;
                    let transition = match request {
                        PendingRequestOutcomeV1::Pending(activation) => {
                            let updated_at = self.clock.now().max(activation.created_at);
                            self.store
                                .mark_activation_pending(
                                    &record.id,
                                    record.revision,
                                    activation,
                                    updated_at,
                                )
                                .await
                        }
                        PendingRequestOutcomeV1::Expired(activation) => {
                            let updated_at = self.clock.now().max(activation.expires_at);
                            self.store
                                .mark_expired(&record.id, record.revision, activation, updated_at)
                                .await
                        }
                    };
                    match transition {
                        Ok(record) => {
                            return Ok(match &record.stage {
                                PromotionStageV1::Expired { .. } => {
                                    ResumePromotionOutcomeV1::TerminalExpired(record)
                                }
                                _ => ResumePromotionOutcomeV1::Advanced(record),
                            });
                        }
                        Err(PromotionStoreError::RevisionConflict { .. }) => continue,
                        Err(error) => return Err(PromotionError::Store(error)),
                    }
                }
                PromotionStageV1::ActivationPending { .. } => {
                    return Ok(ResumePromotionOutcomeV1::AlreadyActivationPending(record));
                }
                PromotionStageV1::Expired { .. } => {
                    return Ok(ResumePromotionOutcomeV1::TerminalExpired(record));
                }
            }
        }
        Err(PromotionError::ConcurrentTransitionLimit)
    }

    async fn publish(
        &self,
        record: &PromotionRecordV1,
    ) -> Result<PublicationRecordV1, PromotionError> {
        let authority = &record.intent.authority;
        let outcome = self
            .publication
            .publish_ruleset(PublishAuthoringRuleSetV1 {
                guild_id: authority.guild_id,
                ruleset_key: authority.ruleset_key.clone(),
                definition: record.intent.definition.clone(),
                created_by: authority.requester,
            })
            .await
            .map_err(PromotionError::RuleSet)?;
        let (disposition, artifact) = match outcome {
            PublicationPortOutcomeV1::Created(artifact) => {
                (PublicationDispositionV1::Created, artifact)
            }
            PublicationPortOutcomeV1::Reused(artifact) => {
                (PublicationDispositionV1::Reused, artifact)
            }
        };
        if artifact.guild_id != authority.guild_id
            || artifact.ruleset_key != authority.ruleset_key
            || artifact.definition != record.intent.definition
            || artifact.schema_version != record.intent.registry_schema_version
            || artifact.content_hash != record.intent.expected_registry_content_hash
            || (disposition == PublicationDispositionV1::Created
                && artifact.created_by != authority.requester)
        {
            return Err(PromotionError::PublicationMismatch);
        }
        Ok(PublicationRecordV1 {
            version: artifact.version,
            schema_version: artifact.schema_version,
            content_hash: artifact.content_hash,
            disposition,
            registry_created_by: artifact.created_by,
        })
    }

    async fn request_pending(
        &self,
        record: &PromotionRecordV1,
        publication: &PublicationRecordV1,
    ) -> Result<PendingRequestOutcomeV1, PromotionError> {
        let request_hash =
            activation_request_hash_v1(&record.id, &record.request_digest, publication)?;
        let request_id = ActivationRequestId::parse(request_hash.as_str())
            .map_err(|_| PromotionError::ActivationIdentity)?;
        let authority = &record.intent.authority;
        let target = ActivationTarget {
            guild_id: authority.guild_id,
            ruleset_key: authority.ruleset_key.clone(),
            version: publication.version,
            content_hash: publication.content_hash,
        };
        let ttl_seconds = i64::try_from(authority.policy.ttl_seconds.get())
            .map_err(|_| PromotionError::InvalidPolicy)?;
        let ttl = Duration::try_seconds(ttl_seconds).ok_or(PromotionError::InvalidPolicy)?;
        let receipt = self
            .activation
            .ensure_pending_activation(EnsurePendingActivationV1 {
                id: request_id.clone(),
                target,
                requester: authority.requester,
                required_approvals: authority.policy.required_approvals,
                ttl,
            })
            .await?;
        let state = validate_activation_receipt(record, publication, &request_id, &receipt, ttl)?;
        let link = PendingActivationLinkV1 {
            request_id: receipt.request.id,
            target: receipt.request.target,
            requester: receipt.request.requester,
            required_approvals: authority.policy.required_approvals,
            observed_active: receipt.request.observed_active,
            created_at: receipt.request.created_at,
            expires_at: receipt.request.expires_at,
            disposition: receipt.disposition,
            request_state_at_link: state,
        };
        Ok(match state {
            ActivationRequestState::Pending => PendingRequestOutcomeV1::Pending(link),
            ActivationRequestState::Expired => PendingRequestOutcomeV1::Expired(link),
            _ => return Err(PromotionError::PendingActivationMismatch),
        })
    }
}

fn prepare_promotion(
    input: StartPromotionV1,
    created_at: chrono::DateTime<chrono::Utc>,
) -> Result<NewPromotionV1, PromotionError> {
    if input.artifact.contract().requested_outcome != IntentRequestedOutcome::ValidatedPreview {
        return Err(PromotionError::ValidatedPreviewRequired);
    }
    if input.context.principal_id != input.context.session_owner_id {
        return Err(PromotionError::SessionOwnerMismatch);
    }
    let ttl_seconds = i64::try_from(input.context.policy.ttl_seconds.get())
        .map_err(|_| PromotionError::InvalidPolicy)?;
    let ttl = Duration::try_seconds(ttl_seconds).ok_or(PromotionError::InvalidPolicy)?;
    created_at
        .checked_add_signed(ttl)
        .ok_or(PromotionError::InvalidPolicy)?;
    let contract = input.artifact.contract();
    let receipt = input.artifact.receipt();
    let evidence = AuthoringEvidenceV1 {
        artifact_version: contract.artifact_version,
        intent_protocol_version: contract.intent_protocol_version,
        identity_revision: contract.identity_revision,
        extractor_revision: contract.extractor_revision,
        normalizer_revision: contract.normalizer_revision,
        compiler_revision: contract.compiler_revision,
        simulator_revision: contract.simulator_revision,
        recipe_id: contract.recipe_id.clone(),
        recipe_version: contract.recipe_version,
        recipe_descriptor_digest: parse_hash(
            "recipe_descriptor_digest",
            &contract.recipe_descriptor_digest,
        )?,
        recipe_registry_digest: parse_hash(
            "recipe_registry_digest",
            &contract.recipe_registry_digest,
        )?,
        requested_outcome: contract.requested_outcome,
        intent_revision: receipt.intent_revision,
        candidate_revision: receipt.candidate_revision,
        request_evidence_hash: parse_hash("request_evidence_hash", &receipt.request_evidence_hash)?,
        request_evidence_entries: u64::try_from(receipt.request_evidence_entries).map_err(
            |_| PromotionError::ArtifactCountOverflow {
                field: "request_evidence_entries",
            },
        )?,
        compiler_input_hash: parse_hash("compiler_input_hash", &receipt.compiler_input_hash)?,
        semantic_intent_hash: parse_hash("semantic_intent_hash", &receipt.semantic_intent_hash)?,
        compiled_plan_hash: parse_hash("compiled_plan_hash", &receipt.compiled_plan_hash)?,
        candidate_ruleset_hash: parse_hash(
            "candidate_ruleset_hash",
            &receipt.candidate_ruleset_hash,
        )?,
        candidate_draft_hash: parse_hash("candidate_draft_hash", &receipt.candidate_draft_hash)?,
        compiled_operations: u64::try_from(receipt.compiled_operations).map_err(|_| {
            PromotionError::ArtifactCountOverflow {
                field: "compiled_operations",
            }
        })?,
        context_fingerprint: input.artifact.context_fingerprint().clone(),
        external_channel_bindings: input.artifact.external_channel_bindings().to_vec(),
        stage_binding_digest: parse_hash(
            "stage_binding_digest",
            input.artifact.stage_binding_digest(),
        )?,
    };
    let definition = input.artifact.ruleset().clone();
    let expected_registry_content_hash =
        content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).map_err(PromotionError::Hash)?;
    let idempotency_scope_digest =
        idempotency_scope_digest_v1(&input.context, &input.idempotency_key)?;
    let intent = PromotionIntentV1 {
        idempotency_scope_digest: idempotency_scope_digest.clone(),
        authority: input.context,
        evidence,
        definition,
        preview: AuthoringPreviewV1 {
            revision: input.artifact.preview().revision,
            summary: AuthoringPreviewSummaryV1 {
                panels: preview_count("panels", input.artifact.preview().draft.panels)?,
                modals: preview_count("modals", input.artifact.preview().draft.modals)?,
                rules: preview_count("rules", input.artifact.preview().draft.rules)?,
                actions: preview_count("actions", input.artifact.preview().draft.actions)?,
                unresolved_references: input.artifact.preview().draft.unresolved_references.clone(),
            },
        },
        registry_schema_version: CURRENT_RULESET_SCHEMA_VERSION,
        expected_registry_content_hash,
    };
    let request_digest = promotion_request_digest_v1(&intent)?;
    Ok(NewPromotionV1 {
        id: PromotionId::from_scope_digest(&idempotency_scope_digest),
        request_digest,
        intent,
        created_at,
    })
}

fn parse_hash(field: &'static str, value: &str) -> Result<AuthoringHash, PromotionError> {
    AuthoringHash::parse(value)
        .map_err(|source| PromotionError::InvalidArtifactHash { field, source })
}

fn preview_count(field: &'static str, value: usize) -> Result<u64, PromotionError> {
    u64::try_from(value).map_err(|_| PromotionError::ArtifactCountOverflow { field })
}

fn validate_activation_receipt(
    record: &PromotionRecordV1,
    publication: &PublicationRecordV1,
    expected_request_id: &ActivationRequestId,
    receipt: &PendingActivationReceiptV1,
    ttl: Duration,
) -> Result<ActivationRequestState, PromotionError> {
    let request = &receipt.request;
    let authority = &record.intent.authority;
    let expected_expiry = request
        .created_at
        .checked_add_signed(ttl)
        .ok_or(PromotionError::InvalidPolicy)?;
    if &request.id != expected_request_id
        || request.target.guild_id != authority.guild_id
        || request.target.ruleset_key != authority.ruleset_key
        || request.target.version != publication.version
        || request.target.content_hash != publication.content_hash
        || request.requester != authority.requester
        || request.required_approvals != authority.policy.required_approvals.get()
        || !matches!(
            request.state,
            ActivationRequestState::Pending | ActivationRequestState::Expired
        )
        || !request.approvals.is_empty()
        || request.rejection.is_some()
        || request.apply_attempt_id.is_some()
        || request.apply_attempt_no != 0
        || request.apply_lease_until.is_some()
        || request.last_apply_error.is_some()
        || request.completion.is_some()
        || request.created_at < record.created_at
        || request.expires_at != expected_expiry
    {
        return Err(PromotionError::PendingActivationMismatch);
    }
    if request.state == ActivationRequestState::Expired
        && receipt.disposition != crate::PendingActivationDispositionV1::Reused
    {
        return Err(PromotionError::PendingActivationMismatch);
    }
    Ok(request.state)
}

enum PendingRequestOutcomeV1 {
    Pending(PendingActivationLinkV1),
    Expired(PendingActivationLinkV1),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromotionError {
    #[error("only a validated-preview artifact can be promoted")]
    ValidatedPreviewRequired,
    #[error("authenticated principal does not own the authoring session")]
    SessionOwnerMismatch,
    #[error("authoring artifact hash {field} is invalid: {source}")]
    InvalidArtifactHash {
        field: &'static str,
        source: PromotionIdError,
    },
    #[error("authoring artifact count {field} exceeds the durable u64 range")]
    ArtifactCountOverflow { field: &'static str },
    #[error("approval policy is invalid")]
    InvalidPolicy,
    #[error("promotion was not found")]
    NotFound,
    #[error("published RuleSet does not match the exact prepared artifact")]
    PublicationMismatch,
    #[error("pending activation request does not match the exact publication and policy")]
    PendingActivationMismatch,
    #[error("activation request identity could not be constructed")]
    ActivationIdentity,
    #[error("concurrent promotion transition retry limit exceeded")]
    ConcurrentTransitionLimit,
    #[error("RuleSet hashing failed: {0:?}")]
    Hash(automation_ruleset::RuleSetHashError),
    #[error("RuleSet publication failed: {0:?}")]
    RuleSet(RuleSetStoreError),
    #[error(transparent)]
    PendingActivation(#[from] PendingActivationPortError),
    #[error(transparent)]
    Store(#[from] PromotionStoreError),
    #[error("promotion digest failed: {0}")]
    Digest(String),
}

impl From<DigestError> for PromotionError {
    fn from(error: DigestError) -> Self {
        Self::Digest(error.to_string())
    }
}
