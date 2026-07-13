use std::collections::{BTreeMap, BTreeSet, VecDeque};

use automation_state::{ActionSpec, ButtonRoute, TriggerSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::draft::{Draft, DraftSummary};
use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, MessageRole, ToolCall};
use crate::tools::{dispatch_tool, tool_definitions, ToolDefinition};
use crate::turn::{
    check_scope, control_tool_definitions, parse_empty_control, parse_finish_turn,
    parse_turn_brief, render_preview, required_mutation_tools, AdaptivePhase, AdaptiveTurnState,
    FinishTurnKind, RequestedOutcome, SimulationProfile, TurnBrief, TurnIntent,
};

pub const DEFAULT_SYSTEM_PROMPT: &str = "Design Discord automations only with the provided tools. Never touch live Discord, publish, deploy, or activate. At the start of every human turn call set_turn_brief with only a concise intent, objective, requested outcome, assumptions, and whether validation is required. The harness deterministically enables StudyRoom simulation from the exact human message; set validate to true whenever the human explicitly says StudyRoom. Use discussion or brainstorm for design conversation or a missing structural decision, then finish_turn with one focused question or response without changing the Draft. For build or modify, continue in the same turn with the staged design tools and call check_turn_scope after the requested Draft change is complete. The harness then automatically runs requested validation, harness-selected simulation, and preview steps. When finish_turn is the only available tool, call it with kind ready and summarize the result. Use safe defaults only for non-blocking details. Modification requests must use update or remove tools instead of creating duplicates. Reference created resources by alias. Never ask whether to continue, stop, validate, or review. Legacy QUESTION, PROGRESSED, and READY text are accepted only for compatibility; prefer finish_turn.";

const NUDGE: &str = "Call a design tool to change the Draft; use QUESTION: only for a blocking decision; use PROGRESSED: after useful changes when another user turn is appropriate; use READY: only after validate_draft passes on the current revision.";
const REPAIR_REQUIRED_PREFIX: &str = "REPAIR_REQUIRED:";
const MAX_INTENT_MEMORY_ITEMS: usize = 6;
const MAX_BRIEF_MEMORY_ITEMS: usize = 3;
const MAX_INTENT_MEMORY_CHARS: usize = 240;
const MAX_ERROR_MEMORY_CHARS: usize = 360;

