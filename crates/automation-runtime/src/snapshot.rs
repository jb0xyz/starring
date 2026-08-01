use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use automation_core::preflight::{
    ActionPlanSnapshotIdentityV1, ActionPlanSnapshotRequestV1, ActionPlanSnapshotV1,
};
use automation_ruleset_dispatch::{GuildRoleSnapshot, GuildRoleSnapshotProvider, SnapshotError};
use discord_model::{
    Channel, ChannelId, ChannelType, GuildId, Member, OverwriteTarget, PermissionOverwrite,
    Permissions, Role, RoleId, UserId,
};
use sha2::{Digest, Sha256};
use twilight_http::Client;
use twilight_model::channel::permission_overwrite::{
    PermissionOverwrite as TwilightPermissionOverwrite,
    PermissionOverwriteType as TwilightPermissionOverwriteType,
};
use twilight_model::channel::{Channel as TwilightChannel, ChannelType as TwilightChannelType};
use twilight_model::guild::{Member as TwilightMember, Role as TwilightRole};
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

fn to_twilight_user(user_id: UserId) -> Id<UserMarker> {
    Id::new(user_id.0)
}

fn from_twilight_role(role: TwilightRole) -> Role {
    Role {
        id: RoleId(role.id.get()),
        name: role.name,
        permissions: Permissions::from_bits_retain(role.permissions.bits()),
        position: i32::try_from(role.position).unwrap_or(i32::MIN),
        managed: role.managed,
    }
}

fn from_twilight_channel_type(channel_type: TwilightChannelType) -> ChannelType {
    match channel_type {
        TwilightChannelType::GuildText | TwilightChannelType::GuildAnnouncement => {
            ChannelType::Text
        }
        TwilightChannelType::GuildVoice | TwilightChannelType::GuildStageVoice => {
            ChannelType::Voice
        }
        _ => ChannelType::Category,
    }
}

fn from_twilight_overwrite(overwrite: TwilightPermissionOverwrite) -> PermissionOverwrite {
    let target = match overwrite.kind {
        TwilightPermissionOverwriteType::Member => {
            OverwriteTarget::Member(UserId(overwrite.id.get()))
        }
        _ => OverwriteTarget::Role(RoleId(overwrite.id.get())),
    };
    PermissionOverwrite {
        target,
        allow: Permissions::from_bits_retain(overwrite.allow.bits()),
        deny: Permissions::from_bits_retain(overwrite.deny.bits()),
    }
}

fn from_twilight_channel(channel: TwilightChannel) -> Channel {
    Channel {
        id: ChannelId(channel.id.get()),
        name: channel.name.unwrap_or_default(),
        channel_type: from_twilight_channel_type(channel.kind),
        parent_id: channel.parent_id.map(|id| ChannelId(id.get())),
        position: channel.position.unwrap_or(0),
        overwrites: channel
            .permission_overwrites
            .unwrap_or_default()
            .into_iter()
            .map(from_twilight_overwrite)
            .collect(),
    }
}

fn from_twilight_member(member: TwilightMember) -> Member {
    Member {
        user_id: UserId(member.user.id.get()),
        roles: member
            .roles
            .into_iter()
            .map(|id| RoleId(id.get()))
            .collect(),
    }
}

struct SnapshotIdentityFrameV1 {
    bytes: Vec<u8>,
}

impl SnapshotIdentityFrameV1 {
    fn new(guild_id: GuildId) -> Self {
        let mut frame = Self {
            bytes: Vec::with_capacity(16_384),
        };
        frame.field(1, b"starring.runtime.action_plan_snapshot.v1\0");
        frame.field(2, &guild_id.0.to_be_bytes());
        frame
    }

    fn field(&mut self, tag: u16, value: &[u8]) {
        self.bytes.extend_from_slice(&tag.to_be_bytes());
        self.bytes.extend_from_slice(
            &u64::try_from(value.len())
                .expect("snapshot field length fits u64")
                .to_be_bytes(),
        );
        self.bytes.extend_from_slice(value);
    }

    fn finish(self) -> String {
        let digest = Sha256::digest(self.bytes);
        let mut output = String::with_capacity(64);
        for byte in digest {
            use std::fmt::Write;
            write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
        }
        output
    }
}

