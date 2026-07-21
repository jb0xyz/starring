use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use automation_ruleset_dispatch::{GuildRoleSnapshot, GuildRoleSnapshotProvider, SnapshotError};
use discord_model::{GuildId, Permissions, RoleId};
use twilight_http::Client;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;

pub struct TwilightGuildRoleSnapshotProvider<'a> {
    http: &'a Client,
    bot_user_id: Id<UserMarker>,
}

#[derive(Clone)]
pub struct OwnedTwilightGuildRoleSnapshotProvider {
    http: Arc<Client>,
    bot_user_id: Id<UserMarker>,
}

impl<'a> TwilightGuildRoleSnapshotProvider<'a> {
    pub async fn new(http: &'a Client) -> Result<Self, SnapshotError> {
        let bot_user_id = fetch_bot_user_id(http).await?;
        Ok(Self { http, bot_user_id })
    }
}

impl OwnedTwilightGuildRoleSnapshotProvider {
    pub async fn new(http: Arc<Client>) -> Result<Self, SnapshotError> {
        let bot_user_id = fetch_bot_user_id(&http).await?;
        Ok(Self { http, bot_user_id })
    }
}

async fn fetch_bot_user_id(http: &Client) -> Result<Id<UserMarker>, SnapshotError> {
    let bot = http
        .current_user()
        .await
        .map_err(|error| SnapshotError::new(format!("current user fetch failed: {error}")))?
        .model()
        .await
        .map_err(|error| SnapshotError::new(format!("current user decode failed: {error}")))?;
    Ok(bot.id)
}

fn to_twilight_guild(guild_id: GuildId) -> Id<GuildMarker> {
    Id::new(guild_id.0)
}

async fn snapshot_with_client(
    http: &Client,
    bot_user_id: Id<UserMarker>,
    guild_id: GuildId,
) -> Result<GuildRoleSnapshot, SnapshotError> {
    let roles = http
        .roles(to_twilight_guild(guild_id))
        .await
        .map_err(|error| SnapshotError::new(format!("roles fetch failed: {error}")))?
        .model()
        .await
        .map_err(|error| SnapshotError::new(format!("roles decode failed: {error}")))?;
    let mut role_map = BTreeMap::new();
    for role in &roles {
        role_map.insert(
            RoleId(role.id.get()),
            Permissions::from_bits_retain(role.permissions.bits()),
        );
    }
    let member = http
        .guild_member(to_twilight_guild(guild_id), bot_user_id)
        .await
        .map_err(|error| SnapshotError::new(format!("member fetch failed: {error}")))?
        .model()
        .await
        .map_err(|error| SnapshotError::new(format!("member decode failed: {error}")))?;
    let bot_role_ids = member
        .roles
        .iter()
        .map(|id| RoleId(id.get()))
        .collect::<BTreeSet<_>>();
    Ok(GuildRoleSnapshot {
        roles: role_map,
        bot_role_ids,
    })
}

impl GuildRoleSnapshotProvider for TwilightGuildRoleSnapshotProvider<'_> {
    async fn snapshot(&self, guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError> {
        snapshot_with_client(self.http, self.bot_user_id, guild_id).await
    }
}

impl GuildRoleSnapshotProvider for OwnedTwilightGuildRoleSnapshotProvider {
    async fn snapshot(&self, guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError> {
        snapshot_with_client(&self.http, self.bot_user_id, guild_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::OwnedTwilightGuildRoleSnapshotProvider;

    fn assert_clone_send_sync<T: Clone + Send + Sync>() {}

    #[test]
    fn owned_provider_is_clone_send_and_sync() {
        assert_clone_send_sync::<OwnedTwilightGuildRoleSnapshotProvider>();
    }
}
