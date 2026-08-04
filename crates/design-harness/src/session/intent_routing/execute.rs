use serde_json::json;

use crate::errors::StructuredError;
use crate::intent::{
    apply_existing_channel_decision, candidate_ruleset_hash, compile_intent, draft_state_hash,
    prepare_intent_candidate, prepare_intent_workspace, upgrade_working_draft_to_validated_preview,
    verify_existing_intent_candidate, verify_outcome_only_finalization, IntentRequestedOutcome,
    IntentWorkspaceV2, MissingDecision, PreparedIntentWorkspaceV2, ValidatedIntentV2,
    INTENT_IDENTITY_REVISION,
};
use crate::turn::{
    parse_interpret_intent_core_for_serving,
    parse_private_study_room_details_for_active_serving_with_parameters,
    parse_resolve_intent_decision, IntentRecipeDetailExpectationV4, PrivateStudyRoomDetailTicketV4,
};

use super::super::DesignSession;
use super::adjudicate::{
    adjudicate_intent_core_v4, IntentCoreAdjudicationV4, PrivateStudyRoomPermitV2,
    PrivateStudyRoomSelectionV4,
};
use super::decision::{IntentRouteDecisionKindV2, IntentRouteDecisionV2};
use super::evidence::IntentRecipeEvidenceV4;
use super::grounding::{deterministically_selected_option, unambiguously_selects_option};
use super::request_evidence::{
    AcceptedIntentResolutionV1, AcceptedResolutionEvidenceInputV1, IntentRequestEvidenceChainV1,
    TerminalOutcomeFinalizationEvidenceInputV1,
};
use super::state::{
    intent_error, IntentFallbackV1, IntentRecipeRuntime, IntentRecipeStageSnapshotV2,
};
use super::state_binding::{
    awaiting_decision_binding_digest_v4, preview_ready_binding_digest_v4,
    AwaitingDecisionBindingInputV4, PreviewReadyBindingInputV4,
};
use super::transcript_binding::routed_presentation_digest;
use super::INTENT_RECIPE_PROTOCOL_VERSION_V4;

pub(super) enum IntentCoreExecutionV4 {
    Complete(Box<IntentTurnSuccess>),
    NeedsDetails {
        selection: Box<PrivateStudyRoomSelectionV4>,
        request_evidence: IntentRequestEvidenceChainV1,
        detail_ticket: PrivateStudyRoomDetailTicketV4,
    },
}

pub(super) enum IntentTurnSuccess {
    NeedsInput {
        question: String,
        revision: u64,
        options: Vec<String>,
    },
    Ready {
        summary: String,
        intent_revision: u64,
        draft_revision: u64,
        semantic_intent_hash: String,
        compiled_plan_hash: String,
        candidate_ruleset_hash: String,
        compiled_operations: usize,
    },
    Routed {
        fallback: IntentFallbackV1,
        decision: IntentRouteDecisionV2,
    },
}

impl IntentTurnSuccess {
    pub(super) fn tool_result(&self) -> String {
        let value = match self {
            Self::NeedsInput {
                revision, options, ..
            } => json!({
                "ok": true,
                "status": "awaiting_decision",
                "revision": revision,
                "available_channel_keys": options,
            }),
            Self::Ready {
                intent_revision,
                draft_revision,
                semantic_intent_hash,
                compiled_plan_hash,
                candidate_ruleset_hash,
                compiled_operations,
                ..
            } => json!({
                "ok": true,
                "status": "preview_ready",
                "intent_revision": intent_revision,
                "draft_revision": draft_revision,
                "semantic_intent_hash": semantic_intent_hash,
                "compiled_plan_hash": compiled_plan_hash,
                "candidate_ruleset_hash": candidate_ruleset_hash,
                "compiled_operations": compiled_operations,
            }),
            Self::Routed { fallback, decision } => json!({
                "ok": true,
                "status": "routed",
                "fallback_kind": fallback.kind().as_str(),
                "adjudication_digest": decision.adjudication_digest(),
                "presentation_digest": routed_presentation_digest(fallback.response()),
            }),
        };
        value.to_string()
    }
}

impl<C> DesignSession<C> {
    pub(super) fn record_intent_extraction_failure(&mut self) {
        self.observability.intent_extraction_failures = self
            .observability
            .intent_extraction_failures
            .saturating_add(1);
    }