fn snapshot_identity_v1(
    guild_id: GuildId,
    roles: &[Role],
    channels: &[Channel],
    bot_member: &Member,
    actor_member: &Member,
) -> Result<ActionPlanSnapshotIdentityV1, SnapshotError> {
    let mut frame = SnapshotIdentityFrameV1::new(guild_id);
    let mut roles = roles.to_vec();
    roles.sort_by_key(|role| role.id);
    for role in roles {
        frame.field(10, &role.id.0.to_be_bytes());
        frame.field(11, role.name.as_bytes());
        frame.field(12, &role.permissions.bits().to_be_bytes());
        frame.field(13, &role.position.to_be_bytes());
        frame.field(14, &[u8::from(role.managed)]);
    }
    let mut channels = channels.to_vec();
    channels.sort_by_key(|channel| channel.id);
    for mut channel in channels {
        frame.field(20, &channel.id.0.to_be_bytes());
        frame.field(21, channel.name.as_bytes());
        let channel_type = match channel.channel_type {
            ChannelType::Text => 1,
            ChannelType::Voice => 2,
            ChannelType::Category => 3,
        };
        frame.field(22, &[channel_type]);
        frame.field(
            23,
            &channel.parent_id.map_or(0_u64, |id| id.0).to_be_bytes(),
        );
        frame.field(24, &channel.position.to_be_bytes());
        channel
            .overwrites
            .sort_by_key(|overwrite| match overwrite.target {
                OverwriteTarget::Role(id) => (0_u8, id.0),
                OverwriteTarget::Member(id) => (1_u8, id.0),
            });
        for overwrite in channel.overwrites {
            match overwrite.target {
                OverwriteTarget::Role(id) => {
                    frame.field(25, &[0]);
                    frame.field(26, &id.0.to_be_bytes());
                }
                OverwriteTarget::Member(id) => {
                    frame.field(25, &[1]);
                    frame.field(26, &id.0.to_be_bytes());
                }
            }
            frame.field(27, &overwrite.allow.bits().to_be_bytes());
            frame.field(28, &overwrite.deny.bits().to_be_bytes());
        }
    }
    append_member_identity_v1(&mut frame, 30, bot_member);
    append_member_identity_v1(&mut frame, 40, actor_member);
    ActionPlanSnapshotIdentityV1::new(frame.finish())
        .map_err(|error| SnapshotError::new(error.to_string()))
}

fn append_member_identity_v1(frame: &mut SnapshotIdentityFrameV1, tag: u16, member: &Member) {
    frame.field(tag, &member.user_id.0.to_be_bytes());
    let mut roles = member.roles.clone();
    roles.sort_unstable();
    for role in roles {
        frame.field(tag + 1, &role.0.to_be_bytes());
    }
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

async fn action_plan_snapshot_with_client(
    http: &Client,
    bot_user_id: Id<UserMarker>,
    request: &ActionPlanSnapshotRequestV1,
) -> Result<ActionPlanSnapshotV1, SnapshotError> {
    let guild_id = request.guild_id();
    let guild = to_twilight_guild(guild_id);
    let actor = to_twilight_user(request.actor());
    let (roles_response, channels_response, bot_response, actor_response) = tokio::join!(
        http.roles(guild),
        http.guild_channels(guild),
        http.guild_member(guild, bot_user_id),
        http.guild_member(guild, actor),
    );
    let roles_response = roles_response
        .map_err(|error| SnapshotError::new(format!("roles fetch failed: {error}")))?;
    let channels_response = channels_response
        .map_err(|error| SnapshotError::new(format!("channels fetch failed: {error}")))?;
    let bot_response = bot_response
        .map_err(|error| SnapshotError::new(format!("bot member fetch failed: {error}")))?;
    let actor_response = actor_response
        .map_err(|error| SnapshotError::new(format!("actor member fetch failed: {error}")))?;
    let (roles, channels, bot_member, actor_member) = tokio::join!(
        roles_response.model(),
        channels_response.model(),
        bot_response.model(),
        actor_response.model(),
    );
    let roles = roles
        .map_err(|error| SnapshotError::new(format!("roles decode failed: {error}")))?
        .into_iter()
        .map(from_twilight_role)
        .collect::<Vec<_>>();
    let channels = channels
        .map_err(|error| SnapshotError::new(format!("channels decode failed: {error}")))?
        .into_iter()
        .map(from_twilight_channel)
        .collect::<Vec<_>>();
    let bot_member = from_twilight_member(
        bot_member
            .map_err(|error| SnapshotError::new(format!("bot member decode failed: {error}")))?,
    );
    let actor_member = from_twilight_member(
        actor_member
            .map_err(|error| SnapshotError::new(format!("actor member decode failed: {error}")))?,
    );
    if bot_member.user_id.0 != bot_user_id.get() || actor_member.user_id != request.actor() {
        return Err(SnapshotError::new("member snapshot identity mismatch"));
    }
    let identity = snapshot_identity_v1(guild_id, &roles, &channels, &bot_member, &actor_member)?;
    Ok(ActionPlanSnapshotV1 {
        guild_id,
        identity,
        roles: Some(roles),
        channels: Some(channels),
        bot_member: Some(bot_member),
        actor_member: Some(actor_member),
    })
}

impl GuildRoleSnapshotProvider for TwilightGuildRoleSnapshotProvider<'_> {
    async fn snapshot(&self, guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError> {
        snapshot_with_client(self.http, self.bot_user_id, guild_id).await
    }

    async fn action_plan_snapshot_v1(
        &self,
        request: &ActionPlanSnapshotRequestV1,
    ) -> Result<ActionPlanSnapshotV1, SnapshotError> {
        action_plan_snapshot_with_client(self.http, self.bot_user_id, request).await
    }
}

impl GuildRoleSnapshotProvider for OwnedTwilightGuildRoleSnapshotProvider {
    async fn snapshot(&self, guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError> {
        snapshot_with_client(&self.http, self.bot_user_id, guild_id).await
    }

    async fn action_plan_snapshot_v1(
        &self,
        request: &ActionPlanSnapshotRequestV1,
    ) -> Result<ActionPlanSnapshotV1, SnapshotError> {
        action_plan_snapshot_with_client(&self.http, self.bot_user_id, request).await
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
