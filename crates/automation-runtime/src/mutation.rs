use automation_core::{AdapterError, DiscordMutationAdapter};
use discord_model::{GuildId, RoleId, UserId};
use twilight_http::Client;
use twilight_model::id::Id;

use crate::error::classify_error;

pub struct TwilightMutationAdapter<'a> {
    http: &'a Client,
}

impl<'a> TwilightMutationAdapter<'a> {
    pub fn new(http: &'a Client) -> Self {
        Self { http }
    }
}

impl DiscordMutationAdapter for TwilightMutationAdapter<'_> {
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError> {
        self.http
            .add_guild_member_role(Id::new(guild.0), Id::new(member.0), Id::new(role.0))
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }
}
