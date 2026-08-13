use std::collections::BTreeSet;

use automation_spec::{
    ActionNodeV1, ActionV1, AutomationSpecV1, ConditionExprV1, WorkflowSpecV1,
    AUTOMATION_SPEC_KIND_V1, AUTOMATION_SPEC_SCHEMA_VERSION_V1,
};

use crate::model::{StatefulBranchV1, StatefulSpecV1};

#[derive(Clone, Copy)]
pub(crate) enum BranchViewV1 {
    True,
    False,
}

/// Builds a private legacy-shaped view solely to reuse AutomationSpec's structural validator.
/// The view is never a compilation or deployment artifact.
pub(crate) fn automation_spec_validation_view_v1(
    spec: &StatefulSpecV1,
    selected: BranchViewV1,
) -> AutomationSpecV1 {
    let user_node_ids = all_node_ids(spec);
    let mut workflows = spec.stateless_workflows.clone();
    workflows.extend(spec.stateful_workflows.iter().enumerate().map(
        |(workflow_index, workflow)| {
            let branch = match selected {
                BranchViewV1::True => &workflow.on_true,
                BranchViewV1::False => &workflow.on_false,
            };
            let mut actions = Vec::with_capacity(branch.effects.len() + 2);
            actions.push(ActionNodeV1 {
                id: implicit_ack_id(workflow_index, &user_node_ids),
                action: ActionV1::DeferEphemeral,
            });
            actions.extend(branch.effects.iter().cloned());
            actions.push(ActionNodeV1 {
                id: branch.response.id.clone(),
                action: ActionV1::EditResponse {
                    content: branch.response.content.clone(),
                },
            });
            WorkflowSpecV1 {
                id: workflow.id.clone(),
                trigger: workflow.trigger.clone(),
                condition: ConditionExprV1::Always,
                actions,
            }
        },
    ));

    AutomationSpecV1 {
        schema_version: AUTOMATION_SPEC_SCHEMA_VERSION_V1,
        kind: AUTOMATION_SPEC_KIND_V1.to_string(),
        key: spec.key.clone(),
        display_name: spec.display_name.clone(),
        description: spec.description.clone(),
        panels: spec.panels.clone(),
        modals: spec.modals.clone(),
        workflows,
    }
}

fn all_node_ids(spec: &StatefulSpecV1) -> BTreeSet<&str> {
    let mut ids = BTreeSet::new();
    for workflow in &spec.stateless_workflows {
        ids.extend(workflow.actions.iter().map(|node| node.id.as_str()));
    }
    for workflow in &spec.stateful_workflows {
        for branch in [&workflow.on_true, &workflow.on_false] {
            ids.extend(branch.state_actions.iter().map(|node| node.id.as_str()));
            ids.extend(branch.effects.iter().map(|node| node.id.as_str()));
            ids.insert(branch.response.id.as_str());
        }
    }
    ids
}

fn implicit_ack_id(workflow_index: usize, user_node_ids: &BTreeSet<&str>) -> String {
    let prefix = format!("implicit_ack_{workflow_index}");
    if !user_node_ids.contains(prefix.as_str()) {
        return prefix;
    }
    for nonce in 1..=u16::MAX {
        let candidate = format!("implicit_ack_{workflow_index}_{nonce}");
        if !user_node_ids.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("bounded stateful specs cannot exhaust synthetic ACK identifiers")
}

#[allow(dead_code)]
fn _assert_branch_shape(branch: &StatefulBranchV1) -> usize {
    branch.effects.len() + 2
}
