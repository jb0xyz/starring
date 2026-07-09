use desired_compiler::NormalizedTarget;
use discord_model::Permissions;
use operation_graph::{Operation, OperationGraph};

use crate::finding::Finding;
use crate::rule::PolicyRule;
use crate::verdict::Verdict;

fn privileged_mask() -> Permissions {
    Permissions::ADMINISTRATOR
        | Permissions::MANAGE_GUILD
        | Permissions::MANAGE_ROLES
        | Permissions::MANAGE_CHANNELS
        | Permissions::KICK_MEMBERS
        | Permissions::BAN_MEMBERS
        | Permissions::MENTION_EVERYONE
        | Permissions::MODERATE_MEMBERS
}

pub struct PrivilegedPermissionRule;

impl PolicyRule for PrivilegedPermissionRule {
    fn id(&self) -> &str {
        "privileged-permission"
    }

    fn evaluate(&self, graph: &OperationGraph) -> Vec<Finding> {
        let mask = privileged_mask();
        let mut findings = Vec::new();
        for node in &graph.nodes {
            let (granted, target) = match &node.operation {
                Operation::CreateRole {
                    key, permissions, ..
                }
                | Operation::UpdateRole {
                    key, permissions, ..
                } => (*permissions, format!("role:{}", key.0)),
                Operation::CreateOverwrite { channel, allow, .. }
                | Operation::UpdateOverwrite { channel, allow, .. } => {
                    (Some(*allow), format!("overwrite:{}", channel.0))
                }
                _ => continue,
            };
            if let Some(permissions) = granted {
                if permissions.bits() & mask.bits() != 0 {
                    findings.push(Finding {
                        rule_id: self.id().to_string(),
                        verdict: Verdict::Deny,
                        target,
                        message: "privileged permission granted".to_string(),
                    });
                }
            }
        }
        findings
    }
}

pub struct DestructiveOperationRule;

impl PolicyRule for DestructiveOperationRule {
    fn id(&self) -> &str {
        "destructive-operation"
    }

    fn evaluate(&self, graph: &OperationGraph) -> Vec<Finding> {
        let mut findings = Vec::new();
        for node in &graph.nodes {
            let (verdict, target, message) = match &node.operation {
                Operation::DeleteRole { key } => (
                    Verdict::RequireApproval,
                    format!("role:{}", key.0),
                    "role deletion",
                ),
                Operation::DeleteChannel { key } => (
                    Verdict::RequireSecondApproval,
                    format!("channel:{}", key.0),
                    "channel deletion",
                ),
                _ => continue,
            };
            findings.push(Finding {
                rule_id: self.id().to_string(),
                verdict,
                target,
                message: message.to_string(),
            });
        }
        findings
    }
}

pub struct EveryoneChangeRule;

impl PolicyRule for EveryoneChangeRule {
    fn id(&self) -> &str {
        "everyone-change"
    }

    fn evaluate(&self, graph: &OperationGraph) -> Vec<Finding> {
        let mut findings = Vec::new();
        for node in &graph.nodes {
            let (channel, target) = match &node.operation {
                Operation::CreateOverwrite {
                    channel, target, ..
                }
                | Operation::UpdateOverwrite {
                    channel, target, ..
                } => (channel, target),
                _ => continue,
            };
            if matches!(target, NormalizedTarget::Everyone) {
                findings.push(Finding {
                    rule_id: self.id().to_string(),
                    verdict: Verdict::RequireApproval,
                    target: format!("overwrite:{}:everyone", channel.0),
                    message: "changes @everyone access".to_string(),
                });
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::PolicyRule;
    use crate::verdict::Verdict;
    use desired_compiler::NormalizedTarget;
    use desired_state::ResourceKey;
    use discord_model::Permissions;
    use operation_graph::{OpId, Operation, OperationGraph, OperationNode};

    fn node(operation: Operation) -> OperationNode {
        OperationNode {
            id: OpId(0),
            operation,
            produces: vec![],
            consumes: vec![],
            depends_on: vec![],
        }
    }

    fn graph(operations: Vec<Operation>) -> OperationGraph {
        OperationGraph {
            nodes: operations.into_iter().map(node).collect(),
        }
    }

    #[test]
    fn privileged_admin_denied() {
        let graph = graph(vec![Operation::CreateRole {
            key: ResourceKey("a".to_string()),
            name: Some("Admin".to_string()),
            permissions: Some(Permissions::ADMINISTRATOR),
        }]);
        let findings = PrivilegedPermissionRule.evaluate(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].verdict, Verdict::Deny);
    }

    #[test]
    fn privileged_safe_role_ok() {
        let graph = graph(vec![Operation::CreateRole {
            key: ResourceKey("vip".to_string()),
            name: Some("VIP".to_string()),
            permissions: Some(Permissions::empty()),
        }]);
        assert!(PrivilegedPermissionRule.evaluate(&graph).is_empty());
    }

    #[test]
    fn privileged_in_overwrite_denied() {
        let graph = graph(vec![Operation::CreateOverwrite {
            channel: ResourceKey("c".to_string()),
            target: NormalizedTarget::Role(ResourceKey("r".to_string())),
            allow: Permissions::MANAGE_ROLES,
            deny: Permissions::empty(),
        }]);
        assert_eq!(
            PrivilegedPermissionRule.evaluate(&graph)[0].verdict,
            Verdict::Deny
        );
    }

    #[test]
    fn destructive_verdicts() {
        let graph = graph(vec![
            Operation::DeleteRole {
                key: ResourceKey("r".to_string()),
            },
            Operation::DeleteChannel {
                key: ResourceKey("c".to_string()),
            },
        ]);
        let findings = DestructiveOperationRule.evaluate(&graph);
        assert!(findings
            .iter()
            .any(|finding| finding.verdict == Verdict::RequireApproval));
        assert!(findings
            .iter()
            .any(|finding| finding.verdict == Verdict::RequireSecondApproval));
    }

    #[test]
    fn everyone_change_requires_approval() {
        let g = graph(vec![Operation::CreateOverwrite {
            channel: ResourceKey("gen".to_string()),
            target: NormalizedTarget::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        }]);
        let findings = EveryoneChangeRule.evaluate(&g);
        assert_eq!(findings[0].verdict, Verdict::RequireApproval);
        let g2 = graph(vec![Operation::CreateOverwrite {
            channel: ResourceKey("gen".to_string()),
            target: NormalizedTarget::Role(ResourceKey("r".to_string())),
            allow: Permissions::empty(),
            deny: Permissions::empty(),
        }]);
        assert!(EveryoneChangeRule.evaluate(&g2).is_empty());
    }
}
