use std::collections::HashMap;

use desired_compiler::{
    NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole,
    NormalizedTarget,
};
use desired_state::ResourceKey;
use diff_engine::{ChangeOp, DiffChange, DiffResult, DiffTarget};

use crate::error::OperationGraphError;
use crate::node::{OpId, Operation, OperationGraph, OperationNode};
use crate::symbol::ResourceSymbol;

pub fn compile_operations(
    diff: &DiffResult,
    desired: &NormalizedDesiredState,
) -> Result<OperationGraph, OperationGraphError> {
    if !diff.conflicts.is_empty() {
        return Err(OperationGraphError::DiffHasConflicts(diff.conflicts.len()));
    }

    let mut nodes = Vec::new();
    let mut next_id = 0u32;
    for change in &diff.changes {
        if change.op == ChangeOp::NoOp {
            continue;
        }
        let (operation, produces, consumes) = build_operation(change, desired)?;
        nodes.push(OperationNode {
            id: OpId(next_id),
            operation,
            produces,
            consumes,
            depends_on: Vec::new(),
        });
        next_id += 1;
    }

    derive_dependencies(&mut nodes);
    let graph = OperationGraph { nodes };
    graph.topological_order()?;
    Ok(graph)
}

fn derive_dependencies(nodes: &mut [OperationNode]) {
    let mut producers: HashMap<ResourceSymbol, OpId> = HashMap::new();
    for node in nodes.iter() {
        for symbol in &node.produces {
            producers.insert(symbol.clone(), node.id);
        }
    }
    for node in nodes.iter_mut() {
        let mut deps: Vec<OpId> = node
            .consumes
            .iter()
            .filter_map(|symbol| producers.get(symbol).copied())
            .filter(|dep| *dep != node.id)
            .collect();
        deps.sort();
        deps.dedup();
        node.depends_on = deps;
    }
}

fn build_operation(
    change: &DiffChange,
    desired: &NormalizedDesiredState,
) -> Result<(Operation, Vec<ResourceSymbol>, Vec<ResourceSymbol>), OperationGraphError> {
    match (change.op, &change.target) {
        (ChangeOp::Create, DiffTarget::Role { key }) => {
            let role = find_role(desired, key)?;
            Ok((
                Operation::CreateRole {
                    key: key.clone(),
                    name: role.name.clone(),
                    permissions: role.permissions,
                },
                vec![ResourceSymbol::Role(key.clone())],
                vec![],
            ))
        }
        (ChangeOp::Update, DiffTarget::Role { key }) => {
            let role = find_role(desired, key)?;
            Ok((
                Operation::UpdateRole {
                    key: key.clone(),
                    name: role.name.clone(),
                    permissions: role.permissions,
                },
                vec![],
                vec![],
            ))
        }
        (ChangeOp::Delete, DiffTarget::Role { key }) => {
            Ok((Operation::DeleteRole { key: key.clone() }, vec![], vec![]))
        }
        (ChangeOp::Create, DiffTarget::Channel { key }) => {
            let channel = find_channel(desired, key)?;
            let consumes = channel
                .parent
                .as_ref()
                .map(|parent| vec![ResourceSymbol::Channel(parent.clone())])
                .unwrap_or_default();
            Ok((
                Operation::CreateChannel {
                    key: key.clone(),
                    name: channel.name.clone(),
                    channel_type: channel.channel_type,
                    parent: channel.parent.clone(),
                },
                vec![ResourceSymbol::Channel(key.clone())],
                consumes,
            ))
        }
        (ChangeOp::Update, DiffTarget::Channel { key }) => {
            let channel = find_channel(desired, key)?;
            Ok((
                Operation::UpdateChannel {
                    key: key.clone(),
                    name: channel.name.clone(),
                    channel_type: channel.channel_type,
                },
                vec![],
                vec![],
            ))
        }
        (ChangeOp::Delete, DiffTarget::Channel { key }) => Ok((
            Operation::DeleteChannel { key: key.clone() },
            vec![],
            vec![],
        )),
        (ChangeOp::Create, DiffTarget::Overwrite { channel, target }) => {
            let overwrite = find_overwrite(desired, channel, target)?;
            Ok((
                Operation::CreateOverwrite {
                    channel: channel.clone(),
                    target: target.clone(),
                    allow: overwrite.allow,
                    deny: overwrite.deny,
                },
                vec![],
                overwrite_consumes(channel, target),
            ))
        }
        (ChangeOp::Update, DiffTarget::Overwrite { channel, target }) => {
            let overwrite = find_overwrite(desired, channel, target)?;
            Ok((
                Operation::UpdateOverwrite {
                    channel: channel.clone(),
                    target: target.clone(),
                    allow: overwrite.allow,
                    deny: overwrite.deny,
                },
                vec![],
                overwrite_consumes(channel, target),
            ))
        }
        _ => Err(OperationGraphError::UnsupportedChange),
    }
}

fn overwrite_consumes(channel: &ResourceKey, target: &NormalizedTarget) -> Vec<ResourceSymbol> {
    let mut consumes = vec![ResourceSymbol::Channel(channel.clone())];
    if let NormalizedTarget::Role(role_key) = target {
        consumes.push(ResourceSymbol::Role(role_key.clone()));
    }
    consumes
}

fn find_role<'a>(
    desired: &'a NormalizedDesiredState,
    key: &ResourceKey,
) -> Result<&'a NormalizedRole, OperationGraphError> {
    desired
        .roles
        .iter()
        .find(|role| &role.identity.key == key)
        .ok_or_else(|| OperationGraphError::MissingPayload { key: key.0.clone() })
}

