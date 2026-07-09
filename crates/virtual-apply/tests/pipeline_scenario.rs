use std::collections::BTreeMap;

use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use operation_graph::compile_operations;
use simulator::{can_send, can_view};
use virtual_apply::apply;

fn before_guild() -> GuildState {
    GuildState {
        guild: Guild {
            id: GuildId(1),
            name: "srv".to_string(),
            owner_id: UserId(1),
        },
        roles: vec![Role {
            id: RoleId(1),
            name: "everyone".to_string(),
            permissions: Permissions::VIEW_CHANNEL,
            position: 0,
            managed: false,
        }],
        channels: vec![Channel {
            id: ChannelId(500),
            name: "general".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites: vec![PermissionOverwrite {
                target: OverwriteTarget::Role(RoleId(1)),
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::empty(),
            }],
        }],
        members: vec![],
    }
}

fn desired() -> DesiredState {
    let verified = ResourceKey("verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(
        verified.clone(),
        AccessGrant {
            allow: vec![Capability::View, Capability::Send],
            deny: vec![],
        },
    );
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: verified,
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
                roles,
            }),
            raw_overwrites: None,
        }],
        ..Default::default()
    }
}

#[test]
fn full_pipeline_to_after_state_and_simulation() {
    let before = before_guild();
    let normalized = compile(&desired()).unwrap();
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&before));
    let graph = compile_operations(&diff_result, &normalized).unwrap();

    let resolver = InMemoryMatchResolver::new(&before);
    let result = apply(&before, &graph, &normalized, &resolver).unwrap();
    let after = &result.after;

    let verified_id = result.synthetic_roles[&ResourceKey("verified".to_string())];
    let general = after.channels.iter().find(|c| c.name == "general").unwrap();

    assert!(after.roles.iter().any(|r| r.id == verified_id));
    assert!(general.overwrites.iter().any(|o| {
        o.target == OverwriteTarget::Role(RoleId(1)) && o.deny.contains(Permissions::VIEW_CHANNEL)
    }));
    assert!(general.overwrites.iter().any(|o| {
        o.target == OverwriteTarget::Role(verified_id)
            && o.allow.contains(Permissions::VIEW_CHANNEL)
    }));

    assert!(!can_view(after, &[], general));
    assert!(can_view(after, &[verified_id], general));
    assert!(can_send(after, &[verified_id], general));
}
