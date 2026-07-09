use discord_model::{ChannelId, ChannelType, GuildId, OverwriteTarget, Permissions, RoleId};
use twilight_model::channel::ChannelType as TwilightChannelType;
use twilight_model::guild::Permissions as TwilightPermissions;
use twilight_model::http::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::id::marker::{ChannelMarker, GuildMarker, RoleMarker};
use twilight_model::id::Id;

pub fn to_guild_id(id: GuildId) -> Id<GuildMarker> {
    Id::new(id.0)
}

pub fn to_role_id(id: RoleId) -> Id<RoleMarker> {
    Id::new(id.0)
}

pub fn to_channel_id(id: ChannelId) -> Id<ChannelMarker> {
    Id::new(id.0)
}

pub fn to_twilight_permissions(permissions: Permissions) -> TwilightPermissions {
    TwilightPermissions::from_bits_truncate(permissions.bits())
}

pub fn to_twilight_channel_type(channel_type: ChannelType) -> TwilightChannelType {
    match channel_type {
        ChannelType::Text => TwilightChannelType::GuildText,
        ChannelType::Voice => TwilightChannelType::GuildVoice,
        ChannelType::Category => TwilightChannelType::GuildCategory,
    }
}

pub fn to_permission_overwrite(
    target: OverwriteTarget,
    allow: Permissions,
    deny: Permissions,
) -> PermissionOverwrite {
    let (raw_id, kind) = match target {
        OverwriteTarget::Role(role) => (role.0, PermissionOverwriteType::Role),
        OverwriteTarget::Member(user) => (user.0, PermissionOverwriteType::Member),
    };
    PermissionOverwrite {
        allow: Some(to_twilight_permissions(allow)),
        deny: Some(to_twilight_permissions(deny)),
        id: Id::new(raw_id),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::{ChannelId, ChannelType, GuildId, OverwriteTarget, Permissions, RoleId};
    use twilight_model::channel::ChannelType as TwilightChannelType;
    use twilight_model::http::permission_overwrite::PermissionOverwriteType;

    #[test]
    fn permissions_roundtrip() {
        let p = Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
        assert_eq!(to_twilight_permissions(p).bits(), p.bits());
    }

    #[test]
    fn channel_type_maps() {
        assert_eq!(
            to_twilight_channel_type(ChannelType::Text),
            TwilightChannelType::GuildText
        );
        assert_eq!(
            to_twilight_channel_type(ChannelType::Voice),
            TwilightChannelType::GuildVoice
        );
        assert_eq!(
            to_twilight_channel_type(ChannelType::Category),
            TwilightChannelType::GuildCategory
        );
    }

    #[test]
    fn ids_convert() {
        assert_eq!(to_role_id(RoleId(42)).get(), 42);
        assert_eq!(to_channel_id(ChannelId(500)).get(), 500);
        assert_eq!(to_guild_id(GuildId(1)).get(), 1);
    }

    #[test]
    fn overwrite_role_target() {
        let ow = to_permission_overwrite(
            OverwriteTarget::Role(RoleId(7)),
            Permissions::VIEW_CHANNEL,
            Permissions::empty(),
        );
        assert_eq!(ow.id.get(), 7);
        assert_eq!(ow.kind, PermissionOverwriteType::Role);
        assert_eq!(ow.allow.unwrap().bits(), Permissions::VIEW_CHANNEL.bits());
    }
}
