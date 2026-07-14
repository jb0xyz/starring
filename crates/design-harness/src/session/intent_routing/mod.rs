use std::collections::BTreeSet;

use resource_resolution::ResourceBindingMap;
use serde::Serialize;
use serde_json::json;

use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, ToolCall};
use crate::turn::{
    private_study_room_details_frontier, IntentRecipeDetailFacetV3,
    EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
};

use super::{DesignSession, LimitKind, SessionConfig, SessionSnapshot, SessionSnapshotError};

mod adjudicate;
#[cfg(test)]
mod adjudicate_tests;
#[cfg(test)]
mod adjudicate_v3_tests;
mod decision;
mod evidence;
#[cfg(test)]
mod evidence_tests;
mod execute;
mod frontier;
mod state;
mod state_binding;
#[cfg(test)]
mod tests;

use adjudicate::PrivateStudyRoomSelectionV3;
pub use decision::{
    IntentDecisionSourceV2, IntentRouteDecisionKindV2, IntentRouteDecisionV2, PinnedIntentRecipeV2,
    INTENT_ADJUDICATOR_VERSION_V2, INTENT_ADJUDICATOR_VERSION_V3,
    INTENT_RECIPE_PROTOCOL_VERSION_V2, INTENT_RECIPE_PROTOCOL_VERSION_V3,
};
use execute::{IntentCoreExecutionV3, IntentTurnSuccess};
use frontier::IntentFrontierV3;
pub(super) use state::IntentRecipeRuntime;
pub(crate) use state::IntentRecipeSessionSnapshotV1;
use state::{
    intent_error, snapshot_error, IntentRecipeStageSnapshotV1, INTENT_DETAIL_STATE_PREFIX,
    INTENT_HUMAN_PREFIX, INTENT_RECIPE_DECISION_SYSTEM_PROMPT_V3,
    INTENT_RECIPE_DETAIL_SYSTEM_PROMPT_V3, INTENT_STATE_PREFIX,
};
pub(super) use state::{
    validate_intent_recipe_snapshot, INTENT_RECIPE_SYSTEM_PROMPT, INTENT_RECIPE_SYSTEM_PROMPT_V1,
    INTENT_RECIPE_SYSTEM_PROMPT_V2, INTENT_RECIPE_SYSTEM_PROMPT_V3,
};
pub use state::{
    IntentFallbackKind, IntentFallbackV1, IntentRecipeReceiptV1, IntentRecipeStatusV1,
};

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct IntentStateAnchorV1 {
    stage: &'static str,
    expected_revision: u64,
    available_channel_keys: Vec<String>,
    active_question: Option<String>,
    active_options: Vec<String>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct IntentDetailStateAnchorV3<'a> {
    detail_facets: &'a [IntentRecipeDetailFacetV3],
}

fn intent_human_envelope(human_message: &str) -> String {
    format!("{INTENT_HUMAN_PREFIX}{}", json!({"text": human_message}))
}

enum IntentToolRequestFailure {
    Error(StructuredError),
    Limit(StructuredError, LimitKind),
}
impl<C> DesignSession<C> {
    pub fn with_intent_recipe(client: C, bindings: ResourceBindingMap) -> Self {
        Self::with_intent_recipe_config(client, SessionConfig::default(), bindings)
    }

    pub fn with_intent_recipe_config(
        client: C,
        config: SessionConfig,
        bindings: ResourceBindingMap,
    ) -> Self {
        let mut session = Self::build(client, config, false, false, false);
        session.messages = vec![Message::system(INTENT_RECIPE_SYSTEM_PROMPT)];
        session.tools.clear();
        session.intent_recipe = Some(IntentRecipeRuntime::new(bindings));
        session
    }

