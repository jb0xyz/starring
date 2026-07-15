use serde::Serialize;

use crate::errors::StructuredError;

use super::identity::{canonical_json_digest, IdentityErrorSpec};
use super::model::{
    ExistingChannelKey, FeatureId, IntentLocaleV1, IntentRequestedOutcome, IntentValue, RecipeRef,
    ResolvedCloseControlV1, ResolvedFeatureConfigurationV1, ResolvedFeatureIntentV1,
    ResolvedHelpControlV1, ResolvedIntentV2, ResolvedJoinControlV1,
    ResolvedManagedPrivateRoomControlsV1, ResolvedManagedPrivateRoomCopyV1,
    ResolvedManagedPrivateRoomNamingV1, ResolvedManagedPrivateRoomV1, RoomNamePatternV1,
};
use super::normalize::ValidatedIntentV2;

const SEMANTIC_INTENT_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.semantic_intent.v2\0";

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticIntentProjectionV2 {
    schema_version: u16,
    requested_outcome: IntentRequestedOutcome,
    features: Vec<SemanticFeatureProjectionV1>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticFeatureProjectionV1 {
    feature_id: FeatureId,
    recipe: SemanticRecipeProjectionV1,
    configuration: SemanticFeatureConfigurationProjectionV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticRecipeProjectionV1 {
    id: String,
    version: u32,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    content = "parameters",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum SemanticFeatureConfigurationProjectionV1 {
    ManagedPrivateRoom(SemanticManagedPrivateRoomProjectionV1),
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticManagedPrivateRoomProjectionV1 {
    hub_channel: ExistingChannelKey,
    locale: IntentLocaleV1,
    copy: SemanticManagedPrivateRoomCopyProjectionV1,
    naming: SemanticManagedPrivateRoomNamingProjectionV1,
    controls: SemanticManagedPrivateRoomControlsProjectionV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticManagedPrivateRoomCopyProjectionV1 {
    launcher_content: String,
    create_button_label: String,
    modal_title: String,
    room_name_label: String,
    welcome_content: RoomNamePatternV1,
    hub_announcement: RoomNamePatternV1,
    completed_response: RoomNamePatternV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticManagedPrivateRoomNamingProjectionV1 {
    channel_name: RoomNamePatternV1,
    member_role_name: RoomNamePatternV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticManagedPrivateRoomControlsProjectionV1 {
    help: SemanticHelpControlProjectionV1,
    join: SemanticJoinControlProjectionV1,
    close: SemanticCloseControlProjectionV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticHelpControlProjectionV1 {
    label: String,
    response: String,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticJoinControlProjectionV1 {
    label: String,
    response: String,
}

#[derive(Serialize)]
#[serde(tag = "policy", rename_all = "snake_case", deny_unknown_fields)]
enum SemanticCloseControlProjectionV1 {
    Disabled,
    AnyMember { label: String, response: String },
}

pub(super) fn semantic_intent_hash(intent: &ValidatedIntentV2) -> Result<String, StructuredError> {
    semantic_resolved_intent_hash(intent.resolved())
}

fn semantic_resolved_intent_hash(intent: &ResolvedIntentV2) -> Result<String, StructuredError> {
    let projection = project_intent(intent);
    canonical_json_digest(
        SEMANTIC_INTENT_DIGEST_DOMAIN_V2,
        &projection,
        IdentityErrorSpec::new(
            "INTENT_COMPILER_SERIALIZATION_FAILED",
            "intent.compiler.semantic_input",
            "The semantic intent projection could not be serialized deterministically",
        ),
    )
}

fn project_intent(intent: &ResolvedIntentV2) -> SemanticIntentProjectionV2 {
    let ResolvedIntentV2 {
        schema_version,
        revision: _,
        requested_outcome,
        features,
    } = intent;
    SemanticIntentProjectionV2 {
        schema_version: *schema_version,
        requested_outcome: *requested_outcome,
        features: features.iter().map(project_feature).collect(),
    }
}

fn project_feature(feature: &ResolvedFeatureIntentV1) -> SemanticFeatureProjectionV1 {
    let ResolvedFeatureIntentV1 {
        feature_id,
        recipe,
        configuration,
    } = feature;
    let RecipeRef { id, version } = recipe;
    SemanticFeatureProjectionV1 {
        feature_id: feature_id.clone(),
        recipe: SemanticRecipeProjectionV1 {
            id: id.clone(),
            version: *version,
        },
        configuration: project_configuration(configuration),
    }
}

fn project_configuration(
    configuration: &ResolvedFeatureConfigurationV1,
) -> SemanticFeatureConfigurationProjectionV1 {
    match configuration {
        ResolvedFeatureConfigurationV1::ManagedPrivateRoom(room) => {
            SemanticFeatureConfigurationProjectionV1::ManagedPrivateRoom(project_private_room(room))
        }
    }
}

fn project_private_room(
    room: &ResolvedManagedPrivateRoomV1,
) -> SemanticManagedPrivateRoomProjectionV1 {
    let ResolvedManagedPrivateRoomV1 {
        hub_channel,
        locale,
        copy,
        naming,
        controls,
    } = room;
    let IntentValue {
        value: hub_channel_value,
        source: _,
    } = hub_channel;
    let IntentValue {
        value: locale_value,
        source: _,
    } = locale;
    SemanticManagedPrivateRoomProjectionV1 {
        hub_channel: hub_channel_value.clone(),
        locale: *locale_value,
        copy: project_copy(copy),
        naming: project_naming(naming),
        controls: project_controls(controls),
    }
}

fn project_copy(
    copy: &ResolvedManagedPrivateRoomCopyV1,
) -> SemanticManagedPrivateRoomCopyProjectionV1 {
    let ResolvedManagedPrivateRoomCopyV1 {
        launcher_content,
        create_button_label,
        modal_title,
        room_name_label,
        welcome_content,
        hub_announcement,
        completed_response,
    } = copy;
    let IntentValue {
        value: launcher_content_value,
        source: _,
    } = launcher_content;
    let IntentValue {
        value: create_button_label_value,
        source: _,
    } = create_button_label;
    let IntentValue {
        value: modal_title_value,
        source: _,
    } = modal_title;
    let IntentValue {
        value: room_name_label_value,
        source: _,
    } = room_name_label;
    let IntentValue {
        value: welcome_content_value,
        source: _,
    } = welcome_content;
    let IntentValue {
        value: hub_announcement_value,
        source: _,
    } = hub_announcement;
    let IntentValue {
        value: completed_response_value,
        source: _,
    } = completed_response;
    SemanticManagedPrivateRoomCopyProjectionV1 {
        launcher_content: launcher_content_value.clone(),
        create_button_label: create_button_label_value.clone(),
        modal_title: modal_title_value.clone(),
        room_name_label: room_name_label_value.clone(),
        welcome_content: welcome_content_value.clone(),
        hub_announcement: hub_announcement_value.clone(),
        completed_response: completed_response_value.clone(),
    }
}

fn project_naming(
    naming: &ResolvedManagedPrivateRoomNamingV1,
) -> SemanticManagedPrivateRoomNamingProjectionV1 {
    let ResolvedManagedPrivateRoomNamingV1 {
        channel_name,
        member_role_name,
    } = naming;
    let IntentValue {
        value: channel_name_value,
        source: _,
    } = channel_name;
    let IntentValue {
        value: member_role_name_value,
        source: _,
    } = member_role_name;
    SemanticManagedPrivateRoomNamingProjectionV1 {
        channel_name: channel_name_value.clone(),
        member_role_name: member_role_name_value.clone(),
    }
}

fn project_controls(
    controls: &ResolvedManagedPrivateRoomControlsV1,
) -> SemanticManagedPrivateRoomControlsProjectionV1 {
    let ResolvedManagedPrivateRoomControlsV1 { help, join, close } = controls;
    SemanticManagedPrivateRoomControlsProjectionV1 {
        help: project_help(help),
        join: project_join(join),
        close: project_close(close),
    }
}

fn project_help(help: &ResolvedHelpControlV1) -> SemanticHelpControlProjectionV1 {
    let ResolvedHelpControlV1 { label, response } = help;
    let IntentValue {
        value: label_value,
        source: _,
    } = label;
    let IntentValue {
        value: response_value,
        source: _,
    } = response;
    SemanticHelpControlProjectionV1 {
        label: label_value.clone(),
        response: response_value.clone(),
    }
}

fn project_join(join: &ResolvedJoinControlV1) -> SemanticJoinControlProjectionV1 {
    let ResolvedJoinControlV1 { label, response } = join;
    let IntentValue {
        value: label_value,
        source: _,
    } = label;
    let IntentValue {
        value: response_value,
        source: _,
    } = response;
    SemanticJoinControlProjectionV1 {
        label: label_value.clone(),
        response: response_value.clone(),
    }
}

fn project_close(close: &ResolvedCloseControlV1) -> SemanticCloseControlProjectionV1 {
    match close {
        ResolvedCloseControlV1::Disabled { source: _ } => {
            SemanticCloseControlProjectionV1::Disabled
        }
        ResolvedCloseControlV1::AnyMember {
            source: _,
            label,
            response,
        } => {
            let IntentValue {
                value: label_value,
                source: _,
            } = label;
            let IntentValue {
                value: response_value,
                source: _,
            } = response;
            SemanticCloseControlProjectionV1::AnyMember {
                label: label_value.clone(),
                response: response_value.clone(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{
        propose_private_study_room, ExistingChannelKey, IntentProposalOutcomeV2,
        IntentResolutionContext, PrivateStudyRoomControlsProposalV1,
        PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1,
        PrivateStudyRoomProposalV2,
    };

    fn resolved() -> ResolvedIntentV2 {
        let proposal = PrivateStudyRoomProposalV2 {
            requested_outcome: IntentRequestedOutcome::ValidatedPreview,
            hub_channel: Some(ExistingChannelKey("community_hub".to_string())),
            locale: Some(IntentLocaleV1::En),
            copy: PrivateStudyRoomCopyProposalV1::default(),
            naming: PrivateStudyRoomNamingProposalV1::default(),
            controls: PrivateStudyRoomControlsProposalV1::default(),
        };
        let context = IntentResolutionContext::from_channel_bindings([ExistingChannelKey(
            "community_hub".to_string(),
        )]);
        let IntentProposalOutcomeV2::Resolved { intent, .. } =
            propose_private_study_room(proposal, &context).expect("proposal should resolve")
        else {
            panic!("expected resolved intent");
        };
        intent.resolved().clone()
    }

    #[test]
    fn semantic_projection_binds_outcome_feature_and_recipe_identity_independently() {
        let baseline = resolved();
        let baseline_hash = semantic_resolved_intent_hash(&baseline).expect("baseline should hash");

        let mut outcome = baseline.clone();
        outcome.requested_outcome = IntentRequestedOutcome::WorkingDraft;

        let mut feature = baseline.clone();
        feature.features[0].feature_id = FeatureId("alternate_feature".to_string());

        let mut recipe_id = baseline.clone();
        recipe_id.features[0].recipe.id = "starring.alternate".to_string();

        let mut recipe_version = baseline;
        recipe_version.features[0].recipe.version += 1;

        for (name, variant) in [
            ("requested_outcome", outcome),
            ("feature_id", feature),
            ("recipe_id", recipe_id),
            ("recipe_version", recipe_version),
        ] {
            assert_ne!(
                baseline_hash,
                semantic_resolved_intent_hash(&variant).expect("variant should hash"),
                "{name} was omitted from semantic identity"
            );
        }
    }
}
