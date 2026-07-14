use std::collections::BTreeSet;

use crate::llm::{Message, MessageRole};
use crate::tools::tool_definitions;
use crate::turn::AdaptivePhase;

use super::context::{is_anchor, DraftStateMemory};
use super::intent_routing::{
    validate_intent_recipe_snapshot, INTENT_RECIPE_SYSTEM_PROMPT_V1,
    INTENT_RECIPE_SYSTEM_PROMPT_V2, INTENT_RECIPE_SYSTEM_PROMPT_V3,
};
use super::repair::{has_matching_repair_directive, is_argument_failure};
use super::routing::{is_mutation_tool, routed_tool_definitions};
use super::{
    RepairKind, RepairState, SessionSnapshot, SessionSnapshotError, TurnPhase,
    DEFAULT_SYSTEM_PROMPT, PLANNED_SYSTEM_PROMPT, SESSION_SNAPSHOT_VERSION,
};

pub(super) fn validate_snapshot(snapshot: &SessionSnapshot) -> Result<(), SessionSnapshotError> {
    if snapshot.schema_version != SESSION_SNAPSHOT_VERSION {
        return Err(SessionSnapshotError::UnsupportedVersion {
            expected: SESSION_SNAPSHOT_VERSION,
            found: snapshot.schema_version,
        });
    }
    let Some(system) = snapshot.messages.first() else {
        return Err(snapshot_invariant("canonical messages are empty"));
    };
    if system.role != MessageRole::System
        || !matches!(
            system.content.as_str(),
            DEFAULT_SYSTEM_PROMPT
                | PLANNED_SYSTEM_PROMPT
                | INTENT_RECIPE_SYSTEM_PROMPT_V1
                | INTENT_RECIPE_SYSTEM_PROMPT_V2
                | INTENT_RECIPE_SYSTEM_PROMPT_V3
        )
    {
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
    validate_intent_recipe_snapshot(snapshot)?;
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
    if !has_matching_repair_directive(&snapshot.messages, ticket) {
        return Err(snapshot_invariant(
            "repair ticket has no matching repair directive",
        ));
    }
    Ok(())
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
