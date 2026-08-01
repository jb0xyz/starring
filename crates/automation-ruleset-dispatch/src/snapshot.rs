use std::collections::{BTreeMap, BTreeSet};

use automation_core::preflight::{ActionPlanSnapshotRequestV1, ActionPlanSnapshotV1};
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

impl GuildRoleSnapshot {
    pub fn from_action_plan_snapshot_v1(
        snapshot: &ActionPlanSnapshotV1,
    ) -> Result<Self, SnapshotError> {
        let roles = snapshot
            .roles
            .as_ref()
            .ok_or_else(|| SnapshotError::new("guild role evidence is unavailable"))?
            .iter()
            .map(|role| (role.id, role.permissions))
            .collect();
        let bot_role_ids = snapshot
            .bot_member
            .as_ref()
            .ok_or_else(|| SnapshotError::new("bot member evidence is unavailable"))?
            .roles
            .iter()
            .copied()
            .collect();
        Ok(Self {
            roles,
            bot_role_ids,
        })
    }
}

#[allow(async_fn_in_trait)]
pub trait GuildRoleSnapshotProvider {
    async fn snapshot(&self, guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError>;

    async fn action_plan_snapshot_v1(
        &self,
        _request: &ActionPlanSnapshotRequestV1,
    ) -> Result<ActionPlanSnapshotV1, SnapshotError> {
        Err(SnapshotError::new(
            "complete action plan snapshot is unavailable",
        ))
    }
}
