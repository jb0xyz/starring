use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::draft::{Draft, DraftSummary};
use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, MessageRole, ToolCall};
use crate::tools::{dispatch_tool, ToolDefinition};
use crate::turn::{
    check_scope, normalize_turn_plan, parse_empty_control, parse_finish_turn, parse_turn_brief,
    parse_turn_plan, render_preview, AdaptivePhase, AdaptiveTurnState, FinishTurnKind,
    RequestedOutcome, SimulationProfile, TurnBrief, TurnIntent,
};

mod adaptive;
mod context;
mod frontier;
mod repair;
mod routing;
mod snapshot;

use adaptive::simulation_profile_for_current_human_turn;
use context::compact_text;
use repair::is_argument_failure;
use routing::{
    all_tool_definitions, is_control_tool, is_mutation_tool, legacy_tool_definitions,
    tool_is_available,
};
use snapshot::validate_snapshot;

pub const DEFAULT_SYSTEM_PROMPT: &str = "Design Discord automations only with the provided tools. Never touch live Discord, publish, deploy, or activate. At the start of every human turn call set_turn_brief with only a concise intent, objective, requested outcome, assumptions, and whether validation is required. Classify adding new Draft structure as build and changing or removing existing structure as modify. Build turns use set_turn_plan for exact ordered semantic requirements. A modify turn may offer Draft-legal update or remove tools beside set_turn_plan; use set_turn_plan when the requested work is actually additive, and use update or remove only for a true edit or removal. After attempting set_turn_plan, repair only through set_turn_plan. The harness executes accepted plans deterministically. The harness deterministically enables StudyRoom simulation from the exact human message; set validate to true whenever the human explicitly says StudyRoom. Use discussion or brainstorm for design conversation or a missing structural decision, then finish_turn with one focused question or response without changing the Draft. For build or modify, continue in the same turn with the staged design tools and call check_turn_scope after the requested Draft change is complete. For an unchanged existing Draft verification turn, use inspect with validated_preview and validate true; the harness scopes the current revision and automatically validates, runs the selected simulation, and renders the preview without a mutation. The harness then automatically runs requested validation, harness-selected simulation, and preview steps. When finish_turn is the only available tool, call it with kind ready and summarize the result. Use safe defaults only for non-blocking details. Actual edits and removals must use update or remove tools instead of creating duplicates. Reference created resources by alias. Never ask whether to continue, stop, validate, or review. Legacy QUESTION, PROGRESSED, and READY text are accepted only for compatibility; prefer finish_turn.";

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
    #[serde(default)]
    pub plan_submissions: usize,
    #[serde(default)]
    pub plan_acceptances: usize,
    #[serde(default)]
    pub planned_requirements: usize,
    #[serde(default)]
    pub plan_compiled_tool_calls: usize,
    #[serde(default)]
    pub plan_execution_failures: usize,
    #[serde(default)]
    pub plan_rollbacks: usize,
    #[serde(default)]
    pub plan_commits: usize,
    #[serde(default)]
    pub plan_conflicts: usize,
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
    planned_enabled: bool,
    planned_execution_attempts: u8,
    planned_root_draft: Option<Draft>,
    brief_history: Vec<TurnBrief>,
}

impl<C> DesignSession<C> {
    pub fn new(client: C) -> Self {
        Self::with_config(client, SessionConfig::default())
    }

    pub fn with_config(client: C, config: SessionConfig) -> Self {
        Self::build(client, config, false, false)
    }

    pub fn with_adaptive_config(client: C, config: SessionConfig) -> Self {
        Self::build(client, config, true, false)
    }

    pub fn with_planned_config(client: C, config: SessionConfig) -> Self {
        Self::build(client, config, true, true)
    }

