use automation_core::ValidationError;
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};

use crate::key::RuleSetKey;
use crate::model::{RuleSetActivation, RuleSetVersion};
use crate::version::RuleSetVersionId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishRuleSetRequest {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub definition: InteractionRuleSet,
    pub created_by: UserId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishOutcome {
    Created(RuleSetVersion),
    Reused(RuleSetVersion),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleSetStoreError {
    InvalidDefinition(Vec<ValidationError>),
    VersionNotFound,
    VersionOverflow,
    HashCollision,
    Canonicalization(String),
    Backend(String),
}

#[allow(async_fn_in_trait)]
pub trait RuleSetStore {
    async fn publish(
        &self,
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError>;

    async fn get_version(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError>;

    async fn list_versions(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError>;

    async fn activate(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError>;

    async fn active(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError>;
}