    pub fn restore_intent_recipe(
        client: C,
        config: SessionConfig,
        snapshot: SessionSnapshot,
        bindings: ResourceBindingMap,
    ) -> Result<Self, SessionSnapshotError> {
        super::validate_snapshot(&snapshot)?;
        let intent_snapshot = snapshot
            .intent_recipe
            .clone()
            .ok_or_else(|| snapshot_error("snapshot is not an intent recipe session"))?;
        let runtime = IntentRecipeRuntime::restore(bindings, intent_snapshot)?;
        let mut session = Self::build(client, config, false, false, false);
        session.draft = snapshot.draft;
        session.messages = snapshot.messages;
        session.observability = snapshot.observability;
        session.last_error = snapshot.last_error;
        session.prose_nudged = snapshot.prose_nudged;
        session.repair_state = snapshot.repair_state;
        session.turn_state = snapshot.turn_state;
        session.adaptive_turn = snapshot.adaptive_turn;
        session.adaptive_enabled = snapshot.adaptive_enabled;
        session.brief_history = snapshot.brief_history;
        session.tools.clear();
        session.intent_recipe = Some(runtime);
        Ok(session)
    }

    pub fn intent_recipe_enabled(&self) -> bool {
        self.intent_recipe.is_some()
    }

    pub fn intent_recipe_binding_fingerprint(&self) -> Option<&str> {
        self.intent_recipe
            .as_ref()
            .map(|runtime| runtime.snapshot.context_fingerprint.as_str())
    }

    pub fn intent_recipe_route_decision(&self) -> Option<&IntentRouteDecisionV2> {
        self.intent_recipe
            .as_ref()
            .and_then(IntentRecipeRuntime::route_decision)
    }

    pub fn intent_recipe_status(&self) -> Option<IntentRecipeStatusV1> {
        let runtime = self.intent_recipe.as_ref()?;
        Some(match &runtime.snapshot.stage {
            IntentRecipeStageSnapshotV1::Empty => IntentRecipeStatusV1::Empty {
                expected_revision: runtime.expected_revision(self.draft.draft_revision),
            },
            IntentRecipeStageSnapshotV1::AwaitingDecision {
                root_draft_revision,
                workspace,
                active_decision,
                ..
            } => IntentRecipeStatusV1::AwaitingDecision {
                root_draft_revision: *root_draft_revision,
                workspace_revision: workspace.revision,
                question: active_decision.question.clone(),
                available_channel_keys: active_decision.options.clone(),
            },
            IntentRecipeStageSnapshotV1::PreviewReady {
                root_draft_revision,
                workspace,
                intent_revision,
                candidate_revision,
                input_intent_hash,
                semantic_intent_hash,
                compiled_plan_hash,
                compiled_operations,
                ..
            } => IntentRecipeStatusV1::PreviewReady {
                root_draft_revision: *root_draft_revision,
                workspace_revision: workspace.revision,
                receipt: IntentRecipeReceiptV1 {
                    intent_revision: *intent_revision,
                    candidate_revision: *candidate_revision,
                    input_intent_hash: input_intent_hash.clone(),
                    semantic_intent_hash: semantic_intent_hash.clone(),
                    compiled_plan_hash: compiled_plan_hash.clone(),
                    compiled_operations: *compiled_operations,
                },
            },
        })
    }

