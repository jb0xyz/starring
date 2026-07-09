use desired_state::{Identity, MatchStrategy};
use discord_model::{Channel, GuildState, OverwriteTarget, Role, RoleId};

pub enum ResolveResult<T> {
    Existing(T),
    Missing,
    Conflict { reason: String },
}

pub trait ResourceResolver {
    fn resolve_role(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Role>;
    fn resolve_channel(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Channel>;
    fn everyone_overwrite_target(&self) -> OverwriteTarget;
}

pub struct InMemoryMatchResolver<'a> {
    guild: &'a GuildState,
}

impl<'a> InMemoryMatchResolver<'a> {
    pub fn new(guild: &'a GuildState) -> Self {
        Self { guild }
    }
}

impl ResourceResolver for InMemoryMatchResolver<'_> {
    fn resolve_role(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Role> {
        match &identity.match_by {
            MatchStrategy::ByName => {
                let name = match name {
                    Some(name) => name,
                    None => {
                        return ResolveResult::Conflict {
                            reason: "ByName requires a name".to_string(),
                        };
                    }
                };
                let matches: Vec<Role> = self
                    .guild
                    .roles
                    .iter()
                    .filter(|role| role.name == name)
                    .cloned()
                    .collect();
                match matches.len() {
                    0 => ResolveResult::Missing,
                    1 => ResolveResult::Existing(matches.into_iter().next().unwrap()),
                    _ => ResolveResult::Conflict {
                        reason: format!("multiple roles named {name}"),
                    },
                }
            }
            MatchStrategy::ByExplicitId(id) => match id.parse::<u64>() {
                Err(_) => ResolveResult::Conflict {
                    reason: format!("invalid id {id}"),
                },
                Ok(raw) => match self
                    .guild
                    .roles
                    .iter()
                    .find(|role| role.id == RoleId(raw))
                    .cloned()
                {
                    Some(role) => ResolveResult::Existing(role),
                    None => ResolveResult::Missing,
                },
            },
        }
    }

    fn resolve_channel(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Channel> {
        match &identity.match_by {
            MatchStrategy::ByName => {
                let name = match name {
                    Some(name) => name,
                    None => {
                        return ResolveResult::Conflict {
                            reason: "ByName requires a name".to_string(),
                        };
                    }
                };
                let matches: Vec<Channel> = self
                    .guild
                    .channels
                    .iter()
                    .filter(|channel| channel.name == name)
                    .cloned()
                    .collect();
                match matches.len() {
                    0 => ResolveResult::Missing,
                    1 => ResolveResult::Existing(matches.into_iter().next().unwrap()),
                    _ => ResolveResult::Conflict {
                        reason: format!("multiple channels named {name}"),
                    },
                }
            }
            MatchStrategy::ByExplicitId(id) => match id.parse::<u64>() {
                Err(_) => ResolveResult::Conflict {
                    reason: format!("invalid id {id}"),
                },
                Ok(raw) => match self
                    .guild
                    .channels
                    .iter()
                    .find(|channel| channel.id.0 == raw)
                    .cloned()
                {
                    Some(channel) => ResolveResult::Existing(channel),
                    None => ResolveResult::Missing,
                },
            },
        }
    }

    fn everyone_overwrite_target(&self) -> OverwriteTarget {
        OverwriteTarget::Role(RoleId(self.guild.guild.id.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::{Identity, MatchStrategy, ResourceKey};
    use discord_model::{Guild, GuildId, GuildState, Permissions, Role, RoleId, UserId};

    fn guild_with_roles(roles: Vec<Role>) -> GuildState {
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

    fn role(id: u64, name: &str) -> Role {
        Role {
            id: RoleId(id),
            name: name.to_string(),
            permissions: Permissions::empty(),
            position: 0,
            managed: false,
        }
    }

    fn ident(key: &str, match_by: MatchStrategy) -> Identity {
        Identity {
            key: ResourceKey(key.to_string()),
            match_by,
            ..Default::default()
        }
    }

    #[test]
    fn by_name_zero_one_two() {
        let guild = guild_with_roles(vec![role(10, "a"), role(11, "b"), role(12, "b")]);
        let resolver = InMemoryMatchResolver::new(&guild);
        assert!(matches!(
            resolver.resolve_role(&ident("k", MatchStrategy::ByName), Some("a")),
            ResolveResult::Existing(_)
        ));
        assert!(matches!(
            resolver.resolve_role(&ident("k", MatchStrategy::ByName), Some("missing")),
            ResolveResult::Missing
        ));
        assert!(matches!(
            resolver.resolve_role(&ident("k", MatchStrategy::ByName), Some("b")),
            ResolveResult::Conflict { .. }
        ));
    }

    #[test]
    fn by_explicit_id() {
        let guild = guild_with_roles(vec![role(10, "a")]);
        let resolver = InMemoryMatchResolver::new(&guild);
        assert!(matches!(
            resolver.resolve_role(
                &ident("k", MatchStrategy::ByExplicitId("10".to_string())),
                None
            ),
            ResolveResult::Existing(_)
        ));
        assert!(matches!(
            resolver.resolve_role(
                &ident("k", MatchStrategy::ByExplicitId("99".to_string())),
                None
            ),
            ResolveResult::Missing
        ));
    }

    #[test]
    fn everyone_target_uses_guild_id() {
        use discord_model::OverwriteTarget;
        let guild = guild_with_roles(vec![]);
        let resolver = InMemoryMatchResolver::new(&guild);
        assert_eq!(
            resolver.everyone_overwrite_target(),
            OverwriteTarget::Role(RoleId(1))
        );
    }
}
