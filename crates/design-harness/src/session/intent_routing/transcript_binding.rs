use std::collections::BTreeMap;

use crate::draft::Draft;
use crate::errors::StructuredError;
use crate::intent::identity::{domain_separated_length_framed_digest, is_lowercase_sha256_hex};
use crate::intent::{
    compile_intent, prepare_intent_workspace, replay_intent_candidate_preparation,
    upgrade_working_draft_to_validated_preview, verify_outcome_only_finalization,
    ExistingChannelKey, IntentRequestedOutcome, IntentResolutionContext, IntentWorkspaceV2,
    PreparedIntentWorkspaceV2, ValidatedIntentV2,
};
use crate::llm::Message;
use crate::turn::{
    parse_private_study_room_details_for_active_serving_with_parameters, IntentRecipeDetailFacetV3,
    INTERPRET_INTENT_CORE,
};
use resource_resolution::ResourceBindingMap;

use super::super::SessionSnapshotError;
use super::adjudicate::PrivateStudyRoomPermitV2;
use super::request_evidence::{IntentRequestEvidenceChainV1, IntentRequestEvidenceEntryV1};
use super::state::{snapshot_error, IntentRecipeStageSnapshotV2};
use super::transcript_replay::{
    core_replay_snapshot_error, replay_core_semantics, restored_semantics_error, CoreReplayErrorV4,
    ReplayedCoreSemanticsV4, ReplayedPrivateSemanticsV4, ReplayedRoutedCoreV4,
    ReplayedRoutedSemanticsV4,
};
use super::transcript_restore::{IntentTranscriptTurnV4, IntentTranscriptV4};