    fn expected_intent_revision(&self) -> Result<u64, StructuredError> {
        self.intent_recipe
            .as_ref()
            .map(|runtime| runtime.expected_revision(self.draft.draft_revision))
            .ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })
    }

    fn validate_expected_revision(&mut self, actual: u64) -> Result<(), StructuredError> {
        let expected = self.expected_intent_revision()?;
        if actual == expected {
            return Ok(());
        }
        self.observability.intent_stale_revision_rejections = self
            .observability
            .intent_stale_revision_rejections
            .saturating_add(1);
        Err(intent_error(
            "STALE_INTENT_WORKSPACE_REVISION",
            "intent.expected_revision",
            format!("Intent revision {actual} does not match the current revision {expected}"),
            format!("Retry with expected_revision {expected}"),
        ))
    }

    fn set_awaiting_decision(
        &mut self,
        root_draft_revision: u64,
        workspace: IntentWorkspaceV2,
        active_decision: MissingDecision,
        request_evidence: IntentRequestEvidenceChainV1,
        route_decision: IntentRouteDecisionV2,
        recipe_evidence: IntentRecipeEvidenceV4,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let options = active_decision.options.clone();
        let question = active_decision.question.clone();
        let revision = workspace.revision;
        let root_draft_hash = draft_state_hash(&self.draft)?;
        let context_fingerprint = self
            .intent_recipe
            .as_ref()
            .map(|runtime| runtime.snapshot.context_fingerprint.clone())
            .ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })?;
        let decision_binding_digest =
            awaiting_decision_binding_digest_v4(AwaitingDecisionBindingInputV4 {
                protocol_version: INTENT_RECIPE_PROTOCOL_VERSION_V4,
                context_fingerprint: &context_fingerprint,
                root_draft_revision,
                root_draft_hash: &root_draft_hash,
                workspace: &workspace,
                active_decision: &active_decision,
                request_evidence: &request_evidence,
                route_decision: &route_decision,
                recipe_evidence: &recipe_evidence,
            })?;
        let runtime = self.intent_recipe.as_mut().ok_or_else(|| {
            intent_error(
                "INTENT_SESSION_DISABLED",
                "intent.session",
                "Intent recipe mode is not enabled",
                "Construct the session with resource bindings",
            )
        })?;
        runtime.snapshot.stage = IntentRecipeStageSnapshotV2::AwaitingDecision {
            root_draft_revision,
            workspace,
            active_decision,
            request_evidence,
            root_draft_hash,
            route_decision,
            recipe_evidence,
            decision_binding_digest,
        };
        Ok(IntentTurnSuccess::NeedsInput {
            question,
            revision,
            options,
        })
    }

    async fn prepare_and_commit_intent(
        &mut self,
        workspace: IntentWorkspaceV2,
        intent: ValidatedIntentV2,
        request_evidence: IntentRequestEvidenceChainV1,
        route_decision: IntentRouteDecisionV2,
        recipe_evidence: IntentRecipeEvidenceV4,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let (bindings, context_fingerprint) = self
            .intent_recipe
            .as_ref()
            .map(|runtime| {
                (
                    runtime.bindings.clone(),
                    runtime.snapshot.context_fingerprint.clone(),
                )
            })
            .ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })?;
        let root_revision = self.draft.draft_revision;
        self.observability.intent_compile_attempts =
            self.observability.intent_compile_attempts.saturating_add(1);
        let prepared = match prepare_intent_candidate(&self.draft, &intent, &bindings).await {
            Ok(prepared) => prepared,
            Err(error) => {
                self.record_intent_rollback(&error);
                return Err(error);
            }
        };
        self.observability.intent_compile_successes = self
            .observability
            .intent_compile_successes
            .saturating_add(1);
        let compiler_input_hash = prepared.compilation().manifest.compiler_input_hash.clone();
        let semantic_intent_hash = prepared.compilation().manifest.semantic_intent_hash.clone();
        let compiled_plan_hash = prepared.compilation().manifest.compiled_plan_hash.clone();
        let identity_revision = prepared.compilation().manifest.identity_revision;
        if identity_revision != INTENT_IDENTITY_REVISION {
            return Err(intent_error(
                "INTENT_IDENTITY_REVISION_MISMATCH",
                "intent.compiler.identity_revision",
                "The compiler manifest does not use the active identity revision",
                "Compile with the active V4 identity contract",
            ));
        }
        let external_channel_bindings = prepared
            .compilation()
            .manifest
            .external_channel_bindings
            .clone();
        let compiled_operations = prepared.execution().compiled_operations;
        let intent_revision = intent.revision();
        let candidate_revision = prepared.execution().candidate_revision;
        let candidate_ruleset_hash = candidate_ruleset_hash(prepared.candidate())?;
        let candidate_draft_hash = draft_state_hash(prepared.candidate())?;
        let decision_binding_digest =
            preview_ready_binding_digest_v4(PreviewReadyBindingInputV4 {
                protocol_version: INTENT_RECIPE_PROTOCOL_VERSION_V4,
                context_fingerprint: &context_fingerprint,
                root_draft_revision: root_revision,
                workspace: &workspace,
                identity_revision,
                intent_revision,
                candidate_revision,
                compiler_input_hash: &compiler_input_hash,
                semantic_intent_hash: &semantic_intent_hash,
                compiled_plan_hash: &compiled_plan_hash,
                candidate_ruleset_hash: &candidate_ruleset_hash,
                candidate_draft_hash: &candidate_draft_hash,
                external_channel_bindings: &external_channel_bindings,
                compiled_operations,
                request_evidence: &request_evidence,
                route_decision: &route_decision,
                recipe_evidence: &recipe_evidence,
            })?;
        match prepared.commit(&mut self.draft) {
            Ok(_) => {}
            Err(error) => {
                self.record_intent_rollback(&error);
                return Err(error);
            }
        }
        self.observability.intent_commits = self.observability.intent_commits.saturating_add(1);
        self.observability.intent_compiled_operations = self
            .observability
            .intent_compiled_operations
            .saturating_add(compiled_operations);
        let runtime = self.intent_recipe.as_mut().ok_or_else(|| {
            intent_error(
                "INTENT_SESSION_DISABLED",
                "intent.session",
                "Intent recipe mode is not enabled",
                "Construct the session with resource bindings",
            )
        })?;
        runtime.snapshot.stage = IntentRecipeStageSnapshotV2::PreviewReady {
            root_draft_revision: root_revision,
            workspace,
            identity_revision,
            intent_revision,
            candidate_revision,
            compiler_input_hash,
            semantic_intent_hash: semantic_intent_hash.clone(),
            compiled_plan_hash: compiled_plan_hash.clone(),
            candidate_ruleset_hash: candidate_ruleset_hash.clone(),
            candidate_draft_hash,
            external_channel_bindings,
            compiled_operations,
            request_evidence,
            route_decision,
            recipe_evidence,
            decision_binding_digest,
        };
        Ok(IntentTurnSuccess::Ready {
            summary: format!(
                "Prepared, validated, simulated, and previewed the private study room design at Draft revision {candidate_revision}"
            ),
            intent_revision,
            draft_revision: candidate_revision,
            semantic_intent_hash,
            compiled_plan_hash,
            candidate_ruleset_hash,
            compiled_operations,
        })
    }

    async fn try_finalize_existing_preview(
        &mut self,
        current_workspace: &IntentWorkspaceV2,
        current_intent: &ValidatedIntentV2,
        standalone_request_evidence: &IntentRequestEvidenceChainV1,
        standalone_route_decision: &IntentRouteDecisionV2,
        standalone_recipe_evidence: &IntentRecipeEvidenceV4,
    ) -> Result<Option<IntentTurnSuccess>, StructuredError> {
        let Some(stage) = self
            .intent_recipe
            .as_ref()
            .map(|runtime| runtime.snapshot.stage.clone())
        else {
            return Ok(None);
        };
        let IntentRecipeStageSnapshotV2::PreviewReady {
            root_draft_revision,
            workspace: persisted_workspace,
            identity_revision,
            intent_revision: persisted_intent_revision,
            candidate_revision,
            compiler_input_hash: persisted_compiler_input_hash,
            semantic_intent_hash: persisted_semantic_intent_hash,
            compiled_plan_hash: persisted_compiled_plan_hash,
            candidate_ruleset_hash: persisted_candidate_ruleset_hash,
            candidate_draft_hash: persisted_candidate_draft_hash,
            external_channel_bindings: persisted_external_channel_bindings,
            compiled_operations,
            request_evidence: persisted_request_evidence,
            route_decision: persisted_route_decision,
            recipe_evidence: persisted_recipe_evidence,
            ..
        } = stage
        else {
            return Ok(None);
        };
        if persisted_workspace.requested_outcome != IntentRequestedOutcome::WorkingDraft
            || current_workspace.requested_outcome != IntentRequestedOutcome::ValidatedPreview
            || current_intent.requested_outcome() != IntentRequestedOutcome::ValidatedPreview
        {
            return Ok(None);
        }
        self.observability.intent_compile_attempts =
            self.observability.intent_compile_attempts.saturating_add(1);
        let result = async {
            let (bindings, context, context_fingerprint) = self
                .intent_recipe
                .as_ref()
                .map(|runtime| {
                    (
                        runtime.bindings.clone(),
                        runtime.resolution_context(),
                        runtime.snapshot.context_fingerprint.clone(),
                    )
                })
                .ok_or_else(|| {
                    intent_error(
                        "INTENT_SESSION_DISABLED",
                        "intent.session",
                        "Intent recipe mode is not enabled",
                        "Construct the session with resource bindings",
                    )
                })?;
            standalone_request_evidence.validate_against_transcript(&self.messages)?;
            if standalone_request_evidence.entries().len() != 1
                || standalone_request_evidence.terminal_outcome_finalization_count() != 0
                || standalone_request_evidence.initial_expected_revision()? != candidate_revision
                || standalone_request_evidence.initial_head()?
                    != standalone_request_evidence.head()
                || standalone_route_decision.request_evidence_hash()
                    != Some(standalone_request_evidence.head())
                || standalone_recipe_evidence.core_semantic_digest()
                    != standalone_route_decision.semantic_ir_digest()
                || standalone_recipe_evidence.source_human_turn_digest()
                    != standalone_request_evidence.initial_human_turn_digest()?
            {
                return Err(intent_error(
                    "INTENT_OUTCOME_FINALIZATION_EVIDENCE_MISMATCH",
                    "intent.request_evidence",
                    "The outcome finalization request is not bound to one current private intent turn",
                    "Retry the finalization from the current preview-ready Draft",
                ));
            }
            let PreparedIntentWorkspaceV2::Resolved {
                workspace: normalized_persisted_workspace,
                intent: persisted_intent,
            } = prepare_intent_workspace(persisted_workspace.clone(), &context)?
            else {
                return Err(intent_error(
                    "INTENT_OUTCOME_FINALIZATION_SOURCE_UNRESOLVED",
                    "intent.workspace",
                    "The committed working Draft workspace no longer resolves",
                    "Start a new intent from the current canonical Draft",
                ));
            };
            let persisted_compilation = compile_intent(&persisted_intent)?;
            let actual_candidate_ruleset_hash = candidate_ruleset_hash(&self.draft)?;
            let actual_candidate_draft_hash = draft_state_hash(&self.draft)?;
            let expected_candidate_revision = u64::try_from(compiled_operations)
                .ok()
                .and_then(|operations| root_draft_revision.checked_add(operations));
            if normalized_persisted_workspace != persisted_workspace
                || persisted_intent.revision() != persisted_intent_revision
                || identity_revision != INTENT_IDENTITY_REVISION
                || persisted_compilation.manifest.identity_revision != identity_revision
                || persisted_compilation.manifest.compiler_input_hash
                    != persisted_compiler_input_hash
                || persisted_compilation.manifest.semantic_intent_hash
                    != persisted_semantic_intent_hash
                || persisted_compilation.manifest.compiled_plan_hash
                    != persisted_compiled_plan_hash
                || persisted_compilation.manifest.external_channel_bindings
                    != persisted_external_channel_bindings
                || persisted_compilation.requirements.len() != compiled_operations
                || expected_candidate_revision != Some(candidate_revision)
                || self.draft.draft_revision != candidate_revision
                || self.draft.validated_revision != Some(candidate_revision)
                || self.draft.simulated_revision != Some(candidate_revision)
                || actual_candidate_ruleset_hash != persisted_candidate_ruleset_hash
                || actual_candidate_draft_hash != persisted_candidate_draft_hash
            {
                return Err(intent_error(
                    "INTENT_OUTCOME_FINALIZATION_SOURCE_MISMATCH",
                    "intent.session.stage",
                    "The committed working Draft no longer matches its compiler and candidate identities",
                    "Start a new intent from the current canonical Draft",
                ));
            }
            let standalone_compilation = compile_intent(current_intent)?;
            let PreparedIntentWorkspaceV2::Resolved {
                workspace: finalized_workspace,
                intent: finalized_intent,
            } = upgrade_working_draft_to_validated_preview(
                &persisted_workspace,
                persisted_workspace.revision,
                &context,
            )?
            else {
                return Err(intent_error(
                    "INTENT_OUTCOME_FINALIZATION_UNRESOLVED",
                    "intent.workspace",
                    "The outcome-only workspace transition no longer resolves",
                    "Start a new intent from the current canonical Draft",
                ));
            };
            let finalized_compilation = compile_intent(&finalized_intent)?;
            verify_outcome_only_finalization(
                &persisted_compilation,
                &standalone_compilation,
                &finalized_compilation,
            )?;
            let verified = verify_existing_intent_candidate(
                &self.draft,
                &finalized_intent,
                &persisted_compilation,
                &bindings,
            )
            .await?;
            let finalized_compiler_input_hash =
                verified.compilation().manifest.compiler_input_hash.clone();
            let finalized_semantic_intent_hash =
                verified.compilation().manifest.semantic_intent_hash.clone();
            let finalized_compiled_plan_hash =
                verified.compilation().manifest.compiled_plan_hash.clone();
            if verified.candidate() != &self.draft
                || verified.candidate().draft_revision != candidate_revision
                || verified.compiled_operations() != compiled_operations
                || verified.compilation().manifest.identity_revision != identity_revision
                || verified.compilation().manifest.external_channel_bindings
                    != persisted_external_channel_bindings
                || finalized_compiled_plan_hash != persisted_compiled_plan_hash
            {
                return Err(intent_error(
                    "INTENT_OUTCOME_FINALIZATION_CANDIDATE_CHANGED",
                    "intent.candidate",
                    "Finalization verification changed the committed candidate footprint",
                    "Start a new intent from the current canonical Draft",
                ));
            }
            let transcript_message_index = u64::try_from(
                self.current_human_message_index.ok_or_else(|| {
                    intent_error(
                        "INTENT_HUMAN_MESSAGE_MISSING",
                        "intent.request_evidence",
                        "The finalization request is not present in the append-only transcript",
                        "Retry the finalization as a new intent turn",
                    )
                })?,
            )
            .map_err(|_| {
                intent_error(
                    "INTENT_HUMAN_MESSAGE_INDEX_OVERFLOW",
                    "intent.request_evidence",
                    "The finalization transcript index exceeds the supported range",
                    "Start a compacted intent session",
                )
            })?;
            let standalone_recipe_evidence_digest =
                standalone_recipe_evidence.binding_digest()?;
            let mut finalized_request_evidence = persisted_request_evidence;
            finalized_request_evidence.append_terminal_outcome_finalization(
                &self.messages,
                TerminalOutcomeFinalizationEvidenceInputV1 {
                    transcript_message_index,
                    expected_draft_revision: candidate_revision,
                    prior_workspace_revision: persisted_workspace.revision,
                    next_workspace_revision: finalized_workspace.revision,
                    standalone_request_evidence_digest: standalone_request_evidence.head(),
                    standalone_adjudication_digest: standalone_route_decision
                        .adjudication_digest(),
                    standalone_recipe_evidence_digest: &standalone_recipe_evidence_digest,
                },
            )?;
            let decision_binding_digest =
                preview_ready_binding_digest_v4(PreviewReadyBindingInputV4 {
                    protocol_version: INTENT_RECIPE_PROTOCOL_VERSION_V4,
                    context_fingerprint: &context_fingerprint,
                    root_draft_revision,
                    workspace: &finalized_workspace,
                    identity_revision,
                    intent_revision: finalized_intent.revision(),
                    candidate_revision,
                    compiler_input_hash: &finalized_compiler_input_hash,
                    semantic_intent_hash: &finalized_semantic_intent_hash,
                    compiled_plan_hash: &finalized_compiled_plan_hash,
                    candidate_ruleset_hash: &persisted_candidate_ruleset_hash,
                    candidate_draft_hash: &persisted_candidate_draft_hash,
                    external_channel_bindings: &persisted_external_channel_bindings,
                    compiled_operations,
                    request_evidence: &finalized_request_evidence,
                    route_decision: &persisted_route_decision,
                    recipe_evidence: &persisted_recipe_evidence,
                })?;
            let runtime = self.intent_recipe.as_mut().ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })?;
            runtime.snapshot.stage = IntentRecipeStageSnapshotV2::PreviewReady {
                root_draft_revision,
                workspace: finalized_workspace,
                identity_revision,
                intent_revision: finalized_intent.revision(),
                candidate_revision,
                compiler_input_hash: finalized_compiler_input_hash,
                semantic_intent_hash: finalized_semantic_intent_hash.clone(),
                compiled_plan_hash: finalized_compiled_plan_hash.clone(),
                candidate_ruleset_hash: persisted_candidate_ruleset_hash.clone(),
                candidate_draft_hash: persisted_candidate_draft_hash,
                external_channel_bindings: persisted_external_channel_bindings,
                compiled_operations,
                request_evidence: finalized_request_evidence,
                route_decision: persisted_route_decision,
                recipe_evidence: persisted_recipe_evidence,
                decision_binding_digest,
            };
            Ok(IntentTurnSuccess::Ready {
                summary: format!(
                    "Finalized the existing private study room candidate as a validated preview at Draft revision {candidate_revision}"
                ),
                intent_revision: finalized_intent.revision(),
                draft_revision: candidate_revision,
                semantic_intent_hash: finalized_semantic_intent_hash,
                compiled_plan_hash: finalized_compiled_plan_hash,
                candidate_ruleset_hash: persisted_candidate_ruleset_hash,
                compiled_operations,
            })
        }
        .await;
        match result {
            Ok(success) => {
                self.observability.intent_compile_successes = self
                    .observability
                    .intent_compile_successes
                    .saturating_add(1);
                self.observability.intent_finalizations =
                    self.observability.intent_finalizations.saturating_add(1);
                Ok(Some(success))
            }
            Err(error) => {
                self.record_intent_rollback(&error);
                Err(error)
            }
        }
    }

    fn record_intent_rollback(&mut self, error: &StructuredError) {
        self.observability.intent_rollbacks = self.observability.intent_rollbacks.saturating_add(1);
        if error.code.contains("CONFLICT") || error.code == "INTENT_CANDIDATE_STALE" {
            self.observability.intent_conflicts =
                self.observability.intent_conflicts.saturating_add(1);
        }
    }

    pub(super) async fn execute_intent_core(
        &mut self,
        arguments: &str,
        human_message: &str,
    ) -> Result<IntentCoreExecutionV4, StructuredError> {
        self.observability.intent_route_calls =
            self.observability.intent_route_calls.saturating_add(1);
        let expected_revision = self.expected_intent_revision()?;
        let mut core =
            parse_interpret_intent_core_for_serving(arguments, human_message, expected_revision)
                .inspect_err(|_| {
                    self.record_intent_extraction_failure();
                })?;
        self.validate_expected_revision(core.expected_revision())?;
        let transcript_message_index =
            u64::try_from(self.current_human_message_index.ok_or_else(|| {
                intent_error(
                    "INTENT_HUMAN_MESSAGE_MISSING",
                    "intent.request_evidence",
                    "The current human message is not present in the append-only transcript",
                    "Start a new intent turn with the human request",
                )
            })?)
            .map_err(|_| {
                intent_error(
                    "INTENT_HUMAN_MESSAGE_INDEX_OVERFLOW",
                    "intent.request_evidence",
                    "The current human transcript index exceeds the supported range",
                    "Start a compacted intent session",
                )
            })?;
        let channel_options = self
            .intent_recipe
            .as_ref()
            .map(IntentRecipeRuntime::resolution_context)
            .ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })?
            .channel_bindings
            .into_iter()
            .map(|key| key.0)
            .collect::<Vec<_>>();
        let grounded_channel = deterministically_selected_option(human_message, &channel_options)
            .map(crate::intent::ExistingChannelKey);
        let detail_ticket = core
            .apply_human_grounding_with_detail_ticket(human_message, grounded_channel.as_ref())
            .inspect_err(|_| {
                self.record_intent_extraction_failure();
            })?;
        let request_evidence = IntentRequestEvidenceChainV1::from_initial_human(
            &self.messages,
            transcript_message_index,
            core.expected_revision(),
        )?;
        let initial_head = request_evidence.initial_head()?;
        let source_human_turn_digest = request_evidence.initial_human_turn_digest()?.to_string();
        let adjudication = adjudicate_intent_core_v4(core, &initial_head).inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        match adjudication {
            IntentCoreAdjudicationV4::PrivateStudyRoom(selection) => {
                if selection.detail_facets() != detail_ticket.facets() {
                    return Err(intent_error(
                        "INCONSISTENT_INTENT_DETAIL_TICKET",
                        "intent.details",
                        "The grounded detail ticket does not match the adjudicated Core",
                        "Derive the detail ticket and Core facets from the same human analysis",
                    ));
                }
                if selection.detail_facets().is_empty() {
                    let recipe_evidence = IntentRecipeEvidenceV4::deterministic_default(
                        selection.semantic_ir_digest(),
                        &source_human_turn_digest,
                    )?;
                    let permit = selection.finalize(None).inspect_err(|_| {
                        self.record_intent_extraction_failure();
                    })?;
                    self.complete_private_study_room(permit, recipe_evidence, request_evidence)
                        .await
                        .map(|success| IntentCoreExecutionV4::Complete(Box::new(success)))
                } else {
                    Ok(IntentCoreExecutionV4::NeedsDetails {
                        selection,
                        request_evidence,
                        detail_ticket,
                    })
                }
            }
            IntentCoreAdjudicationV4::TypedPlanner(permit) => {
                let (reason, _, decision, response) = permit.into_parts();
                self.accept_fallback(
                    IntentFallbackV1::TypedPlanner { reason, response },
                    decision,
                )
                .map(|success| IntentCoreExecutionV4::Complete(Box::new(success)))
            }
            IntentCoreAdjudicationV4::Terminal(permit) => {
                let (decision, response) = permit.into_parts();
                let fallback = terminal_fallback(&decision, response)?;
                self.accept_fallback(fallback, decision)
                    .map(|success| IntentCoreExecutionV4::Complete(Box::new(success)))
            }
        }
    }

    pub(super) async fn execute_intent_details(
        &mut self,
        selection: Box<PrivateStudyRoomSelectionV4>,
        request_evidence: IntentRequestEvidenceChainV1,
        arguments: &str,
        human_message: &str,
        detail_expectations: &[IntentRecipeDetailExpectationV4],
        detail_parameters: &serde_json::Value,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let expected_revision = selection.expected_revision();
        let core_semantic_digest = selection.semantic_ir_digest().to_string();
        let source_human_turn_digest = request_evidence.initial_human_turn_digest()?.to_string();
        let detail_facets = selection.detail_facets().to_vec();
        let details = parse_private_study_room_details_for_active_serving_with_parameters(
            arguments,
            &detail_facets,
            detail_expectations,
            detail_parameters,
            expected_revision,
            &core_semantic_digest,
            human_message,
        )
        .inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        let detail_result_digest = selection.details_digest(&source_human_turn_digest, &details)?;
        let recipe_evidence = IntentRecipeEvidenceV4::model_detail(
            &core_semantic_digest,
            &source_human_turn_digest,
            &detail_facets,
            detail_result_digest,
        )?;
        let permit = selection.finalize(Some(details)).inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        self.complete_private_study_room(permit, recipe_evidence, request_evidence)
            .await
    }

    async fn complete_private_study_room(
        &mut self,
        permit: PrivateStudyRoomPermitV2,
        recipe_evidence: IntentRecipeEvidenceV4,
        request_evidence: IntentRequestEvidenceChainV1,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let context = self
            .intent_recipe
            .as_ref()
            .map(IntentRecipeRuntime::resolution_context)
            .ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })?;
        let (route_decision, prepared) = match permit.prepare(&context) {
            Ok(result) => result,
            Err(error) => {
                self.record_intent_extraction_failure();
                return Err(error);
            }
        };
        self.observability.intent_proposal_acceptances = self
            .observability
            .intent_proposal_acceptances
            .saturating_add(1);
        match prepared {
            PreparedIntentWorkspaceV2::NeedsInput {
                workspace,
                decisions,
            } => {
                let decision = exactly_one_decision(decisions)?;
                self.set_awaiting_decision(
                    self.draft.draft_revision,
                    workspace,
                    decision,
                    request_evidence,
                    route_decision,
                    recipe_evidence,
                )
            }
            PreparedIntentWorkspaceV2::Resolved { workspace, intent } => {
                if let Some(success) = self
                    .try_finalize_existing_preview(
                        &workspace,
                        &intent,
                        &request_evidence,
                        &route_decision,
                        &recipe_evidence,
                    )
                    .await?
                {
                    return Ok(success);
                }
                self.prepare_and_commit_intent(
                    workspace,
                    intent,
                    request_evidence,
                    route_decision,
                    recipe_evidence,
                )
                .await
            }
        }
    }

    fn accept_fallback(
        &mut self,
        fallback: IntentFallbackV1,
        decision: IntentRouteDecisionV2,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        *self
            .observability
            .intent_fallback_routes
            .entry(fallback.kind().as_str().to_string())
            .or_default() += 1;
        Ok(IntentTurnSuccess::Routed { fallback, decision })
    }

    pub(super) async fn execute_intent_resolution(
        &mut self,
        arguments: &str,
        human_message: &str,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let input = parse_resolve_intent_decision(arguments).inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        self.validate_expected_revision(input.expected_revision)?;
        let (
            root_draft_revision,
            workspace,
            active_decision,
            mut request_evidence,
            route_decision,
            recipe_evidence,
        ) = match self
            .intent_recipe
            .as_ref()
            .map(|runtime| runtime.snapshot.stage.clone())
        {
            Some(IntentRecipeStageSnapshotV2::AwaitingDecision {
                root_draft_revision,
                workspace,
                active_decision,
                request_evidence,
                route_decision,
                recipe_evidence,
                ..
            }) => (
                root_draft_revision,
                workspace,
                active_decision,
                request_evidence,
                route_decision,
                recipe_evidence,
            ),
            _ => {
                return Err(intent_error(
                    "INTENT_DECISION_NOT_PENDING",
                    "intent.session.stage",
                    "There is no active intent decision",
                    "Route a new user request instead",
                ));
            }
        };
        if !unambiguously_selects_option(
            human_message,
            input.channel.as_str(),
            &active_decision.options,
        ) {
            return Err(intent_error(
                "UNGROUNDED_INTENT_DECISION_EVIDENCE",
                "intent.decision.channel",
                "The selected channel is not the one unambiguous active option named in the human reply",
                "Name one active channel key or its unique normalized display form",
            ));
        }
        let context = self
            .intent_recipe
            .as_ref()
            .map(IntentRecipeRuntime::resolution_context)
            .ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })?;
        let prepared = apply_existing_channel_decision(
            &workspace,
            input.expected_revision,
            input.channel.clone(),
            &context,
        )?;
        let transcript_message_index =
            u64::try_from(self.current_human_message_index.ok_or_else(|| {
                intent_error(
                    "INTENT_HUMAN_MESSAGE_MISSING",
                    "intent.request_evidence",
                    "The current clarification is not present in the append-only transcript",
                    "Start a new intent turn with the clarification reply",
                )
            })?)
            .map_err(|_| {
                intent_error(
                    "INTENT_HUMAN_MESSAGE_INDEX_OVERFLOW",
                    "intent.request_evidence",
                    "The clarification transcript index exceeds the supported range",
                    "Start a compacted intent session",
                )
            })?;
        let turn_index = u64::try_from(request_evidence.entries().len()).map_err(|_| {
            intent_error(
                "INTENT_REQUEST_EVIDENCE_OVERFLOW",
                "intent.request_evidence",
                "The accepted request evidence chain exceeds the supported range",
                "Start a new intent session",
            )
        })?;
        request_evidence.append_accepted_resolution(
            &self.messages,
            AcceptedResolutionEvidenceInputV1 {
                turn_index,
                transcript_message_index,
                expected_revision: input.expected_revision,
                decision_id: &active_decision.id,
                decision_path: &active_decision.path,
                active_options: &active_decision.options,
                accepted_typed_value: AcceptedIntentResolutionV1::ExistingChannel(
                    input.channel.0.clone(),
                ),
            },
        )?;
        self.observability.intent_resolution_acceptances = self
            .observability
            .intent_resolution_acceptances
            .saturating_add(1);
        match prepared {
            PreparedIntentWorkspaceV2::NeedsInput {
                workspace,
                decisions,
            } => {
                let decision = exactly_one_decision(decisions)?;
                self.set_awaiting_decision(
                    root_draft_revision,
                    workspace,
                    decision,
                    request_evidence,
                    route_decision,
                    recipe_evidence,
                )
            }
            PreparedIntentWorkspaceV2::Resolved { workspace, intent } => {
                self.prepare_and_commit_intent(
                    workspace,
                    intent,
                    request_evidence,
                    route_decision,
                    recipe_evidence,
                )
                .await
            }
        }
    }
}

