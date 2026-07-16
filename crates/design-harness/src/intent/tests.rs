use super::model::{
    ClosePolicyV1, ExistingChannelKey, FeatureConfigurationV1, FeatureId, FeatureIntentV1,
    IntentLocaleV1, IntentRequestedOutcome, IntentResolutionContext, IntentValue,
    IntentValueSource, IntentWorkspaceV2, ManagedPrivateRoomControlsDraftV1,
    ManagedPrivateRoomCopyDraftV1, ManagedPrivateRoomDraftV1, ManagedPrivateRoomNamingDraftV1,
    RecipeRef, RoomNamePatternV1, INTENT_SCHEMA_VERSION, PRIVATE_STUDY_ROOM_RECIPE_ID,
    PRIVATE_STUDY_ROOM_RECIPE_VERSION,
};
use super::normalize::{resolve_intent_workspace, IntentResolutionV2, ValidatedIntentV2};
use super::proposal::{
    propose_private_study_room, IntentProposalOutcomeV2, PrivateStudyRoomControlsProposalV1,
    PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV2,
};
use serde_json::json;

fn recipe() -> RecipeRef {
    RecipeRef {
        id: PRIVATE_STUDY_ROOM_RECIPE_ID.to_string(),
        version: PRIVATE_STUDY_ROOM_RECIPE_VERSION,
    }
}

fn workspace(configuration: ManagedPrivateRoomDraftV1) -> IntentWorkspaceV2 {
    IntentWorkspaceV2 {
        schema_version: INTENT_SCHEMA_VERSION,
        revision: 1,
        requested_outcome: IntentRequestedOutcome::ValidatedPreview,
        features: vec![FeatureIntentV1 {
            feature_id: FeatureId(" study_rooms ".to_string()),
            recipe: recipe(),
            configuration: FeatureConfigurationV1::ManagedPrivateRoom(configuration),
        }],
    }
}

fn explicit<T>(value: T) -> IntentValue<T> {
    IntentValue::new(value, IntentValueSource::UserExplicit)
}

fn resolve(input: IntentWorkspaceV2) -> Result<IntentResolutionV2, crate::StructuredError> {
    let context = IntentResolutionContext::from_channel_bindings([
        ExistingChannelKey("study_hub".to_string()),
        ExistingChannelKey("second_hub".to_string()),
    ]);
    resolve_intent_workspace(input, &context)
}

fn resolved_room(intent: &ValidatedIntentV2) -> serde_json::Value {
    serde_json::to_value(intent).expect("validated intent should serialize")["features"][0]
        ["configuration"]["parameters"]
        .clone()
}

#[test]
fn missing_hub_channel_returns_one_stable_decision_without_guessing() {
    let resolution = resolve(workspace(ManagedPrivateRoomDraftV1::default()))
        .expect("workspace should normalize");
    let IntentResolutionV2::NeedsInput {
        workspace,
        decisions,
    } = resolution
    else {
        panic!("expected a missing decision");
    };
    assert_eq!(workspace.schema_version, 2);
    assert_eq!(workspace.features[0].feature_id.as_str(), "study_rooms");
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].id, "study_rooms.hub_channel");
    assert_eq!(decisions[0].options, vec!["second_hub", "study_hub"]);
}

