use std::fmt::{Debug, Formatter};
use std::num::NonZeroU32;

use automation_ruleset::{
    PublishOutcome, PublishRuleSetRequest, RuleSetContentHash, RuleSetKey, RuleSetSchemaVersion,
    RuleSetStore, RuleSetStoreError, RuleSetVersionId,
};
use automation_ruleset_activation::{ActivationRequest, ActivationRequestId, ActivationTarget};
use automation_state::InteractionRuleSet;
use chrono::Duration;
use discord_model::{GuildId, UserId};

use crate::PendingActivationDispositionV1;

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
pub struct EnsurePendingActivationV1 {
    pub id: ActivationRequestId,
    pub target: ActivationTarget,
    pub requester: UserId,
    pub required_approvals: NonZeroU32,
    pub ttl: Duration,
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
    async fn ensure_pending_activation(
        &self,
        request: EnsurePendingActivationV1,
    ) -> Result<PendingActivationReceiptV1, PendingActivationPortError>;
}
