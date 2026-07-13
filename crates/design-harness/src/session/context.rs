use std::collections::{BTreeMap, BTreeSet, VecDeque};

use automation_state::{ActionSpec, ButtonRoute, TriggerSpec};
use serde::{Deserialize, Serialize};

use crate::draft::Draft;
use crate::errors::StructuredError;
use crate::llm::{Message, MessageRole};
use crate::tools::ToolDefinition;
use crate::turn::{AdaptiveTurnState, TurnBrief};

use super::{
    is_genuine_human_message, DesignSession, Observability, RepairKind, RepairState,
    MAX_BRIEF_MEMORY_ITEMS, MAX_ERROR_MEMORY_CHARS, MAX_INTENT_MEMORY_CHARS,
    MAX_INTENT_MEMORY_ITEMS,
};

impl<C> DesignSession<C> {
    fn total_context_chars(messages: &[Message], tools: &[ToolDefinition]) -> usize {
        let tools = serde_json::to_string(tools).map_or(usize::MAX, |value| value.len());
        let messages = serde_json::to_string(messages).map_or(usize::MAX, |value| value.len());
        tools.saturating_add(messages)
    }

    pub(super) fn append_anchor(&mut self) {
        self.messages.push(Message::system(anchor_content(
            &self.draft,
            &self.observability,
            self.last_error.as_ref(),
            self.repair_state.as_ref(),
            self.adaptive_turn.as_ref(),
            &self.brief_history,
            &self.messages,
        )));
    }

    pub(super) fn fit_context(&self, tools: &[ToolDefinition]) -> Option<Vec<Message>> {
        if Self::total_context_chars(&self.messages, tools) <= self.config.context_char_budget {
            return Some(self.messages.clone());
        }
        let system = self.messages.first()?.clone();
        let anchor_index = self.messages.iter().rposition(is_anchor)?;
        let anchor = self.messages.get(anchor_index)?.clone();
        let groups = canonical_message_groups(&self.messages[1..anchor_index])?;
        let mut selected = VecDeque::new();
        let base = vec![system.clone(), anchor.clone()];
        if Self::total_context_chars(&base, tools) > self.config.context_char_budget {
            return None;
        }
        for group in groups.into_iter().rev() {
            let mut candidate = Vec::new();
            candidate.push(system.clone());
            candidate.extend(group.iter().cloned());
            candidate.extend(selected.iter().flatten().cloned());
            candidate.push(anchor.clone());
            if Self::total_context_chars(&candidate, tools) > self.config.context_char_budget {
                break;
            }
            selected.push_front(group);
        }
        let mut outbound = vec![system];
        outbound.extend(selected.into_iter().flatten());
        outbound.push(anchor);
        Some(outbound)
    }
}

pub(super) fn is_anchor(message: &Message) -> bool {
    message.role == MessageRole::System && message.content.starts_with("DRAFT_STATE:")
}

