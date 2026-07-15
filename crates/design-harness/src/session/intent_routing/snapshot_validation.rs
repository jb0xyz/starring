use crate::draft::Draft;
use crate::errors::StructuredError;
use crate::intent::identity::is_lowercase_sha256_hex;
use crate::intent::{
    candidate_ruleset_hash, compile_intent, draft_state_hash, prepare_intent_workspace,
    ExistingChannelKey, IntentResolutionContext, IntentWorkspaceV2, PreparedIntentWorkspaceV2,
    INTENT_IDENTITY_REVISION,
};
use crate::llm::Message;
use crate::turn::{
    parse_interpret_intent_core, parse_private_study_room_details_for_serving,
    INTERPRET_INTENT_CORE, RESOLVE_INTENT_DECISION,
};

use super::super::{SessionSnapshot, SessionSnapshotError};
use super::adjudicate::{
    adjudicate_intent_core_v4, validate_persisted_private_study_room_decision_v4,
    IntentCoreAdjudicationV4,
};
use super::decision::IntentRouteDecisionV2;
use super::evidence::IntentRecipeEvidenceV4;
use super::grounding::deterministically_selected_option;
use super::request_evidence::{IntentRequestEvidenceChainV1, IntentRequestEvidenceEntryV1};
use super::state::{
    snapshot_error, IntentRecipeRuntime, IntentRecipeStageSnapshotV2,
    INTENT_RECIPE_PROTOCOL_VERSION, INTENT_RECIPE_SYSTEM_PROMPT_V1, INTENT_RECIPE_SYSTEM_PROMPT_V2,
    INTENT_RECIPE_SYSTEM_PROMPT_V3, INTENT_RECIPE_SYSTEM_PROMPT_V4,
};
use super::state_binding::{
    awaiting_decision_binding_digest_v4, preview_ready_binding_digest_v4,
    AwaitingDecisionBindingInputV4, PreviewReadyBindingInputV4,
};
use super::transcript_restore::{
    parse_intent_state_anchor, restored_human_text, validate_intent_state_anchor,
    validate_v4_transcript, IntentTranscriptV4,
};
use super::{INTENT_RECIPE_PROTOCOL_VERSION_V2, INTENT_RECIPE_PROTOCOL_VERSION_V3};