    fn intent_frontier(&self) -> Result<IntentFrontierV3, StructuredError> {
        self.intent_recipe
            .as_ref()
            .map(IntentRecipeRuntime::frontier)
            .ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })
    }

    fn append_intent_state_anchor(&mut self) -> Result<(), StructuredError> {
        let runtime = self.intent_recipe.as_ref().ok_or_else(|| {
            intent_error(
                "INTENT_SESSION_DISABLED",
                "intent.session",
                "Intent recipe mode is not enabled",
                "Construct the session with resource bindings",
            )
        })?;
        let (stage, active_question, active_options) = match &runtime.snapshot.stage {
            IntentRecipeStageSnapshotV1::Empty => ("empty", None, Vec::new()),
            IntentRecipeStageSnapshotV1::AwaitingDecision {
                active_decision, ..
            } => (
                "awaiting_decision",
                Some(active_decision.question.clone()),
                active_decision.options.clone(),
            ),
            IntentRecipeStageSnapshotV1::PreviewReady { .. } => ("preview_ready", None, Vec::new()),
        };
        let available_channel_keys = runtime
            .bindings
            .channel_bindings
            .keys()
            .map(|key| key.0.clone())
            .collect();
        let anchor = IntentStateAnchorV1 {
            stage,
            expected_revision: runtime.expected_revision(self.draft.draft_revision),
            available_channel_keys,
            active_question,
            active_options,
        };
        let content = serde_json::to_string(&anchor).map_err(|error| {
            intent_error(
                "INTENT_STATE_SERIALIZATION_FAILED",
                "intent.session.state",
                error.to_string(),
                "Start a new intent recipe session",
            )
        })?;
        self.messages
            .push(Message::user(format!("{INTENT_STATE_PREFIX}{content}")));
        Ok(())
    }

    fn fit_intent_context(
        &self,
        tools: &[crate::tools::ToolDefinition],
        system_prompt: &str,
    ) -> Option<Vec<Message>> {
        let mut messages = self.messages.clone();
        let system = messages.first_mut()?;
        if system.role != crate::llm::MessageRole::System {
            return None;
        }
        system.content = system_prompt.to_string();
        let message_chars = serde_json::to_string(&messages).ok()?.len();
        let tool_chars = serde_json::to_string(tools).ok()?.len();
        (message_chars.saturating_add(tool_chars) <= self.config.context_char_budget)
            .then_some(messages)
    }

    fn finish_intent_success(&mut self, success: IntentTurnSuccess) -> super::BurstOutcome {
        match success {
            IntentTurnSuccess::NeedsInput { question, .. } => {
                self.observability.clarification_count =
                    self.observability.clarification_count.saturating_add(1);
                self.needs_input(question)
            }
            IntentTurnSuccess::Ready { summary, .. } => self.ready(summary),
            IntentTurnSuccess::Routed { fallback, decision } => self.routed(fallback, decision),
        }
    }

    fn fail_intent(&mut self, error: StructuredError) -> super::BurstOutcome {
        let code = error.code.clone();
        let message = error.message.clone();
        let result = ToolResult::failure_from(&self.draft, error);
        self.record_failure(None, &result);
        self.halt(&code, &message, None)
    }

    fn fail_intent_with_limit(
        &mut self,
        error: StructuredError,
        limit: LimitKind,
    ) -> super::BurstOutcome {
        let code = error.code.clone();
        let message = error.message.clone();
        let result = ToolResult::failure_from(&self.draft, error);
        self.record_failure(None, &result);
        self.halt(&code, &message, Some(limit))
    }

    fn intent_detail_state<'a>(
        &self,
        selection: &'a PrivateStudyRoomSelectionV3,
    ) -> Result<(IntentDetailStateAnchorV3<'a>, String), StructuredError> {
        let state = IntentDetailStateAnchorV3 {
            detail_facets: selection.detail_facets(),
        };
        let content = serde_json::to_string(&state).map_err(|error| {
            intent_error(
                "INTENT_DETAIL_STATE_SERIALIZATION_FAILED",
                "intent.session.detail_state",
                error.to_string(),
                "Start a new intent recipe session",
            )
        })?;
        Ok((state, format!("{INTENT_DETAIL_STATE_PREFIX}{content}")))
    }

    fn finish_intent_request_failure(
        &mut self,
        failure: IntentToolRequestFailure,
    ) -> super::BurstOutcome {
        match failure {
            IntentToolRequestFailure::Error(error) => self.fail_intent(error),
            IntentToolRequestFailure::Limit(error, limit) => {
                self.fail_intent_with_limit(error, limit)
            }
        }
    }

    fn finish_intent_tool_execution(
        &mut self,
        call: ToolCall,
        result: Result<IntentTurnSuccess, StructuredError>,
    ) -> super::BurstOutcome {
        match result {
            Ok(success) => {
                self.last_error = None;
                self.messages
                    .push(Message::tool(call.id, success.tool_result()));
                self.finish_intent_success(success)
            }
            Err(error) => {
                let result = ToolResult::failure_from(&self.draft, error.clone());
                self.messages.push(Message::tool(call.id, result.as_json()));
                self.fail_intent(error)
            }
        }
    }
}

