use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use executor_core::{AdapterError, GuildStateReader};
use twilight_model::channel::permission_overwrite::{
    PermissionOverwrite as TwOverwrite, PermissionOverwriteType as TwOverwriteType,
};
use twilight_model::channel::{Channel as TwChannel, ChannelType as TwChannelType};
use twilight_model::guild::Role as TwRole;

use crate::adapter::TwilightDiscordAdapter;
use crate::convert::to_guild_id;
use crate::error::{classify_body_error, classify_error};

fn from_twilight_role(role: TwRole) -> Role {
    Role {
        id: RoleId(role.id.get()),
        name: role.name,
        permissions: Permissions::from_bits_retain(role.permissions.bits()),
        position: i32::try_from(role.position).unwrap_or(0),
        managed: role.managed,
    }
}

fn from_twilight_channel_type(kind: TwChannelType) -> ChannelType {
    match kind {
        TwChannelType::GuildVoice => ChannelType::Voice,
        TwChannelType::GuildCategory => ChannelType::Category,
        _ => ChannelType::Text,
    }
}

fn from_twilight_overwrite(overwrite: TwOverwrite) -> PermissionOverwrite {
    let target = match overwrite.kind {
        TwOverwriteType::Member => OverwriteTarget::Member(UserId(overwrite.id.get())),
        _ => OverwriteTarget::Role(RoleId(overwrite.id.get())),
    };
    PermissionOverwrite {
        target,
        allow: Permissions::from_bits_retain(overwrite.allow.bits()),
        deny: Permissions::from_bits_retain(overwrite.deny.bits()),
    }
}

fn from_twilight_channel(channel: TwChannel) -> Channel {
    Channel {
        id: ChannelId(channel.id.get()),
        name: channel.name.unwrap_or_default(),
        channel_type: from_twilight_channel_type(channel.kind),
        parent_id: channel.parent_id.map(|p| ChannelId(p.get())),
        position: channel.position.unwrap_or(0),
        overwrites: channel
            .permission_overwrites
            .unwrap_or_default()
            .into_iter()
            .map(from_twilight_overwrite)
            .collect(),
    }
}

impl GuildStateReader for TwilightDiscordAdapter {
    async fn read_guild_state(&self, guild_id: GuildId) -> Result<GuildState, AdapterError> {
        let tw_guild = to_guild_id(guild_id);
        let roles = self
            .http
            .roles(tw_guild)
            .await
            .map_err(|e| classify_error(&e))?
            .model()
            .await
            .map_err(|e| classify_body_error(&e))?;
        let channels = self
            .http
            .guild_channels(tw_guild)
            .await
            .map_err(|e| classify_error(&e))?
            .model()
            .await
            .map_err(|e| classify_body_error(&e))?;
        Ok(GuildState {
            guild: Guild {
                id: guild_id,
                name: String::new(),
                owner_id: UserId(0),
            },
            roles: roles.into_iter().map(from_twilight_role).collect(),
            channels: channels.into_iter().map(from_twilight_channel).collect(),
            members: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use twilight_model::id::Id;

    #[test]
    fn channel_type_maps_back() {
        assert_eq!(
            from_twilight_channel_type(TwChannelType::GuildText),
            ChannelType::Text
        );
        assert_eq!(
            from_twilight_channel_type(TwChannelType::GuildVoice),
            ChannelType::Voice
        );
        assert_eq!(
            from_twilight_channel_type(TwChannelType::GuildCategory),
            ChannelType::Category
        );
    }

    #[test]
    fn overwrite_role_and_member() {
        let role_ow = TwOverwrite {
            allow: twilight_model::guild::Permissions::from_bits_truncate(
                Permissions::VIEW_CHANNEL.bits(),
            ),
            deny: twilight_model::guild::Permissions::empty(),
            id: Id::new(7),
            kind: TwOverwriteType::Role,
        };
        let converted = from_twilight_overwrite(role_ow);
        assert_eq!(converted.target, OverwriteTarget::Role(RoleId(7)));
        assert_eq!(converted.allow.bits(), Permissions::VIEW_CHANNEL.bits());

        let member_ow = TwOverwrite {
            allow: twilight_model::guild::Permissions::empty(),
            deny: twilight_model::guild::Permissions::empty(),
            id: Id::new(9),
            kind: TwOverwriteType::Member,
        };
        assert_eq!(
            from_twilight_overwrite(member_ow).target,
            OverwriteTarget::Member(UserId(9))
        );
    }
}
