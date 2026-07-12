use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::draft::{Draft, DraftSummary};
use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, MessageRole, ToolCall};
use crate::tools::{dispatch_tool, tool_definitions, ToolDefinition};

pub const DEFAULT_SYSTEM_PROMPT: &str = "Design Discord automations only by calling the provided Draft tools. Never touch live Discord, publish, deploy, or activate. Make one logical change per tool call and reference created resources by alias. Validate before simulate. Text replies must be exactly QUESTION: followed by a question or DONE: followed by a summary. Use DONE: only after simulate_draft passes for the current revision.";

const NUDGE: &str = "Call a design tool to change the Draft; use QUESTION: to ask the human; only use DONE: after simulate_draft passes on the current revision.";

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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
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
        let messages = vec![
            Message::system(DEFAULT_SYSTEM_PROMPT),
            Message::system(anchor_content(&draft)),
        ];
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

    fn refresh_anchor(&mut self) {
        self.messages[1].content = anchor_content(&self.draft);
    }

    fn total_context_chars(&self) -> usize {
        let tools = serde_json::to_string(&self.tools).map_or(usize::MAX, |value| value.len());
        let messages =
            serde_json::to_string(&self.messages).map_or(usize::MAX, |value| value.len());
        tools.saturating_add(messages)
    }

    fn fit_context(&mut self) -> bool {
        while self.total_context_chars() > self.config.context_char_budget {
            let Some(index) = self
                .messages
                .iter()
                .enumerate()
                .skip(2)
                .find_map(|(index, message)| (message.role == MessageRole::Tool).then_some(index))
            else {
                return false;
            };
            let tool_call_id = self.messages[index].tool_call_id.clone();
            self.messages.remove(index);
            if let Some(tool_call_id) = tool_call_id {
                self.remove_trimmed_tool_call(&tool_call_id);
            }
        }
        true
    }

    fn remove_trimmed_tool_call(&mut self, tool_call_id: &str) {
        let mut empty_assistant = None;
        for (index, message) in self.messages.iter_mut().enumerate().skip(2) {
            if message.role != MessageRole::Assistant {
                continue;
            }
            let before = message.tool_calls.len();
            message.tool_calls.retain(|call| call.id != tool_call_id);
            if before != message.tool_calls.len()
                && message.tool_calls.is_empty()
                && message.content.is_empty()
            {
                empty_assistant = Some(index);
                break;
            }
        }
        if let Some(index) = empty_assistant {
            self.messages.remove(index);
        }
    }

    fn add_nudge(&mut self) {
        self.messages.push(Message::user(NUDGE));
        self.observability.nudge_count += 1;
    }

    fn record_failure(&mut self, name: &str, result: &ToolResult) {
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
            "validate_draft" => self.observability.validation_failures += 1,
            "simulate_draft" => self.observability.simulation_failures += 1,
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
        self.messages.push(Message::user(human_message));
        self.prose_nudged = false;
        loop {
            self.refresh_anchor();
            if self.observability.model_calls >= self.config.max_model_calls {
                return self.halt(
                    "MODEL_CALL_LIMIT_EXHAUSTED",
                    "The session exhausted its model call budget",
                    Some(LimitKind::ModelCalls),
                );
            }
            if !self.fit_context() {
                return self.halt(
                    "CONTEXT_CHAR_LIMIT_EXHAUSTED",
                    "The system prompt, tool schemas, and current Draft anchor do not fit",
                    Some(LimitKind::ContextChars),
                );
            }

            self.observability.model_calls += 1;
            let response = match self.client.complete(&self.messages, &self.tools).await {
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
                            return self.halt(
                                "TOOL_CALL_LIMIT_EXHAUSTED",
                                "The session exhausted its executed tool call budget",
                                Some(LimitKind::ToolCalls),
                            );
                        }
                        self.observability.tool_calls += 1;
                        let result =
                            dispatch_tool(&mut self.draft, &call.name, &call.arguments).await;
                        if result.is_ok() && is_mutation_tool(&call.name) {
                            self.observability
                                .distinct_mutation_tools
                                .insert(call.name.clone());
                        }
                        self.record_failure(&call.name, &result);
                        let is_failure = !result.is_ok();
                        self.messages
                            .push(Message::tool(call.id.clone(), result.as_json()));
                        self.refresh_anchor();
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

fn anchor_content(draft: &Draft) -> String {
    let summary = serde_json::to_string(&draft.summary()).unwrap_or_else(|_| "{}".to_string());
    format!(
        "DRAFT_STATE:{{\"revision\":{},\"validated_revision\":{},\"simulated_revision\":{},\"summary\":{summary}}}",
        draft.draft_revision,
        optional_revision(draft.validated_revision),
        optional_revision(draft.simulated_revision)
    )
}

fn optional_revision(revision: Option<u64>) -> String {
    revision
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
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
