use desired_compiler::{NormalizedDesiredState, NormalizedRole};
use desired_state::{Identity, MatchStrategy, Ownership, ResourceState};
use discord_model::Role;

use crate::resolver::{ResolveResult, ResourceResolver};
use crate::result::{
    ChangeOp, ChangedField, DeferredItem, DiffChange, DiffConflict, DiffResult, DiffTarget,
};

pub fn diff(desired: &NormalizedDesiredState, resolver: &impl ResourceResolver) -> DiffResult {
    let mut out = DiffResult::default();
    for role in &desired.roles {
        diff_role(role, resolver, &mut out);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::InMemoryMatchResolver;
    use desired_compiler::{NormalizedDesiredState, NormalizedRole};
    use desired_state::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};
    use discord_model::{Guild, GuildId, GuildState, Permissions, Role, RoleId, UserId};

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
}