pub const SESSION_SNAPSHOT_VERSION: u32 = 4;

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
            context_char_budget: 44_000,
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
    #[serde(default)]
    pub mutation_tool_calls: BTreeMap<String, usize>,
    pub clarification_count: usize,
    pub validation_failures: usize,
    pub simulation_failures: usize,
    pub failure_signatures: BTreeMap<String, usize>,
    pub repeated_errors: usize,
    pub repair_attempts: usize,
    pub repair_successes: usize,
    pub repair_failures: usize,
    pub repair_escalations: usize,
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
    NeedsInput { question: String },
    Progressed { summary: String },
    Ready { summary: String },
    Halted(Box<HaltReport>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    Active,
    NeedsInput,
    Progressed,
    Ready,
    Halted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnState {
    pub sequence: u64,
    pub phase: TurnPhase,
    pub human_message: String,
    pub started_revision: u64,
    pub current_revision: u64,
    pub model_calls: usize,
    pub tool_calls: usize,
    pub gate_failures: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairKind {
    Arguments,
    Validation,
    Simulation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairTicket {
    pub kind: RepairKind,
    pub original_call: ToolCall,
    pub original_error: StructuredError,
    pub expected_argument_schema: Option<Value>,
    pub allowed_repair_tools: Vec<String>,
    pub verification_path: Vec<String>,
    pub root_revision: u64,
    pub attempts_remaining: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", content = "ticket", rename_all = "snake_case")]
pub enum RepairState {
    AwaitingAttempt(RepairTicket),
    VerifyValidation(RepairTicket),
    VerifySimulation(RepairTicket),
    Failed(RepairTicket),
}

impl RepairState {
    fn ticket(&self) -> &RepairTicket {
        match self {
            Self::AwaitingAttempt(ticket)
            | Self::VerifyValidation(ticket)
            | Self::VerifySimulation(ticket)
            | Self::Failed(ticket) => ticket,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepairOriginal {
    tool: ToolCall,
    error: StructuredError,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepairDirective {
    event: String,
    attempts_remaining: u8,
    original: RepairOriginal,
    expected_argument_schema: Option<Value>,
    allowed_repair_tools: Vec<String>,
    verification_path: Vec<String>,
}

impl RepairDirective {
    fn from_ticket(ticket: &RepairTicket) -> Self {
        Self {
            event: "repair_required".to_string(),
            attempts_remaining: ticket.attempts_remaining,
            original: RepairOriginal {
                tool: ticket.original_call.clone(),
                error: ticket.original_error.clone(),
            },
            expected_argument_schema: ticket.expected_argument_schema.clone(),
            allowed_repair_tools: ticket.allowed_repair_tools.clone(),
            verification_path: ticket.verification_path.clone(),
        }
    }
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
    pub repair_state: Option<RepairState>,
    #[serde(default)]
    pub turn_state: Option<TurnState>,
    #[serde(default)]
    pub adaptive_turn: Option<AdaptiveTurnState>,
    #[serde(default)]
    pub adaptive_enabled: bool,
    #[serde(default)]
    pub brief_history: Vec<TurnBrief>,
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
    repair_state: Option<RepairState>,
    turn_state: Option<TurnState>,
    adaptive_turn: Option<AdaptiveTurnState>,
    adaptive_enabled: bool,
    brief_history: Vec<TurnBrief>,
}

impl<C> DesignSession<C> {
    pub fn new(client: C) -> Self {
        Self::with_config(client, SessionConfig::default())
    }

    pub fn with_config(client: C, config: SessionConfig) -> Self {
        Self::build(client, config, false)
    }

    pub fn with_adaptive_config(client: C, config: SessionConfig) -> Self {
        Self::build(client, config, true)
    }

    fn build(client: C, config: SessionConfig, adaptive_enabled: bool) -> Self {
        let draft = Draft::new();
        let messages = vec![Message::system(DEFAULT_SYSTEM_PROMPT)];
        Self {
            client,
            draft,
            messages,
            tools: if adaptive_enabled {
                all_tool_definitions()
            } else {
                legacy_tool_definitions()
            },
            config,
            observability: Observability::default(),
            last_error: None,
            prose_nudged: false,
            repair_state: None,
            turn_state: None,
            adaptive_turn: None,
            adaptive_enabled,
            brief_history: Vec::new(),
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

    pub fn turn_state(&self) -> Option<&TurnState> {
        self.turn_state.as_ref()
    }

    pub fn adaptive_turn(&self) -> Option<&AdaptiveTurnState> {
        self.adaptive_turn.as_ref()
    }

    pub fn adaptive_enabled(&self) -> bool {
        self.adaptive_enabled
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            schema_version: SESSION_SNAPSHOT_VERSION,
            draft: self.draft.clone(),
            messages: self.messages.clone(),
            observability: self.observability.clone(),
            last_error: self.last_error.clone(),
            prose_nudged: self.prose_nudged,
            repair_state: self.repair_state.clone(),
            turn_state: self.turn_state.clone().map(|mut state| {
                state.current_revision = self.draft.draft_revision;
                state
            }),
            adaptive_turn: self.adaptive_turn.clone(),
            adaptive_enabled: self.adaptive_enabled,
            brief_history: self.brief_history.clone(),
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
            tools: if snapshot.adaptive_enabled {
                all_tool_definitions()
            } else {
                legacy_tool_definitions()
            },
            config,
            observability: snapshot.observability,
            last_error: snapshot.last_error,
            prose_nudged: snapshot.prose_nudged,
            repair_state: snapshot.repair_state,
            turn_state: snapshot.turn_state,
            adaptive_turn: snapshot.adaptive_turn,
            adaptive_enabled: snapshot.adaptive_enabled,
            brief_history: snapshot.brief_history,
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
            self.repair_state.as_ref(),
            self.adaptive_turn.as_ref(),
            &self.brief_history,
            &self.messages,
        )));
    }

    fn routed_tools(&self) -> Vec<ToolDefinition> {
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
            Some("validate_draft") if !is_argument_failure(&failure.code) => {
                self.observability.validation_failures += 1;
                if let Some(state) = self.turn_state.as_mut() {
                    state.gate_failures += 1;
                }
            }
            Some("simulate_draft") if !is_argument_failure(&failure.code) => {
                self.observability.simulation_failures += 1;
                if let Some(state) = self.turn_state.as_mut() {
                    state.gate_failures += 1;
                }
            }
            _ => {}
        }
    }

    fn begin_turn(&mut self, human_message: &str) {
        if self.adaptive_enabled {
            if let Some(brief) = self
                .adaptive_turn
                .as_ref()
                .and_then(|state| state.brief.clone())
            {
                self.brief_history.push(brief);
            }
        }
        let sequence = self
            .turn_state
            .as_ref()
            .map_or(1, |state| state.sequence.saturating_add(1));
        self.turn_state = Some(TurnState {
            sequence,
            phase: TurnPhase::Active,
            human_message: compact_text(human_message),
            started_revision: self.draft.draft_revision,
            current_revision: self.draft.draft_revision,
            model_calls: 0,
            tool_calls: 0,
            gate_failures: 0,
        });
        self.adaptive_turn = self.adaptive_enabled.then(AdaptiveTurnState::default);
    }

    fn turn_model_calls(&self) -> usize {
        self.turn_state
            .as_ref()
            .map_or(0, |state| state.model_calls)
    }

    fn turn_tool_calls(&self) -> usize {
        self.turn_state.as_ref().map_or(0, |state| state.tool_calls)
    }

    fn turn_gate_failures(&self) -> usize {
        self.turn_state
            .as_ref()
            .map_or(0, |state| state.gate_failures)
    }

    fn record_model_call(&mut self) {
        self.observability.model_calls += 1;
        if let Some(state) = self.turn_state.as_mut() {
            state.model_calls += 1;
        }
    }

    fn record_tool_call(&mut self) {
        self.observability.tool_calls += 1;
        if let Some(state) = self.turn_state.as_mut() {
            state.tool_calls += 1;
        }
    }

    fn finish_turn(&mut self, phase: TurnPhase) {
        if let Some(state) = self.turn_state.as_mut() {
            state.phase = phase;
            state.current_revision = self.draft.draft_revision;
        }
    }

    fn needs_input(&mut self, question: String) -> BurstOutcome {
        self.finish_turn(TurnPhase::NeedsInput);
        BurstOutcome::NeedsInput { question }
    }

    fn progressed(&mut self, summary: String) -> BurstOutcome {
        self.finish_turn(TurnPhase::Progressed);
        BurstOutcome::Progressed { summary }
    }

    fn ready(&mut self, summary: String) -> BurstOutcome {
        self.finish_turn(TurnPhase::Ready);
        BurstOutcome::Ready { summary }
    }

    fn halt(
        &mut self,
        code: &str,
        message: &str,
        exhausted_limit: Option<LimitKind>,
    ) -> BurstOutcome {
        self.finish_turn(TurnPhase::Halted);
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

    fn dispatch_control_tool(
        &mut self,
        name: &str,
        arguments: &str,
    ) -> (ToolResult, Option<BurstOutcome>) {
        match name {
            "set_turn_brief" => {
                let mut brief = match parse_turn_brief(arguments) {
                    Ok(brief) => brief,
                    Err(error) => return (ToolResult::failure_from(&self.draft, error), None),
                };
                brief.verification.simulation =
                    simulation_profile_for_current_human_turn(&self.messages);
                if brief.objective.trim().is_empty() {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "EMPTY_TURN_OBJECTIVE",
                                "tool.set_turn_brief.objective",
                                "The turn objective is empty",
                                "Provide a concise objective grounded in the human request",
                            ),
                        ),
                        None,
                    );
                }
                if brief.requested_outcome == RequestedOutcome::ValidatedPreview
                    && !brief.verification.validate
                {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "PREVIEW_REQUIRES_VALIDATION",
                                "tool.set_turn_brief.validate",
                                "A validated preview requires validation",
                                "Set validate to true",
                            ),
                        ),
                        None,
                    );
                }
                if brief.verification.simulation == SimulationProfile::StudyRoom
                    && !brief.verification.validate
                {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "SIMULATION_REQUIRES_VALIDATION",
                                "tool.set_turn_brief.validate",
                                "StudyRoom simulation requires validation",
                                "Set validate to true",
                            ),
                        ),
                        None,
                    );
                }
                let phase = if brief.requested_outcome == RequestedOutcome::Discussion
                    || matches!(brief.intent, TurnIntent::Brainstorm | TurnIntent::Inspect)
                {
                    AdaptivePhase::Reply
                } else {
                    AdaptivePhase::Build
                };
                self.adaptive_turn = Some(AdaptiveTurnState {
                    phase,
                    brief: Some(brief),
                    scoped_revision: None,
                    previewed_revision: None,
                });
                (
                    ToolResult::success(&self.draft, "Recorded the current turn brief"),
                    None,
                )
            }
            "check_turn_scope" => {
                if let Err(error) = parse_empty_control(name, arguments) {
                    return (ToolResult::failure_from(&self.draft, error), None);
                }
                let Some(brief) = self
                    .adaptive_turn
                    .as_ref()
                    .and_then(|state| state.brief.as_ref())
                    .cloned()
                else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.check_turn_scope",
                                "No active turn brief exists",
                                "Call set_turn_brief before checking scope",
                            ),
                        ),
                        None,
                    );
                };
                let scope = check_scope(&self.draft, &brief);
                let requires_change =
                    matches!(brief.intent, TurnIntent::Build | TurnIntent::Modify);
                let changed = self
                    .turn_state
                    .as_ref()
                    .is_some_and(|turn| turn.started_revision < self.draft.draft_revision);
                if !scope.ok || requires_change && !changed {
                    let mut missing = scope.missing;
                    if requires_change && !changed {
                        missing.push("draft_change".to_string());
                    }
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_SCOPE_INCOMPLETE",
                                "turn.requirements",
                                format!(
                                    "The current Draft is missing requirements: {}",
                                    missing.join(", ")
                                ),
                                "Use the routed mutation tools to make the requested Draft change and satisfy every missing requirement",
                            ),
                        ),
                        None,
                    );
                }
                let phase = if brief.verification.validate {
                    AdaptivePhase::Verify
                } else if brief.requested_outcome == RequestedOutcome::ValidatedPreview {
                    AdaptivePhase::Preview
                } else {
                    AdaptivePhase::Reply
                };
                if let Some(state) = self.adaptive_turn.as_mut() {
                    state.scoped_revision = Some(self.draft.draft_revision);
                    state.phase = phase;
                }
                (
                    ToolResult::success(
                        &self.draft,
                        format!("Turn scope satisfied: {}", scope.satisfied.join(", ")),
                    ),
                    None,
                )
            }
            "render_preview" => {
                if let Err(error) = parse_empty_control(name, arguments) {
                    return (ToolResult::failure_from(&self.draft, error), None);
                }
                let Some(brief) = self
                    .adaptive_turn
                    .as_ref()
                    .and_then(|state| state.brief.as_ref())
                    .cloned()
                else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.render_preview",
                                "No active turn brief exists",
                                "Call set_turn_brief before rendering a preview",
                            ),
                        ),
                        None,
                    );
                };
                if brief.verification.validate
                    && self.draft.validated_revision != Some(self.draft.draft_revision)
                {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "PREVIEW_REQUIRES_CURRENT_VALIDATION",
                                "tool.render_preview",
                                "The current Draft revision is not validated",
                                "Call validate_draft before rendering the preview",
                            ),
                        ),
                        None,
                    );
                }
                let preview = render_preview(&self.draft);
                if let Some(state) = self.adaptive_turn.as_mut() {
                    state.previewed_revision = Some(self.draft.draft_revision);
                    state.phase = AdaptivePhase::Reply;
                }
                let change = serde_json::to_string(&preview)
                    .map(|value| format!("Rendered preview {value}"))
                    .unwrap_or_else(|_| "Rendered preview".to_string());
                (ToolResult::success(&self.draft, change), None)
            }
            "finish_turn" => {
                let finish = match parse_finish_turn(arguments) {
                    Ok(finish) => finish,
                    Err(error) => return (ToolResult::failure_from(&self.draft, error), None),
                };
                let Some(state) = self.adaptive_turn.as_ref() else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.finish_turn",
                                "No active adaptive turn exists",
                                "Call set_turn_brief before finishing the turn",
                            ),
                        ),
                        None,
                    );
                };
                let Some(brief) = state.brief.as_ref() else {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_BRIEF_REQUIRED",
                                "tool.finish_turn",
                                "No active turn brief exists",
                                "Call set_turn_brief before finishing the turn",
                            ),
                        ),
                        None,
                    );
                };
                let outcome = match finish.kind {
                    FinishTurnKind::NeedsInput => {
                        let question = finish.question.as_deref().unwrap_or("").trim();
                        let question_allowed = brief.requested_outcome
                            == RequestedOutcome::Discussion
                            || brief.intent == TurnIntent::Brainstorm
                            || brief.intent == TurnIntent::Inspect;
                        if question.is_empty() || !question_allowed {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "UNNECESSARY_TURN_QUESTION",
                                        "tool.finish_turn.question",
                                        "The turn does not contain a justified blocking question",
                                        "Continue building or finish with progressed or ready",
                                    ),
                                ),
                                None,
                            );
                        }
                        self.observability.clarification_count += 1;
                        self.needs_input(question.to_string())
                    }
                    FinishTurnKind::Progressed => {
                        if brief.requested_outcome == RequestedOutcome::ValidatedPreview
                            && brief.blocking_decisions.is_empty()
                        {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "PREMATURE_TURN_PROGRESS",
                                        "tool.finish_turn.kind",
                                        "A fully specified validated-preview request cannot stop as partial progress",
                                        "Complete scope checking, validation, preview, and finish the turn as ready",
                                    ),
                                ),
                                None,
                            );
                        }
                        let changed = self
                            .turn_state
                            .as_ref()
                            .is_some_and(|turn| turn.started_revision < self.draft.draft_revision);
                        if !changed {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "PROGRESS_REQUIRES_CHANGE",
                                        "tool.finish_turn.kind",
                                        "The Draft did not change during this turn",
                                        "Make a useful Draft change or ask a justified question",
                                    ),
                                ),
                                None,
                            );
                        }
                        self.progressed(finish.message.clone())
                    }
                    FinishTurnKind::Ready => {
                        let scope_current =
                            state.scoped_revision == Some(self.draft.draft_revision);
                        let validation_current = !brief.verification.validate
                            || self.draft.validated_revision == Some(self.draft.draft_revision);
                        let simulation_current = brief.verification.simulation
                            != SimulationProfile::StudyRoom
                            || self.draft.simulated_revision == Some(self.draft.draft_revision);
                        let preview_current = brief.requested_outcome
                            != RequestedOutcome::ValidatedPreview
                            || state.previewed_revision == Some(self.draft.draft_revision);
                        if state.phase != AdaptivePhase::Reply
                            || !scope_current
                            || !validation_current
                            || !simulation_current
                            || !preview_current
                        {
                            return (
                                ToolResult::failure_from(
                                    &self.draft,
                                    StructuredError::new(
                                        "TURN_NOT_READY",
                                        "tool.finish_turn.kind",
                                        "The current turn has not completed its scope and verification path",
                                        "Finish scope checking, validation, supported simulation, and preview before ready",
                                    ),
                                ),
                                None,
                            );
                        }
                        self.ready(finish.message.clone())
                    }
                };
                (
                    ToolResult::success(&self.draft, "Prepared the human-facing turn response"),
                    Some(outcome),
                )
            }
            _ => (
                ToolResult::failure_from(
                    &self.draft,
                    StructuredError::new(
                        "UNKNOWN_CONTROL_TOOL",
                        "tool",
                        "The requested turn control tool does not exist",
                        "Use one of the routed turn control tools",
                    ),
                ),
                None,
            ),
        }
    }

    fn advance_adaptive_after_draft_tool(&mut self, name: &str, succeeded: bool) {
        if !succeeded {
            return;
        }
        let Some(state) = self.adaptive_turn.as_mut() else {
            return;
        };
        if is_mutation_tool(name) {
            state.scoped_revision = None;
            state.previewed_revision = None;
        } else if name == "validate_draft" {
            let scope_ok = state
                .brief
                .as_ref()
                .is_some_and(|brief| check_scope(&self.draft, brief).ok);
            if !scope_ok {
                state.phase = AdaptivePhase::Build;
                return;
            }
            state.scoped_revision = Some(self.draft.draft_revision);
            if state.phase == AdaptivePhase::Verify {
                let simulation = state
                    .brief
                    .as_ref()
                    .map_or(SimulationProfile::None, |brief| {
                        brief.verification.simulation
                    });
                state.phase = if simulation == SimulationProfile::StudyRoom {
                    AdaptivePhase::Simulate
                } else {
                    AdaptivePhase::Preview
                };
            }
        } else if name == "simulate_draft" && state.phase == AdaptivePhase::Simulate {
            state.phase = AdaptivePhase::Preview;
        }
    }

    fn append_not_executed(&mut self, calls: &[ToolCall]) {
        for call in calls {
            let result = self.not_executed_result();
            self.messages
                .push(Message::tool(call.id.clone(), result.as_json()));
        }
    }

    fn append_phase_transition_not_executed(&mut self, calls: &[ToolCall]) {
        for call in calls {
            let result = ToolResult::failure_from(
                &self.draft,
                StructuredError::new(
                    "NOT_EXECUTED_AFTER_PHASE_TRANSITION",
                    "tool.batch",
                    "This tool call was not executed because the adaptive turn phase changed",
                    "Continue with the tools routed for the new turn phase",
                ),
            );
            self.messages
                .push(Message::tool(call.id.clone(), result.as_json()));
        }
    }

    fn append_repair_directive(&mut self, ticket: &RepairTicket) {
        let directive = RepairDirective::from_ticket(ticket);
        let json = serde_json::to_string(&directive).unwrap_or_else(|_| {
            r#"{"event":"repair_required","attempts_remaining":1}"#.to_string()
        });
        self.messages
            .push(Message::user(format!("{REPAIR_REQUIRED_PREFIX}{json}")));
    }

    fn root_repair_ticket(
        &self,
        call: &ToolCall,
        result: &ToolResult,
        request_tools: &[ToolDefinition],
    ) -> Option<RepairTicket> {
        let failure = result.failure()?;
        let error = StructuredError::new(
            failure.code.clone(),
            failure.location.clone(),
            failure.message.clone(),
            failure.hint.clone(),
        );
        let tool_name = call.name.as_str();
        let argument_failure = is_argument_failure(failure.code.as_str());
        let kind = if argument_failure {
            RepairKind::Arguments
        } else if tool_name == "validate_draft" {
            RepairKind::Validation
        } else if tool_name == "simulate_draft" {
            RepairKind::Simulation
        } else {
            return None;
        };
        let expected_argument_schema = argument_failure.then(|| {
            request_tools
                .iter()
                .find(|tool| tool.name == tool_name)
                .map(|tool| tool.parameters.clone())
                .unwrap_or(Value::Null)
        });
        let allowed_repair_tools = if argument_failure {
            vec![tool_name.to_string()]
        } else {
            routed_tool_definitions(&self.draft, &self.tools)
                .into_iter()
                .filter(|tool| is_mutation_tool(&tool.name))
                .map(|tool| tool.name)
                .collect()
        };
        let verification_path = match kind {
            RepairKind::Arguments => vec![tool_name.to_string()],
            RepairKind::Validation => {
                vec!["mutation".to_string(), "validate_draft".to_string()]
            }
            RepairKind::Simulation => vec![
                "mutation".to_string(),
                "validate_draft".to_string(),
                "simulate_draft".to_string(),
            ],
        };
        Some(RepairTicket {
            kind,
            original_call: call.clone(),
            original_error: error,
            expected_argument_schema,
            allowed_repair_tools,
            verification_path,
            root_revision: self.draft.draft_revision,
            attempts_remaining: 1,
        })
    }

    fn consume_repair_attempt(&mut self, ticket: &mut RepairTicket) {
        ticket.attempts_remaining = 0;
        self.observability.repair_attempts += 1;
    }

    fn append_repair_rejections(&mut self, calls: &[ToolCall], error: &StructuredError) {
        let result = ToolResult::failure_from(&self.draft, error.clone());
        self.record_failure(None, &result);
        let json = result.as_json();
        for call in calls {
            self.messages
                .push(Message::tool(call.id.clone(), json.clone()));
        }
    }

    fn fail_repair(
        &mut self,
        mut ticket: RepairTicket,
        error: StructuredError,
        record_error: bool,
    ) -> BurstOutcome {
        ticket.attempts_remaining = 0;
        if record_error {
            let result = ToolResult::failure_from(&self.draft, error.clone());
            self.record_failure(None, &result);
        }
        self.last_error = Some(error);
        self.observability.repair_failures += 1;
        self.repair_state = Some(RepairState::Failed(ticket));
        self.halt(
            "REPAIR_ATTEMPT_FAILED",
            "The single automatic repair attempt failed",
            None,
        )
    }
}