impl IntentRecipeRuntime {
    pub(super) fn validate_restored_stage(
        &self,
        draft: &Draft,
        messages: &[Message],
    ) -> Result<(), SessionSnapshotError> {
        let context = self.resolution_context();
        match &self.snapshot.stage {
            IntentRecipeStageSnapshotV2::Empty => Ok(()),
            IntentRecipeStageSnapshotV2::AwaitingDecision {
                root_draft_revision,
                workspace,
                active_decision,
                request_evidence,
                root_draft_hash,
                ..
            } => {
                request_evidence
                    .validate_resolutions_against_workspace(messages, workspace, &context)
                    .map_err(restored_state_error)?;
                if request_evidence
                    .initial_expected_revision()
                    .map_err(restored_state_error)?
                    != *root_draft_revision
                {
                    return Err(snapshot_error(
                        "awaiting intent evidence does not begin at its root Draft revision",
                    ));
                }
                let prepared = prepare_intent_workspace(workspace.clone(), &context)
                    .map_err(restored_state_error)?;
                match prepared {
                    PreparedIntentWorkspaceV2::NeedsInput {
                        workspace: normalized,
                        decisions,
                    } if normalized == *workspace
                        && decisions.as_slice() == std::slice::from_ref(active_decision)
                        && draft_state_hash(draft).map_err(restored_state_error)?
                            == *root_draft_hash =>
                    {
                        Ok(())
                    }
                    _ => Err(snapshot_error(
                        "awaiting intent workspace does not reproduce its active decision",
                    )),
                }
            }
            IntentRecipeStageSnapshotV2::PreviewReady {
                root_draft_revision,
                workspace,
                identity_revision,
                candidate_revision,
                compiler_input_hash,
                semantic_intent_hash,
                compiled_plan_hash,
                candidate_draft_hash,
                external_channel_bindings,
                compiled_operations,
                request_evidence,
                ..
            } => {
                request_evidence
                    .validate_resolutions_against_workspace(messages, workspace, &context)
                    .map_err(restored_state_error)?;
                if request_evidence
                    .initial_expected_revision()
                    .map_err(restored_state_error)?
                    != *root_draft_revision
                {
                    return Err(snapshot_error(
                        "preview-ready intent evidence does not begin at its root Draft revision",
                    ));
                }
                let prepared = prepare_intent_workspace(workspace.clone(), &context)
                    .map_err(restored_state_error)?;
                let PreparedIntentWorkspaceV2::Resolved {
                    workspace: normalized,
                    intent,
                } = prepared
                else {
                    return Err(snapshot_error(
                        "preview-ready intent workspace no longer resolves",
                    ));
                };
                let compiled = compile_intent(&intent).map_err(restored_state_error)?;
                let recompiled_operations = compiled.requirements.len();
                let manifest = compiled.manifest;
                let compiled_revision_delta = u64::try_from(*compiled_operations)
                    .ok()
                    .and_then(|operations| root_draft_revision.checked_add(operations));
                if normalized != *workspace
                    || manifest.identity_revision != *identity_revision
                    || manifest.compiler_input_hash != *compiler_input_hash
                    || manifest.semantic_intent_hash != *semantic_intent_hash
                    || manifest.compiled_plan_hash != *compiled_plan_hash
                    || manifest.external_channel_bindings != *external_channel_bindings
                    || recompiled_operations != *compiled_operations
                    || compiled_revision_delta != Some(*candidate_revision)
                    || draft_state_hash(draft).map_err(restored_state_error)?
                        != *candidate_draft_hash
                {
                    return Err(snapshot_error(
                        "preview-ready intent identities do not reproduce from its typed workspace",
                    ));
                }
                Ok(())
            }
        }
    }
}

