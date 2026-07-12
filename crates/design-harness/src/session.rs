use std::collections::{BTreeMap, BTreeSet, VecDeque};

use automation_state::{ActionSpec, ButtonRoute, TriggerSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::draft::{Draft, DraftSummary};
use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, MessageRole, ToolCall};
use crate::tools::{dispatch_tool, tool_definitions, ToolDefinition};

pub const DEFAULT_SYSTEM_PROMPT: &str = "Design Discord automations only by calling the provided Draft tools. Never touch live Discord, publish, deploy, or activate. Make one logical change per tool call and reference created resources by alias. Validate before simulate. Text replies must be exactly QUESTION: followed by a question or DONE: followed by a summary. Use DONE: only after simulate_draft passes for the current revision.";

const NUDGE: &str = "Call a design tool to change the Draft; use QUESTION: to ask the human; only use DONE: after simulate_draft passes on the current revision.";
const REPAIR_REQUIRED_PREFIX: &str = "REPAIR_REQUIRED:";
const MAX_INTENT_MEMORY_ITEMS: usize = 6;
const MAX_INTENT_MEMORY_CHARS: usize = 240;
const MAX_ERROR_MEMORY_CHARS: usize = 360;

pub const SESSION_SNAPSHOT_VERSION: u32 = 2;

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
    AwaitingHuman { question: String },
    Completed { summary: String },
    Halted(Box<HaltReport>),
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
            repair_state: None,
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
            repair_state: self.repair_state.clone(),
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
            repair_state: snapshot.repair_state,
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
            None => routed_tool_definitions(&self.draft, &self.tools),
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
                self.observability.validation_failures += 1
            }
            Some("simulate_draft") if !is_argument_failure(&failure.code) => {
                self.observability.simulation_failures += 1
            }
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
    validate_repair_snapshot(snapshot)?;
    validate_message_pairing(&snapshot.messages, snapshot.draft.draft_revision)
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
    pub async fn run_burst(&mut self, human_message: &str) -> BurstOutcome {
        if matches!(self.repair_state, Some(RepairState::Failed(_))) {
            self.repair_state = None;
            self.observability.repair_escalations += 1;
        }
        self.messages.push(Message::user(human_message));
        self.prose_nudged = false;
        loop {
            if self.observability.model_calls >= self.config.max_model_calls {
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

            self.observability.model_calls += 1;
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
                        if result.is_ok() {
                            self.last_error = None;
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
                            if available {
                                if let Some(ticket) =
                                    self.root_repair_ticket(call, &result, &routed_tools)
                                {
                                    self.append_repair_directive(&ticket);
                                    self.repair_state = Some(RepairState::AwaitingAttempt(ticket));
                                }
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
                    return Some(BurstOutcome::AwaitingHuman {
                        question: question.trim().to_string(),
                    });
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
                if self.observability.tool_calls >= self.config.max_tool_calls {
                    self.append_not_executed(&calls);
                    let error = StructuredError::new(
                        "REPAIR_TOOL_CALL_LIMIT",
                        "repair.tool_calls",
                        "The repair could not execute because the tool call budget is exhausted",
                        "Escalate to a human before continuing the design",
                    );
                    return Some(self.fail_repair(ticket, error, true));
                }
                self.observability.tool_calls += 1;
                let result = dispatch_tool(&mut self.draft, &call.name, &call.arguments).await;
                if result.is_ok() && is_mutation_tool(&call.name) {
                    self.observability
                        .distinct_mutation_tools
                        .insert(call.name.clone());
                }
                if result.is_ok() {
                    self.last_error = None;
                }
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
    )
}