fn definitions_named(registry: &[ToolDefinition], names: &[&str]) -> Vec<ToolDefinition> {
    registry
        .iter()
        .filter(|tool| names.contains(&tool.name.as_str()))
        .cloned()
        .collect()
}

fn is_argument_failure(code: &str) -> bool {
    matches!(
        code,
        "MISSING_REQUIRED_FIELD"
            | "INVALID_KIND"
            | "INVALID_TOOL_ARGUMENTS"
            | "UNKNOWN_FIELD"
            | "INVALID_FIELD_TYPE"
    )
}

fn valid_tool_call_ids(calls: &[ToolCall]) -> bool {
    let mut ids = BTreeSet::new();
    calls.iter().all(|call| {
        !call.id.trim().is_empty() && !call.name.trim().is_empty() && ids.insert(call.id.as_str())
    })
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
    validate_turn_snapshot(snapshot)?;
    validate_adaptive_turn_snapshot(snapshot)?;
    validate_repair_snapshot(snapshot)?;
    validate_message_pairing(&snapshot.messages, snapshot.draft.draft_revision)
}

fn validate_adaptive_turn_snapshot(snapshot: &SessionSnapshot) -> Result<(), SessionSnapshotError> {
    let Some(state) = snapshot.adaptive_turn.as_ref() else {
        return Ok(());
    };
    if state.brief.is_none() && state.phase != AdaptivePhase::Assess {
        return Err(snapshot_invariant(
            "adaptive turn without a brief is outside assess phase",
        ));
    }
    if state.brief.is_some() && state.phase == AdaptivePhase::Assess {
        return Err(snapshot_invariant(
            "adaptive turn with a brief is still in assess phase",
        ));
    }
    if state
        .scoped_revision
        .is_some_and(|revision| revision > snapshot.draft.draft_revision)
        || state
            .previewed_revision
            .is_some_and(|revision| revision > snapshot.draft.draft_revision)
    {
        return Err(snapshot_invariant(
            "adaptive turn references a future Draft revision",
        ));
    }
    Ok(())
}

