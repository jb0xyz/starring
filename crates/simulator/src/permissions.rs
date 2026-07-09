use discord_model::{
    Channel, GuildState, OverwriteTarget, PermissionOverwrite, Permissions, RoleId,
};

pub fn effective_permissions(
    guild: &GuildState,
    subject_roles: &[RoleId],
    channel: &Channel,
) -> Permissions {
    let everyone_id = RoleId(guild.guild.id.0);

    let mut base = role_permissions(guild, everyone_id);
    for rid in subject_roles {
        base |= role_permissions(guild, *rid);
    }
    if base.contains(Permissions::ADMINISTRATOR) {
        return Permissions::all();
    }

    let mut perms = base;
    if let Some(overwrite) = find_overwrite(channel, everyone_id) {
        perms = apply_overwrite(perms, overwrite.allow, overwrite.deny);
    }

    let mut allow_accum = Permissions::empty();
    let mut deny_accum = Permissions::empty();
    for rid in subject_roles {
        if let Some(overwrite) = find_overwrite(channel, *rid) {
            allow_accum |= overwrite.allow;
            deny_accum |= overwrite.deny;
        }
    }
    apply_overwrite(perms, allow_accum, deny_accum)
}

fn role_permissions(guild: &GuildState, id: RoleId) -> Permissions {
    guild
        .roles
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.permissions)
        .unwrap_or_else(Permissions::empty)
}

fn find_overwrite(channel: &Channel, role_id: RoleId) -> Option<&PermissionOverwrite> {
    channel
        .overwrites
        .iter()
        .find(|o| o.target == OverwriteTarget::Role(role_id))
}

fn apply_overwrite(perms: Permissions, allow: Permissions, deny: Permissions) -> Permissions {
    Permissions::from_bits_retain((perms.bits() & !deny.bits()) | allow.bits())
}

pub fn can_view(guild: &GuildState, subject_roles: &[RoleId], channel: &Channel) -> bool {
    effective_permissions(guild, subject_roles, channel).contains(Permissions::VIEW_CHANNEL)
}

pub fn can_send(guild: &GuildState, subject_roles: &[RoleId], channel: &Channel) -> bool {
    let perms = effective_permissions(guild, subject_roles, channel);
    perms.contains(Permissions::VIEW_CHANNEL) && perms.contains(Permissions::SEND_MESSAGES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::{
        Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
        PermissionOverwrite, Permissions, Role, RoleId, UserId,
    };

    fn guild(roles: Vec<Role>, channels: Vec<Channel>) -> GuildState {
        GuildState {
            guild: Guild {
                id: GuildId(1),
                name: "g".to_string(),
                owner_id: UserId(1),
            },
            roles,
            channels,
            members: vec![],
        }
    }

    fn role(id: u64, perms: Permissions) -> Role {
        Role {
            id: RoleId(id),
            name: format!("r{id}"),
            permissions: perms,
            position: 0,
            managed: false,
        }
    }

    fn channel(overwrites: Vec<PermissionOverwrite>) -> Channel {
        Channel {
            id: ChannelId(10),
            name: "c".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites,
        }
    }

    fn ow(role_id: u64, allow: Permissions, deny: Permissions) -> PermissionOverwrite {
        PermissionOverwrite {
            target: OverwriteTarget::Role(RoleId(role_id)),
            allow,
            deny,
        }
    }

    #[test]
    fn everyone_base_view() {
        let g = guild(vec![role(1, Permissions::VIEW_CHANNEL)], vec![]);
        let c = channel(vec![]);
        assert!(can_view(&g, &[], &c));
    }

    #[test]
    fn everyone_overwrite_deny_hides() {
        let g = guild(vec![role(1, Permissions::VIEW_CHANNEL)], vec![]);
        let c = channel(vec![ow(1, Permissions::empty(), Permissions::VIEW_CHANNEL)]);
        assert!(!can_view(&g, &[], &c));
    }

    #[test]
    fn role_allow_beats_everyone_deny() {
        let g = guild(
            vec![
                role(1, Permissions::VIEW_CHANNEL),
                role(100, Permissions::empty()),
            ],
            vec![],
        );
        let c = channel(vec![
            ow(1, Permissions::empty(), Permissions::VIEW_CHANNEL),
            ow(100, Permissions::VIEW_CHANNEL, Permissions::empty()),
        ]);
        assert!(!can_view(&g, &[], &c));
        assert!(can_view(&g, &[RoleId(100)], &c));
    }

    #[test]
    fn send_requires_view_and_send() {
        let g = guild(
            vec![
                role(1, Permissions::empty()),
                role(100, Permissions::empty()),
            ],
            vec![],
        );
        let with_send = channel(vec![ow(
            100,
            Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
            Permissions::empty(),
        )]);
        assert!(can_send(&g, &[RoleId(100)], &with_send));
        let view_only = channel(vec![ow(
            100,
            Permissions::VIEW_CHANNEL,
            Permissions::empty(),
        )]);
        assert!(!can_send(&g, &[RoleId(100)], &view_only));
    }

    #[test]
    fn administrator_bypasses_overwrites() {
        let g = guild(
            vec![
                role(1, Permissions::VIEW_CHANNEL),
                role(200, Permissions::ADMINISTRATOR),
            ],
            vec![],
        );
        let c = channel(vec![ow(1, Permissions::empty(), Permissions::VIEW_CHANNEL)]);
        assert!(can_view(&g, &[RoleId(200)], &c));
    }
}
