use std::collections::BTreeSet;

use resource_resolution::ResourceBindingMap;
use serde::Serialize;

use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, ToolCall};
use crate::turn::{resolve_intent_decision_frontier, route_intent_turn_frontier};

use super::{DesignSession, LimitKind, SessionConfig, SessionSnapshot, SessionSnapshotError};

mod execute;
mod state;
#[cfg(test)]
mod tests;

use execute::IntentTurnSuccess;
pub(super) use state::IntentRecipeRuntime;
pub(crate) use state::IntentRecipeSessionSnapshotV1;
use state::{
    intent_error, snapshot_error, IntentRecipeStageSnapshotV1, INTENT_RECIPE_PROTOCOL_VERSION,
    INTENT_STATE_PREFIX,
};
pub(super) use state::{validate_intent_recipe_snapshot, INTENT_RECIPE_SYSTEM_PROMPT};
pub use state::{
    IntentFallbackKind, IntentFallbackV1, IntentRecipeReceiptV1, IntentRecipeStatusV1,
};

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct IntentStateAnchorV1 {
    protocol_version: u16,
    stage: &'static str,
    expected_revision: u64,
    draft_revision: u64,
    available_channel_keys: Vec<String>,
    active_question: Option<String>,
    active_options: Vec<String>,
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

    fn intent_frontier(&self) -> Vec<crate::tools::ToolDefinition> {
        match self
            .intent_recipe
            .as_ref()
            .map(IntentRecipeRuntime::expected_tool)
        {
            Some("resolve_intent_decision") => resolve_intent_decision_frontier().into(),
            _ => route_intent_turn_frontier().into(),
        }
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
            protocol_version: INTENT_RECIPE_PROTOCOL_VERSION,
            stage,
            expected_revision: runtime.expected_revision(self.draft.draft_revision),
            draft_revision: self.draft.draft_revision,
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

    fn fit_intent_context(&self, tools: &[crate::tools::ToolDefinition]) -> Option<Vec<Message>> {
        let message_chars = serde_json::to_string(&self.messages).ok()?.len();
        let tool_chars = serde_json::to_string(tools).ok()?.len();
        (message_chars.saturating_add(tool_chars) <= self.config.context_char_budget)
            .then(|| self.messages.clone())
    }

    fn finish_intent_success(&mut self, success: IntentTurnSuccess) -> super::BurstOutcome {
        match success {
            IntentTurnSuccess::NeedsInput { question, .. } => {
                self.observability.clarification_count =
                    self.observability.clarification_count.saturating_add(1);
                self.needs_input(question)
            }
            IntentTurnSuccess::Ready { summary, .. } => self.ready(summary),
            IntentTurnSuccess::Routed(fallback) => self.routed(fallback),
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
}

impl<C: LlmClient> DesignSession<C> {
    pub(super) async fn run_intent_recipe_burst(
        &mut self,
        human_message: &str,
    ) -> super::BurstOutcome {
        self.begin_turn(human_message);
        self.current_human_message_index = Some(self.messages.len());
        self.messages.push(Message::user(human_message));
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
        if self.turn_model_calls() >= self.config.max_model_calls {
            return self.fail_intent_with_limit(
                intent_error(
                    "MODEL_CALL_LIMIT_EXHAUSTED",
                    "intent.session.model_calls",
                    "The intent session cannot make its required model call",
                    "Increase the per-turn model-call limit",
                ),
                LimitKind::ModelCalls,
            );
        }
        if let Err(error) = self.append_intent_state_anchor() {
            return self.fail_intent(error);
        }
        let tools = self.intent_frontier();
        let Some(messages) = self.fit_intent_context(&tools) else {
            return self.fail_intent_with_limit(
                intent_error(
                    "CONTEXT_CHAR_LIMIT_EXHAUSTED",
                    "intent.session.context",
                    "The fixed prompt, append-only conversation, state anchor, and sole tool do not fit the context budget",
                    "Start a compacted session snapshot or increase the context budget",
                ),
                LimitKind::ContextChars,
            );
        };
        let expected_tool = self
            .intent_recipe
            .as_ref()
            .map(IntentRecipeRuntime::expected_tool)
            .unwrap_or("route_intent_turn");
        self.record_model_call();
        let response = match self.client.complete(&messages, &tools).await {
            Ok(response) => response,
            Err(_) => {
                return self.fail_intent(intent_error(
                    "LLM_CLIENT_ERROR",
                    "llm",
                    "The intent router model request failed",
                    "Retry the same user turn after the model gateway is available",
                ));
            }
        };
        let calls = match response {
            LlmResponse::Text(text) => {
                self.messages.push(Message::assistant(text));
                self.record_intent_extraction_failure();
                return self.fail_intent(intent_error(
                    "INTENT_TOOL_CALL_REQUIRED",
                    "intent.response",
                    format!("The model returned prose instead of {expected_tool}"),
                    format!("Call exactly one {expected_tool} tool"),
                ));
            }
            LlmResponse::ToolCalls(calls) => calls,
        };
        if calls.len() != 1 || !valid_tool_call_ids(&calls) {
            if valid_tool_call_ids(&calls) && !calls.is_empty() {
                self.messages
                    .push(Message::assistant_tool_calls(calls.clone()));
                let error = intent_error(
                    "INTENT_FRONTIER_VIOLATION",
                    "intent.response",
                    format!(
                        "The model returned {} tool calls instead of exactly one {expected_tool}",
                        calls.len()
                    ),
                    format!("Call exactly one {expected_tool} tool"),
                );
                let result = ToolResult::failure_from(&self.draft, error.clone()).as_json();
                for call in &calls {
                    self.messages.push(Message::tool(&call.id, result.clone()));
                }
                self.record_intent_extraction_failure();
                return self.fail_intent(error);
            }
            self.messages.push(Message::assistant(format!(
                "INTENT_RESPONSE_REJECTED: expected exactly one {expected_tool} call with a non-empty unique identifier"
            )));
            self.record_intent_extraction_failure();
            return self.fail_intent(intent_error(
                "INTENT_FRONTIER_VIOLATION",
                "intent.response",
                format!("The model did not return one valid {expected_tool} call"),
                format!("Call exactly one {expected_tool} tool with a non-empty identifier"),
            ));
        }
        let call = calls.into_iter().next().expect("single call was checked");
        self.messages
            .push(Message::assistant_tool_calls(vec![call.clone()]));
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
            let result = ToolResult::failure_from(&self.draft, error.clone());
            self.messages.push(Message::tool(call.id, result.as_json()));
            self.record_intent_extraction_failure();
            return self.fail_intent(error);
        }
        if self.turn_tool_calls() >= self.config.max_tool_calls {
            let error = intent_error(
                "TOOL_CALL_LIMIT_EXHAUSTED",
                "intent.session.tool_calls",
                "The intent session cannot execute its required model tool call",
                "Increase the per-turn tool-call limit",
            );
            let result = ToolResult::failure_from(&self.draft, error.clone());
            self.messages.push(Message::tool(call.id, result.as_json()));
            return self.fail_intent_with_limit(error, LimitKind::ToolCalls);
        }
        self.record_tool_call();
        let result = match expected_tool {
            "resolve_intent_decision" => self.execute_intent_resolution(&call.arguments).await,
            _ => self.execute_intent_route(&call.arguments).await,
        };
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
fn valid_tool_call_ids(calls: &[ToolCall]) -> bool {
    let mut ids = BTreeSet::new();
    calls
        .iter()
        .all(|call| !call.id.trim().is_empty() && ids.insert(call.id.as_str()))
}
