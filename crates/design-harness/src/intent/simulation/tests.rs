use futures::executor::block_on;
use serde_json::json;

use crate::gates::validate_candidate_with_bindings;
use crate::intent::{
    propose_private_study_room, ClosePolicyV1, ExistingChannelKey, IntentLocaleV1,
    IntentProposalOutcomeV2, IntentRequestedOutcome, IntentResolutionContext,
    PrivateStudyRoomControlsProposalV1, PrivateStudyRoomCopyProposalV1,
    PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV2,
};
use crate::turn::{
    execute_plan_atomically_with_bindings, RequestedOutcome, SimulationProfile, TurnBrief,
    TurnIntent, TurnVerification,
};

use super::*;

fn resolved_intent(hub: &str, close_policy: ClosePolicyV1) -> ValidatedIntentV2 {
    let context =
        IntentResolutionContext::from_channel_bindings([ExistingChannelKey(hub.to_string())]);
    let proposal = PrivateStudyRoomProposalV2 {
        requested_outcome: IntentRequestedOutcome::ValidatedPreview,
        hub_channel: Some(ExistingChannelKey(hub.to_string())),
        locale: Some(IntentLocaleV1::En),
        copy: PrivateStudyRoomCopyProposalV1::default(),
        naming: PrivateStudyRoomNamingProposalV1::default(),
        controls: PrivateStudyRoomControlsProposalV1 {
            close_policy: Some(close_policy),
            ..PrivateStudyRoomControlsProposalV1::default()
        },
    };
    let outcome = propose_private_study_room(proposal, &context)
        .expect("private room proposal should normalize");
    match outcome {
        IntentProposalOutcomeV2::Resolved { intent, .. } => intent,
        IntentProposalOutcomeV2::NeedsInput { decisions, .. } => {
            panic!("unexpected missing decisions: {decisions:?}")
        }
    }
}

fn bindings(hub: &str, channel: &str) -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    let key = serde_json::from_value(json!(hub)).expect("binding key should parse");
    let channel = channel.parse().expect("channel id should parse");
    bindings.channel_bindings.insert(key, channel);
    bindings
}

fn candidate(compiled: &CompiledIntentV2, bindings: &ResourceBindingMap) -> Draft {
    let brief = TurnBrief {
        intent: TurnIntent::Build,
        objective: "Build managed private study rooms".to_string(),
        requested_outcome: RequestedOutcome::ValidatedPreview,
        requirements: compiled.requirements.clone(),
        assumptions: vec![],
        blocking_decisions: vec![],
        verification: TurnVerification {
            validate: true,
            simulation: SimulationProfile::StudyRoom,
        },
    };
    let execution = block_on(execute_plan_atomically_with_bindings(
        &Draft::new(),
        &brief,
        bindings,
        32,
    ))
    .unwrap_or_else(|failure| panic!("candidate execution failed: {:?}", failure.error));
    let mut candidate = execution.draft;
    validate_candidate_with_bindings(&mut candidate, bindings)
        .expect("compiled candidate should validate");
    candidate
}

#[test]
fn disabled_close_runs_four_traces_with_an_arbitrary_hub_binding() {
    let intent = resolved_intent("community_rooms", ClosePolicyV1::Disabled);
    let compiled = compile_intent(&intent).expect("intent should compile");
    let bindings = bindings("community_rooms", "902");
    let mut candidate = candidate(&compiled, &bindings);

    let report = block_on(simulate_compiled_intent(
        &mut candidate,
        &intent,
        &compiled,
        &bindings,
    ))
    .expect("disabled-close recipe should simulate");

    assert_eq!(
        report,
        IntentSimulationReportV1 {
            traces_run: 4,
            close_executed: false,
        }
    );
    assert_eq!(candidate.simulated_revision, Some(candidate.draft_revision));
}

#[test]
fn any_member_close_runs_five_traces_and_exactly_one_teardown() {
    let intent = resolved_intent("study_hub", ClosePolicyV1::AnyMember);
    let compiled = compile_intent(&intent).expect("intent should compile");
    let bindings = bindings("study_hub", "700");
    let mut candidate = candidate(&compiled, &bindings);

    let report = block_on(simulate_compiled_intent(
        &mut candidate,
        &intent,
        &compiled,
        &bindings,
    ))
    .expect("any-member close recipe should simulate");

    assert_eq!(
        report,
        IntentSimulationReportV1 {
            traces_run: 5,
            close_executed: true,
        }
    );
    assert_eq!(candidate.simulated_revision, Some(candidate.draft_revision));
}

#[test]
fn simulation_failure_clears_a_previous_simulation_revision() {
    let intent = resolved_intent("study_hub", ClosePolicyV1::Disabled);
    let compiled = compile_intent(&intent).expect("intent should compile");
    let bindings = bindings("study_hub", "700");
    let mut candidate = candidate(&compiled, &bindings);
    candidate.simulated_revision = Some(candidate.draft_revision);
    candidate
        .ruleset
        .rules
        .retain(|rule| rule.key != compiled.manifest.generated_objects["show_help"]);

    let error = block_on(simulate_compiled_intent(
        &mut candidate,
        &intent,
        &compiled,
        &bindings,
    ))
    .expect_err("missing deterministic help rule should fail");

    assert_eq!(error.code, "INTENT_SIMULATION_RULE_MISSING");
    assert_eq!(candidate.simulated_revision, None);
}
