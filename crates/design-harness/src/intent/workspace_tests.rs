use serde_json::json;

use super::model::{FeatureConfigurationV1, IntentValueSource, IntentWorkspaceV2};
use super::normalize::PreparedIntentWorkspaceV2;
use super::proposal::{apply_existing_channel_decision, prepare_private_study_room};
use super::{
    ExistingChannelKey, IntentLocaleV1, IntentProposalOutcomeV2, IntentRequestedOutcome,
    IntentResolutionContext, PrivateStudyRoomControlsProposalV1, PrivateStudyRoomCopyProposalV1,
    PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV2,
};

fn context() -> IntentResolutionContext {
    IntentResolutionContext::from_channel_bindings([
        ExistingChannelKey("study_hub".to_string()),
        ExistingChannelKey("community".to_string()),
    ])
}

fn proposal(locale: IntentLocaleV1) -> PrivateStudyRoomProposalV2 {
    PrivateStudyRoomProposalV2 {
        requested_outcome: IntentRequestedOutcome::ValidatedPreview,
        hub_channel: None,
        locale: Some(locale),
        copy: PrivateStudyRoomCopyProposalV1 {
            launcher_content: Some("  Build a focused room  ".to_string()),
            ..PrivateStudyRoomCopyProposalV1::default()
        },
        naming: PrivateStudyRoomNamingProposalV1::default(),
        controls: PrivateStudyRoomControlsProposalV1::default(),
    }
}

fn incomplete_workspace() -> IntentWorkspaceV2 {
    let PreparedIntentWorkspaceV2::NeedsInput {
        workspace,
        decisions,
    } = prepare_private_study_room(proposal(IntentLocaleV1::En), &context())
        .expect("proposal should prepare")
    else {
        panic!("expected missing input");
    };
    assert_eq!(decisions.len(), 1);
    workspace
}

#[test]
fn incomplete_preparation_preserves_normalized_resumable_state() {
    let PreparedIntentWorkspaceV2::NeedsInput {
        workspace,
        decisions,
    } = prepare_private_study_room(proposal(IntentLocaleV1::En), &context())
        .expect("proposal should prepare")
    else {
        panic!("expected missing input");
    };
    assert_eq!(workspace.revision, 1);
    assert_eq!(workspace.schema_version, 2);
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].id, "private_study_room.hub_channel");
    assert_eq!(decisions[0].options, vec!["community", "study_hub"]);
    assert_eq!(
        decisions[0].question,
        "Which existing channel should host the study-room launcher?"
    );
    assert_eq!(
        decisions[0].reason,
        "The recipe must bind its launcher and discovery panel to one existing channel"
    );
    let FeatureConfigurationV1::ManagedPrivateRoom(configuration) =
        &workspace.features[0].configuration;
    assert!(configuration.hub_channel.is_none());
    let launcher = configuration
        .copy
        .launcher_content
        .as_ref()
        .expect("launcher copy should remain in the workspace");
    assert_eq!(launcher.value, "Build a focused room");
    assert_eq!(launcher.source, IntentValueSource::ModelExtracted);
}

#[test]
fn existing_channel_decision_resumes_and_resolves_once() {
    let workspace = incomplete_workspace();
    let PreparedIntentWorkspaceV2::Resolved { workspace, intent } =
        apply_existing_channel_decision(
            &workspace,
            1,
            ExistingChannelKey("study_hub".to_string()),
            &context(),
        )
        .expect("confirmed binding should resolve")
    else {
        panic!("expected resolved intent");
    };
    assert_eq!(workspace.revision, 2);
    assert_eq!(intent.revision(), 2);
    let FeatureConfigurationV1::ManagedPrivateRoom(configuration) =
        &workspace.features[0].configuration;
    let hub = configuration
        .hub_channel
        .as_ref()
        .expect("hub should be confirmed");
    assert_eq!(hub.value.as_str(), "study_hub");
    assert_eq!(hub.source, IntentValueSource::UserConfirmed);
    let value = serde_json::to_value(intent).expect("intent should serialize");
    assert_eq!(
        value["features"][0]["configuration"]["parameters"]["hub_channel"],
        json!({"value": "study_hub", "source": "user_confirmed"})
    );
}

