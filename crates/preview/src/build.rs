use std::collections::{BTreeSet, HashMap};

use desired_compiler::NormalizedTarget;
use desired_state::ResourceKey;
use diff_engine::DiffResult;
use operation_graph::{Operation, OperationGraph};
use policy_engine::{PolicyDecision, Verdict};
use simulator::AccessMatrix;
use virtual_apply::VirtualApplyResult;

use crate::model::{AccessChange, PreviewChange, PreviewChangeKind, PreviewModel, PreviewSeverity};

pub fn build_preview(
    title: &str,
    diff: &DiffResult,
    graph: &OperationGraph,
    policy: &PolicyDecision,
    apply: &VirtualApplyResult,
    before: &AccessMatrix,
    after: &AccessMatrix,
) -> PreviewModel {
    let verdict = policy.verdict;
    let approval_required = matches!(
        verdict,
        Verdict::RequireApproval | Verdict::RequireSecondApproval
    );
    let blocked = verdict == Verdict::Deny;

    let changes = graph
        .nodes
        .iter()
        .map(|node| change_of(&node.operation))
        .collect();
    let access_changes = access_changes(before, after);
    let deferred = diff
        .deferred
        .iter()
        .map(|item| format!("{}:{}", item.kind, item.key.0))
        .collect();

    PreviewModel {
        title: title.to_string(),
        verdict,
        approval_required,
        blocked,
        changes,
        access_changes,
        policy_findings: policy.findings.clone(),
        warnings: apply.warnings.clone(),
        deferred,
    }
}

fn change_of(op: &Operation) -> PreviewChange {
    match op {
        Operation::CreateRole { key, name, .. } => PreviewChange {
            kind: PreviewChangeKind::RoleCreate,
            target: label(name, key),
            severity: PreviewSeverity::Info,
        },
        Operation::UpdateRole { key, name, .. } => PreviewChange {
            kind: PreviewChangeKind::RoleUpdate,
            target: label(name, key),
            severity: PreviewSeverity::Info,
        },
        Operation::DeleteRole { key } => PreviewChange {
            kind: PreviewChangeKind::RoleDelete,
            target: key.0.clone(),
            severity: PreviewSeverity::Warning,
        },
        Operation::CreateChannel { key, name, .. } => PreviewChange {
            kind: PreviewChangeKind::ChannelCreate,
            target: label(name, key),
            severity: PreviewSeverity::Info,
        },
        Operation::UpdateChannel { key, name, .. } => PreviewChange {
            kind: PreviewChangeKind::ChannelUpdate,
            target: label(name, key),
            severity: PreviewSeverity::Info,
        },
        Operation::DeleteChannel { key } => PreviewChange {
            kind: PreviewChangeKind::ChannelDelete,
            target: key.0.clone(),
            severity: PreviewSeverity::Warning,
        },
        Operation::CreateOverwrite {
            channel, target, ..
        } => PreviewChange {
            kind: PreviewChangeKind::OverwriteCreate,
            target: format!("{} / {}", channel.0, target_label(target)),
            severity: overwrite_severity(target),
        },
        Operation::UpdateOverwrite {
            channel, target, ..
        } => PreviewChange {
            kind: PreviewChangeKind::OverwriteUpdate,
            target: format!("{} / {}", channel.0, target_label(target)),
            severity: overwrite_severity(target),
        },
    }
}

fn label(name: &Option<String>, key: &ResourceKey) -> String {
    name.clone().unwrap_or_else(|| key.0.clone())
}

fn target_label(target: &NormalizedTarget) -> String {
    match target {
        NormalizedTarget::Everyone => "@everyone".to_string(),
        NormalizedTarget::Role(key) => format!("role:{}", key.0),
        NormalizedTarget::Member(id) => format!("member:{id}"),
    }
}

fn overwrite_severity(target: &NormalizedTarget) -> PreviewSeverity {
    match target {
        NormalizedTarget::Everyone => PreviewSeverity::Notice,
        _ => PreviewSeverity::Info,
    }
}