fn find_channel<'a>(
    desired: &'a NormalizedDesiredState,
    key: &ResourceKey,
) -> Result<&'a NormalizedChannel, OperationGraphError> {
    desired
        .channels
        .iter()
        .find(|channel| &channel.identity.key == key)
        .ok_or_else(|| OperationGraphError::MissingPayload { key: key.0.clone() })
}

fn find_overwrite<'a>(
    desired: &'a NormalizedDesiredState,
    channel: &ResourceKey,
    target: &NormalizedTarget,
) -> Result<&'a NormalizedOverwrite, OperationGraphError> {
    let channel = find_channel(desired, channel)?;
    channel
        .overwrites
        .iter()
        .find(|overwrite| &overwrite.target == target)
        .ok_or_else(|| OperationGraphError::MissingPayload {
            key: channel.identity.key.0.clone(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use desired_compiler::{
        NormalizedChannel, NormalizedDesiredState, NormalizedOverwrite, NormalizedRole,
        NormalizedTarget,
    };
    use desired_state::{Identity, ResourceKey};
    use diff_engine::{ChangeOp, DiffChange, DiffConflict, DiffResult, DiffTarget};
    use discord_model::Permissions;

    fn nrole(key: &str) -> NormalizedRole {
        NormalizedRole {
            identity: Identity {
                key: ResourceKey(key.to_string()),
                ..Default::default()
            },
            name: Some(key.to_string()),
            permissions: Some(Permissions::empty()),
        }
    }

    fn nchannel(key: &str, overwrites: Vec<NormalizedOverwrite>) -> NormalizedChannel {
        NormalizedChannel {
            identity: Identity {
                key: ResourceKey(key.to_string()),
                ..Default::default()
            },
            name: Some(key.to_string()),
            channel_type: None,
            parent: None,
            overwrites,
        }
    }

    fn create(target: DiffTarget) -> DiffChange {
        DiffChange {
            op: ChangeOp::Create,
            target,
            changed: vec![],
        }
    }

    #[test]
    fn create_role_produces_symbol() {
        let desired = NormalizedDesiredState {
            roles: vec![nrole("r")],
            ..Default::default()
        };
        let diff = DiffResult {
            changes: vec![create(DiffTarget::Role {
                key: ResourceKey("r".to_string()),
            })],
            ..Default::default()
        };
        let graph = compile_operations(&diff, &desired).unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(
            graph.nodes[0].produces,
            vec![ResourceSymbol::Role(ResourceKey("r".to_string()))]
        );
    }

    #[test]
    fn create_overwrite_consumes_channel_and_role() {
        let overwrite = NormalizedOverwrite {
            target: NormalizedTarget::Role(ResourceKey("r".to_string())),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        };
        let desired = NormalizedDesiredState {
            channels: vec![nchannel("c", vec![overwrite])],
            ..Default::default()
        };
        let diff = DiffResult {
            changes: vec![create(DiffTarget::Overwrite {
                channel: ResourceKey("c".to_string()),
                target: NormalizedTarget::Role(ResourceKey("r".to_string())),
            })],
            ..Default::default()
        };
        let graph = compile_operations(&diff, &desired).unwrap();
        assert!(graph.nodes[0]
            .consumes
            .contains(&ResourceSymbol::Channel(ResourceKey("c".to_string()))));
        assert!(graph.nodes[0]
            .consumes
            .contains(&ResourceSymbol::Role(ResourceKey("r".to_string()))));
    }

    #[test]
    fn overwrite_depends_on_role_create() {
        let overwrite = NormalizedOverwrite {
            target: NormalizedTarget::Role(ResourceKey("r".to_string())),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        };
        let desired = NormalizedDesiredState {
            roles: vec![nrole("r")],
            channels: vec![nchannel("c", vec![overwrite])],
            ..Default::default()
        };
        let diff = DiffResult {
            changes: vec![
                create(DiffTarget::Role {
                    key: ResourceKey("r".to_string()),
                }),
                create(DiffTarget::Channel {
                    key: ResourceKey("c".to_string()),
                }),
                create(DiffTarget::Overwrite {
                    channel: ResourceKey("c".to_string()),
                    target: NormalizedTarget::Role(ResourceKey("r".to_string())),
                }),
            ],
            ..Default::default()
        };
        let graph = compile_operations(&diff, &desired).unwrap();
        let role_id = graph
            .nodes
            .iter()
            .find(|node| matches!(&node.operation, Operation::CreateRole { .. }))
            .unwrap()
            .id;
        let overwrite_node = graph
            .nodes
            .iter()
            .find(|node| matches!(&node.operation, Operation::CreateOverwrite { .. }))
            .unwrap();
        assert!(overwrite_node.depends_on.contains(&role_id));
        assert!(graph.topological_order().is_ok());
    }

    #[test]
    fn conflicts_block_compile() {
        let diff = DiffResult {
            conflicts: vec![DiffConflict {
                target: DiffTarget::Role {
                    key: ResourceKey("r".to_string()),
                },
                reason: "x".to_string(),
            }],
            ..Default::default()
        };
        let desired = NormalizedDesiredState::default();
        assert!(matches!(
            compile_operations(&diff, &desired),
            Err(OperationGraphError::DiffHasConflicts(1))
        ));
    }

    #[test]
    fn noop_produces_no_node() {
        let diff = DiffResult {
            changes: vec![DiffChange {
                op: ChangeOp::NoOp,
                target: DiffTarget::Role {
                    key: ResourceKey("r".to_string()),
                },
                changed: vec![],
            }],
            ..Default::default()
        };
        let desired = NormalizedDesiredState {
            roles: vec![nrole("r")],
            ..Default::default()
        };
        assert!(compile_operations(&diff, &desired)
            .unwrap()
            .nodes
            .is_empty());
    }
}