    fn build(
        client: C,
        config: SessionConfig,
        adaptive_enabled: bool,
        planned_enabled: bool,
    ) -> Self {
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
            planned_enabled,
            planned_execution_attempts: 0,
            planned_root_draft: None,
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

    pub fn planned_enabled(&self) -> bool {
        self.planned_enabled
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
        Self::restore_with_mode(client, config, snapshot, false)
    }

    pub fn restore_planned(
        client: C,
        config: SessionConfig,
        snapshot: SessionSnapshot,
    ) -> Result<Self, SessionSnapshotError> {
        Self::restore_with_mode(client, config, snapshot, true)
    }

    fn restore_with_mode(
        client: C,
        config: SessionConfig,
        snapshot: SessionSnapshot,
        planned_enabled: bool,
    ) -> Result<Self, SessionSnapshotError> {
        validate_snapshot(&snapshot)?;
        let adaptive_enabled = snapshot.adaptive_enabled || planned_enabled;
        Ok(Self {
            client,
            draft: snapshot.draft,
            messages: snapshot.messages,
            tools: if adaptive_enabled {
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
            adaptive_enabled,
            planned_enabled,
            planned_execution_attempts: 0,
            planned_root_draft: None,
            brief_history: snapshot.brief_history,
        })
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
        self.planned_execution_attempts = 0;
        self.planned_root_draft = None;
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

    fn awaiting_planned_submission(&self) -> bool {
        self.planned_enabled
            && self.adaptive_turn.as_ref().is_some_and(|state| {
                state.phase == AdaptivePhase::Build
                    && state.brief.as_ref().is_some_and(|brief| {
                        brief.requirements.is_empty()
                            && (brief.intent == TurnIntent::Build
                                || brief.intent == TurnIntent::Modify
                                    && self.planned_execution_attempts > 0)
                    })
            })
    }

    fn awaiting_planned_modify_choice(&self) -> bool {
        self.planned_enabled
            && self.planned_execution_attempts == 0
            && self.adaptive_turn.as_ref().is_some_and(|state| {
                state.phase == AdaptivePhase::Build
                    && state.brief.as_ref().is_some_and(|brief| {
                        brief.intent == TurnIntent::Modify && brief.requirements.is_empty()
                    })
            })
            && self
                .turn_state
                .as_ref()
                .is_some_and(|turn| turn.started_revision == self.draft.draft_revision)
    }

    fn reject_mixed_planned_modify_batch(&mut self, calls: &[ToolCall]) {
        self.messages
            .push(Message::assistant_tool_calls(calls.to_vec()));
        self.planned_execution_attempts = self.planned_execution_attempts.saturating_add(1);
        self.observability.plan_submissions = self.observability.plan_submissions.saturating_add(1);
        let result = ToolResult::failure_from(
            &self.draft,
            StructuredError::new(
                "TURN_PLAN_MIXED_EXECUTION_PATHS",
                "turn.plan.response",
                "A modify-labeled response combined set_turn_plan with another tool call",
                "Submit exactly one set_turn_plan call for additive work, or use only update and remove tools for a true modification",
            ),
        );
        self.record_failure(Some("set_turn_plan"), &result);
        let mut reported = false;
        for call in calls {
            let content = if call.name == "set_turn_plan" && !reported {
                reported = true;
                result.as_json()
            } else {
                self.not_executed_result().as_json()
            };
            self.messages.push(Message::tool(call.id.clone(), content));
        }
    }

    fn consume_planned_response_failure(
        &mut self,
        attempts_before: u8,
        error: Option<StructuredError>,
    ) -> Option<BurstOutcome> {
        if self.planned_execution_attempts == attempts_before {
            self.planned_execution_attempts = self.planned_execution_attempts.saturating_add(1);
        }
        if let Some(error) = error {
            let result = ToolResult::failure_from(&self.draft, error);
            self.record_failure(None, &result);
        }
        if self.planned_execution_attempts >= 2 {
            return Some(self.halt(
                "PLAN_REPAIR_FAILED",
                "The single automatic turn-plan repair failed",
                None,
            ));
        }
        self.add_nudge();
        None
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
                let verification_only = brief.intent == TurnIntent::Inspect
                    && brief.requested_outcome == RequestedOutcome::ValidatedPreview
                    && brief.verification.validate;
                let phase = if verification_only {
                    AdaptivePhase::Verify
                } else if brief.requested_outcome == RequestedOutcome::Discussion
                    || matches!(brief.intent, TurnIntent::Brainstorm | TurnIntent::Inspect)
                {
                    AdaptivePhase::Reply
                } else {
                    AdaptivePhase::Build
                };
                self.adaptive_turn = Some(AdaptiveTurnState {
                    phase,
                    brief: Some(brief),
                    scoped_revision: verification_only.then_some(self.draft.draft_revision),
                    previewed_revision: None,
                });
                (
                    ToolResult::success(&self.draft, "Recorded the current turn brief"),
                    None,
                )
            }
            "set_turn_plan" => {
                if !self.planned_enabled {
                    return (
                        ToolResult::failure_from(
                            &self.draft,
                            StructuredError::new(
                                "TURN_PLAN_MODE_REQUIRED",
                                "tool.set_turn_plan",
                                "Typed turn plans are not enabled for this session",
                                "Use a planned design session before setting a turn plan",
                            ),
                        ),
                        None,
                    );
                }
                self.planned_execution_attempts = self.planned_execution_attempts.saturating_add(1);
                let requirements = match parse_turn_plan(arguments) {
                    Ok(requirements) => requirements,
                    Err(error) => return (ToolResult::failure_from(&self.draft, error), None),
                };
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
                                "tool.set_turn_plan",
                                "No active turn brief exists",
                                "Call set_turn_brief before setting the turn plan",
                            ),
                        ),
                        None,
                    );
                };
                let requirements = match normalize_turn_plan(&self.draft, &brief, requirements) {
                    Ok(requirements) => requirements,
                    Err(error) => {
                        if frontier::is_plan_conflict_code(&error.code) {
                            self.observability.plan_conflicts =
                                self.observability.plan_conflicts.saturating_add(1);
                        }
                        return (ToolResult::failure_from(&self.draft, error), None);
                    }
                };
                self.observability.plan_acceptances =
                    self.observability.plan_acceptances.saturating_add(1);
                self.observability.planned_requirements = self
                    .observability
                    .planned_requirements
                    .saturating_add(requirements.len());
                if let Some(brief) = self
                    .adaptive_turn
                    .as_mut()
                    .and_then(|state| state.brief.as_mut())
                {
                    brief.requirements = requirements;
                }
                (
                    ToolResult::success(&self.draft, "Accepted the deterministic turn plan"),
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

    fn append_not_executed(&mut self, calls: &[ToolCall]) {
        for call in calls {
            let result = self.not_executed_result();
            self.messages
                .push(Message::tool(call.id.clone(), result.as_json()));
        }
    }
}

impl<C: LlmClient> DesignSession<C> {
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
                    let awaiting_plan = self.awaiting_planned_submission();
                    let modify_selects_plan = self.awaiting_planned_modify_choice()
                        && calls.iter().any(|call| call.name == "set_turn_plan");
                    let plan_attempts_before = self.planned_execution_attempts;
                    if calls.is_empty() {
                        if awaiting_plan {
                            self.messages.push(Message::assistant(
                                "TURN_PLAN_RESPONSE_REJECTED: empty tool call batch",
                            ));
                            let error = StructuredError::new(
                                "TURN_PLAN_RESPONSE_REQUIRED",
                                "turn.plan.response",
                                "The planning response did not submit set_turn_plan",
                                "Call the sole routed set_turn_plan tool with exact ordered requirements",
                            );
                            if let Some(outcome) = self
                                .consume_planned_response_failure(plan_attempts_before, Some(error))
                            {
                                return outcome;
                            }
                            continue;
                        }
                        return self.halt(
                            "EMPTY_TOOL_CALL_BATCH",
                            "The model returned an empty tool call batch",
                            None,
                        );
                    }
                    if (awaiting_plan || modify_selects_plan)
                        && !valid_planned_tool_call_ids(&calls)
                    {
                        self.messages.push(Message::assistant(
                            "TURN_PLAN_RESPONSE_REJECTED: invalid tool call identifiers",
                        ));
                        let error = StructuredError::new(
                            "TURN_PLAN_RESPONSE_REJECTED",
                            "turn.plan.response",
                            "The planning response contained empty or duplicate tool call identifiers",
                            "Return one set_turn_plan call with a non-empty unique identifier",
                        );
                        if let Some(outcome) =
                            self.consume_planned_response_failure(plan_attempts_before, Some(error))
                        {
                            return outcome;
                        }
                        continue;
                    }
                    if modify_selects_plan && (calls.len() != 1 || calls[0].name != "set_turn_plan")
                    {
                        self.reject_mixed_planned_modify_batch(&calls);
                        continue;
                    }
                    self.prose_nudged = false;
                    self.messages
                        .push(Message::assistant_tool_calls(calls.clone()));
                    let mut failed = false;
                    let mut submitted_plan = false;
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
                        if available && call.name == "set_turn_plan" {
                            self.observability.plan_submissions =
                                self.observability.plan_submissions.saturating_add(1);
                        }
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
                        if available && call.name == "set_turn_plan" && result.is_ok() {
                            submitted_plan = true;
                        }
                        self.messages
                            .push(Message::tool(call.id.clone(), result.as_json()));
                        if call.name == "set_turn_plan"
                            && !result.is_ok()
                            && self.planned_execution_attempts >= 2
                        {
                            self.append_not_executed(&calls[index + 1..]);
                            return self.halt(
                                "PLAN_REPAIR_FAILED",
                                "The single automatic turn-plan repair failed",
                                None,
                            );
                        }
                        if self.adaptive_enabled
                            && matches!(
                                call.name.as_str(),
                                "set_turn_brief" | "set_turn_plan" | "check_turn_scope"
                            )
                            && result.is_ok()
                        {
                            let outcome = if call.name == "set_turn_plan" {
                                self.run_automatic_planned_execution().await
                            } else {
                                self.run_automatic_adaptive_phases().await
                            };
                            if let Some(outcome) = outcome {
                                self.append_phase_transition_not_executed(&calls[index + 1..]);
                                return outcome;
                            }
                            if call.name == "set_turn_plan" {
                                self.append_phase_transition_not_executed(&calls[index + 1..]);
                                break;
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
                    if awaiting_plan && !submitted_plan {
                        if let Some(outcome) =
                            self.consume_planned_response_failure(plan_attempts_before, None)
                        {
                            return outcome;
                        }
                    }
                    if failed {
                        continue;
                    }
                }
                LlmResponse::Text(text) => {
                    let awaiting_plan = self.awaiting_planned_submission();
                    let plan_attempts_before = self.planned_execution_attempts;
                    self.messages.push(Message::assistant(text.clone()));
                    if awaiting_plan {
                        let error = StructuredError::new(
                            "TURN_PLAN_RESPONSE_REQUIRED",
                            "turn.plan.response",
                            "The planning response returned prose instead of set_turn_plan",
                            "Call the sole routed set_turn_plan tool with exact ordered requirements",
                        );
                        if let Some(outcome) =
                            self.consume_planned_response_failure(plan_attempts_before, Some(error))
                        {
                            return outcome;
                        }
                        continue;
                    }
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
}

fn is_genuine_human_message(message: &Message) -> bool {
    message.role == MessageRole::User
        && message.content != NUDGE
        && !message.content.starts_with(REPAIR_REQUIRED_PREFIX)
}

fn valid_planned_tool_call_ids(calls: &[ToolCall]) -> bool {
    let mut ids = BTreeSet::new();
    calls
        .iter()
        .all(|call| !call.id.trim().is_empty() && ids.insert(call.id.as_str()))
}
