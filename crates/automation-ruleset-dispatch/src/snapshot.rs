use std::collections::{BTreeMap, BTreeSet};

use discord_model::{GuildId, Permissions, RoleId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildRoleSnapshot {
    pub roles: BTreeMap<RoleId, Permissions>,
    pub bot_role_ids: BTreeSet<RoleId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotError(String);

impl SnapshotError {
    pub fn new(detail: impl Into<String>) -> Self {
        SnapshotError(detail.into())
    }

    pub fn detail(&self) -> &str {
        &self.0
    }
}

#[allow(async_fn_in_trait)]
pub trait GuildRoleSnapshotProvider {
    async fn snapshot(&self, guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError>;
}
