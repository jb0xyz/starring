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
use policy_engine::{PolicyEngine, Verdict};
use preview::{build_preview, PreviewChangeKind, PreviewSeverity};
use simulator::{access_matrix, SubjectSpec};
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
fn full_pipeline_to_preview() {
    let before = before_guild();
    let normalized = compile(&desired()).unwrap();
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&before));
    let graph = compile_operations(&diff_result, &normalized).unwrap();
    let policy = PolicyEngine::with_default_rules().evaluate(&graph);
    let applied = apply(
        &before,
        &graph,
        &normalized,
        &InMemoryMatchResolver::new(&before),
    )
    .unwrap();
    let after = &applied.after;

    let verified_id = applied.synthetic_roles[&ResourceKey("verified".to_string())];
    let before_subjects = vec![SubjectSpec {
        name: "new_member".to_string(),
        roles: vec![],
    }];
    let after_subjects = vec![
        SubjectSpec {
            name: "new_member".to_string(),
            roles: vec![],
        },
        SubjectSpec {
            name: "verified_member".to_string(),
            roles: vec![verified_id],
        },
    ];
    let before_matrix = access_matrix(&before, &before_subjects);
    let after_matrix = access_matrix(after, &after_subjects);

    let p = build_preview(
        "인증 시스템 설정",
        &diff_result,
        &graph,
        &policy,
        &applied,
        &before_matrix,
        &after_matrix,
    );

    assert_eq!(p.verdict, Verdict::RequireApproval);
    assert!(p.approval_required);
    assert!(!p.blocked);
    assert!(p
        .changes
        .iter()
        .any(|c| c.kind == PreviewChangeKind::RoleCreate && c.target == "Verified"));
    assert!(p
        .changes
        .iter()
        .any(|c| c.severity == PreviewSeverity::Notice && c.target.contains("@everyone")));
    assert!(p.access_changes.iter().any(|a| {
        a.subject == "new_member"
            && a.channel == "general"
            && a.before_can_view
            && !a.after_can_view
    }));
    assert!(p.access_changes.iter().any(|a| {
        a.subject == "verified_member"
            && a.channel == "general"
            && !a.before_can_view
            && a.after_can_view
    }));
    assert!(p
        .policy_findings
        .iter()
        .any(|f| f.rule_id == "everyone-change"));
}
