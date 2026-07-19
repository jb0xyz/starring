use std::sync::Arc;

use discord_model::{GuildId, Permissions, RoleId, UserId};
use twilight_http::Client;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;

use crate::{
    DiscordApplicationIdV1, DiscordAuthorityClientError, DiscordBotUserIdV1,
    DiscordGuildApplyAuthoritySnapshotV1, DiscordGuildAuthorityClient,
    DiscordGuildAuthoritySnapshotV1, DiscordRoleSnapshotV1,
};

pub struct TwilightDiscordGuildAuthorityClient {
    http: Arc<Client>,
    application_id: DiscordApplicationIdV1,
    bot_user_id: DiscordBotUserIdV1,
}

impl TwilightDiscordGuildAuthorityClient {
    pub fn new(
        http: Arc<Client>,
        application_id: DiscordApplicationIdV1,
        bot_user_id: DiscordBotUserIdV1,
    ) -> Self {
        Self {
            http,
            application_id,
            bot_user_id,
        }
    }
}

impl DiscordGuildAuthorityClient for TwilightDiscordGuildAuthorityClient {
    fn application_id(&self) -> DiscordApplicationIdV1 {
        self.application_id
    }

    fn bot_user_id(&self) -> Option<DiscordBotUserIdV1> {
        Some(self.bot_user_id)
    }

