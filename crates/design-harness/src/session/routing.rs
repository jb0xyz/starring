use std::collections::BTreeSet;

use crate::draft::Draft;
use crate::tools::{tool_definitions, ToolDefinition};
use crate::turn::{control_tool_definitions, required_mutation_tools, AdaptivePhase};

use super::{DesignSession, RepairState};

impl<C> DesignSession<C> {
    pub(super) fn routed_tools(&self) -> Vec<ToolDefinition> {
        match self.repair_state.as_ref() {
            Some(RepairState::AwaitingAttempt(ticket)) => self
                .tools
                .iter()
                .filter(|tool| ticket.allowed_repair_tools.contains(&tool.name))
                .cloned()
                .collect(),
            Some(RepairState::VerifyValidation(_)) => {
                definitions_named(&self.tools, &["validate_draft"])
            }
            Some(RepairState::VerifySimulation(_)) => {
                definitions_named(&self.tools, &["simulate_draft"])
            }
            Some(RepairState::Failed(_)) => Vec::new(),
            None => self.adaptive_routed_tools(),
        }
    }

    fn adaptive_routed_tools(&self) -> Vec<ToolDefinition> {
        let Some(state) = self.adaptive_turn.as_ref() else {
            return routed_tool_definitions(&self.draft, &self.tools);
        };
        match state.phase {
            AdaptivePhase::Assess => definitions_named(&self.tools, &["set_turn_brief"]),
            AdaptivePhase::Build => {
                if self.planned_enabled
                    && state
                        .brief
                        .as_ref()
                        .is_some_and(|brief| brief.intent == crate::turn::TurnIntent::Build)
                {
                    let planned = state
                        .brief
                        .as_ref()
                        .is_some_and(|brief| !brief.requirements.is_empty());
                    return if planned {
                        Vec::new()
                    } else {
                        definitions_named(&self.tools, &["set_turn_plan"])
                    };
                }
                let mut names = state
                    .brief
                    .as_ref()
                    .map(required_mutation_tools)
                    .unwrap_or_default();
                if state
                    .brief
                    .as_ref()
                    .is_some_and(|brief| brief.requirements.is_empty())
                {
                    names.extend(
                        routed_tool_definitions(&self.draft, &self.tools)
                            .into_iter()
                            .filter(|tool| is_mutation_tool(&tool.name))
                            .map(|tool| tool.name),
                    );
                }
                names.insert("check_turn_scope".to_string());
                names.insert("finish_turn".to_string());
                definitions_in_registry_order(&self.tools, &names)
            }
            AdaptivePhase::Verify => definitions_named(&self.tools, &["validate_draft"]),
            AdaptivePhase::Simulate => definitions_named(&self.tools, &["simulate_draft"]),
            AdaptivePhase::Preview => definitions_named(&self.tools, &["render_preview"]),
            AdaptivePhase::Reply => definitions_named(&self.tools, &["finish_turn"]),
        }
    }
}

pub(super) fn definitions_named(
    registry: &[ToolDefinition],
    names: &[&str],
) -> Vec<ToolDefinition> {
    registry
        .iter()
        .filter(|tool| names.contains(&tool.name.as_str()))
        .cloned()
        .collect()
}

pub(super) fn routed_tool_definitions(
    draft: &Draft,
    registry: &[ToolDefinition],
) -> Vec<ToolDefinition> {
    registry
        .iter()
        .filter(|tool| tool_is_available(draft, &tool.name))
        .cloned()
        .collect()
}

pub(super) fn tool_is_available(draft: &Draft, name: &str) -> bool {
    let has_rules = !draft.ruleset.rules.is_empty();
    match name {
        "add_panel" | "add_modal" | "begin_rule" => true,
        "add_button" => !draft.ruleset.panels.is_empty(),
        "update_panel" | "remove_panel" => !draft.ruleset.panels.is_empty(),
        "update_button" | "remove_button" => draft
            .ruleset
            .panels
            .iter()
            .any(|panel| !panel.buttons.is_empty()),
        "update_modal" | "remove_modal" => !draft.ruleset.modals.is_empty(),
        "update_rule" | "remove_rule" | "update_action" | "remove_action" => has_rules,
        "add_resource_action"
        | "add_upsert_overwrite_action"
        | "add_interaction_action"
        | "add_post_panel_action" => has_rules,
        "add_grant_role_action" => has_rules && has_created_role(draft),
        "set_register_instance" => has_rules && has_ownable_action(draft),
        "validate_draft" => {
            has_rules
                && all_rules_have_actions(draft)
                && draft.validated_revision != Some(draft.draft_revision)
        }
        "simulate_draft" => has_rules && draft.validated_revision == Some(draft.draft_revision),
        _ => false,
    }
}

fn has_created_role(draft: &Draft) -> bool {
    draft.ruleset.rules.iter().any(|rule| {
        rule.actions
            .iter()
            .any(|action| matches!(action, automation_state::ActionSpec::CreateRole { .. }))
    })
}

fn has_ownable_action(draft: &Draft) -> bool {
    draft.ruleset.rules.iter().any(|rule| {
        rule.actions.iter().any(|action| {
            matches!(
                action,
                automation_state::ActionSpec::CreateRole { .. }
                    | automation_state::ActionSpec::CreateChannel { .. }
                    | automation_state::ActionSpec::PostPanel { .. }
            )
        })
    })
}

fn all_rules_have_actions(draft: &Draft) -> bool {
    draft
        .ruleset
        .rules
        .iter()
        .all(|rule| !rule.actions.is_empty())
}

pub(super) fn is_mutation_tool(name: &str) -> bool {
    matches!(
        name,
        "add_panel"
            | "add_button"
            | "add_modal"
            | "begin_rule"
            | "add_resource_action"
            | "add_grant_role_action"
            | "add_upsert_overwrite_action"
            | "add_interaction_action"
            | "add_post_panel_action"
            | "set_register_instance"
            | "update_panel"
            | "remove_panel"
            | "update_button"
            | "remove_button"
            | "update_modal"
            | "remove_modal"
            | "update_rule"
            | "remove_rule"
            | "update_action"
            | "remove_action"
    )
}

pub(super) fn is_control_tool(name: &str) -> bool {
    matches!(
        name,
        "set_turn_brief" | "set_turn_plan" | "check_turn_scope" | "render_preview" | "finish_turn"
    )
}

pub(super) fn all_tool_definitions() -> Vec<ToolDefinition> {
    let mut definitions = tool_definitions();
    definitions.extend(control_tool_definitions());
    definitions
}

pub(super) fn legacy_tool_definitions() -> Vec<ToolDefinition> {
    tool_definitions()
        .into_iter()
        .filter(|tool| !is_edit_tool(&tool.name))
        .collect()
}

fn is_edit_tool(name: &str) -> bool {
    matches!(
        name,
        "update_panel"
            | "remove_panel"
            | "update_button"
            | "remove_button"
            | "update_modal"
            | "remove_modal"
            | "update_rule"
            | "remove_rule"
            | "update_action"
            | "remove_action"
    )
}

fn definitions_in_registry_order(
    registry: &[ToolDefinition],
    names: &BTreeSet<String>,
) -> Vec<ToolDefinition> {
    registry
        .iter()
        .filter(|tool| names.contains(&tool.name))
        .cloned()
        .collect()
}
