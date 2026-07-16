use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::errors::StructuredError;

use super::model::{
    ClosePolicyV1, ExistingChannelKey, FeatureConfigurationV1, FeatureId, FeatureIntentV1,
    IntentLocaleV1, IntentRequestedOutcome, IntentResolutionContext, IntentValue,
    IntentValueSource, IntentWorkspaceV2, ManagedPrivateRoomControlsDraftV1,
    ManagedPrivateRoomCopyDraftV1, ManagedPrivateRoomDraftV1, ManagedPrivateRoomNamingDraftV1,
    MissingDecision, RecipeRef, RoomNamePatternV1, INTENT_SCHEMA_VERSION,
    PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION,
};
use super::normalize::{prepare_intent_workspace, PreparedIntentWorkspaceV2, ValidatedIntentV2};

const INITIAL_INTENT_REVISION: u64 = 1;
const PRIVATE_STUDY_ROOM_FEATURE_ID: &str = "private_study_room";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrivateStudyRoomProposalV2 {
    #[schemars(
        description = "Exact requested result: discussion, working_draft, or validated_preview"
    )]
    pub requested_outcome: IntentRequestedOutcome,
    #[serde(default)]
    #[schemars(description = "Exact available channel key explicitly selected by the human")]
    pub hub_channel: Option<ExistingChannelKey>,
    #[serde(default)]
    pub locale: Option<IntentLocaleV1>,
    #[serde(default)]
    pub copy: PrivateStudyRoomCopyProposalV1,
    #[serde(default)]
    pub naming: PrivateStudyRoomNamingProposalV1,
    #[serde(default)]
    pub controls: PrivateStudyRoomControlsProposalV1,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrivateStudyRoomCopyProposalV1 {
    #[serde(default)]
    pub launcher_content: Option<String>,
    #[serde(default)]
    pub create_button_label: Option<String>,
    #[serde(default)]
    pub modal_title: Option<String>,
    #[serde(default)]
    pub room_name_label: Option<String>,
    #[serde(default)]
    pub welcome_content: Option<RoomNamePatternV1>,
    #[serde(default)]
    pub hub_announcement: Option<RoomNamePatternV1>,
    #[serde(default)]
    pub completed_response: Option<RoomNamePatternV1>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrivateStudyRoomNamingProposalV1 {
    #[serde(default)]
    pub channel_name: Option<RoomNamePatternV1>,
    #[serde(default)]
    pub member_role_name: Option<RoomNamePatternV1>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrivateStudyRoomControlsProposalV1 {
    #[serde(default)]
    pub help_label: Option<String>,
    #[serde(default)]
    pub help_response: Option<String>,
    #[serde(default)]
    pub join_label: Option<String>,
    #[serde(default)]
    pub joined_response: Option<String>,
    #[serde(default)]
    pub close_label: Option<String>,
    #[serde(default)]
    pub closed_response: Option<String>,
    #[serde(default)]
    pub close_policy: Option<ClosePolicyV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntentProposalOutcomeV2 {
    NeedsInput {
        revision: u64,
        decisions: Vec<MissingDecision>,
    },
    Resolved {
        revision: u64,
        intent: ValidatedIntentV2,
    },
}

pub fn propose_private_study_room(
    proposal: PrivateStudyRoomProposalV2,
    context: &IntentResolutionContext,
) -> Result<IntentProposalOutcomeV2, StructuredError> {
    match prepare_private_study_room(proposal, context)? {
        PreparedIntentWorkspaceV2::NeedsInput {
            workspace,
            decisions,
        } => Ok(IntentProposalOutcomeV2::NeedsInput {
            revision: workspace.revision,
            decisions,
        }),
        PreparedIntentWorkspaceV2::Resolved { intent, .. } => {
            Ok(IntentProposalOutcomeV2::Resolved {
                revision: intent.revision(),
                intent,
            })
        }
    }
}

pub(crate) fn prepare_private_study_room(
    proposal: PrivateStudyRoomProposalV2,
    context: &IntentResolutionContext,
) -> Result<PreparedIntentWorkspaceV2, StructuredError> {
    prepare_intent_workspace(workspace_from_proposal(proposal), context)
}

pub(crate) fn apply_existing_channel_decision(
    workspace: &IntentWorkspaceV2,
    expected_revision: u64,
    channel: ExistingChannelKey,
    context: &IntentResolutionContext,
) -> Result<PreparedIntentWorkspaceV2, StructuredError> {
    if workspace.revision != expected_revision {
        return Err(StructuredError::new(
            "STALE_INTENT_WORKSPACE_REVISION",
            "intent.revision",
            format!(
                "Intent workspace revision {} does not match expected revision {expected_revision}",
                workspace.revision
            ),
            format!("Retry with current revision {}", workspace.revision),
        ));
    }
    let next_revision = workspace.revision.checked_add(1).ok_or_else(|| {
        StructuredError::new(
            "INTENT_WORKSPACE_REVISION_OVERFLOW",
            "intent.revision",
            "Intent workspace revision cannot be incremented",
            "Start a new intent workspace",
        )
    })?;
    let mut updated = workspace.clone();
    let Some(feature) = updated.features.first_mut() else {
        return prepare_intent_workspace(updated, context);
    };
    let FeatureConfigurationV1::ManagedPrivateRoom(configuration) = &mut feature.configuration;
    if configuration.hub_channel.is_some() {
        return Err(StructuredError::new(
            "INTENT_DECISION_NOT_PENDING",
            "intent.features.0.configuration.parameters.hub_channel",
            "The existing-channel decision is not pending",
            "Apply only the active missing decision",
        ));
    }
    configuration.hub_channel = Some(IntentValue::new(channel, IntentValueSource::UserConfirmed));
    updated.revision = next_revision;
    prepare_intent_workspace(updated, context)
}

fn workspace_from_proposal(proposal: PrivateStudyRoomProposalV2) -> IntentWorkspaceV2 {
    IntentWorkspaceV2 {
        schema_version: INTENT_SCHEMA_VERSION,
        revision: INITIAL_INTENT_REVISION,
        requested_outcome: proposal.requested_outcome,
        features: vec![FeatureIntentV1 {
            feature_id: FeatureId(PRIVATE_STUDY_ROOM_FEATURE_ID.to_string()),
            recipe: RecipeRef {
                id: PRIVATE_STUDY_ROOM_RECIPE_ID.to_string(),
                version: PRIVATE_STUDY_ROOM_RECIPE_VERSION,
            },
            configuration: FeatureConfigurationV1::ManagedPrivateRoom(ManagedPrivateRoomDraftV1 {
                hub_channel: extracted(proposal.hub_channel),
                locale: extracted(proposal.locale),
                copy: ManagedPrivateRoomCopyDraftV1 {
                    launcher_content: extracted(proposal.copy.launcher_content),
                    create_button_label: extracted(proposal.copy.create_button_label),
                    modal_title: extracted(proposal.copy.modal_title),
                    room_name_label: extracted(proposal.copy.room_name_label),
                    welcome_content: extracted(proposal.copy.welcome_content),
                    hub_announcement: extracted(proposal.copy.hub_announcement),
                    completed_response: extracted(proposal.copy.completed_response),
                },
                naming: ManagedPrivateRoomNamingDraftV1 {
                    channel_name: extracted(proposal.naming.channel_name),
                    member_role_name: extracted(proposal.naming.member_role_name),
                },
                controls: ManagedPrivateRoomControlsDraftV1 {
                    help_label: extracted(proposal.controls.help_label),
                    help_response: extracted(proposal.controls.help_response),
                    join_label: extracted(proposal.controls.join_label),
                    joined_response: extracted(proposal.controls.joined_response),
                    close_label: extracted(proposal.controls.close_label),
                    closed_response: extracted(proposal.controls.closed_response),
                    close_policy: extracted(proposal.controls.close_policy),
                },
            }),
        }],
    }
}

fn extracted<T>(value: Option<T>) -> Option<IntentValue<T>> {
    value.map(|value| IntentValue::new(value, IntentValueSource::ModelExtracted))
}
