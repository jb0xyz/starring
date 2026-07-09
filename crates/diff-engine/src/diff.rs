use std::collections::HashMap;

use desired_compiler::{
    NormalizedChannel, NormalizedDesiredState, NormalizedRole, NormalizedTarget,
};
use desired_state::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};
use discord_model::{Channel, OverwriteTarget, Role};

use crate::resolver::{ResolveResult, ResourceResolver};
use crate::result::{
    ChangeOp, ChangedField, DeferredItem, DiffChange, DiffConflict, DiffResult, DiffTarget,
};

pub fn diff(desired: &NormalizedDesiredState, resolver: &impl ResourceResolver) -> DiffResult {
    let mut out = DiffResult::default();
    for role in &desired.roles {
        diff_role(role, resolver, &mut out);
    }
    let roles_by_key: HashMap<&ResourceKey, &NormalizedRole> = desired
        .roles
        .iter()
        .map(|role| (&role.identity.key, role))
        .collect();
    for channel in &desired.channels {
        diff_channel(channel, resolver, &roles_by_key, &mut out);
    }
    for panel in &desired.verification_panels {
        out.deferred.push(DeferredItem {
            kind: "verification_panel".to_string(),
            key: panel.identity.key.clone(),
            reason: "panel state not tracked in Phase 4".to_string(),
        });
    }
    out
}

fn is_explicit_id(identity: &Identity) -> bool {
    matches!(identity.match_by, MatchStrategy::ByExplicitId(_))
}

fn noop(target: DiffTarget) -> DiffChange {
    DiffChange {
        op: ChangeOp::NoOp,
        target,
        changed: vec![],
    }
}

fn push_conflict(out: &mut DiffResult, target: DiffTarget, reason: &str) {
    out.conflicts.push(DiffConflict {
        target,
        reason: reason.to_string(),
    });
}

fn diff_role(role: &NormalizedRole, resolver: &impl ResourceResolver, out: &mut DiffResult) {
    let identity = &role.identity;
    let target = DiffTarget::Role {
        key: identity.key.clone(),
    };
    let resolved = resolver.resolve_role(identity, role.name.as_deref());
    match identity.state {
        ResourceState::Present => match resolved {
            ResolveResult::Existing(current) => {
                if identity.ownership == Ownership::Referenced {
                    out.changes.push(noop(target));
                } else {
                    let changed = role_changed(role, &current);
                    if changed.is_empty() {
                        out.changes.push(noop(target));
                    } else {
                        out.changes.push(DiffChange {
                            op: ChangeOp::Update,
                            target,
                            changed,
                        });
                    }
                }
            }
            ResolveResult::Missing => {
                if identity.ownership == Ownership::Referenced {
                    push_conflict(out, target, "referenced role not found");
                } else if is_explicit_id(identity) {
                    push_conflict(out, target, "explicit id not found");
                } else {
                    out.changes.push(DiffChange {
                        op: ChangeOp::Create,
                        target,
                        changed: vec![],
                    });
                }
            }
            ResolveResult::Conflict { reason } => push_conflict(out, target, &reason),
        },
        ResourceState::Absent => match resolved {
            ResolveResult::Existing(_) => {
                if identity.ownership == Ownership::Referenced {
                    push_conflict(out, target, "cannot delete referenced role");
                } else {
                    out.changes.push(DiffChange {
                        op: ChangeOp::Delete,
                        target,
                        changed: vec![],
                    });
                }
            }
            ResolveResult::Missing => out.changes.push(noop(target)),
            ResolveResult::Conflict { reason } => push_conflict(out, target, &reason),
        },
    }
}

fn role_changed(role: &NormalizedRole, current: &Role) -> Vec<ChangedField> {
    let mut changed = Vec::new();
    if let Some(name) = &role.name {
        if name != &current.name {
            changed.push(ChangedField::Name);
        }
    }
    if let Some(permissions) = &role.permissions {
        if permissions != &current.permissions {
            changed.push(ChangedField::Permissions);
        }
    }
    changed
}

