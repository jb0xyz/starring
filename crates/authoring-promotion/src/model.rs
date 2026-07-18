use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU32, NonZeroU64};

use automation_ruleset::{
    content_hash, RuleSetContentHash, RuleSetKey, RuleSetSchemaVersion, RuleSetVersionId,
};
use automation_ruleset_activation::{ActivationRequestId, ActivationTarget, ObservedActive};
use automation_state::InteractionRuleSet;
use chrono::{DateTime, Utc};
use design_harness::IntentRequestedOutcome;
use discord_model::{GuildId, UserId};
use resource_resolution::ResourceBindingFingerprint;
use serde::{Deserialize, Serialize};

use crate::digest::{activation_request_hash_v1, promotion_request_digest_v1};
use crate::id::{
    AuthoringHash, AuthoringSessionId, AutomationInstallationId, BindingRevision,
    IdempotencyScopeDigest, PolicyRevision, PrincipalId, PromotionId, PromotionRequestDigest,
    PromotionRevision, SessionGeneration, TenantId,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalPolicyV1 {
    pub revision: PolicyRevision,
    pub required_approvals: NonZeroU32,
    pub ttl_seconds: NonZeroU64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedPromotionContext {
    pub tenant_id: TenantId,
    pub principal_id: PrincipalId,
    pub session_owner_id: PrincipalId,
    pub session_id: AuthoringSessionId,
    pub session_generation: SessionGeneration,
    pub guild_id: GuildId,
    pub installation_id: AutomationInstallationId,
    pub ruleset_key: RuleSetKey,
    pub requester: UserId,
    pub binding_revision: BindingRevision,
    pub policy: ApprovalPolicyV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringEvidenceV1 {
    pub artifact_version: u16,
    pub intent_protocol_version: u16,
    pub identity_revision: u16,
    pub extractor_revision: u32,
    pub normalizer_revision: u32,
    pub compiler_revision: u32,
    pub simulator_revision: u32,
    pub recipe_id: String,
    pub recipe_version: u32,
    pub recipe_descriptor_digest: AuthoringHash,
    pub recipe_registry_digest: AuthoringHash,
    pub requested_outcome: IntentRequestedOutcome,
    pub intent_revision: u64,
    pub candidate_revision: u64,
    pub request_evidence_hash: AuthoringHash,
    pub request_evidence_entries: u64,
    pub compiler_input_hash: AuthoringHash,
    pub semantic_intent_hash: AuthoringHash,
    pub compiled_plan_hash: AuthoringHash,
    pub candidate_ruleset_hash: AuthoringHash,
    pub candidate_draft_hash: AuthoringHash,
    pub compiled_operations: u64,
    pub context_fingerprint: ResourceBindingFingerprint,
    pub external_channel_bindings: Vec<String>,
    pub stage_binding_digest: AuthoringHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringPreviewV1 {
    pub revision: u64,
    pub summary: AuthoringPreviewSummaryV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringPreviewSummaryV1 {
    pub panels: u64,
    pub modals: u64,
    pub rules: u64,
    pub actions: u64,
    pub unresolved_references: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionIntentV1 {
    pub idempotency_scope_digest: IdempotencyScopeDigest,
    pub authority: AuthenticatedPromotionContext,
    pub evidence: AuthoringEvidenceV1,
    pub definition: InteractionRuleSet,
    pub preview: AuthoringPreviewV1,
    pub registry_schema_version: RuleSetSchemaVersion,
    pub expected_registry_content_hash: RuleSetContentHash,
}

impl Debug for PromotionIntentV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PromotionIntentV1")
            .field("idempotency_scope_digest", &self.idempotency_scope_digest)
            .field("authority", &self.authority)
            .field("evidence", &self.evidence)
            .field("definition", &"<redacted>")
            .field("preview", &self.preview)
            .field("registry_schema_version", &self.registry_schema_version)
            .field(
                "expected_registry_content_hash",
                &self.expected_registry_content_hash,
            )
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationDispositionV1 {
    Created,
    Reused,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationRecordV1 {
    pub version: RuleSetVersionId,
    pub schema_version: RuleSetSchemaVersion,
    pub content_hash: RuleSetContentHash,
    pub disposition: PublicationDispositionV1,
    pub registry_created_by: UserId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingActivationDispositionV1 {
    Created,
    Reused,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingActivationLinkV1 {
    pub request_id: ActivationRequestId,
    pub target: ActivationTarget,
    pub requester: UserId,
    pub required_approvals: NonZeroU32,
    pub observed_active: Option<ObservedActive>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub disposition: PendingActivationDispositionV1,
    pub request_state_at_link: automation_ruleset_activation::ActivationRequestState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PromotionStageV1 {
    Prepared,
    Published {
        publication: PublicationRecordV1,
    },
    ActivationPending {
        publication: PublicationRecordV1,
        activation: PendingActivationLinkV1,
    },
    Expired {
        publication: PublicationRecordV1,
        activation: PendingActivationLinkV1,
    },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewPromotionV1 {
    pub id: PromotionId,
    pub request_digest: PromotionRequestDigest,
    pub intent: PromotionIntentV1,
    pub created_at: DateTime<Utc>,
}

impl Debug for NewPromotionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NewPromotionV1")
            .field("id", &self.id)
            .field("request_digest", &self.request_digest)
            .field("intent", &self.intent)
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionRecordV1 {
    pub id: PromotionId,
    pub revision: PromotionRevision,
    pub request_digest: PromotionRequestDigest,
    pub intent: PromotionIntentV1,
    pub stage: PromotionStageV1,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Debug for PromotionRecordV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PromotionRecordV1")
            .field("id", &self.id)
            .field("revision", &self.revision)
            .field("request_digest", &self.request_digest)
            .field("intent", &self.intent)
            .field("stage", &self.stage)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl PromotionRecordV1 {
    pub fn validate(&self) -> Result<(), PromotionRecordValidationError> {
        if self.id.as_str() != self.intent.idempotency_scope_digest.as_str() {
            return Err(PromotionRecordValidationError::Identity);
        }
        let expected_request = promotion_request_digest_v1(&self.intent)
            .map_err(|_| PromotionRecordValidationError::Identity)?;
        if expected_request != self.request_digest {
            return Err(PromotionRecordValidationError::Identity);
        }
        if self.intent.authority.principal_id != self.intent.authority.session_owner_id {
            return Err(PromotionRecordValidationError::SessionOwner);
        }
        validate_evidence(&self.intent)?;
        if self.updated_at < self.created_at {
            return Err(PromotionRecordValidationError::Timestamp);
        }
        let ttl_seconds = i64::try_from(self.intent.authority.policy.ttl_seconds.get())
            .map_err(|_| PromotionRecordValidationError::Policy)?;
        let ttl = chrono::Duration::try_seconds(ttl_seconds)
            .ok_or(PromotionRecordValidationError::Policy)?;
        self.created_at
            .checked_add_signed(ttl)
            .ok_or(PromotionRecordValidationError::Policy)?;
        match &self.stage {
            PromotionStageV1::Prepared => {
                if self.revision != PromotionRevision::FIRST {
                    return Err(PromotionRecordValidationError::Revision);
                }
            }
            PromotionStageV1::Published { publication } => {
                validate_publication(&self.intent, publication)?;
                if self.revision.get() != 2 {
                    return Err(PromotionRecordValidationError::Revision);
                }
            }
            PromotionStageV1::ActivationPending {
                publication,
                activation,
            } => {
                validate_publication(&self.intent, publication)?;
                validate_activation(
                    &self.id,
                    &self.request_digest,
                    self.created_at,
                    self.updated_at,
                    &self.intent,
                    publication,
                    activation,
                )?;
                if activation.request_state_at_link
                    != automation_ruleset_activation::ActivationRequestState::Pending
                {
                    return Err(PromotionRecordValidationError::Activation);
                }
                if self.revision.get() != 3 {
                    return Err(PromotionRecordValidationError::Revision);
                }
            }
            PromotionStageV1::Expired {
                publication,
                activation,
            } => {
                validate_publication(&self.intent, publication)?;
                validate_activation(
                    &self.id,
                    &self.request_digest,
                    self.created_at,
                    self.updated_at,
                    &self.intent,
                    publication,
                    activation,
                )?;
                if activation.disposition != PendingActivationDispositionV1::Reused {
                    return Err(PromotionRecordValidationError::Activation);
                }
                if activation.request_state_at_link
                    != automation_ruleset_activation::ActivationRequestState::Expired
                {
                    return Err(PromotionRecordValidationError::Activation);
                }
                if self.updated_at < activation.expires_at {
                    return Err(PromotionRecordValidationError::Timestamp);
                }
                if self.revision.get() != 3 {
                    return Err(PromotionRecordValidationError::Revision);
                }
            }
        }
        Ok(())
    }
}

fn validate_evidence(intent: &PromotionIntentV1) -> Result<(), PromotionRecordValidationError> {
    let evidence = &intent.evidence;
    if evidence.artifact_version != 1
        || evidence.intent_protocol_version == 0
        || evidence.identity_revision == 0
        || evidence.extractor_revision == 0
        || evidence.normalizer_revision == 0
        || evidence.compiler_revision == 0
        || evidence.simulator_revision == 0
        || evidence.recipe_id.is_empty()
        || evidence.recipe_version == 0
        || evidence.requested_outcome != IntentRequestedOutcome::ValidatedPreview
        || evidence.intent_revision == 0
        || evidence.candidate_revision == 0
        || evidence.request_evidence_entries == 0
        || evidence.compiled_operations == 0
        || evidence.external_channel_bindings.is_empty()
        || intent.preview.revision != evidence.candidate_revision
        || !intent.preview.summary.unresolved_references.is_empty()
    {
        return Err(PromotionRecordValidationError::Evidence);
    }
    if !evidence
        .external_channel_bindings
        .windows(2)
        .all(|window| window[0] < window[1])
    {
        return Err(PromotionRecordValidationError::Evidence);
    }
    let actions = intent
        .definition
        .rules
        .iter()
        .map(|rule| rule.actions.len())
        .try_fold(0_u64, |total, count| {
            u64::try_from(count)
                .ok()
                .and_then(|count| total.checked_add(count))
        })
        .ok_or(PromotionRecordValidationError::Preview)?;
    if intent.preview.summary.panels
        != u64::try_from(intent.definition.panels.len()).unwrap_or(u64::MAX)
        || intent.preview.summary.modals
            != u64::try_from(intent.definition.modals.len()).unwrap_or(u64::MAX)
        || intent.preview.summary.rules
            != u64::try_from(intent.definition.rules.len()).unwrap_or(u64::MAX)
        || intent.preview.summary.actions != actions
    {
        return Err(PromotionRecordValidationError::Preview);
    }
    let actual_content_hash = content_hash(intent.registry_schema_version, &intent.definition)
        .map_err(|_| PromotionRecordValidationError::RegistryIdentity)?;
    if actual_content_hash != intent.expected_registry_content_hash {
        return Err(PromotionRecordValidationError::RegistryIdentity);
    }
    let ttl_seconds = i64::try_from(intent.authority.policy.ttl_seconds.get())
        .map_err(|_| PromotionRecordValidationError::Policy)?;
    chrono::Duration::try_seconds(ttl_seconds).ok_or(PromotionRecordValidationError::Policy)?;
    Ok(())
}

fn validate_publication(
    intent: &PromotionIntentV1,
    publication: &PublicationRecordV1,
) -> Result<(), PromotionRecordValidationError> {
    if publication.schema_version != intent.registry_schema_version
        || publication.content_hash != intent.expected_registry_content_hash
        || (publication.disposition == PublicationDispositionV1::Created
            && publication.registry_created_by != intent.authority.requester)
    {
        return Err(PromotionRecordValidationError::Publication);
    }
    Ok(())
}

fn validate_activation(
    promotion_id: &PromotionId,
    promotion_request_digest: &PromotionRequestDigest,
    promotion_created_at: DateTime<Utc>,
    promotion_updated_at: DateTime<Utc>,
    intent: &PromotionIntentV1,
    publication: &PublicationRecordV1,
    activation: &PendingActivationLinkV1,
) -> Result<(), PromotionRecordValidationError> {
    let authority = &intent.authority;
    let expected_request_id =
        activation_request_hash_v1(promotion_id, promotion_request_digest, publication)
            .ok()
            .and_then(|digest| ActivationRequestId::parse(digest.as_str()).ok());
    if expected_request_id.as_ref() != Some(&activation.request_id)
        || activation.target.guild_id != authority.guild_id
        || activation.target.ruleset_key != authority.ruleset_key
        || activation.target.version != publication.version
        || activation.target.content_hash != publication.content_hash
        || activation.requester != authority.requester
        || activation.required_approvals != authority.policy.required_approvals
        || activation.created_at < promotion_created_at
        || activation.created_at > promotion_updated_at
        || activation.expires_at <= activation.created_at
    {
        return Err(PromotionRecordValidationError::Activation);
    }
    let ttl_seconds = i64::try_from(authority.policy.ttl_seconds.get())
        .map_err(|_| PromotionRecordValidationError::Policy)?;
    let ttl =
        chrono::Duration::try_seconds(ttl_seconds).ok_or(PromotionRecordValidationError::Policy)?;
    let expected_expiry = activation
        .created_at
        .checked_add_signed(ttl)
        .ok_or(PromotionRecordValidationError::Policy)?;
    if expected_expiry != activation.expires_at {
        return Err(PromotionRecordValidationError::Activation);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromotionRecordValidationError {
    #[error("promotion identity is inconsistent")]
    Identity,
    #[error("authenticated principal does not own the authoring session")]
    SessionOwner,
    #[error("authoring evidence is inconsistent")]
    Evidence,
    #[error("authoring preview is inconsistent")]
    Preview,
    #[error("registry identity is inconsistent")]
    RegistryIdentity,
    #[error("approval policy is invalid")]
    Policy,
    #[error("publication record is inconsistent")]
    Publication,
    #[error("activation link is inconsistent")]
    Activation,
    #[error("promotion revision is inconsistent with its stage")]
    Revision,
    #[error("promotion timestamps are inconsistent")]
    Timestamp,
}