#[test]
fn complete_workspace_materializes_deterministic_defaults_with_provenance() {
    let configuration = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey(" study_hub ".to_string()))),
        locale: None,
        copy: ManagedPrivateRoomCopyDraftV1::default(),
        naming: ManagedPrivateRoomNamingDraftV1::default(),
        controls: ManagedPrivateRoomControlsDraftV1::default(),
    };
    let first = resolve(workspace(configuration.clone())).expect("workspace should resolve");
    let second = resolve(workspace(configuration)).expect("repeat should resolve");
    assert_eq!(first, second);
    let IntentResolutionV2::Resolved { intent } = first else {
        panic!("expected a resolved intent");
    };
    let room = resolved_room(&intent);
    assert_eq!(room["hub_channel"]["value"], "study_hub");
    assert_eq!(room["hub_channel"]["source"], "user_explicit");
    assert_eq!(room["copy"]["create_button_label"]["value"], "Create room");
    assert_eq!(
        room["copy"]["create_button_label"]["source"],
        "recipe_default"
    );
    assert_eq!(room["locale"]["value"], "en");
    assert_eq!(room["controls"]["close"]["policy"], "disabled");
    assert_eq!(room["controls"]["close"]["source"], "recipe_default");
    assert_eq!(
        room["naming"]["member_role_name"]["value"],
        json!({ "prefix": "", "suffix": " members" })
    );
}

#[test]
fn locale_selects_one_consistent_recipe_copy_set() {
    let configuration = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey("study_hub".to_string()))),
        locale: Some(explicit(IntentLocaleV1::Ko)),
        ..ManagedPrivateRoomDraftV1::default()
    };
    let IntentResolutionV2::Resolved { intent } =
        resolve(workspace(configuration)).expect("workspace should resolve")
    else {
        panic!("expected a resolved intent");
    };
    let room = resolved_room(&intent);
    assert_eq!(
        room["copy"]["create_button_label"]["value"],
        "스터디룸 만들기"
    );
    assert_eq!(
        room["naming"]["member_role_name"]["value"]["suffix"],
        " 멤버"
    );
    assert_eq!(room["controls"]["join"]["label"]["value"], "참가하기");
    assert_eq!(
        room["controls"]["join"]["response"]["value"],
        "스터디룸에 참가했습니다"
    );
}

#[test]
fn close_control_has_no_inactive_copy_and_requires_an_explicit_policy() {
    let inactive = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey("study_hub".to_string()))),
        controls: ManagedPrivateRoomControlsDraftV1 {
            close_label: Some(explicit("Close now".to_string())),
            ..ManagedPrivateRoomControlsDraftV1::default()
        },
        ..ManagedPrivateRoomDraftV1::default()
    };
    assert_eq!(
        resolve(workspace(inactive)).unwrap_err().code,
        "INACTIVE_INTENT_CLOSE_SLOT"
    );

    let enabled = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey("study_hub".to_string()))),
        controls: ManagedPrivateRoomControlsDraftV1 {
            close_policy: Some(explicit(ClosePolicyV1::AnyMember)),
            close_label: Some(explicit("Close for everyone".to_string())),
            closed_response: Some(explicit("Closed".to_string())),
            ..ManagedPrivateRoomControlsDraftV1::default()
        },
        ..ManagedPrivateRoomDraftV1::default()
    };
    let IntentResolutionV2::Resolved { intent } =
        resolve(workspace(enabled)).expect("workspace should resolve")
    else {
        panic!("expected a resolved intent");
    };
    let room = resolved_room(&intent);
    assert_eq!(room["controls"]["close"]["policy"], "any_member");
    assert_eq!(room["controls"]["close"]["source"], "user_explicit");
    assert_eq!(
        room["controls"]["close"]["label"]["value"],
        "Close for everyone"
    );
    assert_eq!(room["controls"]["close"]["response"]["value"], "Closed");
}

#[test]
fn explicit_copy_is_trimmed_but_affix_spacing_is_preserved() {
    let configuration = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey("study_hub".to_string()))),
        locale: None,
        copy: ManagedPrivateRoomCopyDraftV1 {
            create_button_label: Some(explicit("  Make room  ".to_string())),
            ..ManagedPrivateRoomCopyDraftV1::default()
        },
        naming: ManagedPrivateRoomNamingDraftV1 {
            member_role_name: Some(explicit(RoomNamePatternV1 {
                prefix: String::new(),
                suffix: " participants".to_string(),
            })),
            ..ManagedPrivateRoomNamingDraftV1::default()
        },
        controls: ManagedPrivateRoomControlsDraftV1::default(),
    };
    let IntentResolutionV2::Resolved { intent } =
        resolve(workspace(configuration)).expect("workspace should resolve")
    else {
        panic!("expected a resolved intent");
    };
    let room = resolved_room(&intent);
    assert_eq!(room["copy"]["create_button_label"]["value"], "Make room");
    assert_eq!(
        room["naming"]["member_role_name"]["value"]["suffix"],
        " participants"
    );
}