pub(in crate::session) fn validate_intent_recipe_snapshot(
    snapshot: &SessionSnapshot,
) -> Result<(), SessionSnapshotError> {
    let prompt = snapshot
        .messages
        .first()
        .map(|message| message.content.as_str());
    let Some(intent) = snapshot.intent_recipe.as_ref() else {
        if matches!(
            prompt,
            Some(
                INTENT_RECIPE_SYSTEM_PROMPT_V1
                    | INTENT_RECIPE_SYSTEM_PROMPT_V2
                    | INTENT_RECIPE_SYSTEM_PROMPT_V3
                    | INTENT_RECIPE_SYSTEM_PROMPT_V4
            )
        ) {
            return Err(snapshot_error(
                "intent recipe prompt is present without intent recipe state",
            ));
        }
        return Ok(());
    };
    match (prompt, intent.protocol_version) {
        (Some(INTENT_RECIPE_SYSTEM_PROMPT_V4), INTENT_RECIPE_PROTOCOL_VERSION) => {}
        (Some(INTENT_RECIPE_SYSTEM_PROMPT_V1), 1)
        | (Some(INTENT_RECIPE_SYSTEM_PROMPT_V2), INTENT_RECIPE_PROTOCOL_VERSION_V2)
        | (Some(INTENT_RECIPE_SYSTEM_PROMPT_V3), INTENT_RECIPE_PROTOCOL_VERSION_V3) => {
            return Err(SessionSnapshotError::UnsupportedIntentProtocolVersion {
                expected: INTENT_RECIPE_PROTOCOL_VERSION,
                found: intent.protocol_version,
            });
        }
        (
            Some(
                INTENT_RECIPE_SYSTEM_PROMPT_V1
                | INTENT_RECIPE_SYSTEM_PROMPT_V2
                | INTENT_RECIPE_SYSTEM_PROMPT_V3
                | INTENT_RECIPE_SYSTEM_PROMPT_V4,
            ),
            _,
        ) => {
            return Err(snapshot_error(
                "intent recipe prompt and protocol version do not match",
            ));
        }
        _ => {
            return Err(snapshot_error(
                "intent recipe state does not use a fixed intent recipe system prompt",
            ));
        }
    }
    if !valid_hash(&intent.context_fingerprint) {
        return Err(snapshot_error(
            "intent recipe context fingerprint is malformed",
        ));
    }
    if snapshot.adaptive_enabled
        || snapshot.adaptive_turn.is_some()
        || snapshot.repair_state.is_some()
        || !snapshot.brief_history.is_empty()
        || snapshot.prose_nudged
    {
        return Err(snapshot_error(
            "intent recipe snapshot contains incompatible adaptive or repair state",
        ));
    }
    let transcript = validate_v4_transcript(snapshot)?;
    match &intent.stage {
        IntentRecipeStageSnapshotV2::Empty => Ok(()),
        IntentRecipeStageSnapshotV2::AwaitingDecision {
            root_draft_revision,
            workspace,
            active_decision,
            request_evidence,
            root_draft_hash,
            route_decision,
            recipe_evidence,
            decision_binding_digest,
        } => {
            if *root_draft_revision != snapshot.draft.draft_revision
                || workspace.schema_version != 2
                || workspace.revision == 0
                || workspace.features.len() != 1
                || active_decision.id.trim().is_empty()
                || active_decision.path.trim().is_empty()
                || active_decision.question.trim().is_empty()
                || active_decision.options.is_empty()
            {
                return Err(snapshot_error(
                    "awaiting intent decision state is inconsistent",
                ));
            }
            validate_persisted_evidence_chain(
                request_evidence,
                &snapshot.messages,
                &transcript,
                workspace,
                route_decision,
                recipe_evidence,
            )?;
            let actual_draft_hash = draft_state_hash(&snapshot.draft).map_err(|error| {
                snapshot_error(format!(
                    "persisted root Draft hashing failed {}: {}",
                    error.code, error.message
                ))
            })?;
            if !valid_hash(root_draft_hash) || root_draft_hash != &actual_draft_hash {
                return Err(snapshot_error(
                    "awaiting intent decision does not match its root Draft",
                ));
            }
            let expected_binding =
                awaiting_decision_binding_digest_v4(AwaitingDecisionBindingInputV4 {
                    protocol_version: intent.protocol_version,
                    context_fingerprint: &intent.context_fingerprint,
                    root_draft_revision: *root_draft_revision,
                    root_draft_hash,
                    workspace,
                    active_decision,
                    request_evidence,
                    route_decision,
                    recipe_evidence,
                })
                .map_err(|error| {
                    snapshot_error(format!(
                        "persisted intent stage binding failed {}: {}",
                        error.code, error.message
                    ))
                })?;
            validate_persisted_binding(decision_binding_digest, &expected_binding)?;
            Ok(())
        }
        IntentRecipeStageSnapshotV2::PreviewReady {
            root_draft_revision,
            workspace,
            identity_revision,
            intent_revision,
            candidate_revision,
            compiler_input_hash,
            semantic_intent_hash,
            compiled_plan_hash,
            candidate_ruleset_hash: persisted_ruleset_hash,
            candidate_draft_hash,
            external_channel_bindings,
            compiled_operations,
            request_evidence,
            route_decision,
            recipe_evidence,
            decision_binding_digest,
        } => {
            let expected_candidate_revision = u64::try_from(*compiled_operations)
                .ok()
                .and_then(|operations| root_draft_revision.checked_add(operations));
            if workspace.schema_version != 2
                || *identity_revision != INTENT_IDENTITY_REVISION
                || workspace.revision != *intent_revision
                || workspace.features.len() != 1
                || *root_draft_revision >= *candidate_revision
                || *candidate_revision != snapshot.draft.draft_revision
                || snapshot.draft.validated_revision != Some(*candidate_revision)
                || snapshot.draft.simulated_revision != Some(*candidate_revision)
                || !valid_hash(compiler_input_hash)
                || !valid_hash(semantic_intent_hash)
                || !valid_hash(compiled_plan_hash)
                || !valid_hash(persisted_ruleset_hash)
                || !valid_hash(candidate_draft_hash)
                || external_channel_bindings.is_empty()
                || *compiled_operations == 0
                || expected_candidate_revision != Some(*candidate_revision)
            {
                return Err(snapshot_error(
                    "preview-ready intent recipe state is inconsistent",
                ));
            }
            validate_persisted_evidence_chain(
                request_evidence,
                &snapshot.messages,
                &transcript,
                workspace,
                route_decision,
                recipe_evidence,
            )?;
            let actual_ruleset_hash = candidate_ruleset_hash(&snapshot.draft).map_err(|error| {
                snapshot_error(format!(
                    "persisted candidate RuleSet hashing failed {}: {}",
                    error.code, error.message
                ))
            })?;
            let actual_draft_hash = draft_state_hash(&snapshot.draft).map_err(|error| {
                snapshot_error(format!(
                    "persisted candidate Draft hashing failed {}: {}",
                    error.code, error.message
                ))
            })?;
            if persisted_ruleset_hash != &actual_ruleset_hash
                || candidate_draft_hash != &actual_draft_hash
            {
                return Err(snapshot_error(
                    "preview-ready intent state does not match its candidate Draft",
                ));
            }
            let expected_binding = preview_ready_binding_digest_v4(PreviewReadyBindingInputV4 {
                protocol_version: intent.protocol_version,
                context_fingerprint: &intent.context_fingerprint,
                root_draft_revision: *root_draft_revision,
                workspace,
                identity_revision: *identity_revision,
                intent_revision: *intent_revision,
                candidate_revision: *candidate_revision,
                compiler_input_hash,
                semantic_intent_hash,
                compiled_plan_hash,
                candidate_ruleset_hash: persisted_ruleset_hash,
                candidate_draft_hash,
                external_channel_bindings,
                compiled_operations: *compiled_operations,
                request_evidence,
                route_decision,
                recipe_evidence,
            })
            .map_err(|error| {
                snapshot_error(format!(
                    "persisted intent stage binding failed {}: {}",
                    error.code, error.message
                ))
            })?;
            validate_persisted_binding(decision_binding_digest, &expected_binding)?;
            Ok(())
        }
    }
}