const ROUTED_PRESENTATION_DIGEST_DOMAIN_V1: &[u8] = b"starring.intent.routed_presentation.v1\0";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedRoutedToolResultV4 {
    ok: bool,
    status: String,
    fallback_kind: String,
    adjudication_digest: String,
    presentation_digest: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedAwaitingToolResultV4 {
    ok: bool,
    status: String,
    revision: u64,
    available_channel_keys: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedPreviewToolResultV4 {
    ok: bool,
    status: String,
    intent_revision: u64,
    draft_revision: u64,
    semantic_intent_hash: String,
    compiled_plan_hash: String,
    candidate_ruleset_hash: String,
    compiled_operations: usize,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedDetailsRequiredToolResultV4 {
    ok: bool,
    status: String,
    detail_facets: Vec<IntentRecipeDetailFacetV3>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedToolFailureV4 {
    ok: bool,
    code: String,
    location: String,
    message: String,
    hint: String,
    revision: u64,
}

pub(super) struct PreviewToolResultExpectationV4<'a> {
    pub(super) intent_revision: u64,
    pub(super) draft_revision: u64,
    pub(super) semantic_intent_hash: &'a str,
    pub(super) compiled_plan_hash: &'a str,
    pub(super) candidate_ruleset_hash: &'a str,
    pub(super) compiled_operations: usize,
}

pub(super) struct ValidatedCoreTranscriptV4 {
    private_turns: Vec<(u64, ReplayedPrivateSemanticsV4)>,
    deferred_candidate_failures: Vec<u64>,
}

impl ValidatedCoreTranscriptV4 {
    pub(super) fn private_turn(
        &self,
        human_message_index: u64,
    ) -> Option<&ReplayedPrivateSemanticsV4> {
        self.private_turns
            .iter()
            .find_map(|(index, replayed)| (*index == human_message_index).then_some(replayed))
    }
}

struct TerminalPrivateOutcomeV4<'a> {
    human_message_index: u64,
    status: &'a str,
}

#[derive(Clone, Copy)]
struct FinalizationTranscriptExpectationV1 {
    human_message_index: u64,
    intent_revision: u64,
    draft_revision: u64,
}

struct WorkingDraftFinalizationReplayV1 {
    workspace: IntentWorkspaceV2,
    candidate_revision: u64,
}

struct PrivateCoreValidationContextV4<'a> {
    draft: &'a Draft,
    bindings: &'a ResourceBindingMap,
    candidate_replay_outcomes: &'a mut BTreeMap<String, Result<(), StructuredError>>,
    finalization: Option<FinalizationTranscriptExpectationV1>,
    working_finalization: Option<&'a WorkingDraftFinalizationReplayV1>,
}

pub(super) fn routed_presentation_digest(response: &str) -> String {
    domain_separated_length_framed_digest(
        ROUTED_PRESENTATION_DIGEST_DOMAIN_V1,
        &[response.as_bytes()],
    )
}

pub(super) fn validate_terminal_private_stage(
    stage: &IntentRecipeStageSnapshotV2,
    transcript: &IntentTranscriptV4,
    core_transcript: &ValidatedCoreTranscriptV4,
) -> Result<(), SessionSnapshotError> {
    let terminal = terminal_private_outcome(transcript);
    let expected = match stage {
        IntentRecipeStageSnapshotV2::Empty => {
            if terminal.is_some() {
                return Err(snapshot_error(
                    "empty intent recipe stage contains a successful private outcome",
                ));
            }
            if !core_transcript.deferred_candidate_failures.is_empty() {
                return Err(snapshot_error(
                    "empty intent recipe stage contains an unreproducible historical candidate failure",
                ));
            }
            return Ok(());
        }
        IntentRecipeStageSnapshotV2::AwaitingDecision {
            request_evidence, ..
        } => ("awaiting_decision", request_evidence),
        IntentRecipeStageSnapshotV2::PreviewReady {
            request_evidence, ..
        } => ("preview_ready", request_evidence),
    };
    let terminal = terminal.ok_or_else(|| {
        snapshot_error("persisted intent stage has no successful private transcript outcome")
    })?;
    if terminal.status != expected.0
        || terminal.human_message_index != final_evidence_message_index(expected.1)?
    {
        return Err(snapshot_error(
            "persisted intent stage does not match its terminal private transcript outcome",
        ));
    }
    if core_transcript
        .deferred_candidate_failures
        .iter()
        .any(|index| *index >= terminal.human_message_index)
    {
        return Err(snapshot_error(
            "persisted candidate failure is not followed by the terminal private outcome",
        ));
    }
    Ok(())
}

pub(super) fn validate_final_awaiting_tool_result(
    request_evidence: &IntentRequestEvidenceChainV1,
    transcript: &IntentTranscriptV4,
    revision: u64,
    options: &[String],
) -> Result<(), SessionSnapshotError> {
    let persisted: PersistedAwaitingToolResultV4 =
        serde_json::from_value(final_success_result(request_evidence, transcript)?.clone())
            .map_err(|_| snapshot_error("awaiting intent tool result has an invalid shape"))?;
    if !persisted.ok
        || persisted.status != "awaiting_decision"
        || persisted.revision != revision
        || persisted.available_channel_keys != options
    {
        return Err(snapshot_error(
            "awaiting intent tool result does not match its persisted stage",
        ));
    }
    Ok(())
}

pub(super) fn validate_final_preview_tool_result(
    request_evidence: &IntentRequestEvidenceChainV1,
    transcript: &IntentTranscriptV4,
    expected: PreviewToolResultExpectationV4<'_>,
) -> Result<(), SessionSnapshotError> {
    let persisted: PersistedPreviewToolResultV4 =
        serde_json::from_value(final_success_result(request_evidence, transcript)?.clone())
            .map_err(|_| snapshot_error("preview-ready intent tool result has an invalid shape"))?;
    if !persisted.ok
        || persisted.status != "preview_ready"
        || persisted.intent_revision != expected.intent_revision
        || persisted.draft_revision != expected.draft_revision
        || persisted.semantic_intent_hash != expected.semantic_intent_hash
        || persisted.compiled_plan_hash != expected.compiled_plan_hash
        || persisted.candidate_ruleset_hash != expected.candidate_ruleset_hash
        || persisted.compiled_operations != expected.compiled_operations
    {
        return Err(snapshot_error(
            "preview-ready intent tool result does not match its persisted stage",
        ));
    }
    Ok(())
}

pub(super) fn validate_details_required_tool_result(
    result: &serde_json::Value,
    facets: &[IntentRecipeDetailFacetV3],
) -> Result<(), SessionSnapshotError> {
    let persisted: PersistedDetailsRequiredToolResultV4 = serde_json::from_value(result.clone())
        .map_err(|_| snapshot_error("detail-required Core result has an invalid shape"))?;
    if !persisted.ok || persisted.status != "details_required" || persisted.detail_facets != facets
    {
        return Err(snapshot_error(
            "detail-required Core result does not match its selected facets",
        ));
    }
    Ok(())
}

pub(super) fn validate_initial_awaiting_tool_result(
    result: &serde_json::Value,
    revision: u64,
    options: &[String],
) -> Result<(), SessionSnapshotError> {
    let persisted: PersistedAwaitingToolResultV4 = serde_json::from_value(result.clone())
        .map_err(|_| snapshot_error("initial awaiting intent tool result has an invalid shape"))?;
    if !persisted.ok
        || persisted.status != "awaiting_decision"
        || persisted.revision != revision
        || persisted.available_channel_keys != options
    {
        return Err(snapshot_error(
            "initial awaiting intent tool result does not reproduce from its Core",
        ));
    }
    Ok(())
}

pub(super) fn validate_core_transcript_results(
    messages: &[Message],
    transcript: &IntentTranscriptV4,
    draft: &Draft,
    bindings: &ResourceBindingMap,
    stage: &IntentRecipeStageSnapshotV2,
) -> Result<ValidatedCoreTranscriptV4, SessionSnapshotError> {
    let mut private_turns = Vec::new();
    let mut deferred_candidate_failures = Vec::new();
    let mut candidate_replay_outcomes = BTreeMap::new();
    let finalization = finalization_transcript_expectation(stage);
    let working_finalization = working_draft_finalization_replay(stage);
    for turn in &transcript.turns {
        if turn.primary_tool.as_deref() != Some(INTERPRET_INTENT_CORE) {
            continue;
        }
        let arguments = turn
            .primary_arguments
            .as_deref()
            .ok_or_else(|| snapshot_error("Core transcript is missing its arguments"))?;
        let result = turn
            .primary_result
            .as_ref()
            .ok_or_else(|| snapshot_error("Core transcript is missing its tool result"))?;
        match replay_core_semantics(messages, turn.human_message_index, arguments) {
            Ok(ReplayedCoreSemanticsV4::Routed(replayed)) => {
                validate_persisted_routed_result(result, &replayed)?
            }
            Ok(ReplayedCoreSemanticsV4::Private(replayed)) => {
                let mut validation = PrivateCoreValidationContextV4 {
                    draft,
                    bindings,
                    candidate_replay_outcomes: &mut candidate_replay_outcomes,
                    finalization: finalization.filter(|expected| {
                        expected.human_message_index == turn.human_message_index
                    }),
                    working_finalization: working_finalization.as_ref(),
                };
                if validate_private_core_result(turn, result, &replayed, &mut validation)? {
                    deferred_candidate_failures.push(turn.human_message_index);
                }
                private_turns.push((turn.human_message_index, replayed));
            }
            Err(CoreReplayErrorV4::Snapshot(error)) => return Err(error),
            Err(CoreReplayErrorV4::Semantic { error, revision }) => {
                validate_persisted_core_failure(result, &error, revision)?
            }
        }
    }
    Ok(ValidatedCoreTranscriptV4 {
        private_turns,
        deferred_candidate_failures,
    })
}

pub(super) fn replay_successful_routed_core_turn(
    messages: &[Message],
    human_message_index: u64,
    arguments: &str,
    result: &serde_json::Value,
) -> Result<Option<ReplayedRoutedCoreV4>, SessionSnapshotError> {
    let replayed = match replay_core_semantics(messages, human_message_index, arguments)
        .map_err(core_replay_snapshot_error)?
    {
        ReplayedCoreSemanticsV4::Routed(replayed) => replayed,
        ReplayedCoreSemanticsV4::Private(_) => {
            if has_routed_binding(result) {
                return Err(snapshot_error(
                    "successful routed Core result no longer reproduces a deterministic fallback",
                ));
            }
            return Ok(None);
        }
    };
    if !result
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        if has_routed_binding(result) {
            return Err(snapshot_error(
                "successful routed Core result no longer reproduces a deterministic fallback",
            ));
        }
        return Ok(None);
    }
    validate_persisted_routed_result(result, &replayed)?;
    Ok(Some(ReplayedRoutedCoreV4::from_semantics(replayed)))
}

