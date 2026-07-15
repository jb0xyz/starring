use std::collections::BTreeSet;

use crate::llm::{Message, MessageRole};
use crate::turn::{
    EXTRACT_PRIVATE_STUDY_ROOM_DETAILS, INTERPRET_INTENT_CORE, RESOLVE_INTENT_DECISION,
};
use serde::Deserialize;

use super::super::{SessionSnapshot, SessionSnapshotError};
use super::state::{
    snapshot_error, INTENT_DETAIL_STATE_PREFIX, INTENT_HUMAN_PREFIX, INTENT_STATE_PREFIX,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RestoredIntentStateAnchorV1 {
    stage: String,
    pub(super) expected_revision: u64,
    pub(super) available_channel_keys: Vec<String>,
    active_question: Option<String>,
    active_options: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoredIntentDetailStateAnchorV3 {
    detail_facets: Vec<crate::turn::IntentRecipeDetailFacetV3>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoredIntentHumanEnvelopeV1 {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoredIntentToolFailureV4 {
    ok: bool,
    code: String,
    location: String,
    message: String,
    hint: String,
    revision: u64,
}

pub(super) struct IntentTranscriptTurnV4 {
    pub(super) human_message_index: u64,
    pub(super) primary_tool: Option<String>,
    pub(super) primary_arguments: Option<String>,
    pub(super) detail_arguments: Option<String>,
    pub(super) detail_result: Option<serde_json::Value>,
    pub(super) detail_facets: Vec<crate::turn::IntentRecipeDetailFacetV3>,
    pub(super) succeeded: bool,
    pub(super) primary_result: Option<serde_json::Value>,
    model_responses: usize,
    open_model_request: bool,
    tool_calls: usize,
}

pub(super) struct IntentTranscriptV4 {
    pub(super) turns: Vec<IntentTranscriptTurnV4>,
}

pub(super) fn validate_v4_transcript(
    snapshot: &SessionSnapshot,
) -> Result<IntentTranscriptV4, SessionSnapshotError> {
    let messages = &snapshot.messages;
    let mut turns = Vec::new();
    let mut index = 1usize;
    while index < messages.len() {
        let human_message_index = u64::try_from(index)
            .map_err(|_| snapshot_error("intent transcript message index overflowed"))?;
        validate_human_envelope(&messages[index])?;
        index = index.saturating_add(1);
        let state_message = messages.get(index).ok_or_else(|| {
            snapshot_error("intent human message is missing its following INTENT_STATE anchor")
        })?;
        let state = parse_intent_state_anchor(state_message)?;
        let expected_tool = match state.stage.as_str() {
            "empty" | "preview_ready" => INTERPRET_INTENT_CORE,
            "awaiting_decision" => RESOLVE_INTENT_DECISION,
            _ => {
                return Err(snapshot_error(
                    "intent transcript contains an unknown INTENT_STATE stage",
                ));
            }
        };
        validate_intent_state_anchor(&state)?;
        index = index.saturating_add(1);
        let mut turn = IntentTranscriptTurnV4 {
            human_message_index,
            primary_tool: None,
            primary_arguments: None,
            detail_arguments: None,
            detail_result: None,
            detail_facets: Vec::new(),
            succeeded: false,
            primary_result: None,
            model_responses: 0,
            open_model_request: false,
            tool_calls: 0,
        };
        let Some(response) = messages.get(index) else {
            turn.open_model_request = true;
            turns.push(turn);
            break;
        };
        if is_intent_human_message(response) {
            turn.open_model_request = true;
            turns.push(turn);
            continue;
        }
        validate_assistant_response(response)?;
        turn.model_responses = turn.model_responses.saturating_add(1);
        if response.tool_calls.is_empty() {
            index = index.saturating_add(1);
            turns.push(turn);
            continue;
        }
        let call = &response.tool_calls[0];
        if call.name != expected_tool {
            return Err(snapshot_error(
                "intent transcript tool call does not match its preceding state frontier",
            ));
        }
        turn.primary_tool = Some(call.name.clone());
        turn.primary_arguments = Some(call.arguments.clone());
        turn.tool_calls = turn.tool_calls.saturating_add(1);
        index = index.saturating_add(1);
        let result = messages.get(index).ok_or_else(|| {
            snapshot_error("intent transcript tool call is missing its tool result")
        })?;
        let result_value = parse_intent_tool_result(result, &call.id)?;
        turn.primary_result = Some(result_value.clone());
        index = index.saturating_add(1);
        if call.name == INTERPRET_INTENT_CORE
            && result_value
                .get("status")
                .and_then(serde_json::Value::as_str)
                == Some("details_required")
        {
            if result_value.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                return Err(snapshot_error(
                    "intent detail request is not a successful core tool result",
                ));
            }
            let detail_state = messages.get(index).ok_or_else(|| {
                snapshot_error("intent detail request is missing its detail state anchor")
            })?;
            turn.detail_facets = validate_intent_detail_state(detail_state)?;
            index = index.saturating_add(1);
            let Some(detail_response) = messages.get(index) else {
                turn.open_model_request = true;
                turns.push(turn);
                break;
            };
            if is_intent_human_message(detail_response) {
                turn.open_model_request = true;
                turns.push(turn);
                continue;
            }
            validate_assistant_response(detail_response)?;
            turn.model_responses = turn.model_responses.saturating_add(1);
            if detail_response.tool_calls.is_empty() {
                index = index.saturating_add(1);
                turns.push(turn);
                continue;
            }
            let detail_call = &detail_response.tool_calls[0];
            if detail_call.name != EXTRACT_PRIVATE_STUDY_ROOM_DETAILS {
                return Err(snapshot_error(
                    "intent detail transcript uses an unrelated tool frontier",
                ));
            }
            turn.detail_arguments = Some(detail_call.arguments.clone());
            turn.tool_calls = turn.tool_calls.saturating_add(1);
            index = index.saturating_add(1);
            let detail_result = messages.get(index).ok_or_else(|| {
                snapshot_error("intent detail tool call is missing its tool result")
            })?;
            let detail_value = parse_intent_tool_result(detail_result, &detail_call.id)?;
            turn.detail_result = Some(detail_value.clone());
            turn.succeeded =
                detail_value.get("ok").and_then(serde_json::Value::as_bool) == Some(true);
            index = index.saturating_add(1);
        } else {
            turn.succeeded =
                result_value.get("ok").and_then(serde_json::Value::as_bool) == Some(true);
        }
        turns.push(turn);
    }
    validate_transcript_counters(snapshot, &turns)?;
    Ok(IntentTranscriptV4 { turns })
}

fn validate_human_envelope(message: &Message) -> Result<(), SessionSnapshotError> {
    if message.role != MessageRole::User
        || message.tool_call_id.is_some()
        || !message.tool_calls.is_empty()
    {
        return Err(snapshot_error(
            "intent transcript turn does not begin with a plain user message",
        ));
    }
    let envelope = message
        .content
        .strip_prefix(INTENT_HUMAN_PREFIX)
        .ok_or_else(|| {
            snapshot_error("intent transcript user message is not an INTENT_HUMAN envelope")
        })?;
    let parsed: RestoredIntentHumanEnvelopeV1 = serde_json::from_str(envelope)
        .map_err(|_| snapshot_error("intent transcript contains a malformed human envelope"))?;
    let _ = parsed.text;
    Ok(())
}

fn is_intent_human_message(message: &Message) -> bool {
    message.role == MessageRole::User
        && message.tool_call_id.is_none()
        && message.tool_calls.is_empty()
        && message.content.starts_with(INTENT_HUMAN_PREFIX)
}

pub(super) fn parse_intent_state_anchor(
    message: &Message,
) -> Result<RestoredIntentStateAnchorV1, SessionSnapshotError> {
    if message.role != MessageRole::User
        || message.tool_call_id.is_some()
        || !message.tool_calls.is_empty()
    {
        return Err(snapshot_error(
            "intent transcript state anchor is not a plain user message",
        ));
    }
    let state = message
        .content
        .strip_prefix(INTENT_STATE_PREFIX)
        .ok_or_else(|| {
            snapshot_error("intent transcript human message is not followed by INTENT_STATE")
        })?;
    serde_json::from_str(state)
        .map_err(|_| snapshot_error("intent transcript contains a malformed INTENT_STATE anchor"))
}

pub(super) fn validate_intent_state_anchor(
    state: &RestoredIntentStateAnchorV1,
) -> Result<(), SessionSnapshotError> {
    let mut available = BTreeSet::new();
    if state
        .available_channel_keys
        .iter()
        .any(|value| value.trim().is_empty() || !available.insert(value))
    {
        return Err(snapshot_error(
            "intent transcript state anchor contains invalid available channel keys",
        ));
    }
    let mut active = BTreeSet::new();
    if state
        .active_options
        .iter()
        .any(|value| value.trim().is_empty() || !active.insert(value))
    {
        return Err(snapshot_error(
            "intent transcript state anchor contains invalid active options",
        ));
    }
    let shape_valid = match state.stage.as_str() {
        "awaiting_decision" => {
            state
                .active_question
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
                && !state.active_options.is_empty()
        }
        "empty" | "preview_ready" => {
            state.active_question.is_none() && state.active_options.is_empty()
        }
        _ => false,
    };
    let _ = state.expected_revision;
    if shape_valid {
        Ok(())
    } else {
        Err(snapshot_error(
            "intent transcript state anchor fields do not match its stage",
        ))
    }
}

fn validate_assistant_response(message: &Message) -> Result<(), SessionSnapshotError> {
    if message.role != MessageRole::Assistant
        || message.tool_call_id.is_some()
        || message.tool_calls.len() > 1
        || (!message.tool_calls.is_empty() && !message.content.is_empty())
        || message.tool_calls.iter().any(|call| {
            call.id.trim().is_empty()
                || !matches!(
                    call.name.as_str(),
                    INTERPRET_INTENT_CORE
                        | EXTRACT_PRIVATE_STUDY_ROOM_DETAILS
                        | RESOLVE_INTENT_DECISION
                )
        })
    {
        return Err(snapshot_error(
            "intent transcript contains an invalid assistant response",
        ));
    }
    Ok(())
}

fn parse_intent_tool_result(
    message: &Message,
    expected_call_id: &str,
) -> Result<serde_json::Value, SessionSnapshotError> {
    if message.role != MessageRole::Tool
        || message.tool_call_id.as_deref() != Some(expected_call_id)
        || !message.tool_calls.is_empty()
    {
        return Err(snapshot_error(
            "intent transcript tool result does not match its assistant call",
        ));
    }
    let value: serde_json::Value = serde_json::from_str(&message.content)
        .map_err(|_| snapshot_error("intent transcript tool result is not valid JSON"))?;
    if !value.is_object()
        || value
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .is_none()
    {
        return Err(snapshot_error(
            "intent transcript tool result does not contain a typed outcome",
        ));
    }
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        let failure: RestoredIntentToolFailureV4 = serde_json::from_value(value.clone())
            .map_err(|_| snapshot_error("intent transcript tool failure has an invalid shape"))?;
        if failure.ok
            || failure.code.trim().is_empty()
            || failure.location.trim().is_empty()
            || failure.message.trim().is_empty()
            || failure.hint.trim().is_empty()
        {
            return Err(snapshot_error(
                "intent transcript tool failure has invalid fields",
            ));
        }
        let _ = failure.revision;
    }
    Ok(value)
}

fn validate_intent_detail_state(
    message: &Message,
) -> Result<Vec<crate::turn::IntentRecipeDetailFacetV3>, SessionSnapshotError> {
    if message.role != MessageRole::User
        || message.tool_call_id.is_some()
        || !message.tool_calls.is_empty()
    {
        return Err(snapshot_error(
            "intent detail state anchor is not a plain user message",
        ));
    }
    let value = message
        .content
        .strip_prefix(INTENT_DETAIL_STATE_PREFIX)
        .ok_or_else(|| snapshot_error("intent detail request is missing INTENT_DETAIL_STATE"))?;
    let state: RestoredIntentDetailStateAnchorV3 = serde_json::from_str(value)
        .map_err(|_| snapshot_error("intent transcript contains malformed detail state"))?;
    let unique = state.detail_facets.iter().copied().collect::<BTreeSet<_>>();
    if state.detail_facets.is_empty() || unique.len() != state.detail_facets.len() {
        return Err(snapshot_error(
            "intent detail state contains empty or duplicate facets",
        ));
    }
    Ok(state.detail_facets)
}

fn validate_transcript_counters(
    snapshot: &SessionSnapshot,
    turns: &[IntentTranscriptTurnV4],
) -> Result<(), SessionSnapshotError> {
    let sequence = u64::try_from(turns.len())
        .map_err(|_| snapshot_error("intent transcript turn count overflowed"))?;
    match (snapshot.turn_state.as_ref(), turns.last()) {
        (None, None) => {}
        (Some(state), Some(last)) => {
            let model_upper = last
                .model_responses
                .saturating_add(usize::from(last.open_model_request));
            if state.sequence != sequence
                || state.tool_calls != last.tool_calls
                || state.model_calls < last.model_responses
                || state.model_calls > model_upper
            {
                return Err(snapshot_error(
                    "intent transcript does not match its current turn counters",
                ));
            }
        }
        _ => {
            return Err(snapshot_error(
                "intent transcript turn history does not match its lifecycle state",
            ));
        }
    }
    let visible_model_responses = turns.iter().map(|turn| turn.model_responses).sum::<usize>();
    let open_model_requests = turns.iter().filter(|turn| turn.open_model_request).count();
    let visible_tool_calls = turns.iter().map(|turn| turn.tool_calls).sum::<usize>();
    if snapshot.observability.tool_calls != visible_tool_calls
        || snapshot.observability.model_calls < visible_model_responses
        || snapshot.observability.model_calls
            > visible_model_responses.saturating_add(open_model_requests)
    {
        return Err(snapshot_error(
            "intent transcript does not match its cumulative model and tool counters",
        ));
    }
    Ok(())
}

pub(super) fn restored_human_text(message: &Message) -> Result<String, SessionSnapshotError> {
    let envelope = message
        .content
        .strip_prefix(INTENT_HUMAN_PREFIX)
        .ok_or_else(|| snapshot_error("initial human message is not an INTENT_HUMAN envelope"))?;
    let parsed: RestoredIntentHumanEnvelopeV1 = serde_json::from_str(envelope)
        .map_err(|_| snapshot_error("initial human envelope is malformed"))?;
    Ok(parsed.text)
}
