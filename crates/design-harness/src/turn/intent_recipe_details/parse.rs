use serde_json::Value;

use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::intent::{
    PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1, RoomNamePatternV1,
};

use super::super::schema::inline_schema_value;
use super::schema::{private_study_room_details_serving_schema, validate_serving_root_keys};
use super::validation::finalize_private_study_room_details;
use super::{
    ExtractPrivateStudyRoomDetailsServingWireV2, ExtractPrivateStudyRoomDetailsWireV1,
    IntentRecipeDetailFacetV3, PrivateStudyRoomCopyServingWireV2,
    PrivateStudyRoomDetailsCandidateV1, PrivateStudyRoomDetailsV1,
    PrivateStudyRoomNamingServingWireV2, EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
};

pub fn parse_private_study_room_details(
    arguments: &str,
    required_facets: &[IntentRecipeDetailFacetV3],
    expected_revision: u64,
    expected_core_semantic_digest: &str,
) -> Result<PrivateStudyRoomDetailsV1, StructuredError> {
    let input = serde_json::from_str::<ExtractPrivateStudyRoomDetailsWireV1>(arguments).map_err(
        |error| {
            translate_tool_arguments_error(
                EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
                &error,
                &inline_schema_value::<ExtractPrivateStudyRoomDetailsWireV1>(),
            )
        },
    )?;
    finalize_private_study_room_details(
        PrivateStudyRoomDetailsCandidateV1 {
            copy: input.copy,
            naming: input.naming,
            controls: input.controls,
            unmapped_facets: input.unmapped_facets,
        },
        required_facets,
        expected_revision,
        expected_core_semantic_digest,
    )
}

pub(crate) fn parse_private_study_room_details_for_serving(
    arguments: &str,
    required_facets: &[IntentRecipeDetailFacetV3],
    expected_revision: u64,
    expected_core_semantic_digest: &str,
    human_message: &str,
) -> Result<PrivateStudyRoomDetailsV1, StructuredError> {
    let parameters = private_study_room_details_serving_schema(required_facets)?;
    let value = serde_json::from_str::<Value>(arguments).map_err(|error| {
        translate_tool_arguments_error(EXTRACT_PRIVATE_STUDY_ROOM_DETAILS, &error, &parameters)
    })?;
    validate_serving_root_keys(&value, required_facets)?;
    let input = serde_json::from_value::<ExtractPrivateStudyRoomDetailsServingWireV2>(value)
        .map_err(|error| {
            translate_tool_arguments_error(EXTRACT_PRIVATE_STUDY_ROOM_DETAILS, &error, &parameters)
        })?;
    let details = finalize_private_study_room_details(
        PrivateStudyRoomDetailsCandidateV1 {
            copy: serving_copy(input.copy),
            naming: serving_naming(input.naming),
            controls: input.controls,
            unmapped_facets: Vec::new(),
        },
        required_facets,
        expected_revision,
        expected_core_semantic_digest,
    )?;
    details.validate_human_literals(human_message)?;
    Ok(details)
}

fn serving_copy(value: PrivateStudyRoomCopyServingWireV2) -> PrivateStudyRoomCopyProposalV1 {
    PrivateStudyRoomCopyProposalV1 {
        launcher_content: value.launcher_content,
        create_button_label: value.create_button_label,
        modal_title: value.modal_title,
        room_name_label: value.room_name_label,
        welcome_content: serving_pattern(
            value.welcome_content_prefix,
            value.welcome_content_suffix,
        ),
        hub_announcement: serving_pattern(
            value.hub_announcement_prefix,
            value.hub_announcement_suffix,
        ),
        completed_response: serving_pattern(
            value.completed_response_prefix,
            value.completed_response_suffix,
        ),
    }
}

fn serving_naming(value: PrivateStudyRoomNamingServingWireV2) -> PrivateStudyRoomNamingProposalV1 {
    PrivateStudyRoomNamingProposalV1 {
        channel_name: serving_pattern(value.channel_name_prefix, value.channel_name_suffix),
        member_role_name: serving_pattern(
            value.member_role_name_prefix,
            value.member_role_name_suffix,
        ),
    }
}

fn serving_pattern(prefix: Option<String>, suffix: Option<String>) -> Option<RoomNamePatternV1> {
    if prefix.is_none() && suffix.is_none() {
        return None;
    }
    Some(RoomNamePatternV1 {
        prefix: prefix.unwrap_or_default(),
        suffix: suffix.unwrap_or_default(),
    })
}