pub(super) fn validate_persisted_binding(
    binding: &str,
    expected: &str,
) -> Result<(), SessionSnapshotError> {
    if valid_hash(binding) && binding == expected {
        Ok(())
    } else {
        Err(snapshot_error(
            "persisted intent stage does not match its route decision binding",
        ))
    }
}

pub(super) fn valid_hash(value: &str) -> bool {
    is_lowercase_sha256_hex(value)
}

fn final_evidence_message_index(
    request_evidence: &IntentRequestEvidenceChainV1,
) -> Result<u64, SessionSnapshotError> {
    match request_evidence
        .entries()
        .last()
        .ok_or_else(|| snapshot_error("persisted request evidence is empty"))?
    {
        IntentRequestEvidenceEntryV1::InitialHuman {
            transcript_message_index,
            ..
        }
        | IntentRequestEvidenceEntryV1::AcceptedResolution {
            transcript_message_index,
            ..
        }
        | IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
            transcript_message_index,
            ..
        } => Ok(*transcript_message_index),
    }
}

fn finalization_transcript_expectation(
    stage: &IntentRecipeStageSnapshotV2,
) -> Option<FinalizationTranscriptExpectationV1> {
    let IntentRecipeStageSnapshotV2::PreviewReady {
        request_evidence, ..
    } = stage
    else {
        return None;
    };
    request_evidence
        .terminal_outcome_finalization()
        .map(|finalization| FinalizationTranscriptExpectationV1 {
            human_message_index: finalization.transcript_message_index(),
            intent_revision: finalization.next_workspace_revision(),
            draft_revision: finalization.expected_draft_revision(),
        })
}

