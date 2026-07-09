use std::collections::BTreeMap;

use desired_compiler::{NormalizedDesiredState, NormalizedTarget};
use desired_state::ResourceKey;
use diff_engine::{ResolveResult, ResourceResolver};
use discord_model::{
    Channel, ChannelId, ChannelType, GuildState, OverwriteTarget, PermissionOverwrite, Role,
    RoleId, UserId,
};
use operation_graph::{OpId, Operation, OperationGraph};

use crate::error::VirtualApplyError;
use crate::result::VirtualApplyResult;

pub fn apply(
    current: &GuildState,
    graph: &OperationGraph,
    normalized: &NormalizedDesiredState,
    resolver: &impl ResourceResolver,
) -> Result<VirtualApplyResult, VirtualApplyError> {
    let mut ctx = ApplyContext::new(current, normalized, resolver);
    let order = graph
        .topological_order()
        .map_err(|_| VirtualApplyError::GraphCycle)?;
    for id in order {
        if let Some(node) = graph.nodes.iter().find(|n| n.id == id) {
            ctx.apply_operation(&node.operation)?;
            ctx.applied.push(id);
        }
    }
    Ok(ctx.into_result())
}

struct ApplyContext<'a, R: ResourceResolver> {
    after: GuildState,
    normalized: &'a NormalizedDesiredState,
    resolver: &'a R,
    next_id: u64,
    role_ids: BTreeMap<ResourceKey, RoleId>,
    channel_ids: BTreeMap<ResourceKey, ChannelId>,
    synthetic_roles: BTreeMap<ResourceKey, RoleId>,
    synthetic_channels: BTreeMap<ResourceKey, ChannelId>,
    applied: Vec<OpId>,
    warnings: Vec<String>,
}

