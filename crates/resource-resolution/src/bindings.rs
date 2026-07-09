use std::collections::BTreeMap;

use desired_state::ResourceKey;
use discord_model::{ChannelId, RoleId};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceBindingMap {
    pub role_bindings: BTreeMap<ResourceKey, RoleId>,
    pub channel_bindings: BTreeMap<ResourceKey, ChannelId>,
}
