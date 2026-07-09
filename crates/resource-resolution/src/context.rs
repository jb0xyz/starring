use desired_compiler::{NormalizedDesiredState, NormalizedTarget};
use desired_state::ResourceKey;
use diff_engine::{ResolveResult, ResourceResolver};
use discord_model::{ChannelId, GuildId, OverwriteTarget, RoleId, UserId};

use crate::bindings::ResourceBindingMap;
use crate::error::ResolutionError;

pub struct ResourceResolutionContext<'a, R: ResourceResolver> {
    pub bindings: ResourceBindingMap,
    normalized: &'a NormalizedDesiredState,
    resolver: &'a R,
    guild_id: GuildId,
}

impl<'a, R: ResourceResolver> ResourceResolutionContext<'a, R> {
    pub fn new(normalized: &'a NormalizedDesiredState, resolver: &'a R, guild_id: GuildId) -> Self {
        Self {
            bindings: ResourceBindingMap::default(),
            normalized,
            resolver,
            guild_id,
        }
    }

    pub fn bind_role(&mut self, key: ResourceKey, id: RoleId) {
        self.bindings.role_bindings.insert(key, id);
    }

    pub fn bind_channel(&mut self, key: ResourceKey, id: ChannelId) {
        self.bindings.channel_bindings.insert(key, id);
    }

    pub fn resolve_role_key(&mut self, key: &ResourceKey) -> Result<RoleId, ResolutionError> {
        if let Some(id) = self.bindings.role_bindings.get(key) {
            return Ok(*id);
        }
        let resolved = {
            let nr = self
                .normalized
                .roles
                .iter()
                .find(|r| &r.identity.key == key)
                .ok_or_else(|| ResolutionError::MissingIdentity { key: key.0.clone() })?;
            self.resolver.resolve_role(&nr.identity, nr.name.as_deref())
        };
        match resolved {
            ResolveResult::Existing(role) => {
                self.bindings.role_bindings.insert(key.clone(), role.id);
                Ok(role.id)
            }
            _ => Err(ResolutionError::UnresolvedKey { key: key.0.clone() }),
        }
    }

    pub fn resolve_channel_key(&mut self, key: &ResourceKey) -> Result<ChannelId, ResolutionError> {
        if let Some(id) = self.bindings.channel_bindings.get(key) {
            return Ok(*id);
        }
        let resolved = {
            let nc = self
                .normalized
                .channels
                .iter()
                .find(|c| &c.identity.key == key)
                .ok_or_else(|| ResolutionError::MissingIdentity { key: key.0.clone() })?;
            self.resolver
                .resolve_channel(&nc.identity, nc.name.as_deref())
        };
        match resolved {
            ResolveResult::Existing(ch) => {
                self.bindings.channel_bindings.insert(key.clone(), ch.id);
                Ok(ch.id)
            }
            _ => Err(ResolutionError::UnresolvedKey { key: key.0.clone() }),
        }
    }

    pub fn resolve_target(
        &mut self,
        target: &NormalizedTarget,
    ) -> Result<OverwriteTarget, ResolutionError> {
        match target {
            NormalizedTarget::Everyone => Ok(OverwriteTarget::Role(RoleId(self.guild_id.0))),
            NormalizedTarget::Role(key) => Ok(OverwriteTarget::Role(self.resolve_role_key(key)?)),
            NormalizedTarget::Member(id) => {
                let raw = id
                    .parse::<u64>()
                    .map_err(|_| ResolutionError::UnresolvedKey { key: id.clone() })?;
                Ok(OverwriteTarget::Member(UserId(raw)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desired_compiler::NormalizedRole;
    use desired_state::Identity;
    use diff_engine::InMemoryMatchResolver;
    use discord_model::{Guild, GuildState, Permissions, Role};

    fn empty_guild(guild_id: u64) -> GuildState {
        GuildState {
            guild: Guild {
                id: GuildId(guild_id),
                name: "g".to_string(),
                owner_id: UserId(1),
            },
            roles: vec![],
            channels: vec![],
            members: vec![],
        }
    }

    fn nrole(key: &str, name: &str) -> NormalizedRole {
        NormalizedRole {
            identity: Identity {
                key: ResourceKey(key.to_string()),
                ..Default::default()
            },
            name: Some(name.to_string()),
            permissions: Some(Permissions::empty()),
        }
    }

    #[test]
    fn bound_role_resolves_to_binding() {
        let normalized = NormalizedDesiredState::default();
        let guild = empty_guild(1);
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(1));
        ctx.bind_role(ResourceKey("verified".to_string()), RoleId(999));
        assert_eq!(
            ctx.resolve_role_key(&ResourceKey("verified".to_string()))
                .unwrap(),
            RoleId(999)
        );
    }

    #[test]
    fn existing_role_resolves_via_resolver() {
        let normalized = NormalizedDesiredState {
            roles: vec![nrole("mod", "Moderator")],
            ..Default::default()
        };
        let guild = GuildState {
            guild: Guild {
                id: GuildId(1),
                name: "g".to_string(),
                owner_id: UserId(1),
            },
            roles: vec![Role {
                id: RoleId(42),
                name: "Moderator".to_string(),
                permissions: Permissions::empty(),
                position: 0,
                managed: false,
            }],
            channels: vec![],
            members: vec![],
        };
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(1));
        assert_eq!(
            ctx.resolve_role_key(&ResourceKey("mod".to_string()))
                .unwrap(),
            RoleId(42)
        );
    }

    #[test]
    fn missing_role_errors_unresolved() {
        let normalized = NormalizedDesiredState {
            roles: vec![nrole("ghost", "Ghost")],
            ..Default::default()
        };
        let guild = empty_guild(1);
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(1));
        assert!(matches!(
            ctx.resolve_role_key(&ResourceKey("ghost".to_string())),
            Err(ResolutionError::UnresolvedKey { .. })
        ));
    }

    #[test]
    fn unknown_key_errors_missing_identity() {
        let normalized = NormalizedDesiredState::default();
        let guild = empty_guild(1);
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(1));
        assert!(matches!(
            ctx.resolve_role_key(&ResourceKey("nope".to_string())),
            Err(ResolutionError::MissingIdentity { .. })
        ));
    }

    #[test]
    fn resolve_target_everyone_uses_guild_id() {
        let normalized = NormalizedDesiredState::default();
        let guild = empty_guild(7);
        let resolver = InMemoryMatchResolver::new(&guild);
        let mut ctx = ResourceResolutionContext::new(&normalized, &resolver, GuildId(7));
        assert_eq!(
            ctx.resolve_target(&NormalizedTarget::Everyone).unwrap(),
            OverwriteTarget::Role(RoleId(7))
        );
    }
}
