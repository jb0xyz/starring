use std::collections::{BTreeMap, BTreeSet, VecDeque};

use automation_state::{ActionSpec, ButtonRoute, TriggerSpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::draft::{Draft, DraftSummary};
use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, MessageRole, ToolCall};
use crate::tools::{dispatch_tool, tool_definitions, ToolDefinition};

pub const DEFAULT_SYSTEM_PROMPT: &str = "Design Discord automations only by calling the provided Draft tools. Never touch live Discord, publish, deploy, or activate. Make one logical change per tool call and reference created resources by alias. Validate before simulate. Text replies must be exactly QUESTION: followed by a question or DONE: followed by a summary. Use DONE: only after simulate_draft passes for the current revision.";

const NUDGE: &str = "Call a design tool to change the Draft; use QUESTION: to ask the human; only use DONE: after simulate_draft passes on the current revision.";
const MAX_INTENT_MEMORY_ITEMS: usize = 6;
const MAX_INTENT_MEMORY_CHARS: usize = 240;

pub const SESSION_SNAPSHOT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionConfig {
    pub max_model_calls: usize,
    pub max_tool_calls: usize,
    pub max_gate_failures: usize,
    pub context_char_budget: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_model_calls: 12,
            max_tool_calls: 24,
            max_gate_failures: 4,
            context_char_budget: 16_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitKind {
    ModelCalls,
    ToolCalls,
    GateFailures,
    ContextChars,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observability {
    pub model_calls: usize,
    pub tool_calls: usize,
    pub distinct_mutation_tools: BTreeSet<String>,
    pub clarification_count: usize,
    pub validation_failures: usize,
    pub simulation_failures: usize,
    pub failure_signatures: BTreeMap<String, usize>,
    pub repeated_errors: usize,
    pub nudge_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HaltReport {
    pub code: String,
    pub message: String,
    pub exhausted_limit: Option<LimitKind>,
    pub draft: DraftSummary,
    pub last_error: Option<StructuredError>,
    pub observability: Observability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BurstOutcome {
    AwaitingHuman { question: String },
    Completed { summary: String },
    Halted(Box<HaltReport>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSnapshot {
    pub schema_version: u32,
    pub draft: Draft,
    pub messages: Vec<Message>,
    pub observability: Observability,
    pub last_error: Option<StructuredError>,
    pub prose_nudged: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum SessionSnapshotError {
    #[error("unsupported session snapshot version {found}; expected {expected}")]
    UnsupportedVersion { expected: u32, found: u32 },
    #[error("invalid session snapshot: {message}")]
    InvalidInvariant { message: String },
}

pub struct DesignSession<C> {
    client: C,
    draft: Draft,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
    config: SessionConfig,
    observability: Observability,
    last_error: Option<StructuredError>,
    prose_nudged: bool,
}

impl<C> DesignSession<C> {
    pub fn new(client: C) -> Self {
        Self::with_config(client, SessionConfig::default())
    }

    pub fn with_config(client: C, config: SessionConfig) -> Self {
        let draft = Draft::new();
        let messages = vec![Message::system(DEFAULT_SYSTEM_PROMPT)];
        Self {
            client,
            draft,
            messages,
            tools: tool_definitions(),
            config,
            observability: Observability::default(),
            last_error: None,
            prose_nudged: false,
        }
    }

    pub fn draft(&self) -> &Draft {
        &self.draft
    }

    pub fn draft_mut(&mut self) -> &mut Draft {
        &mut self.draft
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn observability(&self) -> &Observability {
        &self.observability
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            schema_version: SESSION_SNAPSHOT_VERSION,
            draft: self.draft.clone(),
            messages: self.messages.clone(),
            observability: self.observability.clone(),
            last_error: self.last_error.clone(),
            prose_nudged: self.prose_nudged,
        }
    }

    pub fn restore(
        client: C,
        config: SessionConfig,
        snapshot: SessionSnapshot,
    ) -> Result<Self, SessionSnapshotError> {
        validate_snapshot(&snapshot)?;
        Ok(Self {
            client,
            draft: snapshot.draft,
            messages: snapshot.messages,
            tools: tool_definitions(),
            config,
            observability: snapshot.observability,
            last_error: snapshot.last_error,
            prose_nudged: snapshot.prose_nudged,
        })
    }

    fn total_context_chars(messages: &[Message], tools: &[ToolDefinition]) -> usize {
        let tools = serde_json::to_string(tools).map_or(usize::MAX, |value| value.len());
        let messages = serde_json::to_string(messages).map_or(usize::MAX, |value| value.len());
        tools.saturating_add(messages)
    }

    fn append_anchor(&mut self) {
        self.messages.push(Message::system(anchor_content(
            &self.draft,
            &self.observability,
            self.last_error.as_ref(),
            &self.messages,
        )));
    }

    fn routed_tools(&self) -> Vec<ToolDefinition> {
        routed_tool_definitions(&self.draft, &self.tools)
    }

    fn fit_context(&self, tools: &[ToolDefinition]) -> Option<Vec<Message>> {
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

    fn add_nudge(&mut self) {
        self.messages.push(Message::user(NUDGE));
        self.observability.nudge_count += 1;
    }

    fn record_failure(&mut self, name: Option<&str>, result: &ToolResult) {
        let Some(failure) = result.failure() else {
            return;
        };
        let signature = format!("{}@{}", failure.code, failure.location);
        let count = self
            .observability
            .failure_signatures
            .entry(signature)
            .or_default();
        if *count > 0 {
            self.observability.repeated_errors += 1;
        }
        *count += 1;
        self.last_error = Some(StructuredError::new(
            failure.code.clone(),
            failure.location.clone(),
            failure.message.clone(),
            failure.hint.clone(),
        ));
        match name {
            Some("validate_draft") => self.observability.validation_failures += 1,
            Some("simulate_draft") => self.observability.simulation_failures += 1,
            _ => {}
        }
    }

    fn gate_failures(&self) -> usize {
        self.observability.validation_failures + self.observability.simulation_failures
    }

    fn halt(&self, code: &str, message: &str, exhausted_limit: Option<LimitKind>) -> BurstOutcome {
        BurstOutcome::Halted(Box::new(HaltReport {
            code: code.to_string(),
            message: message.to_string(),
            exhausted_limit,
            draft: self.draft.summary(),
            last_error: self.last_error.clone(),
            observability: self.observability.clone(),
        }))
    }

    fn not_executed_result(&self) -> ToolResult {
        ToolResult::failure_from(
            &self.draft,
            StructuredError::new(
                "NOT_EXECUTED_AFTER_PREVIOUS_FAILURE",
                "tool.batch",
                "This tool call was not executed because an earlier call failed",
                "Correct the previous failure before retrying this change",
            ),
        )
    }

    fn unavailable_tool_result(&self, name: &str, tools: &[ToolDefinition]) -> ToolResult {
        let available = tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        ToolResult::failure_from(
            &self.draft,
            StructuredError::new(
                "TOOL_NOT_AVAILABLE_FOR_DRAFT_STATE",
                format!("tool.{name}"),
                "The requested design tool was not exposed for this model call or is no longer available for the current Draft state",
                format!("Use one of the currently available tools: {available}"),
            ),
        )
    }

    fn append_not_executed(&mut self, calls: &[ToolCall]) {
        for call in calls {
            let result = self.not_executed_result();
            self.messages
                .push(Message::tool(call.id.clone(), result.as_json()));
        }
    }
}

fn validate_snapshot(snapshot: &SessionSnapshot) -> Result<(), SessionSnapshotError> {
    if snapshot.schema_version != SESSION_SNAPSHOT_VERSION {
        return Err(SessionSnapshotError::UnsupportedVersion {
            expected: SESSION_SNAPSHOT_VERSION,
            found: snapshot.schema_version,
        });
    }
    let Some(system) = snapshot.messages.first() else {
        return Err(snapshot_invariant("canonical messages are empty"));
    };
    if system.role != MessageRole::System || system.content != DEFAULT_SYSTEM_PROMPT {
        return Err(snapshot_invariant(
            "canonical messages do not begin with the fixed system prompt",
        ));
    }
    if snapshot.draft.ruleset.version != 1 {
        return Err(snapshot_invariant("draft ruleset version is not supported"));
    }
    for (name, revision) in [
        ("validated_revision", snapshot.draft.validated_revision),
        ("simulated_revision", snapshot.draft.simulated_revision),
    ] {
        if revision.is_some_and(|revision| revision > snapshot.draft.draft_revision) {
            return Err(snapshot_invariant(format!(
                "{name} is newer than draft_revision"
            )));
        }
    }
    if snapshot.draft.simulated_revision.is_some()
        && snapshot.draft.simulated_revision != snapshot.draft.validated_revision
    {
        return Err(snapshot_invariant(
            "simulated_revision does not match validated_revision",
        ));
    }
    let repeated_errors = snapshot
        .observability
        .failure_signatures
        .values()
        .map(|count| count.saturating_sub(1))
        .sum::<usize>();
    if repeated_errors != snapshot.observability.repeated_errors {
        return Err(snapshot_invariant(
            "repeated_errors does not match failure signature counts",
        ));
    }
    validate_message_pairing(&snapshot.messages, snapshot.draft.draft_revision)
}

fn validate_message_pairing(
    messages: &[Message],
    draft_revision: u64,
) -> Result<(), SessionSnapshotError> {
    let mut expected = BTreeSet::new();
    for (index, message) in messages.iter().enumerate() {
        validate_message_shape(message, index)?;
        if is_anchor(message) && parse_anchor(message)?.revision > draft_revision {
            return Err(snapshot_invariant(
                "draft anchor revision is newer than the snapshot draft",
            ));
        }
        if index == 0 {
            continue;
        }
        if message.role == MessageRole::Assistant && !message.tool_calls.is_empty() {
            if !expected.is_empty() {
                return Err(snapshot_invariant(
                    "assistant tool call batch is missing tool results",
                ));
            }
            expected.extend(message.tool_calls.iter().map(|call| call.id.clone()));
            continue;
        }
        if message.role == MessageRole::Tool {
            let Some(tool_call_id) = message.tool_call_id.as_deref() else {
                return Err(snapshot_invariant("tool message is missing tool_call_id"));
            };
            if !expected.remove(tool_call_id) {
                return Err(snapshot_invariant(
                    "tool message does not match the preceding assistant batch",
                ));
            }
            continue;
        }
        if !expected.is_empty() {
            return Err(snapshot_invariant(
                "assistant tool call batch is missing tool results",
            ));
        }
    }
    if !expected.is_empty() {
        return Err(snapshot_invariant(
            "assistant tool call batch is missing tool results",
        ));
    }
    Ok(())
}

fn validate_message_shape(message: &Message, index: usize) -> Result<(), SessionSnapshotError> {
    match message.role {
        MessageRole::System | MessageRole::User => {
            if message.tool_call_id.is_some() || !message.tool_calls.is_empty() {
                return Err(snapshot_invariant(format!(
                    "message {index} has tool fields that are not valid for its role"
                )));
            }
        }
        MessageRole::Assistant => {
            if message.tool_call_id.is_some() {
                return Err(snapshot_invariant(format!(
                    "assistant message {index} has a tool_call_id"
                )));
            }
            if !message.tool_calls.is_empty() && !message.content.is_empty() {
                return Err(snapshot_invariant(format!(
                    "assistant tool call message {index} also has text content"
                )));
            }
            let mut ids = BTreeSet::new();
            for call in &message.tool_calls {
                if call.id.trim().is_empty() || call.name.trim().is_empty() {
                    return Err(snapshot_invariant(format!(
                        "assistant tool call message {index} has an empty id or name"
                    )));
                }
                if !ids.insert(call.id.as_str()) {
                    return Err(snapshot_invariant(format!(
                        "assistant tool call message {index} has duplicate ids"
                    )));
                }
            }
        }
        MessageRole::Tool => {
            if !message.tool_calls.is_empty()
                || message
                    .tool_call_id
                    .as_deref()
                    .is_none_or(|id| id.trim().is_empty())
            {
                return Err(snapshot_invariant(format!(
                    "tool message {index} has invalid tool fields"
                )));
            }
        }
    }
    if message.role == MessageRole::System && index > 0 && !is_anchor(message) {
        return Err(snapshot_invariant(
            "canonical messages contain an unexpected system message",
        ));
    }
    Ok(())
}

fn parse_anchor(message: &Message) -> Result<DraftStateMemory, SessionSnapshotError> {
    let value = message
        .content
        .strip_prefix("DRAFT_STATE:")
        .ok_or_else(|| snapshot_invariant("draft anchor prefix is missing"))?;
    serde_json::from_str(value).map_err(|_| snapshot_invariant("draft anchor JSON is malformed"))
}

fn snapshot_invariant(message: impl Into<String>) -> SessionSnapshotError {
    SessionSnapshotError::InvalidInvariant {
        message: message.into(),
    }
}

impl<C: LlmClient> DesignSession<C> {
    pub async fn run_burst(&mut self, human_message: &str) -> BurstOutcome {
        self.messages.push(Message::user(human_message));
        self.prose_nudged = false;
        loop {
            if self.observability.model_calls >= self.config.max_model_calls {
                return self.halt(
                    "MODEL_CALL_LIMIT_EXHAUSTED",
                    "The session exhausted its model call budget",
                    Some(LimitKind::ModelCalls),
                );
            }
            self.append_anchor();
            let routed_tools = self.routed_tools();
            let Some(outbound_messages) = self.fit_context(&routed_tools) else {
                return self.halt(
                    "CONTEXT_CHAR_LIMIT_EXHAUSTED",
                    "The system prompt, tool schemas, and current Draft anchor do not fit",
                    Some(LimitKind::ContextChars),
                );
            };

            self.observability.model_calls += 1;
            let response = match self
                .client
                .complete(&outbound_messages, &routed_tools)
                .await
            {
                Ok(response) => response,
                Err(_) => {
                    self.last_error = Some(StructuredError::new(
                        "LLM_CLIENT_ERROR",
                        "llm",
                        "The model request failed",
                        "Stop the burst and retry after the model gateway is available",
                    ));
                    return self.halt("LLM_CLIENT_ERROR", "The model client failed", None);
                }
            };

            match response {
                LlmResponse::ToolCalls(calls) => {
                    if calls.is_empty() {
                        return self.halt(
                            "EMPTY_TOOL_CALL_BATCH",
                            "The model returned an empty tool call batch",
                            None,
                        );
                    }
                    self.prose_nudged = false;
                    self.messages
                        .push(Message::assistant_tool_calls(calls.clone()));
                    let mut failed = false;
                    for (index, call) in calls.iter().enumerate() {
                        if failed {
                            let result = self.not_executed_result();
                            self.messages
                                .push(Message::tool(call.id.clone(), result.as_json()));
                            continue;
                        }
                        if self.observability.tool_calls >= self.config.max_tool_calls {
                            self.append_not_executed(&calls[index..]);
                            return self.halt(
                                "TOOL_CALL_LIMIT_EXHAUSTED",
                                "The session exhausted its executed tool call budget",
                                Some(LimitKind::ToolCalls),
                            );
                        }
                        self.observability.tool_calls += 1;
                        let available = routed_tools.iter().any(|tool| tool.name == call.name)
                            && tool_is_available(&self.draft, &call.name);
                        let result = if available {
                            dispatch_tool(&mut self.draft, &call.name, &call.arguments).await
                        } else {
                            self.unavailable_tool_result(&call.name, &self.routed_tools())
                        };
                        if result.is_ok() && is_mutation_tool(&call.name) {
                            self.observability
                                .distinct_mutation_tools
                                .insert(call.name.clone());
                        }
                        self.record_failure(available.then_some(call.name.as_str()), &result);
                        let is_failure = !result.is_ok();
                        self.messages
                            .push(Message::tool(call.id.clone(), result.as_json()));
                        if is_failure {
                            failed = true;
                            self.append_not_executed(&calls[index + 1..]);
                            if self.gate_failures() >= self.config.max_gate_failures {
                                return self.halt(
                                    "GATE_FAILURE_LIMIT_EXHAUSTED",
                                    "The session exhausted its validation and simulation failure budget",
                                    Some(LimitKind::GateFailures),
                                );
                            }
                            break;
                        }
                    }
                    if failed {
                        continue;
                    }
                }
                LlmResponse::Text(text) => {
                    self.messages.push(Message::assistant(text.clone()));
                    if let Some(question) = text.strip_prefix("QUESTION:") {
                        self.observability.clarification_count += 1;
                        return BurstOutcome::AwaitingHuman {
                            question: question.trim().to_string(),
                        };
                    }
                    if let Some(summary) = text.strip_prefix("DONE:") {
                        if self.draft.simulated_revision == Some(self.draft.draft_revision)
                            && self.observability.distinct_mutation_tools.len() >= 2
                        {
                            return BurstOutcome::Completed {
                                summary: summary.trim().to_string(),
                            };
                        }
                        self.add_nudge();
                        continue;
                    }
                    if !self.prose_nudged {
                        self.prose_nudged = true;
                        self.add_nudge();
                        continue;
                    }
                    return self.halt("UNSTRUCTURED_MODEL_TEXT", &text, None);
                }
            }
        }
    }
}

fn routed_tool_definitions(draft: &Draft, registry: &[ToolDefinition]) -> Vec<ToolDefinition> {
    registry
        .iter()
        .filter(|tool| tool_is_available(draft, &tool.name))
        .cloned()
        .collect()
}

fn tool_is_available(draft: &Draft, name: &str) -> bool {
    let has_rules = !draft.ruleset.rules.is_empty();
    match name {
        "add_panel" | "add_modal" | "begin_rule" => true,
        "add_button" => !draft.ruleset.panels.is_empty(),
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

fn is_anchor(message: &Message) -> bool {
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
struct DraftStateMemory {
    revision: u64,
    validated_revision: Option<u64>,
    simulated_revision: Option<u64>,
    panels: Vec<PanelMemory>,
    modals: Vec<ModalMemory>,
    rules: Vec<RuleMemory>,
    created_aliases: CreatedAliasMemory,
    unresolved_references: Vec<String>,
    failure_signatures: BTreeMap<String, usize>,
    last_error: Option<StructuredError>,
    current_human_intent: Option<String>,
    recent_human_intent: Vec<String>,
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
        last_error: last_error.cloned(),
        current_human_intent: current_human_intent(messages),
        recent_human_intent: recent_human_intent(messages),
    };
    let state = serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string());
    format!("DRAFT_STATE:{state}")
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
        .filter(|message| message.role == MessageRole::User && message.content != NUDGE)
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
        .find(|message| message.role == MessageRole::User && message.content != NUDGE)
        .map(|message| compact_text(&message.content))
}

fn compact_text(value: &str) -> String {
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

fn is_mutation_tool(name: &str) -> bool {
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
    )
}
