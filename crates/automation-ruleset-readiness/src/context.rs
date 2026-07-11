use std::collections::BTreeMap;

use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions, RoleId};
use resource_resolution::ResourceBindingMap;

use crate::types::{GuildCapabilities, ReadinessContextError};

pub fn build_readiness_context(
    guild_id: GuildId,
    bindings: &ResourceBindingMap,
    roles_snapshot: &BTreeMap<RoleId, Permissions>,
    bot_role_ids: &[RoleId],
) -> Result<(GuildCapabilities, BTreeMap<ResourceKey, Permissions>), ReadinessContextError> {
    let everyone = RoleId(guild_id.0);
    let mut base = *roles_snapshot
        .get(&everyone)
        .ok_or(ReadinessContextError::EveryoneRoleMissing)?;
    for role_id in bot_role_ids {
        if let Some(perms) = roles_snapshot.get(role_id) {
            base |= *perms;
        }
    }
    let mut role_permissions = BTreeMap::new();
    for (key, role_id) in &bindings.role_bindings {
        let perms =
            roles_snapshot
                .get(role_id)
                .ok_or_else(|| ReadinessContextError::BoundRoleMissing {
                    key: key.clone(),
                    role_id: *role_id,
                })?;
        role_permissions.insert(key.clone(), *perms);
    }
    Ok((
        GuildCapabilities {
            base_permissions: base,
        },
        role_permissions,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(pairs: &[(u64, Permissions)]) -> BTreeMap<RoleId, Permissions> {
        pairs.iter().map(|(id, p)| (RoleId(*id), *p)).collect()
    }

    #[test]
    fn everyone_missing_fails() {
        let err = build_readiness_context(
            GuildId(7),
            &ResourceBindingMap::default(),
            &snapshot(&[]),
            &[],
        )
        .unwrap_err();
        assert_eq!(err, ReadinessContextError::EveryoneRoleMissing);
    }

    #[test]
    fn bound_role_missing_fails_closed() {
        let mut bindings = ResourceBindingMap::default();
        bindings
            .role_bindings
            .insert(ResourceKey("mod".to_string()), RoleId(123));
        let err = build_readiness_context(
            GuildId(7),
            &bindings,
            &snapshot(&[(7, Permissions::empty())]),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            err,
            ReadinessContextError::BoundRoleMissing {
                key: ResourceKey("mod".to_string()),
                role_id: RoleId(123),
            }
        );
    }

    #[test]
    fn base_is_everyone_or_bot_roles() {
        let (caps, roles) = build_readiness_context(
            GuildId(7),
            &ResourceBindingMap::default(),
            &snapshot(&[
                (7, Permissions::VIEW_CHANNEL),
                (900, Permissions::MANAGE_ROLES),
            ]),
            &[RoleId(900)],
        )
        .unwrap();
        assert!(caps.base_permissions.contains(Permissions::MANAGE_ROLES));
        assert!(caps.base_permissions.contains(Permissions::VIEW_CHANNEL));
        assert!(roles.is_empty());
    }
}