#[test]
fn raw_template_syntax_is_rejected_before_resolution() {
    let configuration = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey("study_hub".to_string()))),
        locale: None,
        copy: ManagedPrivateRoomCopyDraftV1 {
            welcome_content: Some(explicit(RoomNamePatternV1 {
                prefix: "Welcome ${input.room_name}".to_string(),
                suffix: String::new(),
            })),
            ..ManagedPrivateRoomCopyDraftV1::default()
        },
        naming: ManagedPrivateRoomNamingDraftV1::default(),
        controls: ManagedPrivateRoomControlsDraftV1::default(),
    };
    let error = resolve(workspace(configuration)).unwrap_err();
    assert_eq!(error.code, "RAW_INTENT_TEMPLATE_FORBIDDEN");
    assert!(error.location.ends_with("welcome_content.value.prefix"));
}

#[test]
fn unknown_recipe_version_is_rejected_without_latest_resolution() {
    let mut input = workspace(ManagedPrivateRoomDraftV1::default());
    input.features[0].recipe.version = 2;
    let error = resolve(input).unwrap_err();
    assert_eq!(error.code, "UNKNOWN_INTENT_RECIPE_VERSION");
}

#[test]
fn empty_and_oversized_feature_sets_are_rejected() {
    let mut empty = workspace(ManagedPrivateRoomDraftV1::default());
    empty.features.clear();
    assert_eq!(resolve(empty).unwrap_err().code, "EMPTY_INTENT_FEATURES");

    let mut oversized = workspace(ManagedPrivateRoomDraftV1::default());
    oversized.features = (0..2)
        .map(|index| FeatureIntentV1 {
            feature_id: FeatureId(format!("study_rooms_{index}")),
            recipe: recipe(),
            configuration: FeatureConfigurationV1::ManagedPrivateRoom(
                ManagedPrivateRoomDraftV1::default(),
            ),
        })
        .collect();
    assert_eq!(
        resolve(oversized).unwrap_err().code,
        "TOO_MANY_INTENT_FEATURES"
    );
}

#[test]
fn malformed_identifiers_and_binding_keys_are_rejected() {
    let mut bad_feature = workspace(ManagedPrivateRoomDraftV1::default());
    bad_feature.features[0].feature_id = FeatureId("Study Rooms".to_string());
    assert_eq!(
        resolve(bad_feature).unwrap_err().code,
        "INVALID_INTENT_FEATURE_ID"
    );

    let bad_binding = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey("study hub".to_string()))),
        ..ManagedPrivateRoomDraftV1::default()
    };
    assert_eq!(
        resolve(workspace(bad_binding)).unwrap_err().code,
        "INVALID_INTENT_CHANNEL_BINDING"
    );
}

#[test]
fn syntactically_valid_but_unavailable_channel_binding_is_rejected() {
    let configuration = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey("unknown_hub".to_string()))),
        ..ManagedPrivateRoomDraftV1::default()
    };
    let error = resolve(workspace(configuration)).unwrap_err();
    assert_eq!(error.code, "UNKNOWN_INTENT_CHANNEL_BINDING");
}