impl<C: LlmClient> DesignSession<C> {
    pub(super) async fn run_intent_recipe_burst(
        &mut self,
        human_message: &str,
    ) -> super::BurstOutcome {
        self.begin_turn(human_message);
        self.current_human_message_index = Some(self.messages.len());
        self.messages
            .push(Message::user(intent_human_envelope(human_message)));
        self.prose_nudged = false;

        let revision_check = self
            .intent_recipe
            .as_ref()
            .ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })
            .and_then(|runtime| runtime.ensure_draft_revision(self.draft.draft_revision));
        if let Err(error) = revision_check {
            return self.fail_intent(error);
        }
        if let Err(error) = self.append_intent_state_anchor() {
            return self.fail_intent(error);
        }
        let frontier = match self.intent_frontier() {
            Ok(frontier) => frontier,
            Err(error) => return self.fail_intent(error),
        };
        let tools = frontier.tools();
        let expected_tool = frontier.name();
        let call = match self.request_intent_tool_once(expected_tool, &tools).await {
            Ok(call) => call,
            Err(failure) => return self.finish_intent_request_failure(failure),
        };
        match frontier {
            IntentFrontierV3::Resolve => {
                let result = self.execute_intent_resolution(&call.arguments).await;
                self.finish_intent_tool_execution(call, result)
            }
            IntentFrontierV3::InterpretCore => {
                let result = self
                    .execute_intent_core(&call.arguments, human_message)
                    .await;
                match result {
                    Ok(IntentCoreExecutionV3::Complete(success)) => {
                        self.finish_intent_tool_execution(call, Ok(*success))
                    }
                    Ok(IntentCoreExecutionV3::NeedsDetails(selection)) => {
                        let (detail_state, detail_anchor) =
                            match self.intent_detail_state(&selection) {
                                Ok(state) => state,
                                Err(error) => {
                                    return self.finish_intent_tool_execution(call, Err(error));
                                }
                            };
                        self.messages.push(Message::tool(
                            call.id,
                            json!({
                                "ok": true,
                                "status": "details_required",
                                "detail_facets": detail_state.detail_facets,
                            })
                            .to_string(),
                        ));
                        self.messages.push(Message::user(detail_anchor));
                        let tools: Vec<_> = private_study_room_details_frontier().into();
                        let detail_call = match self
                            .request_intent_tool_once(EXTRACT_PRIVATE_STUDY_ROOM_DETAILS, &tools)
                            .await
                        {
                            Ok(call) => call,
                            Err(failure) => return self.finish_intent_request_failure(failure),
                        };
                        let result = self
                            .execute_intent_details(selection, &detail_call.arguments)
                            .await;
                        self.finish_intent_tool_execution(detail_call, result)
                    }
                    Err(error) => self.finish_intent_tool_execution(call, Err(error)),
                }
            }
        }
    }

    async fn request_intent_tool_once(
        &mut self,
        expected_tool: &str,
        tools: &[crate::tools::ToolDefinition],
    ) -> Result<ToolCall, IntentToolRequestFailure> {
        if self.turn_model_calls() >= self.config.max_model_calls {
            return Err(IntentToolRequestFailure::Limit(
                intent_error(
                    "MODEL_CALL_LIMIT_EXHAUSTED",
                    "intent.session.model_calls",
                    "The intent session cannot make its required model call",
                    "Increase the per-turn model-call limit",
                ),
                LimitKind::ModelCalls,
            ));
        }
        if self.turn_tool_calls() >= self.config.max_tool_calls {
            return Err(IntentToolRequestFailure::Limit(
                intent_error(
                    "TOOL_CALL_LIMIT_EXHAUSTED",
                    "intent.session.tool_calls",
                    "The intent session cannot execute its required model tool call",
                    "Increase the per-turn tool-call limit",
                ),
                LimitKind::ToolCalls,
            ));
        }
        let system_prompt = match expected_tool {
            EXTRACT_PRIVATE_STUDY_ROOM_DETAILS => INTENT_RECIPE_DETAIL_SYSTEM_PROMPT_V3,
            crate::turn::RESOLVE_INTENT_DECISION => INTENT_RECIPE_DECISION_SYSTEM_PROMPT_V3,
            _ => INTENT_RECIPE_SYSTEM_PROMPT_V3,
        };
        let Some(messages) = self.fit_intent_context(tools, system_prompt) else {
            return Err(IntentToolRequestFailure::Limit(
                intent_error(
                    "CONTEXT_CHAR_LIMIT_EXHAUSTED",
                    "intent.session.context",
                    "The fixed prompt, append-only conversation, state anchor, and sole tool do not fit the context budget",
                    "Start a compacted session snapshot or increase the context budget",
                ),
                LimitKind::ContextChars,
            ));
        };
        self.record_model_call();
        let response = self.client.complete(&messages, tools).await.map_err(|_| {
            IntentToolRequestFailure::Error(intent_error(
                "LLM_CLIENT_ERROR",
                "llm",
                "The intent router model request failed",
                "Retry the same user turn after the model gateway is available",
            ))
        })?;
        let calls = match response {
            LlmResponse::Text(text) => {
                self.messages.push(Message::assistant(text));
                self.record_intent_extraction_failure();
                return Err(IntentToolRequestFailure::Error(intent_error(
                    "INTENT_TOOL_CALL_REQUIRED",
                    "intent.response",
                    format!("The model returned prose instead of {expected_tool}"),
                    format!("Call exactly one {expected_tool} tool"),
                )));
            }
            LlmResponse::ToolCalls(calls) => calls,
        };
        if calls.len() != 1 || !valid_tool_call_ids(&calls) {
            self.messages.push(Message::assistant(format!(
                "INTENT_RESPONSE_REJECTED: expected exactly one {expected_tool} call with a non-empty unique identifier"
            )));
            self.record_intent_extraction_failure();
            return Err(IntentToolRequestFailure::Error(intent_error(
                "INTENT_FRONTIER_VIOLATION",
                "intent.response",
                format!(
                    "The model returned {} calls instead of one valid {expected_tool} call",
                    calls.len()
                ),
                format!("Call exactly one {expected_tool} tool with a non-empty identifier"),
            )));
        }
        let Some(call) = calls.into_iter().next() else {
            return Err(IntentToolRequestFailure::Error(intent_error(
                "INTENT_FRONTIER_VIOLATION",
                "intent.response",
                "The model returned no intent tool call",
                format!("Call exactly one {expected_tool} tool"),
            )));
        };
        if call.name != expected_tool {
            let error = intent_error(
                "INTENT_FRONTIER_VIOLATION",
                "intent.response.tool",
                format!(
                    "The model called {} while the active frontier requires {expected_tool}",
                    call.name
                ),
                format!("Call exactly one {expected_tool} tool"),
            );
            self.messages.push(Message::assistant(format!(
                "INTENT_RESPONSE_REJECTED: expected {expected_tool}, received {}",
                call.name
            )));
            self.record_intent_extraction_failure();
            return Err(IntentToolRequestFailure::Error(error));
        }
        self.messages
            .push(Message::assistant_tool_calls(vec![call.clone()]));
        self.record_tool_call();
        Ok(call)
    }
}
fn valid_tool_call_ids(calls: &[ToolCall]) -> bool {
    let mut ids = BTreeSet::new();
    calls
        .iter()
        .all(|call| !call.id.trim().is_empty() && ids.insert(call.id.as_str()))
}
