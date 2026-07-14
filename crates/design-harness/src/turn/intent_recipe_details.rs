use std::collections::BTreeSet;

use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::intent::{PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1};
use crate::tools::ToolDefinition;

use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_interpretation::{
    normalize_private_study_room_details, PrivateStudyRoomControlsInterpretationV2,
};

pub const EXTRACT_PRIVATE_STUDY_ROOM_DETAILS: &str = "extract_private_study_room_details";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExtractPrivateStudyRoomDetailsWireV1 {
    expected_revision: u64,
    #[schemars(length(min = 64, max = 64))]
    core_semantic_digest: String,
    #[serde(deserialize_with = "deserialize_default_on_null")]
    copy: PrivateStudyRoomCopyProposalV1,
    #[serde(deserialize_with = "deserialize_default_on_null")]
    naming: PrivateStudyRoomNamingProposalV1,
    #[serde(deserialize_with = "deserialize_default_on_null")]
    controls: PrivateStudyRoomControlsInterpretationV2,
    #[schemars(length(max = 3))]
    covered_facets: Vec<IntentRecipeDetailFacetV3>,
    #[schemars(length(max = 3))]
    unmapped_facets: Vec<IntentRecipeDetailFacetV3>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateStudyRoomDetailsV1(ExtractPrivateStudyRoomDetailsWireV1);

impl PrivateStudyRoomDetailsV1 {
    pub fn expected_revision(&self) -> u64 {
        self.0.expected_revision
    }

    pub fn copy(&self) -> &PrivateStudyRoomCopyProposalV1 {
        &self.0.copy
    }

    pub fn core_semantic_digest(&self) -> &str {
        &self.0.core_semantic_digest
    }

    pub fn naming(&self) -> &PrivateStudyRoomNamingProposalV1 {
        &self.0.naming
    }

    pub fn controls(&self) -> &PrivateStudyRoomControlsInterpretationV2 {
        &self.0.controls
    }

    pub fn covered_facets(&self) -> &[IntentRecipeDetailFacetV3] {
        &self.0.covered_facets
    }
}

pub fn private_study_room_details_frontier() -> [ToolDefinition; 1] {
    [ToolDefinition {
        name: EXTRACT_PRIVATE_STUDY_ROOM_DETAILS.to_string(),
        description: "Extract only the requested private StudyRoom copy, naming, and control details from the original human turn. Cover every active detail facet exactly once, leave unrequested objects empty, and never author routes, authorization, actions, permissions, recipe identity, or deployment operations".to_string(),
        parameters: schema_value::<ExtractPrivateStudyRoomDetailsWireV1>(),
    }]
}

pub fn parse_private_study_room_details(
    arguments: &str,
    required_facets: &[IntentRecipeDetailFacetV3],
    expected_revision: u64,
    expected_core_semantic_digest: &str,
) -> Result<PrivateStudyRoomDetailsV1, StructuredError> {
    let mut input = serde_json::from_str::<ExtractPrivateStudyRoomDetailsWireV1>(arguments)
        .map_err(|error| {
            translate_tool_arguments_error(
                EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
                &error,
                &schema_value::<ExtractPrivateStudyRoomDetailsWireV1>(),
            )
        })?;
    normalize_private_study_room_details(&mut input.copy, &mut input.naming, &mut input.controls)?;
    validate_binding(&input, expected_revision, expected_core_semantic_digest)?;
    validate_facets(&input, required_facets)?;
    Ok(PrivateStudyRoomDetailsV1(input))
}

fn validate_binding(
    input: &ExtractPrivateStudyRoomDetailsWireV1,
    expected_revision: u64,
    expected_core_semantic_digest: &str,
) -> Result<(), StructuredError> {
    if input.expected_revision != expected_revision {
        return Err(detail_error(
            "STALE_RECIPE_DETAIL_REVISION",
            "intent.details.expected_revision",
            format!(
                "Recipe detail revision {} does not match the active revision {expected_revision}",
                input.expected_revision
            ),
            format!("Retry with expected_revision {expected_revision}"),
        ));
    }
    if input.core_semantic_digest != expected_core_semantic_digest {
        return Err(detail_error(
            "RECIPE_DETAIL_CORE_DIGEST_MISMATCH",
            "intent.details.core_semantic_digest",
            "Recipe details are not bound to the active Core IR",
            "Copy the exact harness-provided Core semantic digest",
        ));
    }
    Ok(())
}

fn deserialize_default_on_null<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

fn validate_facets(
    input: &ExtractPrivateStudyRoomDetailsWireV1,
    required_facets: &[IntentRecipeDetailFacetV3],
) -> Result<(), StructuredError> {
    let required = exact_facet_set(required_facets, "intent.details.required_facets")?;
    if required.is_empty() {
        return Err(detail_error(
            "EMPTY_RECIPE_DETAIL_REQUEST",
            "intent.details.required_facets",
            "The detail extractor has no active facet",
            "Use the deterministic default path when no detail facet is requested",
        ));
    }
    let covered = exact_facet_set(&input.covered_facets, "intent.details.covered_facets")?;
    let unmapped = exact_facet_set(&input.unmapped_facets, "intent.details.unmapped_facets")?;
    if !unmapped.is_empty() {
        return Err(detail_error(
            "UNMAPPED_RECIPE_DETAIL_FACET",
            "intent.details.unmapped_facets",
            "At least one requested recipe detail facet was not mapped",
            "Map every selected copy, naming, or controls facet",
        ));
    }
    if covered != required {
        return Err(detail_error(
            "RECIPE_DETAIL_COVERAGE_MISMATCH",
            "intent.details.covered_facets",
            "The covered recipe detail facets do not exactly match the active facets",
            "Cover each active facet exactly once and no other facet",
        ));
    }
    for facet in [
        IntentRecipeDetailFacetV3::Copy,
        IntentRecipeDetailFacetV3::Naming,
        IntentRecipeDetailFacetV3::Controls,
    ] {
        let has_value = match facet {
            IntentRecipeDetailFacetV3::Copy => copy_has_value(&input.copy),
            IntentRecipeDetailFacetV3::Naming => naming_has_value(&input.naming),
            IntentRecipeDetailFacetV3::Controls => controls_have_value(&input.controls),
        };
        match (required.contains(&facet), has_value) {
            (true, false) => {
                return Err(detail_error(
                    "EMPTY_REQUIRED_RECIPE_DETAIL",
                    format!("intent.details.{}", facet_name(facet)),
                    "A selected recipe detail facet contains no value",
                    "Extract at least one explicit value for every selected facet",
                ));
            }
            (false, true) => {
                return Err(detail_error(
                    "UNREQUESTED_RECIPE_DETAIL",
                    format!("intent.details.{}", facet_name(facet)),
                    "An unselected recipe detail facet contains a value",
                    "Leave every unselected recipe detail object empty",
                ));
            }
            (true, true) | (false, false) => {}
        }
    }
    Ok(())
}

fn exact_facet_set(
    values: &[IntentRecipeDetailFacetV3],
    location: &str,
) -> Result<BTreeSet<IntentRecipeDetailFacetV3>, StructuredError> {
    if values.len() > 3 {
        return Err(detail_error(
            "TOO_MANY_RECIPE_DETAIL_FACETS",
            location,
            "More than three recipe detail facets were supplied",
            "Use only copy, naming, and controls",
        ));
    }
    let set = values.iter().copied().collect::<BTreeSet<_>>();
    if set.len() != values.len() {
        return Err(detail_error(
            "DUPLICATE_RECIPE_DETAIL_FACET",
            location,
            "A recipe detail facet appears more than once",
            "Provide each facet exactly once",
        ));
    }
    Ok(set)
}

fn copy_has_value(value: &PrivateStudyRoomCopyProposalV1) -> bool {
    value.launcher_content.is_some()
        || value.create_button_label.is_some()
        || value.modal_title.is_some()
        || value.room_name_label.is_some()
        || value.welcome_content.is_some()
        || value.hub_announcement.is_some()
        || value.completed_response.is_some()
}

fn naming_has_value(value: &PrivateStudyRoomNamingProposalV1) -> bool {
    value.channel_name.is_some() || value.member_role_name.is_some()
}

fn controls_have_value(value: &PrivateStudyRoomControlsInterpretationV2) -> bool {
    value.help_label.is_some()
        || value.help_response.is_some()
        || value.join_label.is_some()
        || value.joined_response.is_some()
        || value.close_label.is_some()
        || value.closed_response.is_some()
}

fn facet_name(value: IntentRecipeDetailFacetV3) -> &'static str {
    match value {
        IntentRecipeDetailFacetV3::Copy => "copy",
        IntentRecipeDetailFacetV3::Naming => "naming",
        IntentRecipeDetailFacetV3::Controls => "controls",
    }
}

fn schema_value<T: JsonSchema>() -> Value {
    let mut value = serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({}));
    if let Some(root) = value.as_object_mut() {
        root.remove("$schema");
        root.remove("title");
        if let Some(expected_revision) = root
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .and_then(|properties| properties.get_mut("expected_revision"))
            .and_then(Value::as_object_mut)
        {
            expected_revision.remove("format");
            expected_revision.remove("minimum");
        }
    }
    value
}

fn detail_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError {
        code: code.into(),
        location: location.into(),
        message: message.into(),
        hint: hint.into(),
    }
}