fn working_draft_finalization_replay(
    stage: &IntentRecipeStageSnapshotV2,
) -> Option<WorkingDraftFinalizationReplayV1> {
    let IntentRecipeStageSnapshotV2::PreviewReady {
        workspace,
        candidate_revision,
        request_evidence,
        ..
    } = stage
    else {
        return None;
    };
    let mut working = workspace.clone();
    match working.requested_outcome {
        IntentRequestedOutcome::WorkingDraft => {}
        IntentRequestedOutcome::ValidatedPreview => {
            let finalization = request_evidence.terminal_outcome_finalization()?;
            working.revision = finalization.prior_workspace_revision();
            working.requested_outcome = IntentRequestedOutcome::WorkingDraft;
        }
        IntentRequestedOutcome::Discussion => return None,
    }
    Some(WorkingDraftFinalizationReplayV1 {
        workspace: working,
        candidate_revision: *candidate_revision,
    })
}

fn replay_outcome_finalization_attempt(
    working_workspace: &IntentWorkspaceV2,
    standalone_intent: &ValidatedIntentV2,
    context: &IntentResolutionContext,
) -> Result<(), StructuredError> {
    let PreparedIntentWorkspaceV2::Resolved {
        intent: working_intent,
        ..
    } = prepare_intent_workspace(working_workspace.clone(), context)?
    else {
        return Err(StructuredError::new(
            "INTENT_OUTCOME_FINALIZATION_SOURCE_UNRESOLVED",
            "intent.workspace",
            "The committed working Draft workspace no longer resolves",
            "Start a new intent from the current canonical Draft",
        ));
    };
    let PreparedIntentWorkspaceV2::Resolved {
        intent: finalized_intent,
        ..
    } = upgrade_working_draft_to_validated_preview(
        working_workspace,
        working_workspace.revision,
        context,
    )?
    else {
        return Err(StructuredError::new(
            "INTENT_OUTCOME_FINALIZATION_UNRESOLVED",
            "intent.workspace",
            "The outcome-only workspace transition no longer resolves",
            "Start a new intent from the current canonical Draft",
        ));
    };
    let working_compilation = compile_intent(&working_intent)?;
    let standalone_compilation = compile_intent(standalone_intent)?;
    let finalized_compilation = compile_intent(&finalized_intent)?;
    verify_outcome_only_finalization(
        &working_compilation,
        &standalone_compilation,
        &finalized_compilation,
    )
}

