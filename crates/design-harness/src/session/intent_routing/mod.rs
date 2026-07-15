use std::collections::BTreeSet;

use resource_resolution::ResourceBindingMap;
use serde::Serialize;
use serde_json::json;

use crate::errors::{StructuredError, ToolResult};
use crate::llm::{LlmClient, LlmResponse, Message, MessageRole, ToolCall};
use crate::turn::{
    private_study_room_details_frontier_for, IntentRecipeDetailFacetV3,
    EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
};

use super::{DesignSession, LimitKind, SessionConfig, SessionSnapshot, SessionSnapshotError};

mod adjudicate;
#[cfg(test)]
mod adjudicate_tests;
#[cfg(test)]
mod adjudicate_v3_tests;
mod decision;
mod durability;
mod evidence;
#[cfg(test)]
mod evidence_tests;
mod execute;
mod frontier;
mod grounding;
mod request_evidence;
mod snapshot_validation;
mod state;
mod state_binding;
#[cfg(test)]
mod tests;
mod transcript_binding;
mod transcript_integrity;
mod transcript_replay;
mod transcript_restore;

use adjudicate::PrivateStudyRoomSelectionV4;
pub use decision::{
    IntentDecisionSourceV2, IntentRouteDecisionKindV2, IntentRouteDecisionV2, PinnedIntentRecipeV2,
    INTENT_ADJUDICATOR_VERSION_V2, INTENT_ADJUDICATOR_VERSION_V3, INTENT_ADJUDICATOR_VERSION_V4,
    INTENT_RECIPE_PROTOCOL_VERSION_V2, INTENT_RECIPE_PROTOCOL_VERSION_V3,
    INTENT_RECIPE_PROTOCOL_VERSION_V4,
};
use durability::{durable_transcript_violation, durable_transcript_violation_with_added};
#[cfg(test)]
use durability::{MAX_INTENT_RESTORED_FAILURE_RESULTS, MAX_INTENT_RESTORED_TRANSCRIPT_CHARS};
use execute::{IntentCoreExecutionV4, IntentTurnSuccess};
use frontier::IntentFrontierV4;
pub(super) use state::IntentRecipeRuntime;
pub(crate) use state::IntentRecipeSessionSnapshotV2;
use state::{
    intent_error, snapshot_error, IntentRecipeStageSnapshotV2, INTENT_DETAIL_STATE_PREFIX,
    INTENT_HUMAN_PREFIX, INTENT_RECIPE_DECISION_SYSTEM_PROMPT_V3,
    INTENT_RECIPE_DETAIL_SYSTEM_PROMPT_V3, INTENT_STATE_PREFIX,
};
pub(super) use state::{
    validate_intent_recipe_snapshot, INTENT_RECIPE_SYSTEM_PROMPT, INTENT_RECIPE_SYSTEM_PROMPT_V1,
    INTENT_RECIPE_SYSTEM_PROMPT_V2, INTENT_RECIPE_SYSTEM_PROMPT_V3, INTENT_RECIPE_SYSTEM_PROMPT_V4,
};
pub use state::{
    IntentFallbackKind, IntentFallbackV1, IntentRecipeReceiptV2, IntentRecipeStatusV2,
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

const MAX_INTENT_SERVING_HISTORY_TURNS: usize = 4;

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
        validate_intent_durable_transcript_bound(&snapshot)?;
        super::validate_snapshot_with_intent_bindings(&snapshot, &bindings)?;
        let intent_snapshot = snapshot
            .intent_recipe
            .clone()
            .ok_or_else(|| snapshot_error("snapshot is not an intent recipe session"))?;
        let runtime = IntentRecipeRuntime::restore(
            bindings,
            intent_snapshot,
            &snapshot.draft,
            &snapshot.messages,
        )?;
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

    pub fn intent_recipe_status(&self) -> Option<IntentRecipeStatusV2> {
        let runtime = self.intent_recipe.as_ref()?;
        Some(match &runtime.snapshot.stage {
            IntentRecipeStageSnapshotV2::Empty => IntentRecipeStatusV2::Empty {
                expected_revision: runtime.expected_revision(self.draft.draft_revision),
            },
            IntentRecipeStageSnapshotV2::AwaitingDecision {
                root_draft_revision,
                workspace,
                active_decision,
                ..
            } => IntentRecipeStatusV2::AwaitingDecision {
                root_draft_revision: *root_draft_revision,
                workspace_revision: workspace.revision,
                question: active_decision.question.clone(),
                available_channel_keys: active_decision.options.clone(),
            },
            IntentRecipeStageSnapshotV2::PreviewReady {
                root_draft_revision,
                workspace,
                identity_revision,
                intent_revision,
                candidate_revision,
                compiler_input_hash,
                semantic_intent_hash,
                compiled_plan_hash,
                candidate_ruleset_hash,
                candidate_draft_hash,
                compiled_operations,
                request_evidence,
                ..
            } => IntentRecipeStatusV2::PreviewReady {
                root_draft_revision: *root_draft_revision,
                workspace_revision: workspace.revision,
                receipt: IntentRecipeReceiptV2 {
                    identity_revision: *identity_revision,
                    intent_revision: *intent_revision,
                    candidate_revision: *candidate_revision,
                    request_evidence_hash: request_evidence.head().to_string(),
                    request_evidence_entries: request_evidence.entries().len(),
                    compiler_input_hash: compiler_input_hash.clone(),
                    semantic_intent_hash: semantic_intent_hash.clone(),
                    compiled_plan_hash: compiled_plan_hash.clone(),
                    candidate_ruleset_hash: candidate_ruleset_hash.clone(),
                    candidate_draft_hash: candidate_draft_hash.clone(),
                    compiled_operations: *compiled_operations,
                },
            },
        })
    }

    fn intent_frontier(&self) -> Result<IntentFrontierV4, StructuredError> {
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
            IntentRecipeStageSnapshotV2::Empty => ("empty", None, Vec::new()),
            IntentRecipeStageSnapshotV2::AwaitingDecision {
                active_decision, ..
            } => (
                "awaiting_decision",
                Some(active_decision.question.clone()),
                active_decision.options.clone(),
            ),
            IntentRecipeStageSnapshotV2::PreviewReady { .. } => ("preview_ready", None, Vec::new()),
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
        let current_human_message_index = self.current_human_message_index?;
        let turn_starts = self
            .messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                (message.role == crate::llm::MessageRole::User
                    && message.tool_call_id.is_none()
                    && message.tool_calls.is_empty()
                    && message.content.starts_with(INTENT_HUMAN_PREFIX))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        if turn_starts.last().copied() != Some(current_human_message_index) {
            return None;
        }
        let system = self.messages.first()?;
        if system.role != crate::llm::MessageRole::System {
            return None;
        }
        let maximum = turn_starts.len().min(MAX_INTENT_SERVING_HISTORY_TURNS);
        let history_start = turn_starts.len().saturating_sub(maximum);
        let projected_history = turn_starts[history_start..turn_starts.len().saturating_sub(1)]
            .iter()
            .enumerate()
            .map(|(offset, start)| {
                let end = turn_starts
                    .get(history_start.saturating_add(offset).saturating_add(1))
                    .copied()
                    .unwrap_or(current_human_message_index);
                project_intent_history_turn(&self.messages, *start, end)
            })
            .collect::<Option<Vec<_>>>()?;
        for retained_history in (0..=projected_history.len()).rev() {
            let mut messages = Vec::with_capacity(
                self.messages
                    .len()
                    .saturating_sub(current_human_message_index)
                    .saturating_add(retained_history.saturating_mul(2))
                    .saturating_add(1),
            );
            messages.push(Message::system(system_prompt));
            for projected in
                &projected_history[projected_history.len().saturating_sub(retained_history)..]
            {
                messages.extend_from_slice(projected);
            }
            messages.extend_from_slice(&self.messages[current_human_message_index..]);
            if let Some(messages) = self.fit_intent_messages(messages, tools) {
                return Some(messages);
            }
        }
        None
    }

    fn fit_intent_detail_context(
        &self,
        tools: &[crate::tools::ToolDefinition],
        system_prompt: &str,
    ) -> Option<Vec<Message>> {
        let human = self.messages.get(self.current_human_message_index?)?;
        let detail_state = self.messages.last()?;
        if human.role != crate::llm::MessageRole::User
            || !human.content.starts_with(INTENT_HUMAN_PREFIX)
            || detail_state.role != crate::llm::MessageRole::User
            || !detail_state.content.starts_with(INTENT_DETAIL_STATE_PREFIX)
        {
            return None;
        }
        self.fit_intent_messages(
            vec![
                Message::system(system_prompt),
                human.clone(),
                detail_state.clone(),
            ],
            tools,
        )
    }

    fn fit_intent_messages(
        &self,
        messages: Vec<Message>,
        tools: &[crate::tools::ToolDefinition],
    ) -> Option<Vec<Message>> {
        (intent_openai_context_chars(&messages, tools)? <= self.config.context_char_budget)
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
        selection: &'a PrivateStudyRoomSelectionV4,
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

fn project_intent_history_turn(
    messages: &[Message],
    human_message_index: usize,
    turn_end: usize,
) -> Option<Vec<Message>> {
    let human = messages.get(human_message_index)?;
    if human.role != MessageRole::User
        || human.tool_call_id.is_some()
        || !human.tool_calls.is_empty()
        || !human.content.starts_with(INTENT_HUMAN_PREFIX)
    {
        return None;
    }
    let mut projected = vec![human.clone()];
    if let Some(presentation) = (human_message_index.saturating_add(1)..turn_end)
        .find_map(|index| intent_history_presentation(messages, human_message_index, index))
    {
        projected.push(presentation);
    }
    Some(projected)
}

fn intent_history_presentation(
    messages: &[Message],
    human_message_index: usize,
    call_message_index: usize,
) -> Option<Message> {
    let message = messages.get(call_message_index)?;
    let [call] = message.tool_calls.as_slice() else {
        return None;
    };
    if message.role != MessageRole::Assistant || call.name != crate::turn::INTERPRET_INTENT_CORE {
        return None;
    }
    let result = messages.get(call_message_index.saturating_add(1))?;
    if result.role != MessageRole::Tool || result.tool_call_id.as_deref() != Some(call.id.as_str())
    {
        return None;
    }
    let result_value = serde_json::from_str::<serde_json::Value>(&result.content).ok()?;
    let human_message_index = u64::try_from(human_message_index).ok()?;
    let replayed = transcript_binding::replay_successful_routed_core_turn(
        messages,
        human_message_index,
        &call.arguments,
        &result_value,
    )
    .ok()??;
    replayed
        .is_discussion()
        .then(|| Message::assistant(replayed.response()))
}

fn intent_openai_context_chars(
    messages: &[Message],
    tools: &[crate::tools::ToolDefinition],
) -> Option<usize> {
    let messages = messages
        .iter()
        .map(intent_openai_message)
        .collect::<Option<Vec<_>>>()?;
    let tools_value = tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters
                }
            })
        })
        .collect::<Vec<_>>();
    let mut context = json!({
        "messages": messages,
        "tools": tools_value
    });
    if let [tool] = tools {
        context.as_object_mut()?.insert(
            "response_format".to_string(),
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": format!("{}_arguments", tool.name),
                    "strict": true,
                    "schema": tool.parameters
                }
            }),
        );
    }
    serde_json::to_vec(&context).ok().map(|bytes| bytes.len())
}

