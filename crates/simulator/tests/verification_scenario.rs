use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use simulator::{access_matrix, SubjectSpec};

fn after_guild() -> GuildState {
    let everyone = RoleId(1);
    let verified = RoleId(100);
    GuildState {
        guild: Guild {
            id: GuildId(1),
            name: "srv".to_string(),
            owner_id: UserId(1),
        },
        roles: vec![
            Role {
                id: everyone,
                name: "everyone".to_string(),
                permissions: Permissions::VIEW_CHANNEL,
                position: 0,
                managed: false,
            },
            Role {
                id: verified,
                name: "Verified".to_string(),
                permissions: Permissions::empty(),
                position: 1,
                managed: false,
            },
        ],
        channels: vec![
            Channel {
                id: ChannelId(20),
                name: "verification".to_string(),
                channel_type: ChannelType::Text,
                parent_id: None,
                position: 0,
                overwrites: vec![PermissionOverwrite {
                    target: OverwriteTarget::Role(everyone),
                    allow: Permissions::VIEW_CHANNEL,
                    deny: Permissions::empty(),
                }],
            },
            Channel {
                id: ChannelId(21),
                name: "general".to_string(),
                channel_type: ChannelType::Text,
                parent_id: None,
                position: 1,
                overwrites: vec![
                    PermissionOverwrite {
                        target: OverwriteTarget::Role(everyone),
                        allow: Permissions::empty(),
                        deny: Permissions::VIEW_CHANNEL,
                    },
                    PermissionOverwrite {
                        target: OverwriteTarget::Role(verified),
                        allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
                        deny: Permissions::empty(),
                    },
                ],
            },
        ],
        members: vec![],
    }
}

fn cell<'a>(
    m: &'a simulator::AccessMatrix,
    subject: &str,
    channel: &str,
) -> &'a simulator::AccessCell {
    m.cells
        .iter()
        .find(|c| c.subject == subject && c.channel == channel)
        .unwrap()
}

#[test]
fn verification_visibility_preview() {
    let g = after_guild();
    let subjects = vec![
        SubjectSpec {
            name: "new".to_string(),
            roles: vec![],
        },
        SubjectSpec {
            name: "verified".to_string(),
            roles: vec![RoleId(100)],
        },
    ];
    let m = access_matrix(&g, &subjects);

    assert!(cell(&m, "new", "verification").can_view);
    assert!(!cell(&m, "new", "general").can_view);
    assert!(cell(&m, "verified", "general").can_view);
    assert!(cell(&m, "verified", "general").can_send);
}
