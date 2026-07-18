use std::fmt::{Debug, Formatter};

use automation_ruleset::{
    PublishOutcome, PublishRuleSetRequest, RuleSetContentHash, RuleSetKey, RuleSetSchemaVersion,
    RuleSetStore, RuleSetStoreError, RuleSetVersionId,
};
use automation_ruleset_activation::{ActivationRequest, ActivationRequestId, ActivationTarget};
use automation_ruleset_activation::{
    ApprovalBindingContextV1, CreateProductActivationRequest, ExpectedActiveBaselineV1,
    LinkProductActivation,
};
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};
use resource_resolution::ResourceBindingFingerprint;

use crate::{AutomationInstallationId, BindingRevision, PendingActivationDispositionV1, TenantId};

#[derive(Clone, PartialEq, Eq)]
pub struct PublishAuthoringRuleSetV1 {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub definition: InteractionRuleSet,
    pub created_by: UserId,
}

impl Debug for PublishAuthoringRuleSetV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PublishAuthoringRuleSetV1")
            .field("guild_id", &self.guild_id)
            .field("ruleset_key", &self.ruleset_key)
            .field("definition", &"<redacted>")
            .field("created_by", &self.created_by)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PublishedAuthoringRuleSetV1 {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub schema_version: RuleSetSchemaVersion,
    pub definition: InteractionRuleSet,
    pub content_hash: RuleSetContentHash,
    pub created_by: UserId,
}

impl Debug for PublishedAuthoringRuleSetV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PublishedAuthoringRuleSetV1")
            .field("guild_id", &self.guild_id)
            .field("ruleset_key", &self.ruleset_key)
            .field("version", &self.version)
            .field("schema_version", &self.schema_version)
            .field("definition", &"<redacted>")
            .field("content_hash", &self.content_hash)
            .field("created_by", &self.created_by)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum PublicationPortOutcomeV1 {
    Created(PublishedAuthoringRuleSetV1),
    Reused(PublishedAuthoringRuleSetV1),
}

impl Debug for PublicationPortOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created(artifact) => formatter.debug_tuple("Created").field(artifact).finish(),
            Self::Reused(artifact) => formatter.debug_tuple("Reused").field(artifact).finish(),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait RuleSetPublicationPort {
    async fn publish_ruleset(
        &self,
        request: PublishAuthoringRuleSetV1,
    ) -> Result<PublicationPortOutcomeV1, RuleSetStoreError>;
}

impl<T> RuleSetPublicationPort for T
where
    T: RuleSetStore,
{
    async fn publish_ruleset(
        &self,
        request: PublishAuthoringRuleSetV1,
    ) -> Result<PublicationPortOutcomeV1, RuleSetStoreError> {
        let outcome = RuleSetStore::publish(
            self,
            PublishRuleSetRequest {
                guild_id: request.guild_id,
                ruleset_key: request.ruleset_key,
                definition: request.definition,
                created_by: request.created_by,
            },
        )
        .await?;
        Ok(match outcome {
            PublishOutcome::Created(artifact) => PublicationPortOutcomeV1::Created(artifact.into()),
            PublishOutcome::Reused(artifact) => PublicationPortOutcomeV1::Reused(artifact.into()),
        })
    }
}

impl From<automation_ruleset::RuleSetVersion> for PublishedAuthoringRuleSetV1 {
    fn from(artifact: automation_ruleset::RuleSetVersion) -> Self {
        Self {
            guild_id: artifact.guild_id,
            ruleset_key: artifact.ruleset_key,
            version: artifact.version,
            schema_version: artifact.schema_version,
            definition: artifact.definition,
            content_hash: artifact.content_hash,
            created_by: artifact.created_by,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveProductApprovalContextV1 {
    pub tenant_id: TenantId,
    pub installation_id: AutomationInstallationId,
    pub target: ActivationTarget,
    pub binding_revision: BindingRevision,
    pub context_fingerprint: ResourceBindingFingerprint,
    pub required_channel_bindings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedProductApprovalContextV1 {
    pub binding: ApprovalBindingContextV1,
    pub baseline: ExpectedActiveBaselineV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnsurePendingActivationV1 {
    pub create: CreateProductActivationRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkPendingActivationV1 {
    pub request_id: ActivationRequestId,
    pub link: LinkProductActivation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingActivationReceiptV1 {
    pub request: ActivationRequest,
    pub disposition: PendingActivationDispositionV1,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PendingActivationPortError {
    #[error("pending activation request conflicts with an existing request: {0}")]
    Conflict(String),
    #[error("pending activation request outcome is indeterminate: {0}")]
    Indeterminate(String),
    #[error("pending activation backend failed: {0}")]
    Backend(String),
}

#[allow(async_fn_in_trait)]
pub trait PendingActivationPort {
    async fn resolve_product_approval_context(
        &self,
        request: ResolveProductApprovalContextV1,
    ) -> Result<ResolvedProductApprovalContextV1, PendingActivationPortError>;

    async fn ensure_pending_activation(
        &self,
        request: EnsurePendingActivationV1,
    ) -> Result<PendingActivationReceiptV1, PendingActivationPortError>;

    async fn link_pending_activation(
        &self,
        request: LinkPendingActivationV1,
    ) -> Result<ActivationRequest, PendingActivationPortError>;
}