fn validate_request_evidence_frontiers(
    request_evidence: &IntentRequestEvidenceChainV1,
    transcript: &IntentTranscriptV4,
) -> Result<(), SessionSnapshotError> {
    for entry in request_evidence.entries() {
        let (message_index, expected_tool) = match entry {
            IntentRequestEvidenceEntryV1::InitialHuman {
                transcript_message_index,
                ..
            } => (*transcript_message_index, INTERPRET_INTENT_CORE),
            IntentRequestEvidenceEntryV1::AcceptedResolution {
                transcript_message_index,
                ..
            } => (*transcript_message_index, RESOLVE_INTENT_DECISION),
        };
        let matching = transcript
            .turns
            .iter()
            .find(|turn| turn.human_message_index == message_index);
        if !matching.is_some_and(|turn| {
            turn.succeeded && turn.primary_tool.as_deref() == Some(expected_tool)
        }) {
            return Err(snapshot_error(
                "persisted request evidence is not bound to a successful transcript frontier",
            ));
        }
    }
    Ok(())
}

fn validate_persisted_evidence_chain(
    request_evidence: &IntentRequestEvidenceChainV1,
    messages: &[crate::llm::Message],
    transcript: &IntentTranscriptV4,
    workspace: &IntentWorkspaceV2,
    decision: &IntentRouteDecisionV2,
    evidence: &IntentRecipeEvidenceV4,
) -> Result<(), SessionSnapshotError> {
    request_evidence
        .validate_against_transcript(messages)
        .map_err(|error| {
            snapshot_error(format!(
                "persisted request evidence failed {}: {}",
                error.code, error.message
            ))
        })?;
    validate_request_evidence_frontiers(request_evidence, transcript)?;
    validate_persisted_private_study_room_decision_v4(decision).map_err(|error| {
        snapshot_error(format!(
            "persisted intent route decision failed {}: {}",
            error.code, error.message
        ))
    })?;
    evidence.validate().map_err(|error| {
        snapshot_error(format!(
            "persisted intent recipe evidence failed {}: {}",
            error.code, error.message
        ))
    })?;
    if evidence.core_semantic_digest() != decision.semantic_ir_digest() {
        return Err(snapshot_error(
            "persisted recipe evidence is not bound to its route decision",
        ));
    }
    let initial_head = request_evidence.initial_head().map_err(|error| {
        snapshot_error(format!(
            "persisted initial request evidence failed {}: {}",
            error.code, error.message
        ))
    })?;
    let initial_human_turn_digest =
        request_evidence
            .initial_human_turn_digest()
            .map_err(|error| {
                snapshot_error(format!(
                    "persisted source human evidence failed {}: {}",
                    error.code, error.message
                ))
            })?;
    if decision.request_evidence_hash() != Some(initial_head.as_str())
        || evidence.source_human_turn_digest() != initial_human_turn_digest
    {
        return Err(snapshot_error(
            "persisted route and recipe evidence do not match their human evidence chain",
        ));
    }
    validate_initial_semantics_replay(
        request_evidence,
        messages,
        transcript,
        workspace,
        decision,
        evidence,
        &initial_head,
    )?;
    Ok(())
}

