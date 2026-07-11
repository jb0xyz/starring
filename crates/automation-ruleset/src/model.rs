use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};
use serde::{Deserialize, Serialize};

use crate::hash::RuleSetContentHash;
use crate::key::RuleSetKey;
use crate::version::{RuleSetSchemaVersion, RuleSetVersionId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSetVersion {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub schema_version: RuleSetSchemaVersion,
    pub definition: InteractionRuleSet,
    pub content_hash: RuleSetContentHash,
    pub created_by: UserId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSetActivation {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub active_version: RuleSetVersionId,
}
