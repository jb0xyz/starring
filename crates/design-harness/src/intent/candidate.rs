use resource_resolution::ResourceBindingMap;
use schemars::JsonSchema;
use serde::Serialize;

use crate::draft::Draft;
use crate::errors::StructuredError;
use crate::gates::validate_candidate_with_bindings;
use crate::turn::{
    execute_plan_atomically_with_bindings, normalize_turn_plan, render_preview_with_bindings,
    DraftPreview, RequestedOutcome, SimulationProfile, TurnBrief, TurnIntent, TurnVerification,
};

use super::catalog::MAX_COMPILED_REQUIREMENTS;
use super::compile::{compile_intent, CompiledIntentV1};
use super::model::IntentRequestedOutcome;
use super::normalize::ValidatedIntentV1;
use super::simulation::simulate_compiled_intent;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntentExecutionReportV1 {
    pub intent_revision: u64,
    pub root_revision: u64,
    pub candidate_revision: u64,
    pub compiled_operations: usize,
    pub validation_passed: bool,
    pub simulation_traces: u32,
    pub close_executed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommittedIntentCandidateV1 {
    pub compilation: CompiledIntentV1,
    pub preview: DraftPreview,
    pub execution: IntentExecutionReportV1,
}

#[derive(Debug)]
pub struct PreparedIntentCandidateV1 {
    root: Draft,
    candidate: Draft,
    compilation: CompiledIntentV1,
    preview: DraftPreview,
    execution: IntentExecutionReportV1,
}

impl PreparedIntentCandidateV1 {
    pub fn compilation(&self) -> &CompiledIntentV1 {
        &self.compilation
    }

    pub fn preview(&self) -> &DraftPreview {
        &self.preview
    }

    pub fn execution(&self) -> &IntentExecutionReportV1 {
        &self.execution
    }

    pub fn commit(
        self,
        current_root: &mut Draft,
    ) -> Result<CommittedIntentCandidateV1, StructuredError> {
        if *current_root != self.root {
            return Err(candidate_error(
                "INTENT_CANDIDATE_STALE",
                "intent.candidate.root",
                format!(
                    "The Draft changed after candidate preparation at revision {}",
                    self.root.draft_revision
                ),
                "Prepare a new candidate from the current canonical Draft",
            ));
        }
        *current_root = self.candidate;
        Ok(CommittedIntentCandidateV1 {
            compilation: self.compilation,
            preview: self.preview,
            execution: self.execution,
        })
    }
}

pub async fn prepare_intent_candidate(
    root: &Draft,
    intent: &ValidatedIntentV1,
    bindings: &ResourceBindingMap,
) -> Result<PreparedIntentCandidateV1, StructuredError> {
    let compiled = compile_intent(intent)?;
    verify_external_bindings(&compiled, bindings)?;
    let mut brief = recipe_turn_brief(intent)?;
    brief.requirements = normalize_turn_plan(root, &brief, compiled.requirements.clone())?;
    let execution =
        execute_plan_atomically_with_bindings(root, &brief, bindings, MAX_COMPILED_REQUIREMENTS)
            .await
            .map_err(|failure| failure.error)?;
    let compiled_operations = execution.records.len();
    let mut candidate = execution.draft;
    validate_candidate_with_bindings(&mut candidate, bindings)?;
    let simulation = simulate_compiled_intent(&mut candidate, intent, &compiled, bindings).await?;
    let preview = render_preview_with_bindings(&candidate, bindings)?;
    let report = IntentExecutionReportV1 {
        intent_revision: intent.revision(),
        root_revision: root.draft_revision,
        candidate_revision: candidate.draft_revision,
        compiled_operations,
        validation_passed: candidate.validated_revision == Some(candidate.draft_revision),
        simulation_traces: simulation.traces_run,
        close_executed: simulation.close_executed,
    };
    Ok(PreparedIntentCandidateV1 {
        root: root.clone(),
        candidate,
        compilation: compiled,
        preview,
        execution: report,
    })
}

fn verify_external_bindings(
    compiled: &CompiledIntentV1,
    bindings: &ResourceBindingMap,
) -> Result<(), StructuredError> {
    let missing = compiled
        .manifest
        .external_channel_bindings
        .iter()
        .filter(|required| {
            !bindings
                .channel_bindings
                .keys()
                .any(|available| available.0.as_str() == required.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(candidate_error(
        "INTENT_EXTERNAL_BINDING_MISSING",
        "intent.candidate.bindings",
        format!(
            "The compiled candidate requires unavailable channel bindings: {}",
            missing.join(", ")
        ),
        "Refresh the resource binding map and resolve the intent against the current catalog",
    ))
}

fn recipe_turn_brief(intent: &ValidatedIntentV1) -> Result<TurnBrief, StructuredError> {
    let requested_outcome = match intent.requested_outcome() {
        IntentRequestedOutcome::WorkingDraft => RequestedOutcome::DraftUpdate,
        IntentRequestedOutcome::ValidatedPreview => RequestedOutcome::ValidatedPreview,
        IntentRequestedOutcome::Discussion => {
            return Err(candidate_error(
                "INTENT_OUTCOME_NOT_COMPILABLE",
                "intent.requested_outcome",
                "A discussion intent cannot prepare a Draft candidate",
                "Wait for an explicit build or preview request before preparing the candidate",
            ));
        }
    };
    Ok(TurnBrief {
        intent: TurnIntent::Build,
        objective: intent.objective().to_string(),
        requested_outcome,
        requirements: Vec::new(),
        assumptions: Vec::new(),
        blocking_decisions: Vec::new(),
        verification: TurnVerification {
            validate: true,
            simulation: SimulationProfile::StudyRoom,
        },
    })
}

fn candidate_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use serde_json::json;

    use super::*;
    use crate::intent::{
        propose_private_study_room, ClosePolicyV1, ExistingChannelKey, IntentLocaleV1,
        IntentProposalOutcomeV1, IntentResolutionContext, PrivateStudyRoomControlsProposalV1,
        PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1,
        PrivateStudyRoomProposalV1,
    };

    fn resolved_intent(hub: &str, close_policy: Option<ClosePolicyV1>) -> ValidatedIntentV1 {
        let proposal = PrivateStudyRoomProposalV1 {
            objective: "Create private study rooms".to_string(),
            requested_outcome: IntentRequestedOutcome::ValidatedPreview,
            hub_channel: Some(ExistingChannelKey(hub.to_string())),
            locale: Some(IntentLocaleV1::En),
            copy: PrivateStudyRoomCopyProposalV1::default(),
            naming: PrivateStudyRoomNamingProposalV1::default(),
            controls: PrivateStudyRoomControlsProposalV1 {
                close_policy,
                ..PrivateStudyRoomControlsProposalV1::default()
            },
        };
        let context =
            IntentResolutionContext::from_channel_bindings([ExistingChannelKey(hub.to_string())]);
        let IntentProposalOutcomeV1::Resolved { intent, .. } =
            propose_private_study_room(proposal, &context).unwrap()
        else {
            panic!("expected resolved intent");
        };
        intent
    }

    fn bindings(hub: &str) -> ResourceBindingMap {
        let mut bindings = ResourceBindingMap::default();
        bindings.channel_bindings.insert(
            serde_json::from_value(json!(hub)).unwrap(),
            "700".parse().unwrap(),
        );
        bindings
    }

    #[test]
    fn arbitrary_hub_candidate_prepares_and_commits_atomically() {
        block_on(async {
            let intent = resolved_intent("community_hub", None);
            let mut root = Draft::new();
            let original = root.clone();

            let prepared = prepare_intent_candidate(&root, &intent, &bindings("community_hub"))
                .await
                .unwrap();

            assert_eq!(root, original);
            assert_eq!(prepared.execution().compiled_operations, 22);
            assert_eq!(prepared.execution().simulation_traces, 4);
            assert!(!prepared.execution().close_executed);
            assert!(prepared.preview().draft.unresolved_references.is_empty());
            let committed = prepared.commit(&mut root).unwrap();
            assert_eq!(root.validated_revision, Some(root.draft_revision));
            assert_eq!(root.simulated_revision, Some(root.draft_revision));
            assert_eq!(committed.execution.candidate_revision, root.draft_revision);
        });
    }

    #[test]
    fn close_recipe_uses_its_deterministic_budget_beyond_model_tool_limits() {
        block_on(async {
            let intent = resolved_intent("community_hub", Some(ClosePolicyV1::AnyMember));

            let prepared =
                prepare_intent_candidate(&Draft::new(), &intent, &bindings("community_hub"))
                    .await
                    .unwrap();

            assert_eq!(prepared.execution().compiled_operations, 26);
            assert_eq!(prepared.execution().simulation_traces, 5);
            assert!(prepared.execution().close_executed);
        });
    }

    #[test]
    fn missing_binding_and_conflict_leave_the_root_unchanged() {
        block_on(async {
            let intent = resolved_intent("community_hub", None);
            let mut root = Draft::new();
            let original = root.clone();
            let missing = prepare_intent_candidate(&root, &intent, &ResourceBindingMap::default())
                .await
                .unwrap_err();
            assert_eq!(missing.code, "INTENT_EXTERNAL_BINDING_MISSING");
            assert_eq!(root, original);

            root.ruleset = serde_json::from_value(json!({
                "version": 1,
                "panels": [{
                    "key": "private_study_room__study_panel",
                    "channel": "community_hub",
                    "content": "Conflicting content",
                    "buttons": []
                }],
                "modals": [],
                "rules": []
            }))
            .unwrap();
            root.draft_revision = 1;
            let conflicted_root = root.clone();
            assert!(
                prepare_intent_candidate(&root, &intent, &bindings("community_hub"))
                    .await
                    .is_err()
            );
            assert_eq!(root, conflicted_root);
        });
    }

    #[test]
    fn stale_commit_is_rejected_without_replacing_the_current_root() {
        block_on(async {
            let intent = resolved_intent("community_hub", None);
            let root = Draft::new();
            let prepared = prepare_intent_candidate(&root, &intent, &bindings("community_hub"))
                .await
                .unwrap();
            let mut changed = root.clone();
            changed.draft_revision = 1;
            let expected = changed.clone();

            let error = prepared.commit(&mut changed).unwrap_err();

            assert_eq!(error.code, "INTENT_CANDIDATE_STALE");
            assert_eq!(changed, expected);
        });
    }
}
