use std::fmt::{Debug, Formatter};

use automation_ruleset::{content_hash, CURRENT_RULESET_SCHEMA_VERSION};
use automation_ruleset_activation::{
    approval_policy_digest_v1, product_approval_context_digest_v1, ActivationApprovalContextV1,
    ActivationDigest, ActivationLinkStateV1, ActivationPromotionId, ActivationRequest,
    ActivationRequestId, ActivationRequestState, ActivationTarget, ApprovalPolicyBindingV1,
    CreateProductActivationRequest, LinkProductActivation, ProductApprovalContextV1,
};
use chrono::{DateTime, Duration, Utc};
use design_harness::IntentRequestedOutcome;
use serde::{Deserialize, Serialize};

use crate::digest::{
    activation_request_hash_v1, approval_payload_digest_v1, idempotency_scope_digest_v1,
    promotion_request_digest_v1,
};
use crate::id::AuthoringHash;
use crate::service::{PromotionError, StartPromotionV1};
use crate::{
    AuthoringEvidenceV1, AuthoringPreviewSummaryV1, AuthoringPreviewV1, EnsurePendingActivationV1,
    IdempotencyKey, IdempotencyScopeDigest, LinkPendingActivationV1, NewPromotionV1,
    PendingActivationDispositionV1, PendingActivationLinkV1, PendingActivationReceiptV1,
    PrincipalId, PromotionId, PromotionIntentV1, PromotionRecordV1, PromotionRecordValidationError,
    PromotionRequestDigest, PromotionRevision, PromotionStageV1, PromotionStoreError,
    PublicationDispositionV1, PublicationPortOutcomeV1, PublicationRecordV1,
    PublishAuthoringRuleSetV1, ResolveProductApprovalContextV1, ResolvedProductApprovalContextV1,
    TenantId,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionIdentityV1 {
    pub promotion_id: PromotionId,
    pub idempotency_scope_digest: IdempotencyScopeDigest,
}

pub fn derive_promotion_identity_v1(
    tenant_id: &TenantId,
    principal_id: &PrincipalId,
    idempotency_key: &IdempotencyKey,
) -> Result<PromotionIdentityV1, PromotionError> {
    let idempotency_scope_digest =
        idempotency_scope_digest_v1(tenant_id, principal_id, idempotency_key)?;
    Ok(PromotionIdentityV1 {
        promotion_id: PromotionId::from_scope_digest(&idempotency_scope_digest),
        idempotency_scope_digest,
    })
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedPromotionPlanV1 {
    pub promotion_id: PromotionId,
    pub request_digest: PromotionRequestDigest,
    pub intent: PromotionIntentV1,
}

impl Debug for PreparedPromotionPlanV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedPromotionPlanV1")
            .field("promotion_id", &self.promotion_id)
            .field("request_digest", &self.request_digest)
            .field("intent", &self.intent)
            .finish()
    }
}

impl PreparedPromotionPlanV1 {
    pub fn materialize(
        &self,
        database_created_at: DateTime<Utc>,
    ) -> Result<NewPromotionV1, PromotionError> {
        let ttl = policy_ttl(&self.intent)?;
        database_created_at
            .checked_add_signed(ttl)
            .ok_or(PromotionError::InvalidPolicy)?;
        let promotion = NewPromotionV1 {
            id: self.promotion_id.clone(),
            request_digest: self.request_digest.clone(),
            intent: self.intent.clone(),
            created_at: database_created_at,
        };
        PromotionRecordV1::prepared(promotion.clone()).map_err(PromotionError::Store)?;
        Ok(promotion)
    }

    pub fn validate_admitted_record(
        &self,
        record: &PromotionRecordV1,
    ) -> Result<(), PromotionPlanValidationErrorV1> {
        record
            .validate()
            .map_err(PromotionPlanValidationErrorV1::InvalidRecord)?;
        if record.id != self.promotion_id
            || record.request_digest != self.request_digest
            || record.intent != self.intent
        {
            return Err(PromotionPlanValidationErrorV1::AdmissionMismatch);
        }
        Ok(())
    }

