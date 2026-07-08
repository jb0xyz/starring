use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, Member, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use domain::{Feature, VerificationPanel};

#[test]
fn verification_scenario_snapshot_roundtrips() {
    let verified = RoleId(1001);

    let verified_role = Role {
        id: verified,
        name: "인증됨".to_string(),
        permissions: Permissions::empty(),
        position: 1,
        managed: false,
    };

    let verification_channel = Channel {
        id: ChannelId(2001),
        name: "인증".to_string(),
        channel_type: ChannelType::Text,
        parent_id: None,
        position: 0,
        overwrites: vec![],
    };

    let general_channel = Channel {
        id: ChannelId(2002),
        name: "일반".to_string(),
        channel_type: ChannelType::Text,
        parent_id: None,
        position: 1,
        overwrites: vec![PermissionOverwrite {
            target: OverwriteTarget::Role(verified),
            allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
            deny: Permissions::empty(),
        }],
    };

    let state = GuildState {
        guild: Guild {
            id: GuildId(1),
            name: "커뮤니티".into(),
            owner_id: UserId(1),
        },
        roles: vec![verified_role],
        channels: vec![verification_channel, general_channel],
        members: vec![Member {
            user_id: UserId(5),
            roles: vec![],
        }],
    };

    let panel = Feature::Verification(VerificationPanel {
        channel_id: ChannelId(2001),
        grants_role: verified,
    });

    let state_json = serde_json::to_string(&state).unwrap();
    assert_eq!(
        serde_json::from_str::<GuildState>(&state_json).unwrap(),
        state
    );

    let panel_json = serde_json::to_string(&panel).unwrap();
    assert_eq!(serde_json::from_str::<Feature>(&panel_json).unwrap(), panel);
}
