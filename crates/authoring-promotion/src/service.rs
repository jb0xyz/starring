use automation_ruleset::{content_hash, RuleSetStoreError, CURRENT_RULESET_SCHEMA_VERSION};
use automation_ruleset_activation::{
    approval_policy_digest_v1, product_approval_context_digest_v1, ActivationApprovalContextV1,
    ActivationDigest, ActivationLinkStateV1, ActivationPromotionId, ActivationRequest,
    ActivationRequestId, ActivationRequestState, ActivationTarget, ApprovalPolicyBindingV1,
    CreateProductActivationRequest, LinkProductActivation, ProductApprovalContextV1,
};
use chrono::Duration;
use design_harness::{IntentRequestedOutcome, PreviewReadyArtifactV1};

use crate::digest::{
    activation_request_hash_v1, approval_payload_digest_v1, idempotency_scope_digest_v1,
    promotion_request_digest_v1, DigestError,
};
use crate::id::{AuthoringHash, PromotionIdError};
use crate::{
    AuthenticatedPromotionContext, AuthoringEvidenceV1, AuthoringPreviewSummaryV1,
    AuthoringPreviewV1, CreatePromotionOutcomeV1, EnsurePendingActivationV1, IdempotencyKey,
    LinkPendingActivationV1, NewPromotionV1, PendingActivationLinkV1, PendingActivationPort,
    PendingActivationPortError, PendingActivationReceiptV1, PromotionClock, PromotionId,
    PromotionIntentV1, PromotionRecordV1, PromotionStageV1, PromotionStore, PromotionStoreError,
    PublicationDispositionV1, PublicationPortOutcomeV1, PublicationRecordV1,
    PublishAuthoringRuleSetV1, ResolveProductApprovalContextV1, RuleSetPublicationPort,
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
        let mut advanced = false;
        for _ in 0..8 {
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
                        Ok(_) => {
                            advanced = true;
                            continue;
                        }
                        Err(PromotionStoreError::RevisionConflict { .. }) => continue,
                        Err(error) => return Err(PromotionError::Store(error)),
                    }
                }
                PromotionStageV1::Published { publication } => {
                    let request = self.request_pending(&record, publication).await?;
                    let transition = match request {
                        PendingRequestOutcomeV1::RefreshJournal => continue,
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
                            advanced = true;
                            if matches!(record.stage, PromotionStageV1::ActivationPending { .. }) {
                                continue;
                            }
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
                PromotionStageV1::ActivationPending {
                    publication,
                    activation,
                } => {
                    let linked = self.link_pending(&record, publication, activation).await?;
                    if linked.state == ActivationRequestState::Expired {
                        let expired = expired_link(activation, &linked);
                        let updated_at = self.clock.now().max(expired.expires_at);
                        match self
                            .store
                            .mark_expired(&record.id, record.revision, expired, updated_at)
                            .await
                        {
                            Ok(record) => {
                                return Ok(ResumePromotionOutcomeV1::TerminalExpired(record));
                            }
                            Err(PromotionStoreError::RevisionConflict { .. }) => continue,
                            Err(error) => return Err(PromotionError::Store(error)),
                        }
                    }
                    return Ok(if advanced {
                        ResumePromotionOutcomeV1::Advanced(record)
                    } else {
                        ResumePromotionOutcomeV1::AlreadyActivationPending(record)
                    });
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
        let resolved = self
            .activation
            .resolve_product_approval_context(ResolveProductApprovalContextV1 {
                tenant_id: authority.tenant_id.clone(),
                installation_id: authority.installation_id.clone(),
                target: target.clone(),
                binding_revision: authority.binding_revision,
                context_fingerprint: record.intent.evidence.context_fingerprint.clone(),
                required_channel_bindings: record.intent.evidence.external_channel_bindings.clone(),
            })
            .await?;
        let policy_revision = std::num::NonZeroU64::new(authority.policy.revision.get())
            .ok_or(PromotionError::InvalidPolicy)?;
        let policy = ApprovalPolicyBindingV1 {
            revision: policy_revision,
            required_approvals: authority.policy.required_approvals,
            ttl_seconds: authority.policy.ttl_seconds,
            digest: approval_policy_digest_v1(
                policy_revision,
                authority.policy.required_approvals,
                authority.policy.ttl_seconds,
            ),
        };
        let mut approval_context = ProductApprovalContextV1 {
            promotion_id: ActivationPromotionId::parse(record.id.as_str())
                .map_err(|_| PromotionError::ActivationIdentity)?,
            promotion_request_digest: ActivationDigest::parse(record.request_digest.as_str())
                .map_err(|_| PromotionError::ActivationIdentity)?,
            approval_payload_digest: ActivationDigest::parse(&"0".repeat(64))
                .map_err(|_| PromotionError::ActivationIdentity)?,
            approval_context_digest: ActivationDigest::parse(&"0".repeat(64))
                .map_err(|_| PromotionError::ActivationIdentity)?,
            binding: resolved.binding,
            baseline: resolved.baseline,
            policy,
        };
        let payload = crate::model::product_approval_payload_from_parts(
            &record.id,
            &record.request_digest,
            &record.intent,
            publication,
            &approval_context,
        );
        approval_context.approval_payload_digest = approval_payload_digest_v1(&payload)?;
        approval_context.approval_context_digest = product_approval_context_digest_v1(
            &request_id,
            &target,
            authority.requester,
            &approval_context,
        );
        let receipt = self
            .activation
            .ensure_pending_activation(EnsurePendingActivationV1 {
                create: CreateProductActivationRequest {
                    id: request_id.clone(),
                    target,
                    requester: authority.requester,
                    context: approval_context.clone(),
                },
            })
            .await?;
        let observation = validate_activation_receipt(
            record,
            publication,
            &request_id,
            &approval_context,
            &receipt,
        )?;
        if observation == ActivationReceiptObservationV1::RefreshJournal {
            return Ok(PendingRequestOutcomeV1::RefreshJournal);
        }
        let link = PendingActivationLinkV1 {
            request_id: receipt.request.id,
            target: receipt.request.target,
            requester: receipt.request.requester,
            required_approvals: authority.policy.required_approvals,
            observed_active: receipt.request.observed_active,
            created_at: receipt.request.created_at,
            expires_at: receipt.request.expires_at,
            disposition: receipt.disposition,
            request_state_at_journal: if observation == ActivationReceiptObservationV1::Expired {
                ActivationRequestState::Expired
            } else {
                ActivationRequestState::Pending
            },
            approval_context,
        };
        Ok(match observation {
            ActivationReceiptObservationV1::Pending => PendingRequestOutcomeV1::Pending(link),
            ActivationReceiptObservationV1::Expired => PendingRequestOutcomeV1::Expired(link),
            ActivationReceiptObservationV1::RefreshJournal => {
                PendingRequestOutcomeV1::RefreshJournal
            }
        })
    }

    async fn link_pending(
        &self,
        record: &PromotionRecordV1,
        publication: &PublicationRecordV1,
        activation: &PendingActivationLinkV1,
    ) -> Result<ActivationRequest, PromotionError> {
        let linked = self
            .activation
            .link_pending_activation(LinkPendingActivationV1 {
                request_id: activation.request_id.clone(),
                link: LinkProductActivation {
                    promotion_id: activation.approval_context.promotion_id.clone(),
                    promotion_request_digest: activation
                        .approval_context
                        .promotion_request_digest
                        .clone(),
                    approval_context_digest: activation
                        .approval_context
                        .approval_context_digest
                        .clone(),
                },
            })
            .await?;
        validate_linked_activation(record, publication, activation, &linked)?;
        Ok(linked)
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
    expected_context: &ProductApprovalContextV1,
    receipt: &PendingActivationReceiptV1,
) -> Result<ActivationReceiptObservationV1, PromotionError> {
    let request = &receipt.request;
    let authority = &record.intent.authority;
    let ttl_seconds = i64::try_from(authority.policy.ttl_seconds.get())
        .map_err(|_| PromotionError::InvalidPolicy)?;
    let ttl = Duration::try_seconds(ttl_seconds).ok_or(PromotionError::InvalidPolicy)?;
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
        || request.created_at < record.created_at
        || request.expires_at != expected_expiry
        || request.approval_context
            != (ActivationApprovalContextV1::ProductAuthoring {
                context: Box::new(expected_context.clone()),
            })
        || request.observed_active != expected_context.baseline.as_observed()
    {
        return Err(PromotionError::PendingActivationMismatch);
    }
    match receipt.disposition {
        crate::PendingActivationDispositionV1::Created => {
            if request.state != ActivationRequestState::Pending
                || request.link_state != ActivationLinkStateV1::Unlinked
                || !request.approvals.is_empty()
                || request.rejection.is_some()
                || request.apply_attempt_id.is_some()
                || request.apply_attempt_no != 0
                || request.apply_lease_until.is_some()
                || request.last_apply_error.is_some()
                || request.completion.is_some()
                || request.termination.is_some()
            {
                return Err(PromotionError::PendingActivationMismatch);
            }
        }
        crate::PendingActivationDispositionV1::Reused => {
            if request.state == ActivationRequestState::Expired {
                return Ok(ActivationReceiptObservationV1::Expired);
            }
            if request.link_state != ActivationLinkStateV1::Unlinked {
                return Ok(ActivationReceiptObservationV1::RefreshJournal);
            }
            if request.state != ActivationRequestState::Pending
                || !request.approvals.is_empty()
                || request.rejection.is_some()
                || request.apply_attempt_id.is_some()
                || request.apply_attempt_no != 0
                || request.apply_lease_until.is_some()
                || request.last_apply_error.is_some()
                || request.completion.is_some()
                || request.termination.is_some()
            {
                return Err(PromotionError::PendingActivationMismatch);
            }
        }
    }
    Ok(ActivationReceiptObservationV1::Pending)
}

fn validate_linked_activation(
    record: &PromotionRecordV1,
    publication: &PublicationRecordV1,
    activation: &PendingActivationLinkV1,
    request: &ActivationRequest,
) -> Result<(), PromotionError> {
    let expected_expiry = request
        .created_at
        .checked_add_signed(
            Duration::try_seconds(
                i64::try_from(record.intent.authority.policy.ttl_seconds.get())
                    .map_err(|_| PromotionError::InvalidPolicy)?,
            )
            .ok_or(PromotionError::InvalidPolicy)?,
        )
        .ok_or(PromotionError::InvalidPolicy)?;
    let exact = request.id == activation.request_id
        && request.target == activation.target
        && request.target.version == publication.version
        && request.target.content_hash == publication.content_hash
        && request.requester == activation.requester
        && request.required_approvals == activation.required_approvals.get()
        && request.observed_active == activation.observed_active
        && request.created_at == activation.created_at
        && request.expires_at == activation.expires_at
        && request.expires_at == expected_expiry
        && request.approval_context
            == (ActivationApprovalContextV1::ProductAuthoring {
                context: Box::new(activation.approval_context.clone()),
            });
    let linked = matches!(request.link_state, ActivationLinkStateV1::Linked { .. });
    if !exact || (!linked && request.state != ActivationRequestState::Expired) {
        return Err(PromotionError::PendingActivationMismatch);
    }
    Ok(())
}

fn expired_link(
    activation: &PendingActivationLinkV1,
    request: &ActivationRequest,
) -> PendingActivationLinkV1 {
    PendingActivationLinkV1 {
        request_id: request.id.clone(),
        target: request.target.clone(),
        requester: request.requester,
        required_approvals: activation.required_approvals,
        observed_active: request.observed_active.clone(),
        created_at: request.created_at,
        expires_at: request.expires_at,
        disposition: crate::PendingActivationDispositionV1::Reused,
        request_state_at_journal: ActivationRequestState::Expired,
        approval_context: activation.approval_context.clone(),
    }
}

enum PendingRequestOutcomeV1 {
    RefreshJournal,
    Pending(PendingActivationLinkV1),
    Expired(PendingActivationLinkV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActivationReceiptObservationV1 {
    Pending,
    Expired,
    RefreshJournal,
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