fn validate_turn_snapshot(snapshot: &SessionSnapshot) -> Result<(), SessionSnapshotError> {
    let Some(state) = snapshot.turn_state.as_ref() else {
        return Ok(());
    };
    if state.sequence == 0
        || state.started_revision > state.current_revision
        || state.current_revision != snapshot.draft.draft_revision
        || state.model_calls > snapshot.observability.model_calls
        || state.tool_calls > snapshot.observability.tool_calls
        || state.gate_failures
            > snapshot
                .observability
                .validation_failures
                .saturating_add(snapshot.observability.simulation_failures)
    {
        return Err(snapshot_invariant("turn lifecycle state is inconsistent"));
    }
    if state.phase == TurnPhase::Progressed && state.current_revision == state.started_revision {
        return Err(snapshot_invariant(
            "progressed turn did not change the Draft revision",
        ));
    }
    Ok(())
}

fn validate_repair_snapshot(snapshot: &SessionSnapshot) -> Result<(), SessionSnapshotError> {
    if snapshot.observability.repair_successes > snapshot.observability.repair_attempts {
        return Err(snapshot_invariant(
            "repair successes exceed repair attempts",
        ));
    }
    let Some(state) = snapshot.repair_state.as_ref() else {
        return Ok(());
    };
    let ticket = state.ticket();
    if ticket.original_call.id.trim().is_empty()
        || ticket.original_call.name.trim().is_empty()
        || ticket.root_revision > snapshot.draft.draft_revision
        || ticket.root_revision == u64::MAX
    {
        return Err(snapshot_invariant(
            "repair ticket has an invalid original call or revision",
        ));
    }
    let signature = format!(
        "{}@{}",
        ticket.original_error.code, ticket.original_error.location
    );
    if snapshot
        .observability
        .failure_signatures
        .get(&signature)
        .copied()
        .unwrap_or(0)
        == 0
    {
        return Err(snapshot_invariant(
            "repair ticket root failure is absent from observability",
        ));
    }
    let mut allowed = BTreeSet::new();
    if ticket.allowed_repair_tools.iter().any(|name| {
        name.trim().is_empty()
            || !allowed.insert(name.as_str())
            || !tool_definitions().iter().any(|tool| tool.name == *name)
    }) {
        return Err(snapshot_invariant(
            "repair ticket contains invalid allowed tools",
        ));
    }
    match ticket.kind {
        RepairKind::Arguments => {
            let definition = tool_definitions()
                .into_iter()
                .find(|tool| tool.name == ticket.original_call.name)
                .ok_or_else(|| snapshot_invariant("argument repair tool is not registered"))?;
            if ticket.expected_argument_schema.as_ref() != Some(&definition.parameters)
                || ticket.allowed_repair_tools != vec![ticket.original_call.name.clone()]
                || ticket.verification_path != vec![ticket.original_call.name.clone()]
                || !is_argument_failure(&ticket.original_error.code)
                || !matches!(
                    state,
                    RepairState::AwaitingAttempt(_) | RepairState::Failed(_)
                )
            {
                return Err(snapshot_invariant(
                    "argument repair ticket shape is inconsistent",
                ));
            }
        }
        RepairKind::Validation | RepairKind::Simulation => {
            if ticket.expected_argument_schema.is_some()
                || ticket.allowed_repair_tools.is_empty()
                || ticket
                    .allowed_repair_tools
                    .iter()
                    .any(|name| !is_mutation_tool(name))
            {
                return Err(snapshot_invariant(
                    "gate repair ticket does not contain mutation-only tools",
                ));
            }
            let expected_path = match ticket.kind {
                RepairKind::Validation => vec!["mutation", "validate_draft"],
                RepairKind::Simulation => {
                    vec!["mutation", "validate_draft", "simulate_draft"]
                }
                RepairKind::Arguments => unreachable!(),
            };
            if ticket.verification_path != expected_path
                || ticket.original_call.name
                    != match ticket.kind {
                        RepairKind::Validation => "validate_draft",
                        RepairKind::Simulation => "simulate_draft",
                        RepairKind::Arguments => unreachable!(),
                    }
                || is_argument_failure(&ticket.original_error.code)
            {
                return Err(snapshot_invariant(
                    "gate repair ticket root or verification path is inconsistent",
                ));
            }
            if matches!(state, RepairState::AwaitingAttempt(_)) {
                let expected_allowed =
                    routed_tool_definitions(&snapshot.draft, &tool_definitions())
                        .into_iter()
                        .filter(|tool| is_mutation_tool(&tool.name))
                        .map(|tool| tool.name)
                        .collect::<Vec<_>>();
                if ticket.allowed_repair_tools != expected_allowed {
                    return Err(snapshot_invariant(
                        "gate repair tools do not match the root Draft state",
                    ));
                }
            }
        }
    }
    match state {
        RepairState::AwaitingAttempt(_) => {
            if ticket.attempts_remaining != 1
                || ticket.root_revision != snapshot.draft.draft_revision
                || snapshot.last_error.as_ref() != Some(&ticket.original_error)
            {
                return Err(snapshot_invariant(
                    "awaiting repair ticket does not match the root failure state",
                ));
            }
        }
        RepairState::VerifyValidation(_) => {
            let expected_revision = ticket
                .root_revision
                .checked_add(1)
                .ok_or_else(|| snapshot_invariant("repair revision overflow"))?;
            if ticket.attempts_remaining != 0
                || ticket.kind == RepairKind::Arguments
                || snapshot.observability.repair_attempts <= snapshot.observability.repair_successes
                || snapshot.draft.draft_revision != expected_revision
                || snapshot.draft.validated_revision.is_some()
                || snapshot.draft.simulated_revision.is_some()
            {
                return Err(snapshot_invariant(
                    "validation verification state is inconsistent",
                ));
            }
        }
        RepairState::VerifySimulation(_) => {
            let expected_revision = ticket
                .root_revision
                .checked_add(1)
                .ok_or_else(|| snapshot_invariant("repair revision overflow"))?;
            if ticket.attempts_remaining != 0
                || ticket.kind != RepairKind::Simulation
                || snapshot.observability.repair_attempts <= snapshot.observability.repair_successes
                || snapshot.draft.draft_revision != expected_revision
                || snapshot.draft.validated_revision != Some(snapshot.draft.draft_revision)
                || snapshot.draft.simulated_revision.is_some()
            {
                return Err(snapshot_invariant(
                    "simulation verification state is inconsistent",
                ));
            }
        }
        RepairState::Failed(_) => {
            if ticket.attempts_remaining != 0 || snapshot.observability.repair_failures == 0 {
                return Err(snapshot_invariant("failed repair state is inconsistent"));
            }
        }
    }
    if !snapshot
        .messages
        .iter()
        .filter_map(parse_repair_directive)
        .any(|directive| directive_matches_ticket(&directive, ticket))
    {
        return Err(snapshot_invariant(
            "repair ticket has no matching repair directive",
        ));
    }
    Ok(())
}