fn canonical_message_groups(messages: &[Message]) -> Option<Vec<Vec<Message>>> {
    let mut groups = Vec::new();
    let mut index = 0;
    while index < messages.len() {
        let message = &messages[index];
        if is_anchor(message) {
            index += 1;
            continue;
        }
        if message.role == MessageRole::Tool {
            return None;
        }
        if message.role == MessageRole::Assistant && !message.tool_calls.is_empty() {
            let mut expected = message
                .tool_calls
                .iter()
                .map(|call| call.id.clone())
                .collect::<BTreeSet<_>>();
            if expected.len() != message.tool_calls.len() {
                return None;
            }
            let mut group = vec![message.clone()];
            index += 1;
            while index < messages.len() && messages[index].role == MessageRole::Tool {
                let tool = messages[index].clone();
                let tool_call_id = tool.tool_call_id.as_deref()?;
                if !expected.remove(tool_call_id) {
                    return None;
                }
                group.push(tool);
                index += 1;
            }
            if !expected.is_empty() {
                return None;
            }
            groups.push(group);
            continue;
        }
        groups.push(vec![message.clone()]);
        index += 1;
    }
    Some(groups)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DraftStateMemory {
    pub(super) revision: u64,
    validated_revision: Option<u64>,
    simulated_revision: Option<u64>,
    panels: Vec<PanelMemory>,
    modals: Vec<ModalMemory>,
    rules: Vec<RuleMemory>,
    created_aliases: CreatedAliasMemory,
    unresolved_references: Vec<String>,
    failure_signatures: BTreeMap<String, usize>,
    last_error: Option<ErrorMemory>,
    repair_state: Option<RepairMemory>,
    #[serde(default)]
    adaptive_turn: Option<AdaptiveTurnState>,
    #[serde(default)]
    recent_turn_briefs: Vec<TurnBrief>,
    current_human_intent: Option<String>,
    recent_human_intent: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ErrorMemory {
    code: String,
    location: String,
    message: String,
    hint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RepairStageMemory {
    AwaitingAttempt,
    VerifyValidation,
    VerifySimulation,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RepairMemory {
    stage: RepairStageMemory,
    kind: RepairKind,
    original_tool: String,
    error: ErrorMemory,
    allowed_tools: Vec<String>,
    verification_path: Vec<String>,
    attempts_remaining: u8,
    root_revision: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PanelMemory {
    key: String,
    buttons: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ModalMemory {
    key: String,
    fields: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RuleMemory {
    key: String,
    trigger: String,
    actions: Vec<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CreatedAliasMemory {
    roles: BTreeSet<String>,
    channels: BTreeSet<String>,
    messages: BTreeSet<String>,
    instances: BTreeSet<String>,
}

fn anchor_content(
    draft: &Draft,
    observability: &Observability,
    last_error: Option<&StructuredError>,
    repair_state: Option<&RepairState>,
    adaptive_turn: Option<&AdaptiveTurnState>,
    brief_history: &[TurnBrief],
    messages: &[Message],
) -> String {
    let mut aliases = CreatedAliasMemory::default();
    let panels = draft
        .ruleset
        .panels
        .iter()
        .map(|panel| PanelMemory {
            key: panel.key.clone(),
            buttons: panel.buttons.iter().map(button_memory).collect(),
        })
        .collect();
    let modals = draft
        .ruleset
        .modals
        .iter()
        .map(|modal| ModalMemory {
            key: modal.key.clone(),
            fields: modal.fields.iter().map(|field| field.key.clone()).collect(),
        })
        .collect();
    let rules = draft
        .ruleset
        .rules
        .iter()
        .map(|rule| RuleMemory {
            key: rule.key.clone(),
            trigger: trigger_memory(&rule.trigger),
            actions: rule
                .actions
                .iter()
                .map(|action| action_memory(action, &mut aliases))
                .collect(),
        })
        .collect();
    let state = DraftStateMemory {
        revision: draft.draft_revision,
        validated_revision: draft.validated_revision,
        simulated_revision: draft.simulated_revision,
        panels,
        modals,
        rules,
        created_aliases: aliases,
        unresolved_references: draft.summary().unresolved_references,
        failure_signatures: observability.failure_signatures.clone(),
        last_error: last_error.map(error_memory),
        repair_state: repair_state.map(repair_memory),
        adaptive_turn: adaptive_turn.cloned(),
        recent_turn_briefs: brief_history
            .iter()
            .rev()
            .take(MAX_BRIEF_MEMORY_ITEMS)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
        current_human_intent: current_human_intent(messages),
        recent_human_intent: recent_human_intent(messages),
    };
    let state = serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string());
    format!("DRAFT_STATE:{state}")
}

fn error_memory(error: &StructuredError) -> ErrorMemory {
    ErrorMemory {
        code: error.code.clone(),
        location: error.location.clone(),
        message: truncate_memory_text(&error.message, MAX_ERROR_MEMORY_CHARS),
        hint: truncate_memory_text(&error.hint, MAX_ERROR_MEMORY_CHARS),
    }
}

fn repair_memory(state: &RepairState) -> RepairMemory {
    let (stage, ticket) = match state {
        RepairState::AwaitingAttempt(ticket) => (RepairStageMemory::AwaitingAttempt, ticket),
        RepairState::VerifyValidation(ticket) => (RepairStageMemory::VerifyValidation, ticket),
        RepairState::VerifySimulation(ticket) => (RepairStageMemory::VerifySimulation, ticket),
        RepairState::Failed(ticket) => (RepairStageMemory::Failed, ticket),
    };
    RepairMemory {
        stage,
        kind: ticket.kind,
        original_tool: ticket.original_call.name.clone(),
        error: error_memory(&ticket.original_error),
        allowed_tools: ticket.allowed_repair_tools.clone(),
        verification_path: ticket.verification_path.clone(),
        attempts_remaining: ticket.attempts_remaining,
        root_revision: ticket.root_revision,
    }
}

fn truncate_memory_text(value: &str, limit: usize) -> String {
    let mut characters = value.chars();
    let prefix = characters.by_ref().take(limit).collect::<String>();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn button_memory(button: &automation_state::ButtonSpec) -> String {
    match &button.route {
        ButtonRoute::Static { key } => format!("static:{key}"),
        ButtonRoute::InstanceAction { action, .. } => format!("instance_action:{action}"),
    }
}

fn trigger_memory(trigger: &TriggerSpec) -> String {
    match trigger {
        TriggerSpec::ButtonClick { component } => format!("button_click:{component}"),
        TriggerSpec::ModalSubmit { modal } => format!("modal_submit:{modal}"),
        TriggerSpec::InstanceAction { action } => format!("instance_action:{action}"),
    }
}

fn action_memory(action: &ActionSpec, aliases: &mut CreatedAliasMemory) -> String {
    match action {
        ActionSpec::GrantRole { .. } => "grant_role".to_string(),
        ActionSpec::RespondEphemeral { .. } => "respond_ephemeral".to_string(),
        ActionSpec::OpenModal { modal } => format!("open_modal:{modal}"),
        ActionSpec::CreateChannel { key, .. } => {
            aliases.channels.insert(key.clone());
            format!("create_channel:{key}")
        }
        ActionSpec::CreateRole { key, .. } => {
            aliases.roles.insert(key.clone());
            format!("create_role:{key}")
        }
        ActionSpec::UpsertOverwrite { .. } => "upsert_overwrite".to_string(),
        ActionSpec::PostPanel { key, .. } => {
            aliases.messages.insert(key.clone());
            format!("post_panel:{key}")
        }
        ActionSpec::DeferEphemeral => "defer_ephemeral".to_string(),
        ActionSpec::EditResponse { .. } => "edit_response".to_string(),
        ActionSpec::RegisterInstance { key, .. } => {
            aliases.instances.insert(key.clone());
            format!("register_instance:{key}")
        }
        ActionSpec::TeardownInstance { .. } => "teardown_instance".to_string(),
    }
}

fn recent_human_intent(messages: &[Message]) -> Vec<String> {
    let mut intents = messages
        .iter()
        .filter(|message| is_genuine_human_message(message))
        .map(|message| compact_text(&message.content))
        .filter(|message| !message.is_empty())
        .collect::<Vec<_>>();
    intents.pop();
    intents
        .into_iter()
        .rev()
        .take(MAX_INTENT_MEMORY_ITEMS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn current_human_intent(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| is_genuine_human_message(message))
        .map(|message| compact_text(&message.content))
}

pub(super) fn compact_text(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters = normalized.chars();
    let prefix = characters
        .by_ref()
        .take(MAX_INTENT_MEMORY_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}