fn validate_initial_semantics_replay(
    request_evidence: &IntentRequestEvidenceChainV1,
    messages: &[Message],
    transcript: &IntentTranscriptV4,
    workspace: &IntentWorkspaceV2,
    decision: &IntentRouteDecisionV2,
    evidence: &IntentRecipeEvidenceV4,
    initial_head: &str,
) -> Result<(), SessionSnapshotError> {
    let Some(IntentRequestEvidenceEntryV1::InitialHuman {
        transcript_message_index,
        expected_revision,
        ..
    }) = request_evidence.entries().first()
    else {
        return Err(snapshot_error(
            "persisted request evidence has no initial human entry",
        ));
    };
    let initial_index = usize::try_from(*transcript_message_index)
        .map_err(|_| snapshot_error("initial human transcript index overflowed"))?;
    let human = restored_human_text(
        messages
            .get(initial_index)
            .ok_or_else(|| snapshot_error("initial human transcript message is missing"))?,
    )?;
    let state = parse_intent_state_anchor(
        messages
            .get(initial_index.saturating_add(1))
            .ok_or_else(|| snapshot_error("initial intent state anchor is missing"))?,
    )?;
    validate_intent_state_anchor(&state)?;
    let turn = transcript
        .turns
        .iter()
        .find(|turn| turn.human_message_index == *transcript_message_index)
        .ok_or_else(|| snapshot_error("initial semantic transcript turn is missing"))?;
    if !turn.succeeded || turn.primary_tool.as_deref() != Some(INTERPRET_INTENT_CORE) {
        return Err(snapshot_error(
            "initial semantic transcript frontier did not succeed",
        ));
    }
    let arguments = turn
        .primary_arguments
        .as_deref()
        .ok_or_else(|| snapshot_error("initial Core arguments are missing"))?;
    let mut core = parse_interpret_intent_core(arguments).map_err(restored_semantics_error)?;
    core.validate_human_evidence(&human)
        .map_err(restored_semantics_error)?;
    if core.expected_revision() != *expected_revision
        || state.expected_revision != *expected_revision
    {
        return Err(snapshot_error(
            "initial Core revision does not match its human evidence",
        ));
    }
    let grounded_channel = deterministically_selected_option(&human, &state.available_channel_keys)
        .map(ExistingChannelKey);
    core.apply_human_grounding(&human, grounded_channel.as_ref());
    let adjudication =
        adjudicate_intent_core_v4(core, initial_head).map_err(restored_semantics_error)?;
    let IntentCoreAdjudicationV4::PrivateStudyRoom(selection) = adjudication else {
        return Err(snapshot_error(
            "initial Core no longer adjudicates to the persisted private study-room route",
        ));
    };
    if selection.decision() != decision {
        return Err(snapshot_error(
            "persisted route decision does not reproduce from the initial Core arguments",
        ));
    }
    let source_human_turn_digest = request_evidence
        .initial_human_turn_digest()
        .map_err(restored_semantics_error)?;
    let permit = if selection.detail_facets().is_empty() {
        if turn.detail_arguments.is_some() || !turn.detail_facets.is_empty() {
            return Err(snapshot_error(
                "default recipe extraction contains an unexpected detail frontier",
            ));
        }
        let reproduced = IntentRecipeEvidenceV4::deterministic_default(
            selection.semantic_ir_digest(),
            source_human_turn_digest,
        )
        .map_err(restored_semantics_error)?;
        if &reproduced != evidence {
            return Err(snapshot_error(
                "default recipe evidence does not reproduce from the initial Core",
            ));
        }
        selection.finalize(None).map_err(restored_semantics_error)?
    } else {
        if turn.detail_facets.as_slice() != selection.detail_facets() {
            return Err(snapshot_error(
                "detail state facets do not reproduce from the initial Core",
            ));
        }
        let detail_arguments = turn
            .detail_arguments
            .as_deref()
            .ok_or_else(|| snapshot_error("persisted detail arguments are missing"))?;
        let details = parse_private_study_room_details_for_serving(
            detail_arguments,
            selection.detail_facets(),
            selection.expected_revision(),
            selection.semantic_ir_digest(),
            &human,
        )
        .map_err(restored_semantics_error)?;
        let detail_result_digest = selection
            .details_digest(source_human_turn_digest, &details)
            .map_err(restored_semantics_error)?;
        let reproduced = IntentRecipeEvidenceV4::model_detail(
            selection.semantic_ir_digest(),
            source_human_turn_digest,
            selection.detail_facets(),
            detail_result_digest,
        )
        .map_err(restored_semantics_error)?;
        if &reproduced != evidence {
            return Err(snapshot_error(
                "detail recipe evidence does not reproduce from the preserved model result",
            ));
        }
        selection
            .finalize(Some(details))
            .map_err(restored_semantics_error)?
    };
    let context = IntentResolutionContext::from_channel_bindings(
        state
            .available_channel_keys
            .iter()
            .cloned()
            .map(ExistingChannelKey),
    );
    let (replayed_decision, prepared) =
        permit.prepare(&context).map_err(restored_semantics_error)?;
    if &replayed_decision != decision {
        return Err(snapshot_error(
            "prepared route decision differs from its persisted decision",
        ));
    }
    let replayed_workspace = match prepared {
        PreparedIntentWorkspaceV2::NeedsInput { workspace, .. }
        | PreparedIntentWorkspaceV2::Resolved { workspace, .. } => workspace,
    };
    let initial_workspace = request_evidence
        .initial_workspace(workspace)
        .map_err(restored_semantics_error)?;
    if replayed_workspace != initial_workspace {
        return Err(snapshot_error(
            "typed workspace does not reproduce from the preserved Core and detail arguments",
        ));
    }
    Ok(())
}

fn restored_semantics_error(error: StructuredError) -> SessionSnapshotError {
    snapshot_error(format!(
        "persisted model semantics failed deterministic replay {}: {}",
        error.code, error.message
    ))
}

fn validate_persisted_binding(binding: &str, expected: &str) -> Result<(), SessionSnapshotError> {
    if valid_hash(binding) && binding == expected {
        Ok(())
    } else {
        Err(snapshot_error(
            "persisted intent stage does not match its route decision binding",
        ))
    }
}

fn valid_hash(value: &str) -> bool {
    is_lowercase_sha256_hex(value)
}

fn restored_state_error(error: StructuredError) -> SessionSnapshotError {
    snapshot_error(format!(
        "persisted typed intent failed deterministic reproduction {}: {}",
        error.code, error.message
    ))
}
