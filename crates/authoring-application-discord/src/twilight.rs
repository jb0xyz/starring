use std::sync::Arc;

use discord_model::{GuildId, Permissions, RoleId, UserId};
use twilight_http::Client;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;

use crate::{
    DiscordApplicationIdV1, DiscordAuthorityClientError, DiscordGuildAuthorityClient,
    DiscordGuildAuthoritySnapshotV1, DiscordRoleSnapshotV1,
};

pub struct TwilightDiscordGuildAuthorityClient {
    http: Arc<Client>,
    application_id: DiscordApplicationIdV1,
}

impl TwilightDiscordGuildAuthorityClient {
    pub fn new(http: Arc<Client>, application_id: DiscordApplicationIdV1) -> Self {
        Self {
            http,
            application_id,
        }
    }
}

impl DiscordGuildAuthorityClient for TwilightDiscordGuildAuthorityClient {
    fn application_id(&self) -> DiscordApplicationIdV1 {
        self.application_id
    }

    async fn fetch_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError> {
        let twilight_guild_id = Id::<GuildMarker>::new(guild_id.0);
        let twilight_user_id = Id::<UserMarker>::new(user_id.0);
        let (guild_response, member_response) = tokio::join!(
            self.http.guild(twilight_guild_id),
            self.http.guild_member(twilight_guild_id, twilight_user_id),
        );
        let guild_response = guild_response.map_err(classify_request_error)?;
        let member_response = member_response.map_err(classify_request_error)?;
        let (guild, member) = tokio::join!(guild_response.model(), member_response.model());
        let guild = guild.map_err(|_| DiscordAuthorityClientError::InvalidResponse)?;
        let member = member.map_err(|_| DiscordAuthorityClientError::InvalidResponse)?;
        if guild.id.get() != guild_id.0 || member.user.id.get() != user_id.0 {
            return Err(DiscordAuthorityClientError::InvalidResponse);
        }
        let roles = guild
            .roles
            .into_iter()
            .map(|role| DiscordRoleSnapshotV1 {
                role_id: RoleId(role.id.get()),
                permissions: Permissions::from_bits_retain(role.permissions.bits()),
            })
            .collect();
        let member_role_ids = member
            .roles
            .into_iter()
            .map(|role_id| RoleId(role_id.get()))
            .collect();
        Ok(DiscordGuildAuthoritySnapshotV1 {
            guild_id,
            owner_id: UserId(guild.owner_id.get()),
            member_user_id: user_id,
            member_is_bot: member.user.bot,
            member_is_system: member.user.system.unwrap_or(false),
            member_pending: member.pending,
            member_role_ids,
            roles,
        })
    }
}

fn classify_request_error(error: twilight_http::Error) -> DiscordAuthorityClientError {
    match error.kind() {
        twilight_http::error::ErrorType::Response { status, .. }
            if matches!(status.get(), 401 | 403 | 404) =>
        {
            DiscordAuthorityClientError::Inaccessible
        }
        twilight_http::error::ErrorType::RequestTimedOut => DiscordAuthorityClientError::Timeout,
        _ => DiscordAuthorityClientError::Unavailable,
    }
}
