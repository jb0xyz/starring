use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, MessageRole, ToolCall};
use crate::tools::{dispatch_tool, ToolDefinition};

use super::routing::{is_mutation_tool, routed_tool_definitions};
use super::{
    BurstOutcome, DesignSession, RepairKind, RepairState, RepairTicket, REPAIR_REQUIRED_PREFIX,
};

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

impl<C> DesignSession<C> {
    pub(super) fn append_repair_directive(&mut self, ticket: &RepairTicket) {
        let directive = RepairDirective::from_ticket(ticket);
        let json = serde_json::to_string(&directive).unwrap_or_else(|_| {
            r#"{"event":"repair_required","attempts_remaining":1}"#.to_string()
        });
        self.messages
            .push(Message::user(format!("{REPAIR_REQUIRED_PREFIX}{json}")));
    }

    pub(super) fn root_repair_ticket(
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

    pub(super) fn fail_repair(
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

impl<C: LlmClient> DesignSession<C> {
    pub(super) async fn handle_repair_response(
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
            LlmResponse::Provenanced { .. } => {
                let error = StructuredError::new(
                    "LLM_PROVENANCE_CONFLICT",
                    "llm.provenance",
                    "The model completion contained nested provenance",
                    "Retry the turn with a fresh model completion",
                );
                Some(self.fail_repair(ticket, error, true))
            }
        }
    }
}

pub(super) fn is_argument_failure(code: &str) -> bool {
    matches!(
        code,
        "MISSING_REQUIRED_FIELD"
            | "INVALID_KIND"
            | "INVALID_TOOL_ARGUMENTS"
            | "UNKNOWN_FIELD"
            | "INVALID_FIELD_TYPE"
    )
}

pub(super) fn has_matching_repair_directive(messages: &[Message], ticket: &RepairTicket) -> bool {
    messages
        .iter()
        .filter_map(parse_repair_directive)
        .any(|directive| directive_matches_ticket(&directive, ticket))
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

fn valid_tool_call_ids(calls: &[ToolCall]) -> bool {
    let mut ids = BTreeSet::new();
    calls.iter().all(|call| {
        !call.id.trim().is_empty() && !call.name.trim().is_empty() && ids.insert(call.id.as_str())
    })
}
