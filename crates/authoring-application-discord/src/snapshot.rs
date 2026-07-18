use std::num::NonZeroU64;

use authoring_promotion::{AutomationInstallationId, TenantId};
use discord_model::{GuildId, Permissions, RoleId, UserId};

use crate::DiscordApplicationIdV1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationAuthorityRecordV1 {
    pub tenant_id: TenantId,
    pub installation_id: AutomationInstallationId,
    pub application_id: DiscordApplicationIdV1,
    pub guild_id: GuildId,
    pub acting_user_id: UserId,
    pub authority_revision: NonZeroU64,
    pub authority_digest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscordRoleSnapshotV1 {
    pub role_id: RoleId,
    pub permissions: Permissions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscordGuildAuthoritySnapshotV1 {
    pub guild_id: GuildId,
    pub owner_id: UserId,
    pub member_user_id: UserId,
    pub member_is_bot: bool,
    pub member_is_system: bool,
    pub member_pending: bool,
    pub member_role_ids: Vec<RoleId>,
    pub roles: Vec<DiscordRoleSnapshotV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiscordAuthorityClientError {
    #[error("Discord authority dependency is unavailable")]
    Unavailable,
    #[error("Discord authority request timed out")]
    Timeout,
    #[error("Discord returned an invalid authority response")]
    InvalidResponse,
    #[error("Discord guild or member is inaccessible")]
    Inaccessible,
}

#[allow(async_fn_in_trait)]
pub trait DiscordGuildAuthorityClient {
    fn application_id(&self) -> DiscordApplicationIdV1;

    async fn fetch_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError>;
}
