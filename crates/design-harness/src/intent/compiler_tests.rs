use crate::draft::Draft;
use crate::turn::{
    normalize_turn_plan, RequestedOutcome, ScopeAction, ScopeRequirement, SimulationProfile,
    TurnBrief, TurnIntent, TurnVerification,
};

use super::normalize::PreparedIntentWorkspaceV2;
use super::proposal::{apply_existing_channel_decision, prepare_private_study_room};
use super::{
    compile_intent, compiled_intents_behaviorally_equivalent, propose_private_study_room,
    recipe_descriptor_digest_v1, recipe_registry_digest_v1, verify_outcome_only_finalization,
    ClosePolicyV1, ExistingChannelKey, IntentLocaleV1, IntentProposalOutcomeV2,
    IntentRequestedOutcome, IntentResolutionContext, PrivateStudyRoomControlsProposalV1,
    PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV2,
    RecipeKindV1, ValidatedIntentV2,
};

fn context() -> IntentResolutionContext {
    IntentResolutionContext::from_channel_bindings([ExistingChannelKey("study_hub".to_string())])
}

fn intent(
    requested_outcome: IntentRequestedOutcome,
    close_policy: Option<ClosePolicyV1>,
    launcher_content: Option<&str>,
) -> ValidatedIntentV2 {
    let proposal = proposal(
        requested_outcome,
        close_policy,
        launcher_content,
        Some(ExistingChannelKey("study_hub".to_string())),
    );
    let IntentProposalOutcomeV2::Resolved { intent, .. } =
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
) -> PrivateStudyRoomProposalV2 {
    PrivateStudyRoomProposalV2 {
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

fn resumed_intent() -> ValidatedIntentV2 {
    let prepared = prepare_private_study_room(
        proposal(IntentRequestedOutcome::ValidatedPreview, None, None, None),
        &context(),
    )
    .expect("incomplete proposal should prepare");
    let PreparedIntentWorkspaceV2::NeedsInput {
        workspace,
        decisions,
    } = prepared
    else {
        panic!("expected a pending hub decision");
    };
    assert_eq!(decisions.len(), 1);
    let expected_revision = workspace.revision;
    let PreparedIntentWorkspaceV2::Resolved { intent, .. } = apply_existing_channel_decision(
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
    assert_eq!(
        first.manifest.registry_digest,
        recipe_registry_digest_v1().expect("registry should hash")
    );
    assert_ne!(
        first.manifest.registry_digest,
        recipe_descriptor_digest_v1(RecipeKindV1::PrivateStudyRoomV1)
            .expect("descriptor should hash")
    );
    assert_eq!(first.manifest.compiler_input_hash.len(), 64);
    assert_eq!(first.manifest.semantic_intent_hash.len(), 64);
    assert_eq!(first.manifest.compiled_plan_hash.len(), 64);
    assert_eq!(first.manifest.identity_revision, 2);
    assert_eq!(first.manifest.compiler_revision, 1);
    assert_eq!(
        first.manifest.compiler_input_hash,
        "0a21e7663c7cbed2c83481964e0e9f047038b00e32a51a8aba58dc910616ff08"
    );
    assert_eq!(
        first.manifest.semantic_intent_hash,
        "72d5a64668bae28df5b174a7144414c4ab2b67665a0c853fa33697b87698945f"
    );
    assert_eq!(
        first.manifest.compiled_plan_hash,
        "060723d30880df827504b939dc84c8dd79581c4a33c4116f31cb5dd23218dee3"
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
    assert_ne!(
        compiled.manifest.compiled_plan_hash,
        "060723d30880df827504b939dc84c8dd79581c4a33c4116f31cb5dd23218dee3"
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
        first.manifest.compiler_input_hash,
        second.manifest.compiler_input_hash
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
        one_shot.manifest.compiler_input_hash,
        resumed.manifest.compiler_input_hash
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

#[test]
fn compiler_input_identity_tracks_revision_independently() {
    let baseline = intent(IntentRequestedOutcome::ValidatedPreview, None, None);
    let mut revised = baseline.clone();
    revised.resolved_mut().revision += 1;

    let baseline = compile_intent(&baseline).expect("baseline intent should compile");
    let revised = compile_intent(&revised).expect("revised intent should compile");

    assert_ne!(
        baseline.manifest.compiler_input_hash,
        revised.manifest.compiler_input_hash
    );
    assert_eq!(
        baseline.manifest.semantic_intent_hash,
        revised.manifest.semantic_intent_hash
    );
    assert_eq!(
        baseline.manifest.compiled_plan_hash,
        revised.manifest.compiled_plan_hash
    );
}

#[test]
fn compiler_input_identity_tracks_value_provenance_independently() {
    let baseline = intent(IntentRequestedOutcome::ValidatedPreview, None, None);
    let mut reprovenanced = baseline.clone();
    let super::model::ResolvedFeatureConfigurationV1::ManagedPrivateRoom(room) =
        &mut reprovenanced.resolved_mut().features[0].configuration;
    room.hub_channel.source = super::model::IntentValueSource::UserConfirmed;

    let baseline = compile_intent(&baseline).expect("baseline intent should compile");
    let reprovenanced =
        compile_intent(&reprovenanced).expect("reprovenanced intent should compile");

    assert_ne!(
        baseline.manifest.compiler_input_hash,
        reprovenanced.manifest.compiler_input_hash
    );
    assert_eq!(
        baseline.manifest.semantic_intent_hash,
        reprovenanced.manifest.semantic_intent_hash
    );
    assert_eq!(
        baseline.manifest.compiled_plan_hash,
        reprovenanced.manifest.compiled_plan_hash
    );
}

#[test]
fn requested_outcome_is_semantic_even_when_the_plan_is_identical() {
    let working = compile_intent(&intent(IntentRequestedOutcome::WorkingDraft, None, None))
        .expect("working draft intent should compile");
    let preview = compile_intent(&intent(
        IntentRequestedOutcome::ValidatedPreview,
        None,
        None,
    ))
    .expect("preview intent should compile");

    assert_ne!(
        working.manifest.compiler_input_hash,
        preview.manifest.compiler_input_hash
    );
    assert_ne!(
        working.manifest.semantic_intent_hash,
        preview.manifest.semantic_intent_hash
    );
    assert_eq!(
        working.manifest.compiled_plan_hash,
        preview.manifest.compiled_plan_hash
    );
    assert_eq!(working.requirements, preview.requirements);
    assert!(compiled_intents_behaviorally_equivalent(&working, &preview));
    assert!(verify_outcome_only_finalization(&working, &preview, &preview).is_ok());

    let mut changed_manifest = preview.clone();
    changed_manifest.manifest.compiler_revision += 1;
    assert!(!compiled_intents_behaviorally_equivalent(
        &working,
        &changed_manifest
    ));

    let changed_behavior = compile_intent(&intent(
        IntentRequestedOutcome::ValidatedPreview,
        None,
        Some("Different launcher"),
    ))
    .expect("changed intent should compile");
    assert!(!compiled_intents_behaviorally_equivalent(
        &working,
        &changed_behavior
    ));
    assert_eq!(
        verify_outcome_only_finalization(&working, &changed_behavior, &preview)
            .unwrap_err()
            .code,
        "INTENT_OUTCOME_FINALIZATION_BEHAVIOR_CHANGED"
    );
}

#[test]
fn executable_semantic_mutations_change_semantic_and_plan_identity() {
    let baseline_proposal = proposal(
        IntentRequestedOutcome::ValidatedPreview,
        None,
        None,
        Some(ExistingChannelKey("study_hub".to_string())),
    );
    let baseline = compile_proposal(baseline_proposal.clone(), context());

    let mut hub = baseline_proposal.clone();
    hub.hub_channel = Some(ExistingChannelKey("archive_hub".to_string()));
    let hub_context = IntentResolutionContext::from_channel_bindings([
        ExistingChannelKey("study_hub".to_string()),
        ExistingChannelKey("archive_hub".to_string()),
    ]);

    let mut locale = baseline_proposal.clone();
    locale.locale = Some(IntentLocaleV1::Ko);

    let mut copy = baseline_proposal.clone();
    copy.copy.launcher_content = Some("Open a focused study room".to_string());

    let mut naming = baseline_proposal.clone();
    naming.naming.channel_name = Some(super::RoomNamePatternV1 {
        prefix: "focus-".to_string(),
        suffix: String::new(),
    });

    let mut control = baseline_proposal.clone();
    control.controls.help_label = Some("Guide".to_string());

    let mut close = baseline_proposal;
    close.controls.close_policy = Some(ClosePolicyV1::AnyMember);

    let variants = [
        ("hub", compile_proposal(hub, hub_context)),
        ("locale", compile_proposal(locale, context())),
        ("copy", compile_proposal(copy, context())),
        ("naming", compile_proposal(naming, context())),
        ("control", compile_proposal(control, context())),
        ("close", compile_proposal(close, context())),
    ];

    for (name, variant) in variants {
        assert_ne!(
            baseline.manifest.compiler_input_hash, variant.manifest.compiler_input_hash,
            "{name} did not change compiler input identity"
        );
        assert_ne!(
            baseline.manifest.semantic_intent_hash, variant.manifest.semantic_intent_hash,
            "{name} did not change semantic intent identity"
        );
        assert_ne!(
            baseline.manifest.compiled_plan_hash, variant.manifest.compiled_plan_hash,
            "{name} did not change compiled plan identity"
        );
        assert_ne!(
            baseline.requirements, variant.requirements,
            "{name} did not change executable requirements"
        );
    }
}

fn compile_proposal(
    proposal: PrivateStudyRoomProposalV2,
    context: IntentResolutionContext,
) -> super::CompiledIntentV2 {
    let IntentProposalOutcomeV2::Resolved { intent, .. } =
        propose_private_study_room(proposal, &context).expect("proposal should resolve")
    else {
        panic!("expected a resolved intent");
    };
    compile_intent(&intent).expect("resolved intent should compile")
}
