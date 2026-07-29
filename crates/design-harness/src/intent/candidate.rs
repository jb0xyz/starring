use std::future::Future;
use std::task::{Context, Poll, Waker};

use automation_core::validate::validate_structural;
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::draft::Draft;
use crate::errors::StructuredError;
use crate::gates::validate_candidate_with_bindings;
use crate::turn::{
    execute_plan_atomically_with_bindings, normalize_turn_plan, render_preview_with_bindings,
    DraftPreview, RequestedOutcome, SimulationProfile, TurnBrief, TurnIntent, TurnVerification,
};

use super::catalog::MAX_COMPILED_REQUIREMENTS;
use super::compile::{compile_intent, CompiledIntentV2};
use super::identity::{canonical_json_digest, IdentityErrorSpec};
use super::model::IntentRequestedOutcome;
use super::normalize::ValidatedIntentV2;
use super::simulation::simulate_compiled_intent;

const MANAGED_PRIVATE_STUDY_ROOM_OBJECTIVE: &str = "Build managed private study rooms";
const CANDIDATE_RULESET_DIGEST_DOMAIN_V1: &[u8] = b"starring.intent.candidate_ruleset.v1\0";
const DRAFT_STATE_DIGEST_DOMAIN_V1: &[u8] = b"starring.intent.draft_state.v1\0";

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewRulesetVerificationErrorV1 {
    Malformed,
    UnsupportedVersion,
    NonCanonicalShape,
    StructurallyInvalid,
    SerializationFailed,
    IdentityMismatch,
}

