use std::collections::BTreeMap;

use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{ChannelType, Guild, GuildId, GuildState, Permissions, UserId};
use operation_graph::{compile_operations, Operation};

fn desired() -> DesiredState {
    let verified = ResourceKey("verified_member".to_string());
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
                roles,
            }),
            raw_overwrites: None,
        }],
        ..Default::default()
    }
}

fn empty_guild() -> GuildState {
    GuildState {
        guild: Guild {
            id: GuildId(1),
            name: "g".to_string(),
            owner_id: UserId(1),
        },
        roles: vec![],
        channels: vec![],
        members: vec![],
    }
}

#[test]
fn overwrite_auto_depends_on_creates() {
    let normalized = compile(&desired()).unwrap();
    let changes = diff(&normalized, &InMemoryMatchResolver::new(&empty_guild()));
    let graph = compile_operations(&changes, &normalized).unwrap();

    let role_id = graph
        .nodes
        .iter()
        .find(|node| matches!(&node.operation, Operation::CreateRole { .. }))
        .unwrap()
        .id;
    let channel_id = graph
        .nodes
        .iter()
        .find(|node| matches!(&node.operation, Operation::CreateChannel { .. }))
        .unwrap()
        .id;
    let verified_overwrite = graph
        .nodes
        .iter()
        .find(|node| matches!(&node.operation,
            Operation::CreateOverwrite { target: desired_compiler::NormalizedTarget::Role(key), .. } if key.0 == "verified_member"))
        .unwrap();

    assert!(verified_overwrite.depends_on.contains(&role_id));
    assert!(verified_overwrite.depends_on.contains(&channel_id));
    assert!(graph.topological_order().is_ok());
}
