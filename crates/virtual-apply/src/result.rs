use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildState, RoleId};
use operation_graph::OpId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualApplyResult {
    pub after: GuildState,
    pub applied: Vec<OpId>,
    pub synthetic_roles: BTreeMap<ResourceKey, RoleId>,
    pub synthetic_channels: BTreeMap<ResourceKey, ChannelId>,
    pub warnings: Vec<String>,
}
