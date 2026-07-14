use std::collections::BTreeSet;

use serde_json::json;

use crate::errors::StructuredError;
use crate::intent::{
    apply_existing_channel_decision, prepare_intent_candidate, prepare_private_study_room,
    IntentWorkspaceV1, MissingDecision, PreparedIntentWorkspaceV1,
};
use crate::turn::{parse_resolve_intent_decision, parse_route_intent_turn, IntentRouteInputV1};

use super::super::DesignSession;
use super::state::{
    intent_error, IntentFallbackV1, IntentRecipeRuntime, IntentRecipeStageSnapshotV1,
};

const MAX_FALLBACK_TEXT_CHARS: usize = 2_000;
const MAX_FALLBACK_REASON_CHARS: usize = 512;
const MAX_CAPABILITY_ITEMS: usize = 16;
const MAX_CAPABILITY_CHARS: usize = 64;

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
    Routed(IntentFallbackV1),
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
            Self::Routed(fallback) => json!({
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
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let options = active_decision.options.clone();
        let question = active_decision.question.clone();
        let revision = workspace.revision;
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
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let bindings = self
            .intent_recipe
            .as_ref()
            .map(|runtime| runtime.bindings.clone())
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
        let committed = match prepared.commit(&mut self.draft) {
            Ok(committed) => committed,
            Err(error) => {
                self.record_intent_rollback(&error);
                return Err(error);
            }
        };
        let candidate_revision = committed.execution.candidate_revision;
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

    pub(super) async fn execute_intent_route(
        &mut self,
        arguments: &str,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        self.observability.intent_route_calls =
            self.observability.intent_route_calls.saturating_add(1);
        let input = parse_route_intent_turn(arguments).inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        self.validate_expected_revision(input.expected_revision)?;
        match input.route {
            IntentRouteInputV1::PrivateStudyRoom { proposal } => {
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
                let prepared = match prepare_private_study_room(*proposal, &context) {
                    Ok(prepared) => prepared,
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
                        self.set_awaiting_decision(self.draft.draft_revision, workspace, decision)
                    }
                    PreparedIntentWorkspaceV1::Resolved { workspace, intent } => {
                        self.prepare_and_commit_intent(workspace, intent).await
                    }
                }
            }
            IntentRouteInputV1::TypedPlanner { reason, response } => {
                self.accept_fallback(IntentFallbackV1::TypedPlanner {
                    reason: normalized_fallback_text(
                        reason,
                        MAX_FALLBACK_REASON_CHARS,
                        "intent.route.typed_planner.reason",
                    )?,
                    response: normalized_fallback_text(
                        response,
                        MAX_FALLBACK_TEXT_CHARS,
                        "intent.route.typed_planner.response",
                    )?,
                })
            }
            IntentRouteInputV1::CapabilityGap {
                capabilities,
                response,
            } => self.accept_fallback(IntentFallbackV1::CapabilityGap {
                capabilities: normalized_capabilities(capabilities)?,
                response: normalized_fallback_text(
                    response,
                    MAX_FALLBACK_TEXT_CHARS,
                    "intent.route.capability_gap.response",
                )?,
            }),
            IntentRouteInputV1::Reject { reason, response } => {
                self.accept_fallback(IntentFallbackV1::Reject {
                    reason: normalized_fallback_text(
                        reason,
                        MAX_FALLBACK_REASON_CHARS,
                        "intent.route.reject.reason",
                    )?,
                    response: normalized_fallback_text(
                        response,
                        MAX_FALLBACK_TEXT_CHARS,
                        "intent.route.reject.response",
                    )?,
                })
            }
            IntentRouteInputV1::Discussion { response } => {
                self.accept_fallback(IntentFallbackV1::Discussion {
                    response: normalized_fallback_text(
                        response,
                        MAX_FALLBACK_TEXT_CHARS,
                        "intent.route.discussion.response",
                    )?,
                })
            }
        }
    }

    fn accept_fallback(
        &mut self,
        fallback: IntentFallbackV1,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        *self
            .observability
            .intent_fallback_routes
            .entry(fallback.kind().as_str().to_string())
            .or_default() += 1;
        Ok(IntentTurnSuccess::Routed(fallback))
    }

    pub(super) async fn execute_intent_resolution(
        &mut self,
        arguments: &str,
    ) -> Result<IntentTurnSuccess, StructuredError> {
        let input = parse_resolve_intent_decision(arguments).inspect_err(|_| {
            self.record_intent_extraction_failure();
        })?;
        self.validate_expected_revision(input.expected_revision)?;
        let (root_draft_revision, workspace) = match self
            .intent_recipe
            .as_ref()
            .map(|runtime| runtime.snapshot.stage.clone())
        {
            Some(IntentRecipeStageSnapshotV1::AwaitingDecision {
                root_draft_revision,
                workspace,
                ..
            }) => (root_draft_revision, workspace),
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
                self.set_awaiting_decision(root_draft_revision, workspace, decision)
            }
            PreparedIntentWorkspaceV1::Resolved { workspace, intent } => {
                self.prepare_and_commit_intent(workspace, intent).await
            }
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

fn normalized_fallback_text(
    value: String,
    max_chars: usize,
    location: &str,
) -> Result<String, StructuredError> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let length = normalized.chars().count();
    if length == 0 || length > max_chars {
        return Err(intent_error(
            "INVALID_INTENT_FALLBACK_TEXT",
            location,
            format!("Fallback text contains {length} characters; expected 1 to {max_chars}"),
            "Provide a concise non-empty user-facing value",
        ));
    }
    Ok(normalized)
}

fn normalized_capabilities(values: Vec<String>) -> Result<Vec<String>, StructuredError> {
    if values.is_empty() || values.len() > MAX_CAPABILITY_ITEMS {
        return Err(intent_error(
            "INVALID_INTENT_CAPABILITIES",
            "intent.route.capability_gap.capabilities",
            format!(
                "Capability gap contains {} items; expected 1 to {MAX_CAPABILITY_ITEMS}",
                values.len()
            ),
            "List only the unsupported capabilities that block this request",
        ));
    }
    let mut normalized = BTreeSet::new();
    for value in values {
        let value = value.trim().to_string();
        let valid = !value.is_empty()
            && value.chars().count() <= MAX_CAPABILITY_CHARS
            && value
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '_');
        if !valid || !normalized.insert(value) {
            return Err(intent_error(
                "INVALID_INTENT_CAPABILITIES",
                "intent.route.capability_gap.capabilities",
                "Capability identifiers must be unique lowercase ASCII snake_case values",
                "Use one stable identifier per unsupported capability",
            ));
        }
    }
    Ok(normalized.into_iter().collect())
}
