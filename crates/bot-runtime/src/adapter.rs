use discord_model::{ChannelId, GuildId, OverwriteTarget, Permissions, RoleId};
use executor_core::{AdapterError, ChannelSpec, DiscordAdapter, RoleSpec};
use twilight_http::Client;

use crate::convert::{
    to_channel_id, to_guild_id, to_permission_overwrite, to_role_id, to_twilight_channel_type,
    to_twilight_permissions,
};
use crate::error::{classify_body_error, classify_error};

pub struct TwilightDiscordAdapter {
    pub(crate) http: Client,
}

impl TwilightDiscordAdapter {
    pub fn new(token: String) -> Self {
        Self {
            http: Client::new(token),
        }
    }

    pub fn from_client(http: Client) -> Self {
        Self { http }
    }
}

impl DiscordAdapter for TwilightDiscordAdapter {
    async fn create_role(&self, guild: GuildId, spec: RoleSpec) -> Result<RoleId, AdapterError> {
        let mut req = self.http.create_role(to_guild_id(guild));
        if let Some(name) = &spec.name {
            req = req.name(name.as_str());
        }
        if let Some(perms) = spec.permissions {
            req = req.permissions(to_twilight_permissions(perms));
        }
        let role = req
            .await
            .map_err(|e| classify_error(&e))?
            .model()
            .await
            .map_err(|e| classify_body_error(&e))?;
        Ok(RoleId(role.id.get()))
    }

    async fn update_role(
        &self,
        guild: GuildId,
        id: RoleId,
        spec: RoleSpec,
    ) -> Result<(), AdapterError> {
        let mut req = self.http.update_role(to_guild_id(guild), to_role_id(id));
        if let Some(name) = &spec.name {
            req = req.name(Some(name.as_str()));
        }
        if let Some(perms) = spec.permissions {
            req = req.permissions(to_twilight_permissions(perms));
        }
        req.await.map_err(|e| classify_error(&e))?;
        Ok(())
    }

    async fn delete_role(&self, guild: GuildId, id: RoleId) -> Result<(), AdapterError> {
        self.http
            .delete_role(to_guild_id(guild), to_role_id(id))
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(())
    }

    async fn create_channel(
        &self,
        guild: GuildId,
        spec: ChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        let name = spec.name.as_deref().unwrap_or_default();
        let mut req = self.http.create_guild_channel(to_guild_id(guild), name);
        if let Some(channel_type) = spec.channel_type {
            req = req.kind(to_twilight_channel_type(channel_type));
        }
        if let Some(parent) = spec.parent_id {
            req = req.parent_id(to_channel_id(parent));
        }
        let channel = req
            .await
            .map_err(|e| classify_error(&e))?
            .model()
            .await
            .map_err(|e| classify_body_error(&e))?;
        Ok(ChannelId(channel.id.get()))
    }

    async fn update_channel(
        &self,
        _guild: GuildId,
        id: ChannelId,
        spec: ChannelSpec,
    ) -> Result<(), AdapterError> {
        let mut req = self.http.update_channel(to_channel_id(id));
        if let Some(name) = &spec.name {
            req = req.name(name.as_str());
        }
        if let Some(channel_type) = spec.channel_type {
            req = req.kind(to_twilight_channel_type(channel_type));
        }
        req.await.map_err(|e| classify_error(&e))?;
        Ok(())
    }

    async fn delete_channel(&self, _guild: GuildId, id: ChannelId) -> Result<(), AdapterError> {
        self.http
            .delete_channel(to_channel_id(id))
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(())
    }

    async fn upsert_overwrite(
        &self,
        _guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError> {
        let overwrite = to_permission_overwrite(target, allow, deny);
        self.http
            .update_channel_permission(to_channel_id(channel), &overwrite)
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(())
    }
}
