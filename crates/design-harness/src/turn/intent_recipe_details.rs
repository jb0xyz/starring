use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::intent::{PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1};

use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_interpretation::PrivateStudyRoomControlsInterpretationV2;

mod parse;
mod schema;
mod validation;

pub use parse::parse_private_study_room_details;
pub(crate) use parse::parse_private_study_room_details_for_active_serving_with_parameters;
#[cfg(test)]
pub(crate) use parse::{
    parse_private_study_room_details_for_active_serving,
    parse_private_study_room_details_for_serving,
};
pub use schema::private_study_room_details_frontier;
#[cfg(test)]
pub(crate) use schema::private_study_room_details_frontier_for;
pub(crate) use schema::private_study_room_details_frontier_for_fields;

pub const EXTRACT_PRIVATE_STUDY_ROOM_DETAILS: &str = "extract_private_study_room_details";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExtractPrivateStudyRoomDetailsWireV1 {
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    copy: PrivateStudyRoomCopyProposalV1,
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    naming: PrivateStudyRoomNamingProposalV1,
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    controls: PrivateStudyRoomControlsInterpretationV2,
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    #[schemars(length(max = 3))]
    unmapped_facets: Vec<IntentRecipeDetailFacetV3>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExtractPrivateStudyRoomDetailsServingWireV2 {
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    copy: PrivateStudyRoomCopyServingWireV2,
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    naming: PrivateStudyRoomNamingServingWireV2,
    #[serde(default, deserialize_with = "deserialize_default_on_null")]
    controls: PrivateStudyRoomControlsInterpretationV2,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PrivateStudyRoomCopyServingWireV2 {
    #[serde(default)]
    launcher_content: Option<String>,
    #[serde(default)]
    create_button_label: Option<String>,
    #[serde(default)]
    modal_title: Option<String>,
    #[serde(default)]
    room_name_label: Option<String>,
    #[serde(default)]
    welcome_content_prefix: Option<String>,
    #[serde(default)]
    welcome_content_suffix: Option<String>,
    #[serde(default)]
    hub_announcement_prefix: Option<String>,
    #[serde(default)]
    hub_announcement_suffix: Option<String>,
    #[serde(default)]
    completed_response_prefix: Option<String>,
    #[serde(default)]
    completed_response_suffix: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PrivateStudyRoomNamingServingWireV2 {
    #[serde(default)]
    channel_name_prefix: Option<String>,
    #[serde(default)]
    channel_name_suffix: Option<String>,
    #[serde(default)]
    member_role_name_prefix: Option<String>,
    #[serde(default)]
    member_role_name_suffix: Option<String>,
}

struct PrivateStudyRoomDetailsCandidateV1 {
    copy: PrivateStudyRoomCopyProposalV1,
    naming: PrivateStudyRoomNamingProposalV1,
    controls: PrivateStudyRoomControlsInterpretationV2,
    unmapped_facets: Vec<IntentRecipeDetailFacetV3>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateStudyRoomDetailsV1 {
    expected_revision: u64,
    core_semantic_digest: String,
    copy: PrivateStudyRoomCopyProposalV1,
    naming: PrivateStudyRoomNamingProposalV1,
    controls: PrivateStudyRoomControlsInterpretationV2,
    covered_facets: Vec<IntentRecipeDetailFacetV3>,
}

impl PrivateStudyRoomDetailsV1 {
    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    pub fn copy(&self) -> &PrivateStudyRoomCopyProposalV1 {
        &self.copy
    }

    pub fn core_semantic_digest(&self) -> &str {
        &self.core_semantic_digest
    }

    pub fn naming(&self) -> &PrivateStudyRoomNamingProposalV1 {
        &self.naming
    }

    pub fn controls(&self) -> &PrivateStudyRoomControlsInterpretationV2 {
        &self.controls
    }

    pub fn covered_facets(&self) -> &[IntentRecipeDetailFacetV3] {
        &self.covered_facets
    }
}

fn deserialize_default_on_null<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}