#[test]
fn stale_revision_fails_without_changing_workspace_bytes() {
    let workspace = incomplete_workspace();
    let before = serde_json::to_vec(&workspace).expect("workspace should serialize");
    let error = apply_existing_channel_decision(
        &workspace,
        0,
        ExistingChannelKey("study_hub".to_string()),
        &context(),
    )
    .expect_err("stale decision should fail");
    assert_eq!(error.code, "STALE_INTENT_WORKSPACE_REVISION");
    assert_eq!(
        serde_json::to_vec(&workspace).expect("workspace should serialize"),
        before
    );
}

#[test]
fn unavailable_binding_fails_without_advancing_caller_workspace() {
    let workspace = incomplete_workspace();
    let error = apply_existing_channel_decision(
        &workspace,
        1,
        ExistingChannelKey("missing_hub".to_string()),
        &context(),
    )
    .expect_err("unknown binding should fail");
    assert_eq!(error.code, "UNKNOWN_INTENT_CHANNEL_BINDING");
    assert_eq!(workspace.revision, 1);
    let FeatureConfigurationV1::ManagedPrivateRoom(configuration) =
        &workspace.features[0].configuration;
    assert!(configuration.hub_channel.is_none());
}

#[test]
fn normalized_workspace_json_snapshot_roundtrip_resumes_identically() {
    let workspace = incomplete_workspace();
    let snapshot = serde_json::to_value(&workspace).expect("workspace should serialize");
    let restored: IntentWorkspaceV2 =
        serde_json::from_value(snapshot.clone()).expect("workspace should restore");
    assert_eq!(
        serde_json::to_value(&restored).expect("restored workspace should serialize"),
        snapshot
    );
    let PreparedIntentWorkspaceV2::Resolved {
        workspace: resumed,
        intent,
    } = apply_existing_channel_decision(
        &restored,
        restored.revision,
        ExistingChannelKey("community".to_string()),
        &context(),
    )
    .expect("restored workspace should resume")
    else {
        panic!("expected resolved intent");
    };
    assert_eq!(resumed.revision, 2);
    assert_eq!(intent.revision(), 2);
}

#[test]
fn public_proposal_outcome_keeps_workspace_out_of_its_wire_shape() {
    let outcome = super::propose_private_study_room(proposal(IntentLocaleV1::En), &context())
        .expect("proposal should be accepted");
    let IntentProposalOutcomeV2::NeedsInput {
        revision,
        decisions,
    } = &outcome
    else {
        panic!("expected public missing input outcome");
    };
    assert_eq!(*revision, 1);
    assert_eq!(decisions.len(), 1);
    let value = serde_json::to_value(outcome).expect("outcome should serialize");
    assert_eq!(value["status"], "needs_input");
    assert_eq!(value["revision"], 1);
    assert!(value.get("workspace").is_none());
    assert!(value.get("intent").is_none());
}

#[test]
fn missing_channel_decision_uses_the_proposal_locale() {
    let PreparedIntentWorkspaceV2::NeedsInput { decisions, .. } =
        prepare_private_study_room(proposal(IntentLocaleV1::Ko), &context())
            .expect("proposal should prepare")
    else {
        panic!("expected missing input");
    };
    assert_eq!(decisions.len(), 1);
    assert_eq!(
        decisions[0].question,
        "스터디룸 실행 패널을 어느 기존 채널에 둘까요?"
    );
    assert_eq!(
        decisions[0].reason,
        "레시피가 실행 패널과 탐색 패널을 배치할 기존 채널 하나를 지정해야 합니다"
    );
}
