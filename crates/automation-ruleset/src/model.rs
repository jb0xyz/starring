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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSetVersionIdentity {
    pub version: RuleSetVersionId,
    pub content_hash: RuleSetContentHash,
}

impl From<&RuleSetVersion> for RuleSetVersionIdentity {
    fn from(version: &RuleSetVersion) -> Self {
        Self {
            version: version.version,
            content_hash: version.content_hash,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedActiveRuleSet {
    Absent,
    Exact { identity: RuleSetVersionIdentity },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardedRuleSetActivation {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub target: RuleSetVersionIdentity,
    pub expected_active: ExpectedActiveRuleSet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuardedActivationOutcome {
    Activated(RuleSetActivation),
    AlreadyTarget(RuleSetActivation),
    BaselineMismatch {
        observed_active: Option<RuleSetVersionIdentity>,
    },
}
