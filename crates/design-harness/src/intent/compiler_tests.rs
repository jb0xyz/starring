use crate::draft::Draft;
use crate::turn::{
    normalize_turn_plan, RequestedOutcome, ScopeAction, ScopeRequirement, SimulationProfile,
    TurnBrief, TurnIntent, TurnVerification,
};

use super::normalize::PreparedIntentWorkspaceV1;
use super::proposal::{apply_existing_channel_decision, prepare_private_study_room};
use super::{
    compile_intent, propose_private_study_room, ClosePolicyV1, ExistingChannelKey, IntentLocaleV1,
    IntentProposalOutcomeV1, IntentRequestedOutcome, IntentResolutionContext,
    PrivateStudyRoomControlsProposalV1, PrivateStudyRoomCopyProposalV1,
    PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV1, ValidatedIntentV1,
};

fn context() -> IntentResolutionContext {
    IntentResolutionContext::from_channel_bindings([ExistingChannelKey("study_hub".to_string())])
}

fn intent(
    requested_outcome: IntentRequestedOutcome,
    close_policy: Option<ClosePolicyV1>,
    launcher_content: Option<&str>,
) -> ValidatedIntentV1 {
    let proposal = proposal(
        requested_outcome,
        close_policy,
        launcher_content,
        Some(ExistingChannelKey("study_hub".to_string())),
    );
    let IntentProposalOutcomeV1::Resolved { intent, .. } =
        propose_private_study_room(proposal, &context()).expect("proposal should resolve")
    else {
        panic!("expected a resolved intent");
    };
    intent
}

fn proposal(
    requested_outcome: IntentRequestedOutcome,
    close_policy: Option<ClosePolicyV1>,
    launcher_content: Option<&str>,
    hub_channel: Option<ExistingChannelKey>,
) -> PrivateStudyRoomProposalV1 {
    PrivateStudyRoomProposalV1 {
        objective: "Create a private study room".to_string(),
        requested_outcome,
        hub_channel,
        locale: Some(IntentLocaleV1::En),
        copy: PrivateStudyRoomCopyProposalV1 {
            launcher_content: launcher_content.map(str::to_string),
            ..PrivateStudyRoomCopyProposalV1::default()
        },
        naming: PrivateStudyRoomNamingProposalV1::default(),
        controls: PrivateStudyRoomControlsProposalV1 {
            close_policy,
            ..PrivateStudyRoomControlsProposalV1::default()
        },
    }
}

fn resumed_intent() -> ValidatedIntentV1 {
    let prepared = prepare_private_study_room(
        proposal(IntentRequestedOutcome::ValidatedPreview, None, None, None),
        &context(),
    )
    .expect("incomplete proposal should prepare");
    let PreparedIntentWorkspaceV1::NeedsInput {
        workspace,
        decisions,
    } = prepared
    else {
        panic!("expected a pending hub decision");
    };
    assert_eq!(decisions.len(), 1);
    let expected_revision = workspace.revision;
    let PreparedIntentWorkspaceV1::Resolved { intent, .. } = apply_existing_channel_decision(
        &workspace,
        expected_revision,
        ExistingChannelKey("study_hub".to_string()),
        &context(),
    )
    .expect("confirmed hub decision should resolve") else {
        panic!("expected a resolved resumed intent");
    };
    intent
}

#[test]
fn disabled_close_compiles_deterministically_without_dead_controls() {
    let intent = intent(IntentRequestedOutcome::ValidatedPreview, None, None);
    let first = compile_intent(&intent).expect("intent should compile");
    let second = compile_intent(&intent).expect("repeat should compile");
    assert_eq!(first, second);
    assert_eq!(first.requirements.len(), 22);
    assert_eq!(
        first
            .coverage
            .iter()
            .map(|item| item.requirement_ids.len())
            .sum::<usize>(),
        22
    );
    assert_eq!(first.requirement_provenance.len(), 22);
    assert_eq!(first.verification.actionable_requirements, 22);
    assert_eq!(first.verification.covered_requirements, 22);
    assert_eq!(first.verification.rendered_buttons, 3);
    assert_eq!(first.verification.matched_button_handlers, 3);
    assert_eq!(
        first
            .requirements
            .iter()
            .map(ScopeRequirement::id)
            .collect::<Vec<_>>(),
        [
            "private_study_room.surface.panel",
            "private_study_room.surface.create_button",
            "private_study_room.surface.modal",
            "private_study_room.open.rule",
            "private_study_room.open.open_modal",
            "private_study_room.submit.rule",
            "private_study_room.submit.defer",
            "private_study_room.submit.create_member_role",
            "private_study_room.submit.create_room_channel",
            "private_study_room.submit.deny_everyone_view",
            "private_study_room.submit.allow_member_view",
            "private_study_room.submit.grant_creator",
            "private_study_room.submit.post_welcome",
            "private_study_room.submit.post_hub",
            "private_study_room.submit.register_instance",
            "private_study_room.submit.complete",
            "private_study_room.help.rule",
            "private_study_room.help.respond",
            "private_study_room.join.rule",
            "private_study_room.join.defer",
            "private_study_room.join.grant_member",
            "private_study_room.join.respond",
        ]
    );
    assert!(!first.manifest.generated_objects.contains_key("close"));
    assert!(!first.manifest.generated_objects.contains_key("close_room"));
    assert_eq!(first.manifest.registry_digest.len(), 64);
    assert_eq!(first.manifest.input_intent_hash.len(), 64);
    assert_eq!(first.manifest.semantic_intent_hash.len(), 64);
    assert_eq!(first.manifest.compiled_plan_hash.len(), 64);
    assert_eq!(
        first.manifest.compiled_plan_hash,
        "a0ef96c74dc7605635c960b958dd1ecd47e1ccdf9e5aaeda0f1b771c732b57f1"
    );
}

