use std::collections::BTreeMap;

use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{ChannelType, Guild, GuildId, GuildState, Permissions, UserId};
use operation_graph::compile_operations;
use policy_engine::{PolicyEngine, Verdict};

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

fn decide(desired: &DesiredState) -> Verdict {
    let normalized = compile(desired).unwrap();
    let guild = empty_guild();
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&guild));
    let graph = compile_operations(&diff_result, &normalized).unwrap();
    PolicyEngine::with_default_rules().evaluate(&graph).verdict
}

#[test]
fn admin_grant_is_denied() {
    let desired = DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: ResourceKey("admin".to_string()),
                ..Default::default()
            },
            name: Some("Administrator".to_string()),
            permissions: Some(Permissions::ADMINISTRATOR),
        }],
        ..Default::default()
    };
    assert_eq!(decide(&desired), Verdict::Deny);
}

#[test]
fn verification_scenario_requires_approval() {
    let verified = ResourceKey("verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(
        verified.clone(),
        AccessGrant {
            allow: vec![Capability::View, Capability::Send],
            deny: vec![],
        },
    );
    let desired = DesiredState {
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
    };
    assert_eq!(decide(&desired), Verdict::RequireApproval);
}