    pub fn validate_prepared_record(
        &self,
        record: &PromotionRecordV1,
    ) -> Result<(), PromotionPlanValidationErrorV1> {
        self.validate_admitted_record(record)?;
        if record.stage != PromotionStageV1::Prepared || record.created_at != record.updated_at {
            return Err(PromotionPlanValidationErrorV1::ExactRecordMismatch);
        }
        Ok(())
    }
}

pub fn plan_start_promotion_v1(
    input: StartPromotionV1,
) -> Result<PreparedPromotionPlanV1, PromotionError> {
    if input.artifact.contract().requested_outcome != IntentRequestedOutcome::ValidatedPreview {
        return Err(PromotionError::ValidatedPreviewRequired);
    }
    if input.context.principal_id != input.context.session_owner_id {
        return Err(PromotionError::SessionOwnerMismatch);
    }
    policy_ttl_from_seconds(input.context.policy.ttl_seconds.get())?;
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
    let identity = derive_promotion_identity_v1(
        &input.context.tenant_id,
        &input.context.principal_id,
        &input.idempotency_key,
    )?;
    let intent = PromotionIntentV1 {
        idempotency_scope_digest: identity.idempotency_scope_digest,
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
    Ok(PreparedPromotionPlanV1 {
        promotion_id: identity.promotion_id,
        request_digest,
        intent,
    })
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSetPublicationProposalV1 {
    promotion_id: PromotionId,
    promotion_request_digest: PromotionRequestDigest,
    expected_revision: PromotionRevision,
    guild_id: discord_model::GuildId,
    ruleset_key: automation_ruleset::RuleSetKey,
    definition: automation_state::InteractionRuleSet,
    created_by: discord_model::UserId,
    schema_version: automation_ruleset::RuleSetSchemaVersion,
    content_hash: automation_ruleset::RuleSetContentHash,
}

impl Debug for RuleSetPublicationProposalV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuleSetPublicationProposalV1")
            .field("promotion_id", &self.promotion_id)
            .field("promotion_request_digest", &self.promotion_request_digest)
            .field("expected_revision", &self.expected_revision)
            .field("guild_id", &self.guild_id)
            .field("ruleset_key", &self.ruleset_key)
            .field("definition", &"<redacted>")
            .field("created_by", &self.created_by)
            .field("schema_version", &self.schema_version)
            .field("content_hash", &self.content_hash)
            .finish()
    }
}

impl RuleSetPublicationProposalV1 {
    pub fn request(&self) -> PublishAuthoringRuleSetV1 {
        PublishAuthoringRuleSetV1 {
            guild_id: self.guild_id,
            ruleset_key: self.ruleset_key.clone(),
            definition: self.definition.clone(),
            created_by: self.created_by,
        }
    }