    async fn fetch_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError> {
        let twilight_guild_id = Id::<GuildMarker>::new(guild_id.0);
        let twilight_user_id = Id::<UserMarker>::new(user_id.0);
        let (application_response, guild_response, member_response) = tokio::join!(
            self.http.current_user_application(),
            self.http.guild(twilight_guild_id),
            self.http.guild_member(twilight_guild_id, twilight_user_id),
        );
        let application_response = application_response
            .map_err(|error| classify_request_error(error, AuthorityEndpoint::Application))?;
        let guild_response = guild_response
            .map_err(|error| classify_request_error(error, AuthorityEndpoint::Guild))?;
        let member_response = member_response
            .map_err(|error| classify_request_error(error, AuthorityEndpoint::ActorMember))?;
        let (application, guild, member) = tokio::join!(
            application_response.model(),
            guild_response.model(),
            member_response.model()
        );
        let application = application.map_err(|_| DiscordAuthorityClientError::InvalidResponse)?;
        let guild = guild.map_err(|_| DiscordAuthorityClientError::InvalidResponse)?;
        let member = member.map_err(|_| DiscordAuthorityClientError::InvalidResponse)?;
        validate_bot_application(&application, self.application_id, self.bot_user_id)?;
        if guild.id.get() != guild_id.0 || member.user.id.get() != user_id.0 {
            return Err(DiscordAuthorityClientError::InvalidResponse);
        }
        let roles = guild
            .roles
            .into_iter()
            .map(|role| DiscordRoleSnapshotV1 {
                role_id: RoleId(role.id.get()),
                permissions: Permissions::from_bits_retain(role.permissions.bits()),
                position: role.position,
                managed: role.managed,
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

    async fn fetch_apply_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildApplyAuthoritySnapshotV1, DiscordAuthorityClientError> {
        let twilight_guild_id = Id::<GuildMarker>::new(guild_id.0);
        let twilight_user_id = Id::<UserMarker>::new(user_id.0);
        let bot_user_id = self.bot_user_id.to_user_id();
        let twilight_bot_user_id = Id::<UserMarker>::new(bot_user_id.0);
        let (application_response, guild_response, member_response, bot_member_response) = tokio::join!(
            self.http.current_user_application(),
            self.http.guild(twilight_guild_id),
            self.http.guild_member(twilight_guild_id, twilight_user_id),
            self.http
                .guild_member(twilight_guild_id, twilight_bot_user_id),
        );
        let application_response = application_response
            .map_err(|error| classify_request_error(error, AuthorityEndpoint::Application))?;
        let guild_response = guild_response
            .map_err(|error| classify_request_error(error, AuthorityEndpoint::Guild))?;
        let member_response = member_response
            .map_err(|error| classify_request_error(error, AuthorityEndpoint::ActorMember))?;
        let bot_member_response = bot_member_response
            .map_err(|error| classify_request_error(error, AuthorityEndpoint::BotMember))?;
        let (application, guild, member, bot_member) = tokio::join!(
            application_response.model(),
            guild_response.model(),
            member_response.model(),
            bot_member_response.model(),
        );
        let application = application.map_err(|_| DiscordAuthorityClientError::InvalidResponse)?;
        let guild = guild.map_err(|_| DiscordAuthorityClientError::InvalidResponse)?;
        let member = member.map_err(|_| DiscordAuthorityClientError::InvalidResponse)?;
        let bot_member = bot_member.map_err(|_| DiscordAuthorityClientError::InvalidResponse)?;
        validate_bot_application(&application, self.application_id, self.bot_user_id)?;
        if guild.id.get() != guild_id.0
            || member.user.id.get() != user_id.0
            || bot_member.user.id.get() != bot_user_id.0
        {
            return Err(DiscordAuthorityClientError::InvalidResponse);
        }
        let roles = guild
            .roles
            .into_iter()
            .map(|role| DiscordRoleSnapshotV1 {
                role_id: RoleId(role.id.get()),
                permissions: Permissions::from_bits_retain(role.permissions.bits()),
                position: role.position,
                managed: role.managed,
            })
            .collect();
        let member_role_ids = member
            .roles
            .into_iter()
            .map(|role_id| RoleId(role_id.get()))
            .collect();
        let bot_member_role_ids = bot_member
            .roles
            .into_iter()
            .map(|role_id| RoleId(role_id.get()))
            .collect();
        Ok(DiscordGuildApplyAuthoritySnapshotV1 {
            authority: DiscordGuildAuthoritySnapshotV1 {
                guild_id,
                owner_id: UserId(guild.owner_id.get()),
                member_user_id: user_id,
                member_is_bot: member.user.bot,
                member_is_system: member.user.system.unwrap_or(false),
                member_pending: member.pending,
                member_role_ids,
                roles,
            },
            bot_member_user_id: bot_user_id,
            bot_member_is_bot: bot_member.user.bot,
            bot_member_is_system: bot_member.user.system.unwrap_or(false),
            bot_member_pending: bot_member.pending,
            bot_member_role_ids,
        })
    }
}

#[derive(Clone, Copy)]
enum AuthorityEndpoint {
    Application,
    Guild,
    ActorMember,
    BotMember,
}

fn classify_request_error(
    error: twilight_http::Error,
    endpoint: AuthorityEndpoint,
) -> DiscordAuthorityClientError {
    match error.kind() {
        twilight_http::error::ErrorType::Response { status, .. } => {
            classify_response_status(endpoint, status.get())
        }
        twilight_http::error::ErrorType::RequestTimedOut => DiscordAuthorityClientError::Timeout,
        _ => DiscordAuthorityClientError::Unavailable,
    }
}

fn classify_response_status(
    endpoint: AuthorityEndpoint,
    status: u16,
) -> DiscordAuthorityClientError {
    match (endpoint, status) {
        (_, 401) => DiscordAuthorityClientError::BotCredentialRejected,
        (AuthorityEndpoint::Application, 403 | 404) => {
            DiscordAuthorityClientError::BotCredentialRejected
        }
        (AuthorityEndpoint::Guild, 403 | 404) | (AuthorityEndpoint::ActorMember, 403) => {
            DiscordAuthorityClientError::BotInstallationInaccessible
        }
        (AuthorityEndpoint::ActorMember, 404) => DiscordAuthorityClientError::Inaccessible,
        (AuthorityEndpoint::BotMember, 403 | 404) => {
            DiscordAuthorityClientError::BotMemberInaccessible
        }
        _ => DiscordAuthorityClientError::Unavailable,
    }
}

fn validate_bot_application(
    application: &twilight_model::oauth::Application,
    expected_application_id: DiscordApplicationIdV1,
    expected_bot_user_id: DiscordBotUserIdV1,
) -> Result<(), DiscordAuthorityClientError> {
    let bot = application
        .bot
        .as_ref()
        .ok_or(DiscordAuthorityClientError::BotIdentityMismatch)?;
    if application.id.get() != expected_application_id.get()
        || bot.id.get() != expected_bot_user_id.get()
        || !bot.bot
        || bot.system.unwrap_or(false)
    {
        return Err(DiscordAuthorityClientError::BotIdentityMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_status_is_classified_by_authority_endpoint() {
        assert_eq!(
            classify_response_status(AuthorityEndpoint::Application, 401),
            DiscordAuthorityClientError::BotCredentialRejected
        );
        assert_eq!(
            classify_response_status(AuthorityEndpoint::Guild, 404),
            DiscordAuthorityClientError::BotInstallationInaccessible
        );
        assert_eq!(
            classify_response_status(AuthorityEndpoint::ActorMember, 404),
            DiscordAuthorityClientError::Inaccessible
        );
        assert_eq!(
            classify_response_status(AuthorityEndpoint::ActorMember, 403),
            DiscordAuthorityClientError::BotInstallationInaccessible
        );
        assert_eq!(
            classify_response_status(AuthorityEndpoint::BotMember, 404),
            DiscordAuthorityClientError::BotMemberInaccessible
        );
        assert_eq!(
            classify_response_status(AuthorityEndpoint::Guild, 429),
            DiscordAuthorityClientError::Unavailable
        );
    }
}
