use std::collections::BTreeMap;

use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, FeatureIntent, Identity,
    ResourceKey, RoleIntent, VerificationIntent,
};
use diff_engine::{diff, ChangeOp, InMemoryMatchResolver};
use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};

fn desired() -> DesiredState {
    let verified = ResourceKey("verified_member".to_string());
    let mut general_roles = BTreeMap::new();
    general_roles.insert(
        verified.clone(),
        AccessGrant {
            allow: vec![Capability::View, Capability::Send],
            deny: vec![],
        },
    );
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: verified.clone(),
                ..Default::default()
            },
            name: Some("Verified".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![ChannelIntent {
            identity: Identity {
                key: ResourceKey("general".to_string()),
                ..Default::default()
            },
            name: Some("general".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent {
                everyone: Some(AccessGrant {
                    allow: vec![],
                    deny: vec![Capability::View],
                }),
                roles: general_roles,
            }),
            raw_overwrites: None,
        }],
        features: vec![FeatureIntent::Verification(VerificationIntent {
            identity: Identity {
                key: ResourceKey("panel".to_string()),
                ..Default::default()
            },
            channel: ResourceKey("general".to_string()),
            grants_role: verified,
        })],
        ..Default::default()
    }
}

fn after_guild() -> GuildState {
    GuildState {
        guild: Guild {
            id: GuildId(1),
            name: "g".to_string(),
            owner_id: UserId(1),
        },
        roles: vec![Role {
            id: RoleId(50),
            name: "Verified".to_string(),
            permissions: Permissions::empty(),
            position: 0,
            managed: false,
        }],
        channels: vec![Channel {
            id: ChannelId(20),
            name: "general".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites: vec![
                PermissionOverwrite {
                    target: OverwriteTarget::Role(RoleId(1)),
                    allow: Permissions::empty(),
                    deny: Permissions::VIEW_CHANNEL,
                },
                PermissionOverwrite {
                    target: OverwriteTarget::Role(RoleId(50)),
                    allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
                    deny: Permissions::empty(),
                },
            ],
        }],
        members: vec![],
    }
}

#[test]
fn diff_on_empty_guild_creates() {
    let normalized = compile(&desired()).unwrap();
    let empty = GuildState {
        guild: Guild {
            id: GuildId(1),
            name: "g".to_string(),
            owner_id: UserId(1),
        },
        roles: vec![],
        channels: vec![],
        members: vec![],
    };
    let diff = diff(&normalized, &InMemoryMatchResolver::new(&empty));
    assert!(diff
        .changes
        .iter()
        .any(|change| change.op == ChangeOp::Create));
    assert_eq!(diff.deferred.len(), 1);
}

#[test]
fn diff_on_matching_guild_is_all_noop() {
    let normalized = compile(&desired()).unwrap();
    let guild = after_guild();
    let diff = diff(&normalized, &InMemoryMatchResolver::new(&guild));
    assert!(diff.conflicts.is_empty(), "conflicts: {:?}", diff.conflicts);
    assert!(
        diff.changes
            .iter()
            .all(|change| change.op == ChangeOp::NoOp),
        "changes: {:?}",
        diff.changes
    );
    assert_eq!(diff.deferred.len(), 1);
}