    pub fn complete(
        &self,
        record: &PromotionRecordV1,
        outcome: PublicationPortOutcomeV1,
        database_updated_at: DateTime<Utc>,
    ) -> Result<PublicationTransitionV1, PromotionError> {
        ensure_publication_proposal_matches(self, record)?;
        let publication = publication_from_outcome(self, outcome)?;
        let expected_record = record
            .transition_to_published(record.revision, publication.clone(), database_updated_at)
            .map_err(PromotionError::Store)?;
        Ok(PublicationTransitionV1 {
            publication,
            expected_record,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationTransitionV1 {
    pub publication: PublicationRecordV1,
    pub expected_record: PromotionRecordV1,
}

pub fn plan_ruleset_publication_v1(
    record: &PromotionRecordV1,
) -> Result<RuleSetPublicationProposalV1, PromotionError> {
    validate_record(record)?;
    if record.stage != PromotionStageV1::Prepared {
        return Err(PromotionError::Store(
            PromotionStoreError::InvalidTransition,
        ));
    }
    Ok(publication_proposal_from_record(record))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalEnvironmentProposalV1 {
    promotion_id: PromotionId,
    promotion_request_digest: PromotionRequestDigest,
    expected_revision: PromotionRevision,
    tenant_id: TenantId,
    installation_id: crate::AutomationInstallationId,
    target: ActivationTarget,
    binding_revision: crate::BindingRevision,
    context_fingerprint: resource_resolution::ResourceBindingFingerprint,
    required_channel_bindings: Vec<String>,
}

impl ApprovalEnvironmentProposalV1 {
    pub fn request(&self) -> ResolveProductApprovalContextV1 {
        ResolveProductApprovalContextV1 {
            tenant_id: self.tenant_id.clone(),
            installation_id: self.installation_id.clone(),
            target: self.target.clone(),
            binding_revision: self.binding_revision,
            context_fingerprint: self.context_fingerprint.clone(),
            required_channel_bindings: self.required_channel_bindings.clone(),
        }
    }
}

pub fn plan_approval_environment_v1(
    record: &PromotionRecordV1,
) -> Result<ApprovalEnvironmentProposalV1, PromotionError> {
    validate_record(record)?;
    let PromotionStageV1::Published { publication } = &record.stage else {
        return Err(PromotionError::Store(
            PromotionStoreError::InvalidTransition,
        ));
    };
    let authority = &record.intent.authority;
    Ok(ApprovalEnvironmentProposalV1 {
        promotion_id: record.id.clone(),
        promotion_request_digest: record.request_digest.clone(),
        expected_revision: record.revision,
        tenant_id: authority.tenant_id.clone(),
        installation_id: authority.installation_id.clone(),
        target: ActivationTarget {
            guild_id: authority.guild_id,
            ruleset_key: authority.ruleset_key.clone(),
            version: publication.version,
            content_hash: publication.content_hash,
        },
        binding_revision: authority.binding_revision,
        context_fingerprint: record.intent.evidence.context_fingerprint.clone(),
        required_channel_bindings: record.intent.evidence.external_channel_bindings.clone(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingActivationProposalV1 {
    promotion_id: PromotionId,
    promotion_request_digest: PromotionRequestDigest,
    expected_revision: PromotionRevision,
    request_id: ActivationRequestId,
    target: ActivationTarget,
    requester: discord_model::UserId,
    approval_context: ProductApprovalContextV1,
}

impl PendingActivationProposalV1 {
    pub fn request(&self) -> EnsurePendingActivationV1 {
        EnsurePendingActivationV1 {
            create: CreateProductActivationRequest {
                id: self.request_id.clone(),
                target: self.target.clone(),
                requester: self.requester,
                context: self.approval_context.clone(),
            },
        }
    }

    pub fn complete(
        &self,
        record: &PromotionRecordV1,
        receipt: &PendingActivationReceiptV1,
        database_updated_at: DateTime<Utc>,
    ) -> Result<PendingActivationTransitionV1, PromotionError> {
        ensure_pending_proposal_matches(self, record)?;
        let PromotionStageV1::Published { publication } = &record.stage else {
            return Err(PromotionError::Store(
                PromotionStoreError::InvalidTransition,
            ));
        };
        let observation = validate_activation_receipt(
            record,
            publication,
            &self.request_id,
            &self.approval_context,
            receipt,
        )?;
        if observation == ActivationReceiptObservationV1::RefreshJournal {
            return Ok(PendingActivationTransitionV1::RefreshJournal);
        }
        let activation = PendingActivationLinkV1 {
            request_id: receipt.request.id.clone(),
            target: receipt.request.target.clone(),
            requester: receipt.request.requester,
            required_approvals: record.intent.authority.policy.required_approvals,
            observed_active: receipt.request.observed_active.clone(),
            created_at: receipt.request.created_at,
            expires_at: receipt.request.expires_at,
            disposition: receipt.disposition,
            request_state_at_journal: if observation == ActivationReceiptObservationV1::Expired {
                ActivationRequestState::Expired
            } else {
                ActivationRequestState::Pending
            },
            approval_context: self.approval_context.clone(),
        };
        match observation {
            ActivationReceiptObservationV1::Pending => {
                let updated_at = database_updated_at.max(activation.created_at);
                let expected_record = record
                    .transition_to_activation_pending(
                        record.revision,
                        activation.clone(),
                        updated_at,
                    )
                    .map_err(PromotionError::Store)?;
                Ok(PendingActivationTransitionV1::ActivationPending {
                    activation,
                    expected_record,
                })
            }
            ActivationReceiptObservationV1::Expired => {
                let updated_at = database_updated_at.max(activation.expires_at);
                let expected_record = record
                    .transition_to_expired(record.revision, activation.clone(), updated_at)
                    .map_err(PromotionError::Store)?;
                Ok(PendingActivationTransitionV1::Expired {
                    activation,
                    expected_record,
                })
            }
            ActivationReceiptObservationV1::RefreshJournal => {
                Ok(PendingActivationTransitionV1::RefreshJournal)
            }
        }
    }
}

pub fn plan_pending_activation_v1(
    record: &PromotionRecordV1,
    resolved: ResolvedProductApprovalContextV1,
) -> Result<PendingActivationProposalV1, PromotionError> {
    validate_record(record)?;
    let PromotionStageV1::Published { publication } = &record.stage else {
        return Err(PromotionError::Store(
            PromotionStoreError::InvalidTransition,
        ));
    };
    pending_activation_proposal_from_parts(record, publication, resolved)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum PendingActivationTransitionV1 {
    RefreshJournal,
    ActivationPending {
        activation: PendingActivationLinkV1,
        expected_record: PromotionRecordV1,
    },
    Expired {
        activation: PendingActivationLinkV1,
        expected_record: PromotionRecordV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationLinkProposalV1 {
    promotion_id: PromotionId,
    promotion_request_digest: PromotionRequestDigest,
    expected_revision: PromotionRevision,
    publication: PublicationRecordV1,
    activation: PendingActivationLinkV1,
}

impl ActivationLinkProposalV1 {
    pub fn request(&self) -> LinkPendingActivationV1 {
        LinkPendingActivationV1 {
            request_id: self.activation.request_id.clone(),
            link: LinkProductActivation {
                promotion_id: self.activation.approval_context.promotion_id.clone(),
                promotion_request_digest: self
                    .activation
                    .approval_context
                    .promotion_request_digest
                    .clone(),
                approval_context_digest: self
                    .activation
                    .approval_context
                    .approval_context_digest
                    .clone(),
            },
        }
    }

    pub fn complete(
        &self,
        record: &PromotionRecordV1,
        linked: &ActivationRequest,
        database_updated_at: DateTime<Utc>,
    ) -> Result<LinkedActivationTransitionV1, PromotionError> {
        ensure_link_proposal_matches(self, record)?;
        validate_linked_activation(record, &self.publication, &self.activation, linked)?;
        if linked.state == ActivationRequestState::Expired {
            let activation = expired_link(&self.activation, linked);
            let updated_at = database_updated_at.max(activation.expires_at);
            let expected_record = record
                .transition_to_expired(record.revision, activation.clone(), updated_at)
                .map_err(PromotionError::Store)?;
            return Ok(LinkedActivationTransitionV1::Expired {
                activation: Box::new(activation),
                expected_record: Box::new(expected_record),
            });
        }
        Ok(LinkedActivationTransitionV1::Linked {
            expected_record: Box::new(record.clone()),
        })
    }
}

pub fn plan_activation_link_v1(
    record: &PromotionRecordV1,
) -> Result<ActivationLinkProposalV1, PromotionError> {
    validate_record(record)?;
    let PromotionStageV1::ActivationPending {
        publication,
        activation,
    } = &record.stage
    else {
        return Err(PromotionError::Store(
            PromotionStoreError::InvalidTransition,
        ));
    };
    Ok(ActivationLinkProposalV1 {
        promotion_id: record.id.clone(),
        promotion_request_digest: record.request_digest.clone(),
        expected_revision: record.revision,
        publication: publication.clone(),
        activation: activation.clone(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinkedActivationTransitionV1 {
    Linked {
        expected_record: Box<PromotionRecordV1>,
    },
    Expired {
        activation: Box<PendingActivationLinkV1>,
        expected_record: Box<PromotionRecordV1>,
    },
}

pub fn validate_exact_planned_record_v1(
    expected: &PromotionRecordV1,
    actual: &PromotionRecordV1,
) -> Result<(), PromotionPlanValidationErrorV1> {
    expected
        .validate()
        .map_err(PromotionPlanValidationErrorV1::InvalidPlanRecord)?;
    actual
        .validate()
        .map_err(PromotionPlanValidationErrorV1::InvalidRecord)?;
    if expected != actual {
        return Err(PromotionPlanValidationErrorV1::ExactRecordMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromotionPlanValidationErrorV1 {
    #[error("planned promotion record is invalid: {0}")]
    InvalidPlanRecord(PromotionRecordValidationError),
    #[error("persisted promotion record is invalid: {0}")]
    InvalidRecord(PromotionRecordValidationError),
    #[error("persisted promotion admission does not match the deterministic plan")]
    AdmissionMismatch,
    #[error("persisted promotion record does not match the deterministic transition")]
    ExactRecordMismatch,
}

fn publication_proposal_from_record(record: &PromotionRecordV1) -> RuleSetPublicationProposalV1 {
    let authority = &record.intent.authority;
    RuleSetPublicationProposalV1 {
        promotion_id: record.id.clone(),
        promotion_request_digest: record.request_digest.clone(),
        expected_revision: record.revision,
        guild_id: authority.guild_id,
        ruleset_key: authority.ruleset_key.clone(),
        definition: record.intent.definition.clone(),
        created_by: authority.requester,
        schema_version: record.intent.registry_schema_version,
        content_hash: record.intent.expected_registry_content_hash,
    }
}

fn ensure_publication_proposal_matches(
    proposal: &RuleSetPublicationProposalV1,
    record: &PromotionRecordV1,
) -> Result<(), PromotionError> {
    validate_record(record)?;
    if record.stage != PromotionStageV1::Prepared {
        return Err(PromotionError::Store(
            PromotionStoreError::InvalidTransition,
        ));
    }
    if proposal != &publication_proposal_from_record(record) {
        return Err(PromotionError::PublicationMismatch);
    }
    Ok(())
}

fn publication_from_outcome(
    proposal: &RuleSetPublicationProposalV1,
    outcome: PublicationPortOutcomeV1,
) -> Result<PublicationRecordV1, PromotionError> {
    let (disposition, artifact) = match outcome {
        PublicationPortOutcomeV1::Created(artifact) => {
            (PublicationDispositionV1::Created, artifact)
        }
        PublicationPortOutcomeV1::Reused(artifact) => (PublicationDispositionV1::Reused, artifact),
    };
    if artifact.guild_id != proposal.guild_id
        || artifact.ruleset_key != proposal.ruleset_key
        || artifact.definition != proposal.definition
        || artifact.schema_version != proposal.schema_version
        || artifact.content_hash != proposal.content_hash
        || (disposition == PublicationDispositionV1::Created
            && artifact.created_by != proposal.created_by)
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

fn pending_activation_proposal_from_parts(
    record: &PromotionRecordV1,
    publication: &PublicationRecordV1,
    resolved: ResolvedProductApprovalContextV1,
) -> Result<PendingActivationProposalV1, PromotionError> {
    let request_hash = activation_request_hash_v1(&record.id, &record.request_digest, publication)?;
    let request_id = ActivationRequestId::parse(request_hash.as_str())
        .map_err(|_| PromotionError::ActivationIdentity)?;
    let authority = &record.intent.authority;
    let target = ActivationTarget {
        guild_id: authority.guild_id,
        ruleset_key: authority.ruleset_key.clone(),
        version: publication.version,
        content_hash: publication.content_hash,
    };
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
    Ok(PendingActivationProposalV1 {
        promotion_id: record.id.clone(),
        promotion_request_digest: record.request_digest.clone(),
        expected_revision: record.revision,
        request_id,
        target,
        requester: authority.requester,
        approval_context,
    })
}

fn ensure_pending_proposal_matches(
    proposal: &PendingActivationProposalV1,
    record: &PromotionRecordV1,
) -> Result<(), PromotionError> {
    validate_record(record)?;
    let PromotionStageV1::Published { publication } = &record.stage else {
        return Err(PromotionError::Store(
            PromotionStoreError::InvalidTransition,
        ));
    };
    let expected = pending_activation_proposal_from_parts(
        record,
        publication,
        ResolvedProductApprovalContextV1 {
            binding: proposal.approval_context.binding.clone(),
            baseline: proposal.approval_context.baseline.clone(),
        },
    )?;
    if proposal != &expected {
        return Err(PromotionError::PendingActivationMismatch);
    }
    Ok(())
}

fn ensure_link_proposal_matches(
    proposal: &ActivationLinkProposalV1,
    record: &PromotionRecordV1,
) -> Result<(), PromotionError> {
    validate_record(record)?;
    let PromotionStageV1::ActivationPending {
        publication,
        activation,
    } = &record.stage
    else {
        return Err(PromotionError::Store(
            PromotionStoreError::InvalidTransition,
        ));
    };
    if proposal.promotion_id != record.id
        || proposal.promotion_request_digest != record.request_digest
        || proposal.expected_revision != record.revision
        || proposal.publication != *publication
        || proposal.activation != *activation
    {
        return Err(PromotionError::PendingActivationMismatch);
    }
    Ok(())
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
    let ttl = policy_ttl(&record.intent)?;
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
        PendingActivationDispositionV1::Created => {
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
        PendingActivationDispositionV1::Reused => {
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
        .checked_add_signed(policy_ttl(&record.intent)?)
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
        disposition: PendingActivationDispositionV1::Reused,
        request_state_at_journal: ActivationRequestState::Expired,
        approval_context: activation.approval_context.clone(),
    }
}

fn validate_record(record: &PromotionRecordV1) -> Result<(), PromotionError> {
    record
        .validate()
        .map_err(PromotionStoreError::InvalidRecord)
        .map_err(PromotionError::Store)
}

fn policy_ttl(intent: &PromotionIntentV1) -> Result<Duration, PromotionError> {
    policy_ttl_from_seconds(intent.authority.policy.ttl_seconds.get())
}

fn policy_ttl_from_seconds(seconds: u64) -> Result<Duration, PromotionError> {
    let seconds = i64::try_from(seconds).map_err(|_| PromotionError::InvalidPolicy)?;
    Duration::try_seconds(seconds).ok_or(PromotionError::InvalidPolicy)
}

fn parse_hash(field: &'static str, value: &str) -> Result<AuthoringHash, PromotionError> {
    AuthoringHash::parse(value)
        .map_err(|source| PromotionError::InvalidArtifactHash { field, source })
}

fn preview_count(field: &'static str, value: usize) -> Result<u64, PromotionError> {
    u64::try_from(value).map_err(|_| PromotionError::ArtifactCountOverflow { field })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActivationReceiptObservationV1 {
    Pending,
    Expired,
    RefreshJournal,
}