fn terminal_private_outcome(
    transcript: &IntentTranscriptV4,
) -> Option<TerminalPrivateOutcomeV4<'_>> {
    transcript.turns.iter().rev().find_map(|turn| {
        let result = turn
            .detail_result
            .as_ref()
            .or(turn.primary_result.as_ref())?;
        if result.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return None;
        }
        let status = result.get("status").and_then(serde_json::Value::as_str)?;
        matches!(status, "awaiting_decision" | "preview_ready").then_some(
            TerminalPrivateOutcomeV4 {
                human_message_index: turn.human_message_index,
                status,
            },
        )
    })
}

fn final_success_result<'a>(
    request_evidence: &IntentRequestEvidenceChainV1,
    transcript: &'a IntentTranscriptV4,
) -> Result<&'a serde_json::Value, SessionSnapshotError> {
    let transcript_message_index = final_evidence_message_index(request_evidence)?;
    let turn = transcript
        .turns
        .iter()
        .find(|turn| turn.human_message_index == transcript_message_index)
        .ok_or_else(|| snapshot_error("final successful intent transcript turn is missing"))?;
    turn.detail_result
        .as_ref()
        .or(turn.primary_result.as_ref())
        .ok_or_else(|| snapshot_error("final successful intent tool result is missing"))
}

fn validate_persisted_core_failure(
    result: &serde_json::Value,
    expected: &StructuredError,
    revision: u64,
) -> Result<(), SessionSnapshotError> {
    validate_persisted_failure(
        result,
        expected,
        revision,
        "Core failure result has an invalid transcript binding",
        "Core failure result does not match deterministic transcript replay",
    )
}

fn validate_persisted_private_failure(
    result: &serde_json::Value,
    expected: &StructuredError,
    revision: u64,
) -> Result<(), SessionSnapshotError> {
    validate_persisted_failure(
        result,
        expected,
        revision,
        "private failure result has an invalid transcript binding",
        "private failure result does not match deterministic transcript replay",
    )
}

fn validate_persisted_private_failure_shape(
    result: &serde_json::Value,
    revision: u64,
) -> Result<(), SessionSnapshotError> {
    let persisted: PersistedToolFailureV4 = serde_json::from_value(result.clone())
        .map_err(|_| snapshot_error("private failure result has an invalid transcript binding"))?;
    if persisted.ok
        || persisted.code.trim().is_empty()
        || persisted.location.trim().is_empty()
        || persisted.message.trim().is_empty()
        || persisted.hint.trim().is_empty()
        || persisted.revision != revision
    {
        return Err(snapshot_error(
            "private failure result has an invalid transcript binding",
        ));
    }
    Ok(())
}

fn validate_persisted_failure(
    result: &serde_json::Value,
    expected: &StructuredError,
    revision: u64,
    invalid_shape: &'static str,
    mismatch: &'static str,
) -> Result<(), SessionSnapshotError> {
    let persisted: PersistedToolFailureV4 =
        serde_json::from_value(result.clone()).map_err(|_| snapshot_error(invalid_shape))?;
    if persisted.ok
        || persisted.code != expected.code
        || persisted.location != expected.location
        || persisted.message != expected.message
        || persisted.hint != expected.hint
        || persisted.revision != revision
    {
        return Err(snapshot_error(mismatch));
    }
    Ok(())
}