fn terminal_fallback(
    decision: &IntentRouteDecisionV2,
    response: String,
) -> Result<IntentFallbackV1, StructuredError> {
    match decision.kind() {
        IntentRouteDecisionKindV2::CapabilityGap => Ok(IntentFallbackV1::CapabilityGap {
            capabilities: decision
                .blockers()
                .iter()
                .map(|blocker| blocker.id.as_str().to_string())
                .collect(),
            response,
        }),
        IntentRouteDecisionKindV2::Reject => Ok(IntentFallbackV1::Reject {
            reason: decision
                .boundary_violations()
                .iter()
                .map(|violation| violation.id.as_str())
                .collect::<Vec<_>>()
                .join(","),
            response,
        }),
        IntentRouteDecisionKindV2::Discussion => Ok(IntentFallbackV1::Discussion { response }),
        IntentRouteDecisionKindV2::PrivateStudyRoom | IntentRouteDecisionKindV2::TypedPlanner => {
            Err(intent_error(
                "INCONSISTENT_INTENT_ADJUDICATION",
                "intent.adjudication.kind",
                "A non-terminal intent decision reached the terminal route",
                "Construct the route through the matching adjudication permit",
            ))
        }
    }
}

fn exactly_one_decision(
    decisions: Vec<MissingDecision>,
) -> Result<MissingDecision, StructuredError> {
    let [decision] = decisions.as_slice() else {
        return Err(intent_error(
            "INTENT_DECISION_CARDINALITY_INVALID",
            "intent.decisions",
            format!(
                "The intent resolver returned {} active decisions",
                decisions.len()
            ),
            "Expose exactly one blocking decision per user turn",
        ));
    };
    Ok(decision.clone())
}