#[test]
fn explicit_any_member_close_adds_one_live_control_and_handler() {
    let intent = intent(
        IntentRequestedOutcome::ValidatedPreview,
        Some(ClosePolicyV1::AnyMember),
        None,
    );
    let compiled = compile_intent(&intent).expect("intent should compile");
    assert_eq!(compiled.requirements.len(), 26);
    assert_eq!(compiled.verification.rendered_buttons, 4);
    assert_eq!(compiled.verification.matched_button_handlers, 4);
    assert_eq!(
        compiled.manifest.generated_objects["close"],
        "private_study_room__close"
    );
    assert_eq!(
        compiled.manifest.generated_objects["close_room"],
        "private_study_room__close_room"
    );
    assert_eq!(
        compiled.manifest.compiled_plan_hash,
        "84fd08a36052420663523cc89f44f45649d1900a9f14495b03b8d66aef03e5b5"
    );
    assert!(compiled.requirements.iter().any(|requirement| {
        matches!(
            requirement,
            ScopeRequirement::Action {
                action: ScopeAction::TeardownInstance { .. },
                ..
            }
        )
    }));
}

#[test]
fn discussion_intent_cannot_compile_or_mutate() {
    let intent = intent(IntentRequestedOutcome::Discussion, None, None);
    let error = compile_intent(&intent).unwrap_err();
    assert_eq!(error.code, "INTENT_OUTCOME_NOT_COMPILABLE");
}

#[test]
fn compiled_recipe_is_accepted_by_the_existing_atomic_plan_normalizer() {
    let intent = intent(IntentRequestedOutcome::ValidatedPreview, None, None);
    let compiled = compile_intent(&intent).expect("intent should compile");
    let brief = TurnBrief {
        intent: TurnIntent::Build,
        objective: "Create a private study room".to_string(),
        requested_outcome: RequestedOutcome::ValidatedPreview,
        requirements: vec![],
        assumptions: vec![],
        blocking_decisions: vec![],
        verification: TurnVerification {
            validate: true,
            simulation: SimulationProfile::StudyRoom,
        },
    };
    let normalized = normalize_turn_plan(&Draft::new(), &brief, compiled.requirements)
        .expect("compiled recipe should normalize");
    assert_eq!(normalized.len(), 23);
    assert!(matches!(
        normalized.last(),
        Some(ScopeRequirement::NoUnresolvedReferences { .. })
    ));
}

#[test]
fn copy_changes_plan_hash_without_changing_generated_ownership() {
    let first = compile_intent(&intent(
        IntentRequestedOutcome::ValidatedPreview,
        None,
        None,
    ))
    .expect("default intent should compile");
    let second = compile_intent(&intent(
        IntentRequestedOutcome::ValidatedPreview,
        None,
        Some("Start a focused room"),
    ))
    .expect("custom intent should compile");
    assert_ne!(
        first.manifest.input_intent_hash,
        second.manifest.input_intent_hash
    );
    assert_ne!(
        first.manifest.semantic_intent_hash,
        second.manifest.semantic_intent_hash
    );
    assert_ne!(
        first.manifest.compiled_plan_hash,
        second.manifest.compiled_plan_hash
    );
    assert_eq!(
        first.manifest.generated_objects,
        second.manifest.generated_objects
    );
}

#[test]
fn semantic_hash_ignores_revision_and_value_provenance() {
    let one_shot = intent(IntentRequestedOutcome::ValidatedPreview, None, None);
    let resumed = resumed_intent();
    assert_eq!(one_shot.revision(), 1);
    assert_eq!(resumed.revision(), 2);

    let one_shot = compile_intent(&one_shot).expect("one-shot intent should compile");
    let resumed = compile_intent(&resumed).expect("resumed intent should compile");
    assert_ne!(
        one_shot.manifest.input_intent_hash,
        resumed.manifest.input_intent_hash
    );
    assert_eq!(
        one_shot.manifest.semantic_intent_hash,
        resumed.manifest.semantic_intent_hash
    );
    assert_eq!(
        one_shot.manifest.compiled_plan_hash,
        resumed.manifest.compiled_plan_hash
    );
    assert_eq!(one_shot.requirements, resumed.requirements);
}