fn diff_channel(
    channel: &NormalizedChannel,
    resolver: &impl ResourceResolver,
    roles_by_key: &HashMap<&ResourceKey, &NormalizedRole>,
    out: &mut DiffResult,
) {
    let identity = &channel.identity;
    let target = DiffTarget::Channel {
        key: identity.key.clone(),
    };
    let resolved = resolver.resolve_channel(identity, channel.name.as_deref());
    match identity.state {
        ResourceState::Present => match resolved {
            ResolveResult::Existing(current) => {
                if identity.ownership == Ownership::Referenced {
                    out.changes.push(noop(target));
                } else {
                    let changed = channel_meta_changed(channel, &current);
                    if changed.is_empty() {
                        out.changes.push(noop(target));
                    } else {
                        out.changes.push(DiffChange {
                            op: ChangeOp::Update,
                            target,
                            changed,
                        });
                    }
                    diff_overwrites(channel, &current, resolver, roles_by_key, out);
                }
            }
            ResolveResult::Missing => {
                if identity.ownership == Ownership::Referenced {
                    push_conflict(out, target, "referenced channel not found");
                } else if is_explicit_id(identity) {
                    push_conflict(out, target, "explicit id not found");
                } else {
                    out.changes.push(DiffChange {
                        op: ChangeOp::Create,
                        target,
                        changed: vec![],
                    });
                    for overwrite in &channel.overwrites {
                        out.changes.push(DiffChange {
                            op: ChangeOp::Create,
                            target: DiffTarget::Overwrite {
                                channel: channel.identity.key.clone(),
                                target: overwrite.target.clone(),
                            },
                            changed: vec![],
                        });
                    }
                }
            }
            ResolveResult::Conflict { reason } => push_conflict(out, target, &reason),
        },
        ResourceState::Absent => match resolved {
            ResolveResult::Existing(_) => {
                if identity.ownership == Ownership::Referenced {
                    push_conflict(out, target, "cannot delete referenced channel");
                } else {
                    out.changes.push(DiffChange {
                        op: ChangeOp::Delete,
                        target,
                        changed: vec![],
                    });
                }
            }
            ResolveResult::Missing => out.changes.push(noop(target)),
            ResolveResult::Conflict { reason } => push_conflict(out, target, &reason),
        },
    }
}

fn channel_meta_changed(channel: &NormalizedChannel, current: &Channel) -> Vec<ChangedField> {
    let mut changed = Vec::new();
    if let Some(name) = &channel.name {
        if name != &current.name {
            changed.push(ChangedField::Name);
        }
    }
    if let Some(channel_type) = &channel.channel_type {
        if channel_type != &current.channel_type {
            changed.push(ChangedField::ChannelType);
        }
    }
    changed
}

fn diff_overwrites(
    channel: &NormalizedChannel,
    current: &Channel,
    resolver: &impl ResourceResolver,
    roles_by_key: &HashMap<&ResourceKey, &NormalizedRole>,
    out: &mut DiffResult,
) {
    for overwrite in &channel.overwrites {
        let target = DiffTarget::Overwrite {
            channel: channel.identity.key.clone(),
            target: overwrite.target.clone(),
        };
        let current_target =
            match resolve_overwrite_target(&overwrite.target, resolver, roles_by_key) {
                Ok(target) => target,
                Err(reason) => {
                    push_conflict(out, target, &reason);
                    continue;
                }
            };
        match current_target.and_then(|resolved_target| {
            current
                .overwrites
                .iter()
                .find(|current_overwrite| current_overwrite.target == resolved_target)
        }) {
            Some(current_overwrite) => {
                let mut changed = Vec::new();
                if overwrite.allow != current_overwrite.allow {
                    changed.push(ChangedField::Allow);
                }
                if overwrite.deny != current_overwrite.deny {
                    changed.push(ChangedField::Deny);
                }
                if changed.is_empty() {
                    out.changes.push(noop(target));
                } else {
                    out.changes.push(DiffChange {
                        op: ChangeOp::Update,
                        target,
                        changed,
                    });
                }
            }
            None => out.changes.push(DiffChange {
                op: ChangeOp::Create,
                target,
                changed: vec![],
            }),
        }
    }
}

