use serde_json::json;

use crate::errors::StructuredError;
use crate::intent::{
    apply_existing_channel_decision, prepare_intent_candidate, IntentWorkspaceV1, MissingDecision,
    PreparedIntentWorkspaceV1,
};
use crate::turn::{
    parse_interpret_intent_core, parse_private_study_room_details, parse_resolve_intent_decision,
};

use super::super::DesignSession;
use super::adjudicate::{
    adjudicate_intent_core_v3, IntentCoreAdjudicationV3, PrivateStudyRoomPermitV2,
    PrivateStudyRoomSelectionV3,
};
use super::decision::{IntentRouteDecisionKindV2, IntentRouteDecisionV2};
use super::evidence::IntentRecipeEvidenceV3;
use super::state::{
    intent_error, IntentFallbackV1, IntentRecipeRuntime, IntentRecipeStageSnapshotV1,
};
use super::state_binding::{
    awaiting_decision_binding_digest_v3, preview_ready_binding_digest_v3,
    AwaitingDecisionBindingInputV3, PreviewReadyBindingInputV3,
};
use super::INTENT_RECIPE_PROTOCOL_VERSION_V3;

pub(super) enum IntentCoreExecutionV3 {
    Complete(Box<IntentTurnSuccess>),
    NeedsDetails(Box<PrivateStudyRoomSelectionV3>),
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
                compiled_operations,
                ..
            } => json!({
                "ok": true,
                "status": "preview_ready",
                "intent_revision": intent_revision,
                "draft_revision": draft_revision,
                "semantic_intent_hash": semantic_intent_hash,
                "compiled_plan_hash": compiled_plan_hash,
                "compiled_operations": compiled_operations,
            }),
            Self::Routed { fallback, .. } => json!({
                "ok": true,
                "status": "routed",
                "fallback_kind": fallback.kind().as_str(),
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

    fn validate_expected_revision(&mut self, actual: u64) -> Result<(), StructuredError> {
        let expected = self
            .intent_recipe
            .as_ref()
            .map(|runtime| runtime.expected_revision(self.draft.draft_revision))
            .ok_or_else(|| {
                intent_error(
                    "INTENT_SESSION_DISABLED",
                    "intent.session",
                    "Intent recipe mode is not enabled",
                    "Construct the session with resource bindings",
                )
            })?;
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
        workspace: IntentWorkspaceV1,
        active_decision: MissingDecision,
        route_decision: IntentRouteDecisionV2,
        recipe_evidence: IntentRecipeEvidenceV3,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let options = active_decision.options.clone();
        let question = active_decision.question.clone();
        let revision = workspace.revision;
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
            awaiting_decision_binding_digest_v3(AwaitingDecisionBindingInputV3 {
                protocol_version: INTENT_RECIPE_PROTOCOL_VERSION_V3,
                context_fingerprint: &context_fingerprint,
                root_draft_revision,
                workspace: &workspace,
                active_decision: &active_decision,
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
        runtime.snapshot.stage = IntentRecipeStageSnapshotV1::AwaitingDecision {
            root_draft_revision,
            workspace,
            active_decision,
            route_decision: Some(route_decision),
            recipe_evidence: Some(recipe_evidence),
            decision_binding_digest: Some(decision_binding_digest),
        };
        Ok(IntentTurnSuccess::NeedsInput {
            question,
            revision,
            options,
        })
    }

    async fn prepare_and_commit_intent(
        &mut self,
        workspace: IntentWorkspaceV1,
        intent: crate::intent::ValidatedIntentV1,
        route_decision: IntentRouteDecisionV2,
        recipe_evidence: IntentRecipeEvidenceV3,
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
        let input_intent_hash = prepared.compilation().manifest.input_intent_hash.clone();
        let semantic_intent_hash = prepared.compilation().manifest.semantic_intent_hash.clone();
        let compiled_plan_hash = prepared.compilation().manifest.compiled_plan_hash.clone();
        let external_channel_bindings = prepared
            .compilation()
            .manifest
            .external_channel_bindings
            .clone();
        let compiled_operations = prepared.execution().compiled_operations;
        let intent_revision = intent.revision();
        let candidate_revision = prepared.execution().candidate_revision;
        let decision_binding_digest =
            preview_ready_binding_digest_v3(PreviewReadyBindingInputV3 {
                protocol_version: INTENT_RECIPE_PROTOCOL_VERSION_V3,
                context_fingerprint: &context_fingerprint,
                root_draft_revision: root_revision,
                workspace: &workspace,
                intent_revision,
                candidate_revision,
                input_intent_hash: &input_intent_hash,
                semantic_intent_hash: &semantic_intent_hash,
                compiled_plan_hash: &compiled_plan_hash,
                external_channel_bindings: &external_channel_bindings,
                compiled_operations,
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
        runtime.snapshot.stage = IntentRecipeStageSnapshotV1::PreviewReady {
            root_draft_revision: root_revision,
            workspace,
            intent_revision,
            candidate_revision,
            input_intent_hash,
            semantic_intent_hash: semantic_intent_hash.clone(),
            compiled_plan_hash: compiled_plan_hash.clone(),
            external_channel_bindings,
            compiled_operations,
            route_decision: Some(route_decision),
            recipe_evidence: Some(recipe_evidence),
            decision_binding_digest: Some(decision_binding_digest),
        };
        Ok(IntentTurnSuccess::Ready {
            summary: format!(
                "Prepared, validated, simulated, and previewed the private study room design at Draft revision {candidate_revision}"
            ),
            intent_revision,
            draft_revision: candidate_revision,
            semantic_intent_hash,
            compiled_plan_hash,
            compiled_operations,
        })
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
    ) -> Result<IntentCoreExecutionV3, StructuredError> {
        self.observability.intent_route_calls =
            self.observability.intent_route_calls.saturating_add(1);
        let core = parse_interpret_intent_core(arguments).inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        core.validate_human_evidence(human_message)
            .inspect_err(|_| {
                self.record_intent_extraction_failure();
            })?;
        self.validate_expected_revision(core.expected_revision())?;
        let adjudication = adjudicate_intent_core_v3(core).inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        match adjudication {
            IntentCoreAdjudicationV3::PrivateStudyRoom(selection) => {
                if selection.detail_facets().is_empty() {
                    let recipe_evidence = IntentRecipeEvidenceV3::deterministic_default(
                        selection.semantic_ir_digest(),
                    )?;
                    let permit = selection.finalize(None).inspect_err(|_| {
                        self.record_intent_extraction_failure();
                    })?;
                    self.complete_private_study_room(permit, recipe_evidence)
                        .await
                        .map(|success| IntentCoreExecutionV3::Complete(Box::new(success)))
                } else {
                    Ok(IntentCoreExecutionV3::NeedsDetails(selection))
                }
            }
            IntentCoreAdjudicationV3::TypedPlanner(permit) => {
                let (objective, _, decision, response) = permit.into_parts();
                self.accept_fallback(
                    IntentFallbackV1::TypedPlanner {
                        reason: objective,
                        response,
                    },
                    decision,
                )
                .map(|success| IntentCoreExecutionV3::Complete(Box::new(success)))
            }
            IntentCoreAdjudicationV3::Terminal(permit) => {
                let (decision, response) = permit.into_parts();
                let fallback = terminal_fallback(&decision, response)?;
                self.accept_fallback(fallback, decision)
                    .map(|success| IntentCoreExecutionV3::Complete(Box::new(success)))
            }
        }
    }

    pub(super) async fn execute_intent_details(
        &mut self,
        selection: Box<PrivateStudyRoomSelectionV3>,
        arguments: &str,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let expected_revision = selection.expected_revision();
        let core_semantic_digest = selection.semantic_ir_digest().to_string();
        let detail_facets = selection.detail_facets().to_vec();
        let details = parse_private_study_room_details(
            arguments,
            &detail_facets,
            expected_revision,
            &core_semantic_digest,
        )
        .inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        let detail_result_digest = selection.details_digest(&details)?;
        let recipe_evidence = IntentRecipeEvidenceV3::model_detail(
            &core_semantic_digest,
            &detail_facets,
            detail_result_digest,
        )?;
        let permit = selection.finalize(Some(details)).inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        self.complete_private_study_room(permit, recipe_evidence)
            .await
    }

    async fn complete_private_study_room(
        &mut self,
        permit: PrivateStudyRoomPermitV2,
        recipe_evidence: IntentRecipeEvidenceV3,
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
            PreparedIntentWorkspaceV1::NeedsInput {
                workspace,
                decisions,
            } => {
                let decision = exactly_one_decision(decisions)?;
                self.set_awaiting_decision(
                    self.draft.draft_revision,
                    workspace,
                    decision,
                    route_decision,
                    recipe_evidence,
                )
            }
            PreparedIntentWorkspaceV1::Resolved { workspace, intent } => {
                self.prepare_and_commit_intent(workspace, intent, route_decision, recipe_evidence)
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
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let input = parse_resolve_intent_decision(arguments).inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        self.validate_expected_revision(input.expected_revision)?;
        let (root_draft_revision, workspace, route_decision, recipe_evidence) = match self
            .intent_recipe
            .as_ref()
            .map(|runtime| runtime.snapshot.stage.clone())
        {
            Some(IntentRecipeStageSnapshotV1::AwaitingDecision {
                root_draft_revision,
                workspace,
                route_decision,
                recipe_evidence,
                ..
            }) => (
                root_draft_revision,
                workspace,
                route_decision.ok_or_else(missing_route_decision_error)?,
                recipe_evidence.ok_or_else(missing_recipe_evidence_error)?,
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
            input.channel,
            &context,
        )?;
        self.observability.intent_resolution_acceptances = self
            .observability
            .intent_resolution_acceptances
            .saturating_add(1);
        match prepared {
            PreparedIntentWorkspaceV1::NeedsInput {
                workspace,
                decisions,
            } => {
                let decision = exactly_one_decision(decisions)?;
                self.set_awaiting_decision(
                    root_draft_revision,
                    workspace,
                    decision,
                    route_decision,
                    recipe_evidence,
                )
            }
            PreparedIntentWorkspaceV1::Resolved { workspace, intent } => {
                self.prepare_and_commit_intent(workspace, intent, route_decision, recipe_evidence)
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

fn missing_route_decision_error() -> StructuredError {
    intent_error(
        "INTENT_ROUTE_DECISION_MISSING",
        "intent.session.route_decision",
        "The pending intent decision has no deterministic route decision",
        "Start a new intent recipe session under protocol version 3",
    )
}

fn missing_recipe_evidence_error() -> StructuredError {
    intent_error(
        "INTENT_RECIPE_EVIDENCE_MISSING",
        "intent.session.recipe_evidence",
        "The pending intent decision has no protocol V3 recipe evidence",
        "Start a new intent recipe session under protocol version 3",
    )
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