fn intent_openai_message(message: &Message) -> Option<serde_json::Value> {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };
    if message.role == MessageRole::Tool && message.tool_call_id.is_none() {
        return None;
    }
    let content = if message.role == MessageRole::Assistant
        && message.content.is_empty()
        && !message.tool_calls.is_empty()
    {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(message.content.clone())
    };
    let mut value = json!({
        "role": role,
        "content": content
    });
    let object = value.as_object_mut()?;
    if !message.tool_calls.is_empty() {
        object.insert(
            "tool_calls".to_string(),
            serde_json::Value::Array(
                message
                    .tool_calls
                    .iter()
                    .map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments
                            }
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(tool_call_id) = &message.tool_call_id {
        object.insert(
            "tool_call_id".to_string(),
            serde_json::Value::String(tool_call_id.clone()),
        );
    }
    Some(value)
}

pub(super) fn validate_intent_durable_transcript_bound(
    snapshot: &SessionSnapshot,
) -> Result<(), SessionSnapshotError> {
    if snapshot.intent_recipe.is_none() {
        return Ok(());
    }
    match durable_transcript_violation(&snapshot.messages) {
        None => Ok(()),
        Some(LimitKind::DurableTranscriptChars) => Err(snapshot_error(
            "intent recipe transcript exceeds the durable restore size limit",
        )),
        Some(LimitKind::DurableTranscriptReplayWork) => Err(snapshot_error(
            "intent recipe transcript exceeds the durable replay work limit",
        )),
        Some(_) => Err(snapshot_error(
            "intent recipe transcript violates an unexpected durable limit",
        )),
    }
}

#[cfg(test)]
fn intent_transcript_fits_durable_bound(messages: &[Message]) -> bool {
    durable_transcript_violation(messages).is_none()
}

#[cfg(test)]
fn intent_transcript_fits_added_message(messages: &[Message], message: &Message) -> bool {
    durable_transcript_violation_with_added(messages, message).is_none()
}

fn durable_transcript_limit_error(limit: LimitKind) -> StructuredError {
    match limit {
        LimitKind::DurableTranscriptReplayWork => intent_error(
            "INTENT_DURABLE_REPLAY_WORK_LIMIT_EXHAUSTED",
            "intent.session.durable_transcript.replay_work",
            "The intent turn would make deterministic session restore exceed its work budget",
            "Start a new intent recipe session and preserve the current preview receipt externally",
        ),
        _ => intent_error(
            "INTENT_DURABLE_TRANSCRIPT_LIMIT_EXHAUSTED",
            "intent.session.durable_transcript",
            "The intent turn would make its durable transcript impossible to restore",
            "Start a new intent recipe session and preserve the current preview receipt externally",
        ),
    }
}

fn durable_transcript_limit_outcome<C>(
    session: &DesignSession<C>,
    observability: super::Observability,
    limit: LimitKind,
) -> super::BurstOutcome {
    let error = durable_transcript_limit_error(limit);
    super::BurstOutcome::Halted(Box::new(super::HaltReport {
        code: error.code.clone(),
        message: error.message.clone(),
        exhausted_limit: Some(limit),
        draft: session.draft.summary(),
        last_error: Some(error),
        observability,
    }))
}

impl<C: LlmClient> DesignSession<C> {
    pub(super) async fn run_intent_recipe_burst(
        &mut self,
        human_message: &str,
    ) -> super::BurstOutcome {
        let human_envelope = Message::user(intent_human_envelope(human_message));
        if let Some(limit) =
            durable_transcript_violation_with_added(&self.messages, &human_envelope)
        {
            return durable_transcript_limit_outcome(self, self.observability.clone(), limit);
        }
        let root_draft = self.draft.clone();
        let root_message_len = self.messages.len();
        let root_observability = self.observability.clone();
        let root_last_error = self.last_error.clone();
        let root_prose_nudged = self.prose_nudged;
        let root_turn_state = self.turn_state.clone();
        let root_human_message_index = self.current_human_message_index;
        let root_intent_snapshot = self
            .intent_recipe
            .as_ref()
            .map(|runtime| runtime.snapshot.clone());
        let outcome = self
            .run_intent_recipe_burst_unbounded(human_message, human_envelope)
            .await;
        let Some(limit) = durable_transcript_violation(&self.messages) else {
            return outcome;
        };
        let attempt_observability = self.observability.clone();
        self.draft = root_draft;
        self.messages.truncate(root_message_len);
        self.observability = root_observability;
        self.last_error = root_last_error;
        self.prose_nudged = root_prose_nudged;
        self.turn_state = root_turn_state;
        self.current_human_message_index = root_human_message_index;
        if let (Some(runtime), Some(snapshot)) = (self.intent_recipe.as_mut(), root_intent_snapshot)
        {
            runtime.snapshot = snapshot;
        }
        durable_transcript_limit_outcome(self, attempt_observability, limit)
    }

    async fn run_intent_recipe_burst_unbounded(
        &mut self,
        human_message: &str,
        human_envelope: Message,
    ) -> super::BurstOutcome {
        self.begin_turn(human_message);
        self.current_human_message_index = Some(self.messages.len());
        self.messages.push(human_envelope);
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
            IntentFrontierV4::Resolve => {
                let result = self
                    .execute_intent_resolution(&call.arguments, human_message)
                    .await;
                self.finish_intent_tool_execution(call, result)
            }
            IntentFrontierV4::InterpretCore => {
                let result = self
                    .execute_intent_core(&call.arguments, human_message)
                    .await;
                match result {
                    Ok(IntentCoreExecutionV4::Complete(success)) => {
                        self.finish_intent_tool_execution(call, Ok(*success))
                    }
                    Ok(IntentCoreExecutionV4::NeedsDetails {
                        selection,
                        request_evidence,
                    }) => {
                        let (detail_state, detail_anchor) =
                            match self.intent_detail_state(&selection) {
                                Ok(state) => state,
                                Err(error) => {
                                    return self.finish_intent_tool_execution(call, Err(error));
                                }
                            };
                        let tools: Vec<_> = match private_study_room_details_frontier_for(
                            selection.detail_facets(),
                        ) {
                            Ok(frontier) => frontier.into(),
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
                        let detail_call = match self
                            .request_intent_tool_once(EXTRACT_PRIVATE_STUDY_ROOM_DETAILS, &tools)
                            .await
                        {
                            Ok(call) => call,
                            Err(failure) => return self.finish_intent_request_failure(failure),
                        };
                        let result = self
                            .execute_intent_details(
                                selection,
                                request_evidence,
                                &detail_call.arguments,
                                human_message,
                            )
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
        if let Some(limit) = durable_transcript_violation(&self.messages) {
            return Err(IntentToolRequestFailure::Limit(
                durable_transcript_limit_error(limit),
                limit,
            ));
        }
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
            _ => INTENT_RECIPE_SYSTEM_PROMPT,
        };
        let messages = if expected_tool == EXTRACT_PRIVATE_STUDY_ROOM_DETAILS {
            self.fit_intent_detail_context(tools, system_prompt)
        } else {
            self.fit_intent_context(tools, system_prompt)
        };
        let Some(messages) = messages else {
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