fn validate_private_core_result(
    turn: &IntentTranscriptTurnV4,
    primary_result: &serde_json::Value,
    replayed: &ReplayedPrivateSemanticsV4,
    validation: &mut PrivateCoreValidationContextV4<'_>,
) -> Result<bool, SessionSnapshotError> {
    if has_routed_binding(primary_result) {
        return Err(snapshot_error(
            "private Core result contains an unrelated routed transcript binding",
        ));
    }
    let selection = &replayed.selection;
    let human = &replayed.human;
    let available_channel_keys = &replayed.available_channel_keys;
    let expected_revision = selection.expected_revision();
    let facets = selection.detail_facets().to_vec();
    if facets.is_empty() {
        if turn.detail_arguments.is_some()
            || turn.detail_result.is_some()
            || !turn.detail_facets.is_empty()
            || !turn.detail_fields.is_empty()
            || replayed.detail_parameters.is_some()
        {
            return Err(snapshot_error(
                "default private Core contains an unexpected detail frontier",
            ));
        }
        let permit = match selection.clone().finalize(None) {
            Ok(permit) => permit,
            Err(error) if !turn.succeeded => {
                validate_persisted_private_failure(primary_result, &error, expected_revision)?;
                return Ok(false);
            }
            Err(error) => return Err(restored_semantics_error(error)),
        };
        return validate_prepared_private_result(
            permit,
            available_channel_keys,
            primary_result,
            expected_revision,
            validation,
        );
    }
    validate_details_required_tool_result(primary_result, &facets)?;
    if turn.detail_facets != facets {
        return Err(snapshot_error(
            "private Core detail state does not match its replayed facets",
        ));
    }
    if turn.detail_fields.as_slice() != replayed.detail_ticket.fields() {
        return Err(snapshot_error(
            "private detail state fields do not match the source human turn",
        ));
    }
    let Some(detail_arguments) = turn.detail_arguments.as_deref() else {
        return Ok(false);
    };
    let detail_result = turn
        .detail_result
        .as_ref()
        .ok_or_else(|| snapshot_error("private detail transcript result is missing"))?;
    let detail_parameters = replayed.detail_parameters.as_ref().ok_or_else(|| {
        snapshot_error("replayed private detail frontier is missing its serving parameters")
    })?;
    let details = match parse_private_study_room_details_for_active_serving_with_parameters(
        detail_arguments,
        &facets,
        replayed.detail_ticket.expectations(),
        detail_parameters,
        selection.expected_revision(),
        selection.semantic_ir_digest(),
        human,
    ) {
        Ok(details) => details,
        Err(error) if !turn.succeeded => {
            validate_persisted_private_failure(detail_result, &error, expected_revision)?;
            return Ok(false);
        }
        Err(error) => return Err(restored_semantics_error(error)),
    };
    let permit = match selection.clone().finalize(Some(details)) {
        Ok(permit) => permit,
        Err(error) if !turn.succeeded => {
            validate_persisted_private_failure(detail_result, &error, expected_revision)?;
            return Ok(false);
        }
        Err(error) => return Err(restored_semantics_error(error)),
    };
    validate_prepared_private_result(
        permit,
        available_channel_keys,
        detail_result,
        expected_revision,
        validation,
    )
}