fn resolve_overwrite_target(
    target: &NormalizedTarget,
    resolver: &impl ResourceResolver,
    roles_by_key: &HashMap<&ResourceKey, &NormalizedRole>,
) -> Result<Option<OverwriteTarget>, String> {
    match target {
        NormalizedTarget::Everyone => Ok(Some(resolver.everyone_overwrite_target())),
        NormalizedTarget::Member(id) => match id.parse::<u64>() {
            Ok(raw) => Ok(Some(OverwriteTarget::Member(discord_model::UserId(raw)))),
            Err(_) => Err(format!("invalid member id {id}")),
        },
        NormalizedTarget::Role(key) => match roles_by_key.get(key) {
            None => Err(format!(
                "overwrite references undeclared role key {}",
                key.0
            )),
            Some(role) => match resolver.resolve_role(&role.identity, role.name.as_deref()) {
                ResolveResult::Existing(current) => Ok(Some(OverwriteTarget::Role(current.id))),
                ResolveResult::Missing => Ok(None),
                ResolveResult::Conflict { reason } => Err(reason),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::InMemoryMatchResolver;
    use desired_compiler::{
        NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole,
        NormalizedTarget,
    };
    use desired_state::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};
    use discord_model::{
        Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
        PermissionOverwrite, Permissions, Role, RoleId, UserId,
    };

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

    fn guild_with(roles: Vec<Role>) -> GuildState {
        GuildState {
            guild: Guild {
                id: GuildId(1),
                name: "g".to_string(),
                owner_id: UserId(1),
            },
            roles,
            channels: vec![],
            members: vec![],
        }
    }

    fn guild_full(roles: Vec<Role>, channels: Vec<Channel>) -> GuildState {
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

    fn nrole(
        key: &str,
        name: Option<&str>,
        ownership: Ownership,
        state: ResourceState,
    ) -> NormalizedRole {
        NormalizedRole {
            identity: Identity {
                key: ResourceKey(key.to_string()),
                match_by: MatchStrategy::ByName,
                ownership,
                state,
            },
            name: name.map(|name| name.to_string()),
            permissions: None,
        }
    }

    fn nchannel(key: &str, name: &str, overwrites: Vec<NormalizedOverwrite>) -> NormalizedChannel {
        NormalizedChannel {
            identity: Identity {
                key: ResourceKey(key.to_string()),
                match_by: MatchStrategy::ByName,
                ..Default::default()
            },
            name: Some(name.to_string()),
            channel_type: None,
            parent: None,
            overwrites,
        }
    }

    fn ds(roles: Vec<NormalizedRole>) -> NormalizedDesiredState {
        NormalizedDesiredState {
            roles,
            ..Default::default()
        }
    }

    fn ops(diff: &DiffResult) -> Vec<ChangeOp> {
        diff.changes.iter().map(|change| change.op).collect()
    }

    #[test]
    fn present_missing_creates() {
        let guild = empty_guild();
        let diff = diff(
            &ds(vec![nrole(
                "r",
                Some("New"),
                Ownership::Managed,
                ResourceState::Present,
            )]),
            &InMemoryMatchResolver::new(&guild),
        );
        assert_eq!(ops(&diff), vec![ChangeOp::Create]);
    }

    #[test]
    fn present_existing_same_is_noop() {
        let guild = guild_with(vec![Role {
            id: RoleId(5),
            name: "Keep".to_string(),
            permissions: Permissions::empty(),
            position: 0,
            managed: false,
        }]);
        let diff = diff(
            &ds(vec![nrole(
                "r",
                Some("Keep"),
                Ownership::Managed,
                ResourceState::Present,
            )]),
            &InMemoryMatchResolver::new(&guild),
        );
        assert_eq!(ops(&diff), vec![ChangeOp::NoOp]);
    }

    #[test]
    fn present_existing_diff_name_is_update() {
        let guild = guild_with(vec![Role {
            id: RoleId(5),
            name: "Old".to_string(),
            permissions: Permissions::empty(),
            position: 0,
            managed: false,
        }]);
        let mut role = nrole("r", Some("Old"), Ownership::Managed, ResourceState::Present);
        role.identity.match_by = MatchStrategy::ByExplicitId("5".to_string());
        role.name = Some("Renamed".to_string());
        let diff = diff(&ds(vec![role]), &InMemoryMatchResolver::new(&guild));
        assert_eq!(diff.changes[0].op, ChangeOp::Update);
        assert_eq!(diff.changes[0].changed, vec![ChangedField::Name]);
    }

    #[test]
    fn absent_existing_deletes() {
        let guild = guild_with(vec![Role {
            id: RoleId(5),
            name: "Gone".to_string(),
            permissions: Permissions::empty(),
            position: 0,
            managed: false,
        }]);
        let diff = diff(
            &ds(vec![nrole(
                "r",
                Some("Gone"),
                Ownership::Managed,
                ResourceState::Absent,
            )]),
            &InMemoryMatchResolver::new(&guild),
        );
        assert_eq!(ops(&diff), vec![ChangeOp::Delete]);
    }

    #[test]
    fn explicit_id_missing_is_conflict() {
        let guild = empty_guild();
        let mut role = nrole("r", None, Ownership::Managed, ResourceState::Present);
        role.identity.match_by = MatchStrategy::ByExplicitId("99".to_string());
        let diff = diff(&ds(vec![role]), &InMemoryMatchResolver::new(&guild));
        assert!(diff.changes.is_empty());
        assert_eq!(diff.conflicts.len(), 1);
    }

    #[test]
    fn referenced_missing_is_conflict() {
        let guild = empty_guild();
        let diff = diff(
            &ds(vec![nrole(
                "r",
                Some("X"),
                Ownership::Referenced,
                ResourceState::Present,
            )]),
            &InMemoryMatchResolver::new(&guild),
        );
        assert_eq!(diff.conflicts.len(), 1);
    }

    #[test]
    fn everyone_overwrite_noop_when_matching() {
        let current_channel = Channel {
            id: ChannelId(20),
            name: "general".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites: vec![PermissionOverwrite {
                target: OverwriteTarget::Role(RoleId(1)),
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            }],
        };
        let guild = guild_full(vec![], vec![current_channel]);
        let desired = NormalizedDesiredState {
            channels: vec![nchannel(
                "gen",
                "general",
                vec![NormalizedOverwrite {
                    target: NormalizedTarget::Everyone,
                    allow: Permissions::empty(),
                    deny: Permissions::VIEW_CHANNEL,
                }],
            )],
            ..Default::default()
        };
        let diff = diff(&desired, &InMemoryMatchResolver::new(&guild));
        assert!(diff.conflicts.is_empty());
        assert!(diff
            .changes
            .iter()
            .all(|change| change.op == ChangeOp::NoOp));
    }

    #[test]
    fn role_overwrite_created_when_absent_in_current() {
        let verified = Role {
            id: RoleId(50),
            name: "Verified".to_string(),
            permissions: Permissions::empty(),
            position: 0,
            managed: false,
        };
        let current_channel = Channel {
            id: ChannelId(20),
            name: "general".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites: vec![],
        };
        let guild = guild_full(vec![verified], vec![current_channel]);
        let desired = NormalizedDesiredState {
            roles: vec![nrole(
                "vk",
                Some("Verified"),
                Ownership::Managed,
                ResourceState::Present,
            )],
            channels: vec![nchannel(
                "gen",
                "general",
                vec![NormalizedOverwrite {
                    target: NormalizedTarget::Role(ResourceKey("vk".to_string())),
                    allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
                    deny: Permissions::empty(),
                }],
            )],
            ..Default::default()
        };
        let diff = diff(&desired, &InMemoryMatchResolver::new(&guild));
        assert!(diff.conflicts.is_empty());
        let overwrite = diff
            .changes
            .iter()
            .find(|change| matches!(change.target, DiffTarget::Overwrite { .. }))
            .unwrap();
        assert_eq!(overwrite.op, ChangeOp::Create);
    }

    #[test]
    fn new_channel_emits_overwrite_creates() {
        let guild = empty_guild();
        let desired = NormalizedDesiredState {
            channels: vec![nchannel(
                "gen",
                "general",
                vec![NormalizedOverwrite {
                    target: NormalizedTarget::Everyone,
                    allow: Permissions::empty(),
                    deny: Permissions::VIEW_CHANNEL,
                }],
            )],
            ..Default::default()
        };
        let diff = diff(&desired, &InMemoryMatchResolver::new(&guild));
        let overwrite_creates = diff
            .changes
            .iter()
            .filter(|change| {
                matches!(change.target, DiffTarget::Overwrite { .. })
                    && change.op == ChangeOp::Create
            })
            .count();
        assert_eq!(overwrite_creates, 1);
    }
}