fn access_changes(before: &AccessMatrix, after: &AccessMatrix) -> Vec<AccessChange> {
    let before_map: HashMap<(&str, &str), (bool, bool)> = before
        .cells
        .iter()
        .map(|c| {
            (
                (c.subject.as_str(), c.channel.as_str()),
                (c.can_view, c.can_send),
            )
        })
        .collect();
    let after_map: HashMap<(&str, &str), (bool, bool)> = after
        .cells
        .iter()
        .map(|c| {
            (
                (c.subject.as_str(), c.channel.as_str()),
                (c.can_view, c.can_send),
            )
        })
        .collect();
    let keys: BTreeSet<(&str, &str)> = before_map.keys().chain(after_map.keys()).copied().collect();
    let mut changes = Vec::new();
    for (subject, channel) in keys {
        let b = before_map
            .get(&(subject, channel))
            .copied()
            .unwrap_or((false, false));
        let a = after_map
            .get(&(subject, channel))
            .copied()
            .unwrap_or((false, false));
        if b != a {
            changes.push(AccessChange {
                subject: subject.to_string(),
                channel: channel.to_string(),
                before_can_view: b.0,
                after_can_view: a.0,
                before_can_send: b.1,
                after_can_send: a.1,
            });
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::{Guild, GuildId, GuildState, Permissions, UserId};
    use simulator::AccessCell;
    use std::collections::BTreeMap;

    fn apply_result(warnings: Vec<String>) -> VirtualApplyResult {
        VirtualApplyResult {
            after: GuildState {
                guild: Guild {
                    id: GuildId(1),
                    name: "g".to_string(),
                    owner_id: UserId(1),
                },
                roles: vec![],
                channels: vec![],
                members: vec![],
            },
            applied: vec![],
            synthetic_roles: BTreeMap::new(),
            synthetic_channels: BTreeMap::new(),
            warnings,
        }
    }

    fn cell(subject: &str, channel: &str, v: bool, s: bool) -> AccessCell {
        AccessCell {
            subject: subject.to_string(),
            channel: channel.to_string(),
            can_view: v,
            can_send: s,
        }
    }

    #[test]
    fn delete_role_is_warning_create_is_info() {
        let d = change_of(&Operation::DeleteRole {
            key: ResourceKey("vip".to_string()),
        });
        assert_eq!(d.kind, PreviewChangeKind::RoleDelete);
        assert_eq!(d.severity, PreviewSeverity::Warning);
        let c = change_of(&Operation::CreateRole {
            key: ResourceKey("vip".to_string()),
            name: Some("VIP".to_string()),
            permissions: None,
        });
        assert_eq!(c.severity, PreviewSeverity::Info);
        assert_eq!(c.target, "VIP");
    }

    #[test]
    fn everyone_overwrite_is_notice() {
        let c = change_of(&Operation::CreateOverwrite {
            channel: ResourceKey("general".to_string()),
            target: NormalizedTarget::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::empty(),
        });
        assert_eq!(c.severity, PreviewSeverity::Notice);
        assert_eq!(c.target, "general / @everyone");
    }

    #[test]
    fn access_changes_diff_union_and_unchanged() {
        let before = AccessMatrix {
            cells: vec![cell("new", "general", true, false)],
        };
        let after = AccessMatrix {
            cells: vec![
                cell("new", "general", false, false),
                cell("verified", "general", true, true),
            ],
        };
        let changes = access_changes(&before, &after);
        assert_eq!(changes.len(), 2);
        let v = changes.iter().find(|c| c.subject == "verified").unwrap();
        assert!(!v.before_can_view && v.after_can_view && v.after_can_send);
        let n = changes.iter().find(|c| c.subject == "new").unwrap();
        assert!(n.before_can_view && !n.after_can_view);

        let same = AccessMatrix {
            cells: vec![cell("new", "general", true, false)],
        };
        assert!(access_changes(&same, &same).is_empty());
    }

    #[test]
    fn build_preview_derives_verdict_flags() {
        let policy = PolicyDecision {
            verdict: Verdict::RequireApproval,
            findings: vec![],
        };
        let p = build_preview(
            "t",
            &DiffResult::default(),
            &OperationGraph::default(),
            &policy,
            &apply_result(vec!["w".to_string()]),
            &AccessMatrix::default(),
            &AccessMatrix::default(),
        );
        assert!(p.approval_required);
        assert!(!p.blocked);
        assert_eq!(p.warnings, vec!["w".to_string()]);
        assert!(serde_json::to_string(&p).is_ok());

        let denied = PolicyDecision {
            verdict: Verdict::Deny,
            findings: vec![],
        };
        let p2 = build_preview(
            "t",
            &DiffResult::default(),
            &OperationGraph::default(),
            &denied,
            &apply_result(vec![]),
            &AccessMatrix::default(),
            &AccessMatrix::default(),
        );
        assert!(p2.blocked);
        assert!(!p2.approval_required);
    }
}