fn validate_prepared_private_result(
    permit: PrivateStudyRoomPermitV2,
    available_channel_keys: &[String],
    result: &serde_json::Value,
    expected_revision: u64,
    validation: &mut PrivateCoreValidationContextV4<'_>,
) -> Result<bool, SessionSnapshotError> {
    let context = IntentResolutionContext::from_channel_bindings(
        available_channel_keys
            .iter()
            .cloned()
            .map(ExistingChannelKey),
    );
    let prepared = match permit.prepare(&context) {
        Ok((_, prepared)) => prepared,
        Err(error) if result.get("ok").and_then(serde_json::Value::as_bool) == Some(false) => {
            validate_persisted_private_failure(result, &error, expected_revision)?;
            return Ok(false);
        }
        Err(error) => return Err(restored_semantics_error(error)),
    };
    match prepared {
        PreparedIntentWorkspaceV2::NeedsInput {
            workspace,
            decisions,
        } => {
            let [decision] = decisions.as_slice() else {
                return Err(snapshot_error(
                    "private Core replay did not produce one active decision",
                ));
            };
            let persisted: PersistedAwaitingToolResultV4 =
                serde_json::from_value(result.clone())
                    .map_err(|_| snapshot_error("replayed awaiting result has an invalid shape"))?;
            if !persisted.ok
                || persisted.status != "awaiting_decision"
                || persisted.revision != workspace.revision
                || persisted.available_channel_keys != decision.options
            {
                return Err(snapshot_error(
                    "awaiting result does not reproduce from its private Core",
                ));
            }
            if result.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
                return Err(snapshot_error(
                    "private failure result does not match deterministic transcript replay",
                ));
            }
            Ok(false)
        }
        PreparedIntentWorkspaceV2::Resolved { workspace, intent } => {
            if result.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
                if let Some(finalization) = validation.working_finalization.filter(|finalization| {
                    expected_revision == finalization.candidate_revision
                        && intent.requested_outcome() == IntentRequestedOutcome::ValidatedPreview
                }) {
                    let replay = replay_outcome_finalization_attempt(
                        &finalization.workspace,
                        &intent,
                        &context,
                    );
                    return match replay {
                        Ok(()) => Err(snapshot_error(
                            "private failure result does not match deterministic transcript replay",
                        )),
                        Err(error) => {
                            validate_persisted_private_failure(result, &error, expected_revision)?;
                            Ok(false)
                        }
                    };
                }
                if expected_revision > validation.draft.draft_revision {
                    return Err(snapshot_error(
                        "private candidate failure references a future Draft revision",
                    ));
                }
                if expected_revision < validation.draft.draft_revision {
                    validate_persisted_private_failure_shape(result, expected_revision)?;
                    return Ok(true);
                }
                let replay_key = serde_json::to_string(&intent).map_err(|_| {
                    snapshot_error("private candidate replay identity could not be serialized")
                })?;
                let replay = validation
                    .candidate_replay_outcomes
                    .entry(replay_key)
                    .or_insert_with(|| {
                        replay_intent_candidate_preparation(
                            validation.draft,
                            &intent,
                            validation.bindings,
                        )
                    })
                    .clone();
                return match replay {
                    Ok(()) => Err(snapshot_error(
                        "private failure result does not match deterministic transcript replay",
                    )),
                    Err(error) => {
                        validate_persisted_private_failure(result, &error, expected_revision)?;
                        Ok(false)
                    }
                };
            }
            let persisted: PersistedPreviewToolResultV4 = serde_json::from_value(result.clone())
                .map_err(|_| snapshot_error("replayed preview result has an invalid shape"))?;
            let expected_intent_revision = validation
                .finalization
                .map(|expected| expected.intent_revision)
                .unwrap_or(workspace.revision);
            if !persisted.ok
                || persisted.status != "preview_ready"
                || persisted.intent_revision != expected_intent_revision
                || validation.finalization.is_some_and(|expected| {
                    workspace.requested_outcome != IntentRequestedOutcome::ValidatedPreview
                        || intent.requested_outcome() != IntentRequestedOutcome::ValidatedPreview
                        || persisted.draft_revision != expected.draft_revision
                })
                || !valid_hash(&persisted.semantic_intent_hash)
                || !valid_hash(&persisted.compiled_plan_hash)
                || !valid_hash(&persisted.candidate_ruleset_hash)
                || persisted.compiled_operations == 0
            {
                return Err(snapshot_error(
                    "preview result does not reproduce from its private Core",
                ));
            }
            Ok(false)
        }
    }
}

fn validate_persisted_routed_result(
    result: &serde_json::Value,
    replayed: &ReplayedRoutedSemanticsV4,
) -> Result<(), SessionSnapshotError> {
    let persisted: PersistedRoutedToolResultV4 =
        serde_json::from_value(result.clone()).map_err(|_| {
            snapshot_error("successful routed Core result has an invalid transcript binding")
        })?;
    let expected_presentation_digest = routed_presentation_digest(&replayed.response);
    if !persisted.ok
        || persisted.status != "routed"
        || persisted.fallback_kind != replayed.fallback_kind.as_str()
        || !valid_hash(&persisted.adjudication_digest)
        || persisted.adjudication_digest != replayed.decision.adjudication_digest()
        || !valid_hash(&persisted.presentation_digest)
        || persisted.presentation_digest != expected_presentation_digest
    {
        return Err(snapshot_error(
            "successful routed Core result does not match deterministic transcript replay",
        ));
    }
    Ok(())
}

fn has_routed_binding(result: &serde_json::Value) -> bool {
    result.get("status").and_then(serde_json::Value::as_str) == Some("routed")
        || [
            "fallback_kind",
            "adjudication_digest",
            "presentation_digest",
        ]
        .iter()
        .any(|field| result.get(field).is_some())
}