pub fn verify_preview_ruleset_v1(
    ruleset: &Value,
    expected_candidate_ruleset_hash: &str,
) -> Result<(), PreviewRulesetVerificationErrorV1> {
    let typed = serde_json::from_value::<InteractionRuleSet>(ruleset.clone())
        .map_err(|_| PreviewRulesetVerificationErrorV1::Malformed)?;
    if typed.version != 1 {
        return Err(PreviewRulesetVerificationErrorV1::UnsupportedVersion);
    }
    let canonical = serde_json::to_value(&typed)
        .map_err(|_| PreviewRulesetVerificationErrorV1::SerializationFailed)?;
    if canonical != *ruleset {
        return Err(PreviewRulesetVerificationErrorV1::NonCanonicalShape);
    }
    validate_structural(&typed)
        .map_err(|_| PreviewRulesetVerificationErrorV1::StructurallyInvalid)?;
    let actual = candidate_ruleset_identity_hash_v1(&typed)
        .map_err(|_| PreviewRulesetVerificationErrorV1::SerializationFailed)?;
    if actual != expected_candidate_ruleset_hash {
        return Err(PreviewRulesetVerificationErrorV1::IdentityMismatch);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommittedIntentCandidateV1 {
    pub compilation: CompiledIntentV2,
    pub preview: DraftPreview,
    pub execution: IntentExecutionReportV1,
}

#[derive(Debug)]
pub struct PreparedIntentCandidateV1 {
    root: Draft,
    candidate: Draft,
    compilation: CompiledIntentV2,
    preview: DraftPreview,
    execution: IntentExecutionReportV1,
}

impl PreparedIntentCandidateV1 {
    pub fn compilation(&self) -> &CompiledIntentV2 {
        &self.compilation
    }

    pub(crate) fn candidate(&self) -> &Draft {
        &self.candidate
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
    intent: &ValidatedIntentV2,
    bindings: &ResourceBindingMap,
) -> Result<PreparedIntentCandidateV1, StructuredError> {
    let available_channel_keys = bindings
        .channel_bindings
        .keys()
        .map(|key| key.0.clone())
        .collect::<Vec<_>>();
    let (compiled, brief) = intent_candidate_preflight(root, intent, &available_channel_keys)?;
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

pub(crate) fn replay_intent_candidate_preparation(
    root: &Draft,
    intent: &ValidatedIntentV2,
    bindings: &ResourceBindingMap,
) -> Result<(), StructuredError> {
    block_on_candidate_replay(prepare_intent_candidate(root, intent, bindings)).map(|_| ())
}

fn block_on_candidate_replay<F, T>(future: F) -> Result<T, StructuredError>
where
    F: Future<Output = Result<T, StructuredError>>,
{
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => Err(candidate_error(
            "INTENT_CANDIDATE_REPLAY_NOT_SYNCHRONOUS",
            "intent.candidate.replay",
            "Candidate preparation crossed an asynchronous boundary during snapshot replay",
            "Keep candidate preparation side-effect-free and immediately ready during restore",
        )),
    }
}

fn intent_candidate_preflight(
    root: &Draft,
    intent: &ValidatedIntentV2,
    available_channel_keys: &[String],
) -> Result<(CompiledIntentV2, TurnBrief), StructuredError> {
    let compiled = compile_intent(intent)?;
    verify_external_channel_keys(&compiled, available_channel_keys)?;
    let mut brief = recipe_turn_brief(intent)?;
    brief.requirements = normalize_turn_plan(root, &brief, compiled.requirements.clone())?;
    Ok((compiled, brief))
}

fn verify_external_channel_keys(
    compiled: &CompiledIntentV2,
    available_channel_keys: &[String],
) -> Result<(), StructuredError> {
    let missing = compiled
        .manifest
        .external_channel_bindings
        .iter()
        .filter(|required| !available_channel_keys.contains(required))
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

pub(crate) fn candidate_ruleset_hash(candidate: &Draft) -> Result<String, StructuredError> {
    candidate_ruleset_identity_hash_v1(&candidate.ruleset)
}

fn candidate_ruleset_identity_hash_v1(
    ruleset: &InteractionRuleSet,
) -> Result<String, StructuredError> {
    canonical_json_digest(
        CANDIDATE_RULESET_DIGEST_DOMAIN_V1,
        ruleset,
        IdentityErrorSpec::new(
            "INTENT_CANDIDATE_SERIALIZATION_FAILED",
            "intent.candidate.ruleset_hash",
            "The candidate RuleSet could not be serialized deterministically",
        ),
    )
}

pub(crate) fn draft_state_hash(draft: &Draft) -> Result<String, StructuredError> {
    canonical_json_digest(
        DRAFT_STATE_DIGEST_DOMAIN_V1,
        draft,
        IdentityErrorSpec::new(
            "INTENT_CANDIDATE_SERIALIZATION_FAILED",
            "intent.candidate.draft_state_hash",
            "The candidate Draft state could not be serialized deterministically",
        ),
    )
}

fn recipe_turn_brief(intent: &ValidatedIntentV2) -> Result<TurnBrief, StructuredError> {
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
        objective: MANAGED_PRIVATE_STUDY_ROOM_OBJECTIVE.to_string(),
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
        IntentProposalOutcomeV2, IntentResolutionContext, PrivateStudyRoomControlsProposalV1,
        PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1,
        PrivateStudyRoomProposalV2,
    };

    fn resolved_intent(hub: &str, close_policy: Option<ClosePolicyV1>) -> ValidatedIntentV2 {
        let proposal = PrivateStudyRoomProposalV2 {
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
        let IntentProposalOutcomeV2::Resolved { intent, .. } =
            propose_private_study_room(proposal, &context).unwrap()
        else {
            panic!("expected resolved intent");
        };
        intent
    }

    #[test]
    fn candidate_turn_brief_uses_a_harness_owned_objective() {
        let intent = resolved_intent("community_hub", None);

        let brief = recipe_turn_brief(&intent).unwrap();

        assert_eq!(brief.objective, MANAGED_PRIVATE_STUDY_ROOM_OBJECTIVE);
    }

    #[test]
    fn candidate_and_draft_identities_are_stable_and_separate() {
        block_on(async {
            let intent = resolved_intent("community_hub", None);
            let root = Draft::new();
            let first = prepare_intent_candidate(&root, &intent, &bindings("community_hub"))
                .await
                .unwrap();
            let second = prepare_intent_candidate(&root, &intent, &bindings("community_hub"))
                .await
                .unwrap();

            let first_ruleset = candidate_ruleset_hash(first.candidate()).unwrap();
            let second_ruleset = candidate_ruleset_hash(second.candidate()).unwrap();
            let first_draft = draft_state_hash(first.candidate()).unwrap();
            let second_draft = draft_state_hash(second.candidate()).unwrap();

            assert_eq!(first.candidate(), second.candidate());
            assert_eq!(first_ruleset, second_ruleset);
            assert_eq!(first_draft, second_draft);
            assert_ne!(first_ruleset, first_draft);
            assert_eq!(
                first_ruleset,
                "3b3e4480d50fdc7146c83a14032be818e01e41b12207bcc8ad16b33794de346c"
            );
            assert_eq!(
                first_draft,
                "8f282e34b267099cdf1446451cd43580d8e67e94868bfa50e974d383309c746b"
            );

            let mut metadata_only = first.candidate().clone();
            metadata_only.draft_revision += 1;
            assert_eq!(
                candidate_ruleset_hash(first.candidate()).unwrap(),
                candidate_ruleset_hash(&metadata_only).unwrap()
            );
            assert_ne!(
                draft_state_hash(first.candidate()).unwrap(),
                draft_state_hash(&metadata_only).unwrap()
            );

            let mut content_changed = first.candidate().clone();
            content_changed.ruleset.panels[0]
                .content
                .push_str(" changed");
            assert_ne!(
                candidate_ruleset_hash(first.candidate()).unwrap(),
                candidate_ruleset_hash(&content_changed).unwrap()
            );
            assert_ne!(
                draft_state_hash(first.candidate()).unwrap(),
                draft_state_hash(&content_changed).unwrap()
            );
        });
    }

    #[test]
    fn preview_ruleset_verifier_reuses_the_candidate_identity_contract() {
        let ruleset = json!({
            "version": 1,
            "panels": [{
                "key": "welcome_panel",
                "channel": "welcome_channel",
                "content": "Choose a welcome",
                "buttons": [{
                    "label": "Welcome",
                    "route": {"static": {"key": "welcome"}}
                }]
            }],
            "modals": [],
            "rules": [{
                "key": "welcome_rule",
                "trigger": {"type": "button_click", "component": "welcome"},
                "actions": [{"type": "respond_ephemeral", "content": "Welcome!"}]
            }]
        });

        assert_eq!(
            verify_preview_ruleset_v1(
                &ruleset,
                "f283047e6367d67067822a399200ffd2ea6c1a6940969e0ab9abd399cb43d537"
            ),
            Ok(())
        );
        assert_eq!(
            verify_preview_ruleset_v1(
                &ruleset,
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            Err(PreviewRulesetVerificationErrorV1::IdentityMismatch)
        );
    }

    #[test]
    fn preview_ruleset_verifier_rejects_untyped_noncanonical_and_structural_inputs() {
        assert_eq!(
            verify_preview_ruleset_v1(
                &json!({
                    "version": 1,
                    "panels": [],
                    "modals": [],
                    "rules": [{"key": "broken"}]
                }),
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            Err(PreviewRulesetVerificationErrorV1::Malformed)
        );
        assert_eq!(
            verify_preview_ruleset_v1(
                &json!({"version": 1}),
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            Err(PreviewRulesetVerificationErrorV1::NonCanonicalShape)
        );
        assert_eq!(
            verify_preview_ruleset_v1(
                &json!({
                    "version": 1,
                    "panels": [],
                    "modals": [],
                    "rules": [{
                        "key": "orphan_rule",
                        "trigger": {"type": "button_click", "component": "missing"},
                        "actions": [{"type": "respond_ephemeral", "content": "Welcome!"}]
                    }]
                }),
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            Err(PreviewRulesetVerificationErrorV1::StructurallyInvalid)
        );
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
    fn candidate_replay_runs_the_complete_preparation_boundary() {
        let intent = resolved_intent("community_hub", None);

        replay_intent_candidate_preparation(&Draft::new(), &intent, &bindings("community_hub"))
            .unwrap();

        let close_intent = resolved_intent("community_hub", Some(ClosePolicyV1::AnyMember));
        replay_intent_candidate_preparation(
            &Draft::new(),
            &close_intent,
            &bindings("community_hub"),
        )
        .unwrap();
    }

    #[test]
    fn candidate_replay_fails_closed_at_an_async_boundary() {
        let error =
            block_on_candidate_replay(std::future::pending::<Result<(), StructuredError>>())
                .unwrap_err();

        assert_eq!(error.code, "INTENT_CANDIDATE_REPLAY_NOT_SYNCHRONOUS");
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