impl<'a, R: ResourceResolver> ApplyContext<'a, R> {
    fn new(current: &GuildState, normalized: &'a NormalizedDesiredState, resolver: &'a R) -> Self {
        let mut max_id = current.guild.id.0;
        for r in &current.roles {
            max_id = max_id.max(r.id.0);
        }
        for c in &current.channels {
            max_id = max_id.max(c.id.0);
        }
        Self {
            after: current.clone(),
            normalized,
            resolver,
            next_id: max_id + 1,
            role_ids: BTreeMap::new(),
            channel_ids: BTreeMap::new(),
            synthetic_roles: BTreeMap::new(),
            synthetic_channels: BTreeMap::new(),
            applied: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn next_synthetic(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn apply_operation(&mut self, op: &Operation) -> Result<(), VirtualApplyError> {
        match op {
            Operation::CreateRole {
                key,
                name,
                permissions,
            } => {
                let id = RoleId(self.next_synthetic());
                self.role_ids.insert(key.clone(), id);
                self.synthetic_roles.insert(key.clone(), id);
                self.after.roles.push(Role {
                    id,
                    name: name.clone().unwrap_or_default(),
                    permissions: permissions.unwrap_or_default(),
                    position: 0,
                    managed: false,
                });
            }
            Operation::UpdateRole {
                key,
                name,
                permissions,
            } => {
                let id = self.resolve_role(key)?;
                if let Some(role) = self.after.roles.iter_mut().find(|r| r.id == id) {
                    if let Some(n) = name {
                        role.name = n.clone();
                    }
                    if let Some(p) = permissions {
                        role.permissions = *p;
                    }
                }
            }
            Operation::DeleteRole { key } => {
                let id = self.resolve_role(key)?;
                self.after.roles.retain(|r| r.id != id);
            }
            Operation::CreateChannel {
                key,
                name,
                channel_type,
                parent,
            } => {
                let id = ChannelId(self.next_synthetic());
                self.channel_ids.insert(key.clone(), id);
                self.synthetic_channels.insert(key.clone(), id);
                let parent_id = match parent {
                    Some(pk) => Some(self.resolve_channel(pk)?),
                    None => None,
                };
                self.after.channels.push(Channel {
                    id,
                    name: name.clone().unwrap_or_default(),
                    channel_type: channel_type.unwrap_or(ChannelType::Text),
                    parent_id,
                    position: 0,
                    overwrites: Vec::new(),
                });
            }
            Operation::UpdateChannel {
                key,
                name,
                channel_type,
            } => {
                let id = self.resolve_channel(key)?;
                if let Some(ch) = self.after.channels.iter_mut().find(|c| c.id == id) {
                    if let Some(n) = name {
                        ch.name = n.clone();
                    }
                    if let Some(t) = channel_type {
                        ch.channel_type = *t;
                    }
                }
            }
            Operation::DeleteChannel { key } => {
                let id = self.resolve_channel(key)?;
                self.after.channels.retain(|c| c.id != id);
            }
            Operation::CreateOverwrite {
                channel,
                target,
                allow,
                deny,
            }
            | Operation::UpdateOverwrite {
                channel,
                target,
                allow,
                deny,
            } => {
                let channel_id = self.resolve_channel(channel)?;
                let ow_target = self.resolve_target(target)?;
                if let Some(ch) = self.after.channels.iter_mut().find(|c| c.id == channel_id) {
                    ch.overwrites.retain(|o| o.target != ow_target);
                    ch.overwrites.push(PermissionOverwrite {
                        target: ow_target,
                        allow: *allow,
                        deny: *deny,
                    });
                } else {
                    self.warnings
                        .push(format!("overwrite channel not found: {}", channel.0));
                }
            }
        }
        Ok(())
    }

    fn resolve_role(&mut self, key: &ResourceKey) -> Result<RoleId, VirtualApplyError> {
        if let Some(id) = self.role_ids.get(key) {
            return Ok(*id);
        }
        let resolved = {
            let nr = self
                .normalized
                .roles
                .iter()
                .find(|r| &r.identity.key == key)
                .ok_or_else(|| VirtualApplyError::MissingIdentity { key: key.0.clone() })?;
            self.resolver.resolve_role(&nr.identity, nr.name.as_deref())
        };
        match resolved {
            ResolveResult::Existing(role) => {
                self.role_ids.insert(key.clone(), role.id);
                Ok(role.id)
            }
            _ => Err(VirtualApplyError::UnresolvedKey { key: key.0.clone() }),
        }
    }

    fn resolve_channel(&mut self, key: &ResourceKey) -> Result<ChannelId, VirtualApplyError> {
        if let Some(id) = self.channel_ids.get(key) {
            return Ok(*id);
        }
        let resolved = {
            let nc = self
                .normalized
                .channels
                .iter()
                .find(|c| &c.identity.key == key)
                .ok_or_else(|| VirtualApplyError::MissingIdentity { key: key.0.clone() })?;
            self.resolver
                .resolve_channel(&nc.identity, nc.name.as_deref())
        };
        match resolved {
            ResolveResult::Existing(ch) => {
                self.channel_ids.insert(key.clone(), ch.id);
                Ok(ch.id)
            }
            _ => Err(VirtualApplyError::UnresolvedKey { key: key.0.clone() }),
        }
    }

    fn resolve_target(
        &mut self,
        target: &NormalizedTarget,
    ) -> Result<OverwriteTarget, VirtualApplyError> {
        match target {
            NormalizedTarget::Everyone => Ok(OverwriteTarget::Role(RoleId(self.after.guild.id.0))),
            NormalizedTarget::Role(key) => Ok(OverwriteTarget::Role(self.resolve_role(key)?)),
            NormalizedTarget::Member(id) => {
                let raw = id
                    .parse::<u64>()
                    .map_err(|_| VirtualApplyError::UnresolvedKey { key: id.clone() })?;
                Ok(OverwriteTarget::Member(UserId(raw)))
            }
        }
    }

    fn into_result(self) -> VirtualApplyResult {
        VirtualApplyResult {
            after: self.after,
            applied: self.applied,
            synthetic_roles: self.synthetic_roles,
            synthetic_channels: self.synthetic_channels,
            warnings: self.warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desired_compiler::{
        NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole,
        NormalizedTarget,
    };
    use desired_state::{Identity, ResourceKey};
    use diff_engine::InMemoryMatchResolver;
    use discord_model::{Guild, GuildId, GuildState, OverwriteTarget, Permissions, UserId};
    use operation_graph::{OpId, Operation, OperationGraph, OperationNode};

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

    fn nchannel(key: &str, name: &str, overwrites: Vec<NormalizedOverwrite>) -> NormalizedChannel {
        NormalizedChannel {
            identity: Identity {
                key: ResourceKey(key.to_string()),
                ..Default::default()
            },
            name: Some(name.to_string()),
            channel_type: None,
            parent: None,
            overwrites,
        }
    }

    fn node(id: u32, op: Operation) -> OperationNode {
        OperationNode {
            id: OpId(id),
            operation: op,
            produces: vec![],
            consumes: vec![],
            depends_on: vec![],
        }
    }

    #[test]
    fn create_role_gets_synthetic_id() {
        let normalized = NormalizedDesiredState {
            roles: vec![nrole("vip", "VIP")],
            ..Default::default()
        };
        let graph = OperationGraph {
            nodes: vec![node(
                0,
                Operation::CreateRole {
                    key: ResourceKey("vip".to_string()),
                    name: Some("VIP".to_string()),
                    permissions: Some(Permissions::empty()),
                },
            )],
        };
        let current = empty_guild();
        let resolver = InMemoryMatchResolver::new(&current);
        let result = apply(&current, &graph, &normalized, &resolver).unwrap();
        assert_eq!(result.after.roles.len(), 1);
        assert_eq!(result.after.roles[0].name, "VIP");
        assert!(result
            .synthetic_roles
            .contains_key(&ResourceKey("vip".to_string())));
        assert!(serde_json::to_string(&result).is_ok());
    }

    #[test]
    fn overwrite_threads_synthetic_role_id() {
        let vk = ResourceKey("verified".to_string());
        let ck = ResourceKey("gen".to_string());
        let normalized = NormalizedDesiredState {
            roles: vec![nrole("verified", "Verified")],
            channels: vec![nchannel(
                "gen",
                "gen",
                vec![NormalizedOverwrite {
                    target: NormalizedTarget::Role(vk.clone()),
                    allow: Permissions::VIEW_CHANNEL,
                    deny: Permissions::empty(),
                }],
            )],
            ..Default::default()
        };
        let graph = OperationGraph {
            nodes: vec![
                node(
                    0,
                    Operation::CreateRole {
                        key: vk.clone(),
                        name: Some("Verified".to_string()),
                        permissions: Some(Permissions::empty()),
                    },
                ),
                node(
                    1,
                    Operation::CreateChannel {
                        key: ck.clone(),
                        name: Some("gen".to_string()),
                        channel_type: None,
                        parent: None,
                    },
                ),
                node(
                    2,
                    Operation::CreateOverwrite {
                        channel: ck.clone(),
                        target: NormalizedTarget::Role(vk.clone()),
                        allow: Permissions::VIEW_CHANNEL,
                        deny: Permissions::empty(),
                    },
                ),
            ],
        };
        let current = empty_guild();
        let resolver = InMemoryMatchResolver::new(&current);
        let result = apply(&current, &graph, &normalized, &resolver).unwrap();
        let synthetic_role = result.synthetic_roles[&vk];
        let ch = result
            .after
            .channels
            .iter()
            .find(|c| c.name == "gen")
            .unwrap();
        assert!(ch
            .overwrites
            .iter()
            .any(|o| o.target == OverwriteTarget::Role(synthetic_role)));
    }

    #[test]
    fn missing_existing_key_errors() {
        let normalized = NormalizedDesiredState {
            channels: vec![nchannel("gen", "gen", vec![])],
            ..Default::default()
        };
        let graph = OperationGraph {
            nodes: vec![node(
                0,
                Operation::UpdateChannel {
                    key: ResourceKey("gen".to_string()),
                    name: Some("gen".to_string()),
                    channel_type: None,
                },
            )],
        };
        let current = empty_guild();
        let resolver = InMemoryMatchResolver::new(&current);
        assert!(matches!(
            apply(&current, &graph, &normalized, &resolver),
            Err(VirtualApplyError::UnresolvedKey { .. })
        ));
    }
}
