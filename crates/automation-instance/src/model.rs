use std::collections::BTreeMap;

use discord_model::{ChannelId, GuildId, MessageId, RoleId, UserId};
use serde::{Deserialize, Serialize};

use crate::id::InstanceId;
use crate::version::InstanceRuleSetVersion;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceKind(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceMessageRef {
    pub channel: ChannelId,
    pub id: MessageId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceResources {
    #[serde(default)]
    pub roles: BTreeMap<String, RoleId>,
    #[serde(default)]
    pub channels: BTreeMap<String, ChannelId>,
    #[serde(default)]
    pub messages: BTreeMap<String, InstanceMessageRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    Active,
    Deleting,
    Disabled,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationInstance {
    pub id: InstanceId,
    pub guild_id: GuildId,
    pub ruleset_key: String,
    pub ruleset_version: InstanceRuleSetVersion,
    pub kind: InstanceKind,
    pub created_by: UserId,
    pub resources: InstanceResources,
    pub status: InstanceStatus,
}
