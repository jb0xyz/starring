use std::num::NonZeroU64;

use authoring_promotion::{AutomationInstallationId, TenantId};
use discord_model::{GuildId, Permissions, RoleId, UserId};

use crate::{DiscordApplicationIdV1, DiscordBotUserIdV1};

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
    pub position: i64,
    pub managed: bool,
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

#[derive(Clone, PartialEq, Eq)]
pub struct DiscordGuildApplyAuthoritySnapshotV1 {
    pub authority: DiscordGuildAuthoritySnapshotV1,
    pub bot_member_user_id: UserId,
    pub bot_member_is_bot: bool,
    pub bot_member_is_system: bool,
    pub bot_member_pending: bool,
    pub bot_member_role_ids: Vec<RoleId>,
}

impl std::fmt::Debug for DiscordGuildApplyAuthoritySnapshotV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiscordGuildApplyAuthoritySnapshotV1")
            .field("authority", &"<redacted>")
            .field("bot_member", &"<redacted>")
            .field("bot_role_count", &self.bot_member_role_ids.len())
            .finish()
    }
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
    #[error("Discord bot credential identity does not match the configured application")]
    BotIdentityMismatch,
    #[error("Discord bot credential was rejected")]
    BotCredentialRejected,
    #[error("Discord bot installation is inaccessible")]
    BotInstallationInaccessible,
    #[error("Discord bot member is inaccessible")]
    BotMemberInaccessible,
}

#[allow(async_fn_in_trait)]
pub trait DiscordGuildAuthorityClient {
    fn application_id(&self) -> DiscordApplicationIdV1;

    fn bot_user_id(&self) -> Option<DiscordBotUserIdV1> {
        None
    }

    async fn fetch_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError>;

    async fn fetch_apply_authority_snapshot(
        &self,
        _guild_id: GuildId,
        _user_id: UserId,
    ) -> Result<DiscordGuildApplyAuthoritySnapshotV1, DiscordAuthorityClientError> {
        Err(DiscordAuthorityClientError::InvalidResponse)
    }
}