#[test]
fn strict_serialization_rejects_unknown_recipe_fields() {
    let value = json!({
        "schema_version": 2,
        "revision": 0,
        "requested_outcome": "validated_preview",
        "features": [{
            "feature_id": "study_rooms",
            "recipe": {
                "id": "starring.private_study_room",
                "version": 1
            },
            "configuration": {
                "kind": "managed_private_room",
                "parameters": {
                    "hub_channel": {
                        "value": "study_hub",
                        "source": "user_explicit"
                    },
                    "permissions": ["administrator"]
                }
            }
        }]
    });
    let error = serde_json::from_value::<IntentWorkspaceV2>(value).unwrap_err();
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn multiline_labels_are_rejected_while_messages_remain_multiline() {
    let bad_label = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey("study_hub".to_string()))),
        controls: ManagedPrivateRoomControlsDraftV1 {
            join_label: Some(explicit("Join\nnow".to_string())),
            ..ManagedPrivateRoomControlsDraftV1::default()
        },
        ..ManagedPrivateRoomDraftV1::default()
    };
    assert_eq!(
        resolve(workspace(bad_label)).unwrap_err().code,
        "INVALID_INTENT_TEXT_CONTROL"
    );

    let good_message = ManagedPrivateRoomDraftV1 {
        hub_channel: Some(explicit(ExistingChannelKey("study_hub".to_string()))),
        copy: ManagedPrivateRoomCopyDraftV1 {
            launcher_content: Some(explicit("Study rooms\nCreate one below".to_string())),
            ..ManagedPrivateRoomCopyDraftV1::default()
        },
        ..ManagedPrivateRoomDraftV1::default()
    };
    assert!(matches!(
        resolve(workspace(good_message)),
        Ok(IntentResolutionV2::Resolved { .. })
    ));
}

#[test]
fn model_facing_proposal_rejects_harness_owned_metadata() {
    for field in [
        "schema_version",
        "revision",
        "feature_id",
        "objective",
        "recipe",
        "source",
    ] {
        let mut value = json!({
            "requested_outcome": "validated_preview",
            "hub_channel": "study_hub"
        });
        value
            .as_object_mut()
            .expect("proposal should be an object")
            .insert(field.to_string(), json!(1));
        let error = serde_json::from_value::<PrivateStudyRoomProposalV2>(value).unwrap_err();
        assert!(
            error.to_string().contains("unknown field"),
            "{field}: {error}"
        );
    }
}

#[test]
fn proposal_ingestion_stamps_identity_revision_and_provenance() {
    let context = IntentResolutionContext::from_channel_bindings([ExistingChannelKey(
        "study_hub".to_string(),
    )]);
    let proposal = PrivateStudyRoomProposalV2 {
        requested_outcome: IntentRequestedOutcome::ValidatedPreview,
        hub_channel: Some(ExistingChannelKey("study_hub".to_string())),
        locale: Some(IntentLocaleV1::En),
        copy: PrivateStudyRoomCopyProposalV1::default(),
        naming: PrivateStudyRoomNamingProposalV1::default(),
        controls: PrivateStudyRoomControlsProposalV1::default(),
    };
    let IntentProposalOutcomeV2::Resolved { revision, intent } =
        propose_private_study_room(proposal, &context).expect("proposal should resolve")
    else {
        panic!("expected a resolved proposal");
    };
    assert_eq!(revision, 1);
    assert_eq!(intent.revision(), 1);
    assert_eq!(intent.schema_version(), 2);
    assert_eq!(
        intent.requested_outcome(),
        IntentRequestedOutcome::ValidatedPreview
    );
    let serialized = serde_json::to_value(&intent).expect("validated intent should serialize");
    assert!(serialized.get("objective").is_none());
    assert_eq!(
        serialized["features"][0]["feature_id"],
        "private_study_room"
    );
    assert_eq!(
        serialized["features"][0]["recipe"],
        json!({
            "id": "starring.private_study_room",
            "version": 1
        })
    );
    assert_eq!(
        serialized["features"][0]["configuration"]["parameters"]["hub_channel"]["source"],
        "model_extracted"
    );
    assert_eq!(
        serialized["features"][0]["configuration"]["parameters"]["copy"]["create_button_label"]
            ["source"],
        "recipe_default"
    );
}