fn parse_repair_directive(message: &Message) -> Option<RepairDirective> {
    if message.role != MessageRole::User {
        return None;
    }
    let value = message.content.strip_prefix(REPAIR_REQUIRED_PREFIX)?;
    serde_json::from_str(value).ok()
}

fn directive_matches_ticket(directive: &RepairDirective, ticket: &RepairTicket) -> bool {
    directive.event == "repair_required"
        && directive.attempts_remaining == 1
        && directive.original.tool == ticket.original_call
        && directive.original.error == ticket.original_error
        && directive.expected_argument_schema == ticket.expected_argument_schema
        && directive.allowed_repair_tools == ticket.allowed_repair_tools
        && directive.verification_path == ticket.verification_path
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
    fn automatic_call(&self, name: &str) -> ToolCall {
        let turn = self.turn_state.as_ref().map_or(0, |state| state.sequence);
        ToolCall {
            id: format!("harness-{turn}-{}-{name}", self.draft.draft_revision),
            name: name.to_string(),
            arguments: "{}".to_string(),
        }
    }

    async fn run_automatic_gate(&mut self, name: &str) -> Result<bool, BurstOutcome> {
        if self.turn_tool_calls() >= self.config.max_tool_calls {
            return Err(self.halt(
                "TOOL_CALL_LIMIT_EXHAUSTED",
                "The session exhausted its executed tool call budget",
                Some(LimitKind::ToolCalls),
            ));
        }
        self.record_tool_call();
        let call = self.automatic_call(name);
        let request_tools = definitions_named(&self.tools, &[name]);
        let result = dispatch_tool(&mut self.draft, name, &call.arguments).await;
        if result.is_ok() {
            self.last_error = None;
        }
        self.advance_adaptive_after_draft_tool(name, result.is_ok());
        self.record_failure(Some(name), &result);
        if result.is_ok() {
            return Ok(true);
        }
        if self.turn_gate_failures() >= self.config.max_gate_failures {
            return Err(self.halt(
                "GATE_FAILURE_LIMIT_EXHAUSTED",
                "The session exhausted its validation and simulation failure budget",
                Some(LimitKind::GateFailures),
            ));
        }
        if let Some(state) = self.repair_state.clone() {
            let error = result.failure().map_or_else(
                || {
                    StructuredError::new(
                        "REPAIR_GATE_FAILED",
                        format!("repair.{name}"),
                        "The automatic repair verification gate failed",
                        "Escalate to a human before continuing the design",
                    )
                },
                |failure| {
                    StructuredError::new(
                        failure.code.clone(),
                        failure.location.clone(),
                        failure.message.clone(),
                        failure.hint.clone(),
                    )
                },
            );
            return Err(self.fail_repair(state.ticket().clone(), error, false));
        }
        if let Some(ticket) = self.root_repair_ticket(&call, &result, &request_tools) {
            self.append_repair_directive(&ticket);
            self.repair_state = Some(RepairState::AwaitingAttempt(ticket));
        }
        Ok(false)
    }

    async fn run_automatic_preview(&mut self) -> Result<bool, BurstOutcome> {
        if self.turn_tool_calls() >= self.config.max_tool_calls {
            return Err(self.halt(
                "TOOL_CALL_LIMIT_EXHAUSTED",
                "The session exhausted its executed tool call budget",
                Some(LimitKind::ToolCalls),
            ));
        }
        self.record_tool_call();
        let (result, _) = self.dispatch_control_tool("render_preview", "{}");
        if result.is_ok() {
            self.last_error = None;
            return Ok(true);
        }
        self.record_failure(Some("render_preview"), &result);
        let error = result.failure().map_or_else(
            || {
                StructuredError::new(
                    "AUTOMATIC_PREVIEW_FAILED",
                    "tool.render_preview",
                    "The deterministic preview step failed",
                    "Inspect the validated Draft and preview state before retrying",
                )
            },
            |failure| {
                StructuredError::new(
                    failure.code.clone(),
                    failure.location.clone(),
                    failure.message.clone(),
                    failure.hint.clone(),
                )
            },
        );
        self.last_error = Some(error);
        Err(self.halt(
            "AUTOMATIC_PREVIEW_FAILED",
            "The deterministic preview step failed",
            None,
        ))
    }

    async fn run_automatic_adaptive_phases(&mut self) -> Option<BurstOutcome> {
        if !self.adaptive_enabled || self.repair_state.is_some() {
            return None;
        }
        loop {
            let phase = self.adaptive_turn.as_ref().map(|state| state.phase);
            match phase {
                Some(AdaptivePhase::Verify) => {
                    match self.run_automatic_gate("validate_draft").await {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(outcome) => return Some(outcome),
                    }
                }
                Some(AdaptivePhase::Simulate) => {
                    match self.run_automatic_gate("simulate_draft").await {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(outcome) => return Some(outcome),
                    }
                }
                Some(AdaptivePhase::Preview) => match self.run_automatic_preview().await {
                    Ok(true) => {}
                    Ok(false) => return None,
                    Err(outcome) => return Some(outcome),
                },
                _ => return None,
            }
        }
    }

    async fn run_automatic_repair_verification(&mut self) -> Option<BurstOutcome> {
        if !self.adaptive_enabled {
            return None;
        }
        loop {
            let Some(state) = self.repair_state.clone() else {
                return self.run_automatic_adaptive_phases().await;
            };
            let ticket = state.ticket().clone();
            match state {
                RepairState::VerifyValidation(_) => {
                    match self.run_automatic_gate("validate_draft").await {
                        Ok(true) => match ticket.kind {
                            RepairKind::Validation => {
                                self.repair_state = None;
                                self.last_error = None;
                                self.observability.repair_successes += 1;
                            }
                            RepairKind::Simulation => {
                                self.repair_state = Some(RepairState::VerifySimulation(ticket));
                            }
                            RepairKind::Arguments => {
                                let error = StructuredError::new(
                                    "REPAIR_STATE_INVALID",
                                    "repair.state",
                                    "Argument repair entered validation verification",
                                    "Escalate to a human and restart the repair",
                                );
                                return Some(self.fail_repair(ticket, error, true));
                            }
                        },
                        Ok(false) => return None,
                        Err(outcome) => return Some(outcome),
                    }
                }
                RepairState::VerifySimulation(_) => {
                    match self.run_automatic_gate("simulate_draft").await {
                        Ok(true) => {
                            self.repair_state = None;
                            self.last_error = None;
                            self.observability.repair_successes += 1;
                        }
                        Ok(false) => return None,
                        Err(outcome) => return Some(outcome),
                    }
                }
                _ => return None,
            }
        }
    }

    pub async fn run_burst(&mut self, human_message: &str) -> BurstOutcome {
        self.begin_turn(human_message);
        if matches!(self.repair_state, Some(RepairState::Failed(_))) {
            self.repair_state = None;
            self.observability.repair_escalations += 1;
        }
        self.messages.push(Message::user(human_message));
        self.prose_nudged = false;
        loop {
            if self.turn_model_calls() >= self.config.max_model_calls {
                if let Some(state) = self.repair_state.clone() {
                    let error = StructuredError::new(
                        "REPAIR_MODEL_CALL_LIMIT",
                        "repair.model_calls",
                        "The repair could not continue because the model call budget is exhausted",
                        "Escalate to a human before continuing the design",
                    );
                    return self.fail_repair(state.ticket().clone(), error, true);
                }
                return self.halt(
                    "MODEL_CALL_LIMIT_EXHAUSTED",
                    "The session exhausted its model call budget",
                    Some(LimitKind::ModelCalls),
                );
            }
            self.append_anchor();
            let routed_tools = self.routed_tools();
            let Some(outbound_messages) = self.fit_context(&routed_tools) else {
                if let Some(state) = self.repair_state.clone() {
                    let error = StructuredError::new(
                        "REPAIR_CONTEXT_LIMIT",
                        "repair.context",
                        "The repair directive and current Draft do not fit the context budget",
                        "Increase the context budget or escalate to a human",
                    );
                    return self.fail_repair(state.ticket().clone(), error, true);
                }
                return self.halt(
                    "CONTEXT_CHAR_LIMIT_EXHAUSTED",
                    "The system prompt, tool schemas, and current Draft anchor do not fit",
                    Some(LimitKind::ContextChars),
                );
            };

            self.record_model_call();
            let response = match self
                .client
                .complete(&outbound_messages, &routed_tools)
                .await
            {
                Ok(response) => response,
                Err(_) => {
                    let error = StructuredError::new(
                        "LLM_CLIENT_ERROR",
                        "llm",
                        "The model request failed",
                        "Stop the burst and retry after the model gateway is available",
                    );
                    if let Some(state) = self.repair_state.clone() {
                        return self.fail_repair(state.ticket().clone(), error, true);
                    }
                    self.last_error = Some(error);
                    return self.halt("LLM_CLIENT_ERROR", "The model client failed", None);
                }
            };

            if self.repair_state.is_some() {
                if let Some(outcome) = self.handle_repair_response(response, &routed_tools).await {
                    return outcome;
                }
                continue;
            }

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
                        if self.turn_tool_calls() >= self.config.max_tool_calls {
                            self.append_not_executed(&calls[index..]);
                            return self.halt(
                                "TOOL_CALL_LIMIT_EXHAUSTED",
                                "The session exhausted its executed tool call budget",
                                Some(LimitKind::ToolCalls),
                            );
                        }
                        self.record_tool_call();
                        let phase_before = self.adaptive_turn.as_ref().map(|state| state.phase);
                        let control = is_control_tool(&call.name);
                        let available = routed_tools.iter().any(|tool| tool.name == call.name)
                            && (control || tool_is_available(&self.draft, &call.name));
                        let (result, control_outcome) = if available && control {
                            self.dispatch_control_tool(&call.name, &call.arguments)
                        } else if available {
                            (
                                dispatch_tool(&mut self.draft, &call.name, &call.arguments).await,
                                None,
                            )
                        } else {
                            (
                                self.unavailable_tool_result(&call.name, &self.routed_tools()),
                                None,
                            )
                        };
                        if result.is_ok() && is_mutation_tool(&call.name) {
                            self.observability
                                .distinct_mutation_tools
                                .insert(call.name.clone());
                            *self
                                .observability
                                .mutation_tool_calls
                                .entry(call.name.clone())
                                .or_default() += 1;
                        }
                        if result.is_ok() {
                            self.last_error = None;
                        }
                        self.advance_adaptive_after_draft_tool(&call.name, result.is_ok());
                        self.record_failure(available.then_some(call.name.as_str()), &result);
                        let is_failure = !result.is_ok();
                        self.messages
                            .push(Message::tool(call.id.clone(), result.as_json()));
                        if self.adaptive_enabled
                            && call.name == "check_turn_scope"
                            && result.is_ok()
                        {
                            if let Some(outcome) = self.run_automatic_adaptive_phases().await {
                                self.append_phase_transition_not_executed(&calls[index + 1..]);
                                return outcome;
                            }
                        }
                        if let Some(outcome) = control_outcome {
                            self.append_phase_transition_not_executed(&calls[index + 1..]);
                            return outcome;
                        }
                        if is_failure {
                            failed = true;
                            self.append_not_executed(&calls[index + 1..]);
                            if self.turn_gate_failures() >= self.config.max_gate_failures {
                                return self.halt(
                                    "GATE_FAILURE_LIMIT_EXHAUSTED",
                                    "The session exhausted its validation and simulation failure budget",
                                    Some(LimitKind::GateFailures),
                                );
                            }
                            if available && !control {
                                if let Some(ticket) =
                                    self.root_repair_ticket(call, &result, &routed_tools)
                                {
                                    self.append_repair_directive(&ticket);
                                    self.repair_state = Some(RepairState::AwaitingAttempt(ticket));
                                }
                            }
                            break;
                        }
                        let phase_after = self.adaptive_turn.as_ref().map(|state| state.phase);
                        if phase_before != phase_after {
                            self.append_phase_transition_not_executed(&calls[index + 1..]);
                            break;
                        }
                    }
                    if failed {
                        continue;
                    }
                }
                LlmResponse::Text(text) => {
                    self.messages.push(Message::assistant(text.clone()));
                    if self.adaptive_enabled {
                        if !self.prose_nudged {
                            self.prose_nudged = true;
                            self.add_nudge();
                            continue;
                        }
                        return self.halt("UNSTRUCTURED_MODEL_TEXT", &text, None);
                    }
                    if let Some(question) = text.strip_prefix("QUESTION:") {
                        self.observability.clarification_count += 1;
                        return self.needs_input(question.trim().to_string());
                    }
                    if let Some(summary) = text.strip_prefix("PROGRESSED:") {
                        if self
                            .turn_state
                            .as_ref()
                            .is_some_and(|state| state.started_revision < self.draft.draft_revision)
                        {
                            return self.progressed(summary.trim().to_string());
                        }
                        self.add_nudge();
                        continue;
                    }
                    if let Some(summary) = text
                        .strip_prefix("READY:")
                        .or_else(|| text.strip_prefix("DONE:"))
                    {
                        if self.draft.validated_revision == Some(self.draft.draft_revision) {
                            return self.ready(summary.trim().to_string());
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

    async fn handle_repair_response(
        &mut self,
        response: LlmResponse,
        routed_tools: &[ToolDefinition],
    ) -> Option<BurstOutcome> {
        let state = self.repair_state.clone()?;
        let mut ticket = state.ticket().clone();
        let awaiting_attempt = matches!(state, RepairState::AwaitingAttempt(_));
        match response {
            LlmResponse::Text(text) => {
                self.messages.push(Message::assistant(text.clone()));
                if let Some(question) = text.strip_prefix("QUESTION:") {
                    self.observability.clarification_count += 1;
                    self.observability.repair_escalations += 1;
                    self.repair_state = None;
                    return Some(self.needs_input(question.trim().to_string()));
                }
                if awaiting_attempt {
                    self.consume_repair_attempt(&mut ticket);
                }
                let error = StructuredError::new(
                    "REPAIR_RESPONSE_REJECTED",
                    "repair.response",
                    "The repair response did not contain exactly one tool call",
                    "Call exactly one tool exposed for the active repair stage",
                );
                Some(self.fail_repair(ticket, error, true))
            }
            LlmResponse::ToolCalls(calls) => {
                if !valid_tool_call_ids(&calls) {
                    self.messages.push(Message::assistant(
                        "REPAIR_RESPONSE_REJECTED: invalid tool call identifiers",
                    ));
                    if awaiting_attempt {
                        self.consume_repair_attempt(&mut ticket);
                    }
                    let error = StructuredError::new(
                        "REPAIR_RESPONSE_REJECTED",
                        "repair.tool_calls",
                        "The repair response contained empty or duplicate tool call identifiers",
                        "Return exactly one tool call with a non-empty unique identifier",
                    );
                    return Some(self.fail_repair(ticket, error, true));
                }
                self.messages
                    .push(Message::assistant_tool_calls(calls.clone()));
                if calls.len() != 1 {
                    if awaiting_attempt {
                        self.consume_repair_attempt(&mut ticket);
                    }
                    let error = StructuredError::new(
                        "REPAIR_RESPONSE_REJECTED",
                        "repair.tool_calls",
                        "The repair response did not contain exactly one tool call",
                        "Return exactly one tool call from the tools exposed for repair",
                    );
                    self.append_repair_rejections(&calls, &error);
                    return Some(self.fail_repair(ticket, error, false));
                }
                if awaiting_attempt {
                    self.consume_repair_attempt(&mut ticket);
                }
                let call = &calls[0];
                if !routed_tools.iter().any(|tool| tool.name == call.name) {
                    let error = StructuredError::new(
                        "REPAIR_TOOL_MISMATCH",
                        format!("repair.tool.{}", call.name),
                        "The repair response selected a tool outside the active repair stage",
                        format!(
                            "Use exactly one of: {}",
                            routed_tools
                                .iter()
                                .map(|tool| tool.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                    self.append_repair_rejections(&calls, &error);
                    return Some(self.fail_repair(ticket, error, false));
                }
                if self.turn_tool_calls() >= self.config.max_tool_calls {
                    self.append_not_executed(&calls);
                    let error = StructuredError::new(
                        "REPAIR_TOOL_CALL_LIMIT",
                        "repair.tool_calls",
                        "The repair could not execute because the tool call budget is exhausted",
                        "Escalate to a human before continuing the design",
                    );
                    return Some(self.fail_repair(ticket, error, true));
                }
                self.record_tool_call();
                let result = dispatch_tool(&mut self.draft, &call.name, &call.arguments).await;
                if result.is_ok() && is_mutation_tool(&call.name) {
                    self.observability
                        .distinct_mutation_tools
                        .insert(call.name.clone());
                    *self
                        .observability
                        .mutation_tool_calls
                        .entry(call.name.clone())
                        .or_default() += 1;
                }
                if result.is_ok() {
                    self.last_error = None;
                }
                self.advance_adaptive_after_draft_tool(&call.name, result.is_ok());
                self.record_failure(Some(call.name.as_str()), &result);
                let failed = !result.is_ok();
                let failure = result.failure().map(|failure| {
                    StructuredError::new(
                        failure.code.clone(),
                        failure.location.clone(),
                        failure.message.clone(),
                        failure.hint.clone(),
                    )
                });
                self.messages
                    .push(Message::tool(call.id.clone(), result.as_json()));
                if failed {
                    return Some(self.fail_repair(
                        ticket,
                        failure.unwrap_or_else(|| {
                            StructuredError::new(
                                "REPAIR_TOOL_FAILED",
                                "repair.tool",
                                "The repair tool failed",
                                "Escalate to a human before continuing the design",
                            )
                        }),
                        false,
                    ));
                }
                match state {
                    RepairState::AwaitingAttempt(_) => match ticket.kind {
                        RepairKind::Arguments => {
                            self.repair_state = None;
                            self.last_error = None;
                            self.observability.repair_successes += 1;
                        }
                        RepairKind::Validation | RepairKind::Simulation => {
                            self.repair_state = Some(RepairState::VerifyValidation(ticket));
                        }
                    },
                    RepairState::VerifyValidation(_) => match ticket.kind {
                        RepairKind::Validation => {
                            self.repair_state = None;
                            self.last_error = None;
                            self.observability.repair_successes += 1;
                        }
                        RepairKind::Simulation => {
                            self.repair_state = Some(RepairState::VerifySimulation(ticket));
                        }
                        RepairKind::Arguments => {
                            let error = StructuredError::new(
                                "REPAIR_STATE_INVALID",
                                "repair.state",
                                "Argument repair entered validation verification",
                                "Escalate to a human and restart the repair",
                            );
                            return Some(self.fail_repair(ticket, error, true));
                        }
                    },
                    RepairState::VerifySimulation(_) => {
                        self.repair_state = None;
                        self.last_error = None;
                        self.observability.repair_successes += 1;
                    }
                    RepairState::Failed(_) => {
                        let error = StructuredError::new(
                            "REPAIR_STATE_INVALID",
                            "repair.state",
                            "A failed repair attempted another automatic action",
                            "Escalate to a human before continuing the design",
                        );
                        return Some(self.fail_repair(ticket, error, true));
                    }
                }
                if self.adaptive_enabled
                    && matches!(
                        self.repair_state,
                        Some(RepairState::VerifyValidation(_))
                            | Some(RepairState::VerifySimulation(_))
                    )
                {
                    return self.run_automatic_repair_verification().await;
                }
                None
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

fn simulation_profile_for_current_human_turn(messages: &[Message]) -> SimulationProfile {
    let has_study_room = messages
        .iter()
        .rev()
        .find(|message| is_genuine_human_message(message))
        .is_some_and(|message| message.content.to_ascii_lowercase().contains("studyroom"));
    if has_study_room {
        SimulationProfile::StudyRoom
    } else {
        SimulationProfile::None
    }
}

fn is_genuine_human_message(message: &Message) -> bool {
    message.role == MessageRole::User
        && message.content != NUDGE
        && !message.content.starts_with(REPAIR_REQUIRED_PREFIX)
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

fn is_control_tool(name: &str) -> bool {
    matches!(
        name,
        "set_turn_brief" | "check_turn_scope" | "render_preview" | "finish_turn"
    )
}

fn all_tool_definitions() -> Vec<ToolDefinition> {
    let mut definitions = tool_definitions();
    definitions.extend(control_tool_definitions());
    definitions
}

fn legacy_tool_definitions() -> Vec<ToolDefinition> {
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
