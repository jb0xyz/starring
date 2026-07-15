use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub(crate) const INTENT_SCHEMA_VERSION: u16 = 2;
pub(crate) const PRIVATE_STUDY_ROOM_RECIPE_ID: &str = "starring.private_study_room";
pub(crate) const PRIVATE_STUDY_ROOM_RECIPE_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IntentResolutionContext {
    pub(crate) channel_bindings: BTreeSet<ExistingChannelKey>,
}

impl IntentResolutionContext {
    pub fn from_channel_bindings(values: impl IntoIterator<Item = ExistingChannelKey>) -> Self {
        Self {
            channel_bindings: values.into_iter().collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntentWorkspaceV2 {
    pub schema_version: u16,
    pub revision: u64,
    pub requested_outcome: IntentRequestedOutcome,
    pub features: Vec<FeatureIntentV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntentRequestedOutcome {
    Discussion,
    WorkingDraft,
    ValidatedPreview,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct FeatureId(pub String);

impl FeatureId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ExistingChannelKey(pub String);

impl ExistingChannelKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeRef {
    pub id: String,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeatureIntentV1 {
    pub feature_id: FeatureId,
    pub recipe: RecipeRef,
    pub configuration: FeatureConfigurationV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "parameters",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum FeatureConfigurationV1 {
    ManagedPrivateRoom(ManagedPrivateRoomDraftV1),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntentValue<T> {
    pub value: T,
    pub source: IntentValueSource,
}

impl<T> IntentValue<T> {
    pub fn new(value: T, source: IntentValueSource) -> Self {
        Self { value, source }
    }

    pub fn recipe_default(value: T) -> Self {
        Self::new(value, IntentValueSource::RecipeDefault)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IntentValueSource {
    ModelExtracted,
    UserExplicit,
    UserConfirmed,
    ContextDerived,
    RecipeDefault,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagedPrivateRoomDraftV1 {
    #[serde(default)]
    pub hub_channel: Option<IntentValue<ExistingChannelKey>>,
    #[serde(default)]
    pub locale: Option<IntentValue<IntentLocaleV1>>,
    #[serde(default)]
    pub copy: ManagedPrivateRoomCopyDraftV1,
    #[serde(default)]
    pub naming: ManagedPrivateRoomNamingDraftV1,
    #[serde(default)]
    pub controls: ManagedPrivateRoomControlsDraftV1,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagedPrivateRoomCopyDraftV1 {
    #[serde(default)]
    pub launcher_content: Option<IntentValue<String>>,
    #[serde(default)]
    pub create_button_label: Option<IntentValue<String>>,
    #[serde(default)]
    pub modal_title: Option<IntentValue<String>>,
    #[serde(default)]
    pub room_name_label: Option<IntentValue<String>>,
    #[serde(default)]
    pub welcome_content: Option<IntentValue<RoomNamePatternV1>>,
    #[serde(default)]
    pub hub_announcement: Option<IntentValue<RoomNamePatternV1>>,
    #[serde(default)]
    pub completed_response: Option<IntentValue<RoomNamePatternV1>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagedPrivateRoomNamingDraftV1 {
    #[serde(default)]
    pub channel_name: Option<IntentValue<RoomNamePatternV1>>,
    #[serde(default)]
    pub member_role_name: Option<IntentValue<RoomNamePatternV1>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagedPrivateRoomControlsDraftV1 {
    #[serde(default)]
    pub help_label: Option<IntentValue<String>>,
    #[serde(default)]
    pub help_response: Option<IntentValue<String>>,
    #[serde(default)]
    pub join_label: Option<IntentValue<String>>,
    #[serde(default)]
    pub joined_response: Option<IntentValue<String>>,
    #[serde(default)]
    pub close_label: Option<IntentValue<String>>,
    #[serde(default)]
    pub closed_response: Option<IntentValue<String>>,
    #[serde(default)]
    pub close_policy: Option<IntentValue<ClosePolicyV1>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntentLocaleV1 {
    En,
    Ko,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClosePolicyV1 {
    Disabled,
    AnyMember,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoomNamePatternV1 {
    pub prefix: String,
    pub suffix: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedIntentV2 {
    pub schema_version: u16,
    pub revision: u64,
    pub requested_outcome: IntentRequestedOutcome,
    pub features: Vec<ResolvedFeatureIntentV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedFeatureIntentV1 {
    pub feature_id: FeatureId,
    pub recipe: RecipeRef,
    pub configuration: ResolvedFeatureConfigurationV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "parameters",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum ResolvedFeatureConfigurationV1 {
    ManagedPrivateRoom(ResolvedManagedPrivateRoomV1),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedManagedPrivateRoomV1 {
    pub hub_channel: IntentValue<ExistingChannelKey>,
    pub locale: IntentValue<IntentLocaleV1>,
    pub copy: ResolvedManagedPrivateRoomCopyV1,
    pub naming: ResolvedManagedPrivateRoomNamingV1,
    pub controls: ResolvedManagedPrivateRoomControlsV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedManagedPrivateRoomCopyV1 {
    pub launcher_content: IntentValue<String>,
    pub create_button_label: IntentValue<String>,
    pub modal_title: IntentValue<String>,
    pub room_name_label: IntentValue<String>,
    pub welcome_content: IntentValue<RoomNamePatternV1>,
    pub hub_announcement: IntentValue<RoomNamePatternV1>,
    pub completed_response: IntentValue<RoomNamePatternV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedManagedPrivateRoomNamingV1 {
    pub channel_name: IntentValue<RoomNamePatternV1>,
    pub member_role_name: IntentValue<RoomNamePatternV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedManagedPrivateRoomControlsV1 {
    pub help: ResolvedHelpControlV1,
    pub join: ResolvedJoinControlV1,
    pub close: ResolvedCloseControlV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedHelpControlV1 {
    pub label: IntentValue<String>,
    pub response: IntentValue<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedJoinControlV1 {
    pub label: IntentValue<String>,
    pub response: IntentValue<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "policy", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ResolvedCloseControlV1 {
    Disabled {
        source: IntentValueSource,
    },
    AnyMember {
        source: IntentValueSource,
        label: IntentValue<String>,
        response: IntentValue<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissingDecision {
    pub id: String,
    pub feature_id: FeatureId,
    pub path: String,
    pub kind: MissingDecisionKind,
    pub question: String,
    pub reason: String,
    pub options: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MissingDecisionKind {
    ExistingChannel,
}
