use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::intent::{PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1};
use crate::tools::ToolDefinition;

use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_interpretation::{
    normalize_private_study_room_details, PrivateStudyRoomControlsInterpretationV2,
};
use super::schema::inline_schema_value;

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

pub fn private_study_room_details_frontier() -> [ToolDefinition; 1] {
    [detail_tool(
        inline_schema_value::<ExtractPrivateStudyRoomDetailsWireV1>(),
        "Extract only the requested private StudyRoom copy, naming, and control literals from the original human turn. Omit unrequested objects, report a facet as unmapped instead of inventing a value, and never author bindings, routes, authorization, actions, permissions, recipe identity, or deployment operations",
    )]
}

pub(crate) fn private_study_room_details_frontier_for(
    required_facets: &[IntentRecipeDetailFacetV3],
) -> Result<[ToolDefinition; 1], StructuredError> {
    let required = exact_facet_set(required_facets, "intent.details.required_facets")?;
    if required.is_empty() {
        return Err(detail_error(
            "EMPTY_RECIPE_DETAIL_REQUEST",
            "intent.details.required_facets",
            "The detail extractor has no active facet",
            "Use the deterministic default path when no detail facet is requested",
        ));
    }
    let required_names = required
        .into_iter()
        .map(facet_name)
        .collect::<BTreeSet<_>>();
    let mut parameters = inline_schema_value::<ExtractPrivateStudyRoomDetailsWireV1>();
    let root = parameters.as_object_mut().ok_or_else(|| {
        detail_error(
            "INVALID_RECIPE_DETAIL_SCHEMA",
            "intent.details.schema",
            "The generated recipe detail schema root is not an object",
            "Keep the recipe detail wire schema object-shaped",
        )
    })?;
    let properties = root
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            detail_error(
                "INVALID_RECIPE_DETAIL_SCHEMA",
                "intent.details.schema.properties",
                "The generated recipe detail schema has no object properties",
                "Keep the recipe detail wire fields in the generated schema",
            )
        })?;
    properties.retain(|name, _| required_names.contains(name.as_str()));
    root.insert(
        "required".to_string(),
        Value::Array(
            required_names
                .into_iter()
                .map(|name| Value::String(name.to_string()))
                .collect(),
        ),
    );
    Ok([detail_tool(
        parameters,
        "Extract one nonempty object for every exposed private StudyRoom detail facet using only exact literals from the original human turn. Never author bindings, routes, authorization, actions, permissions, recipe identity, or deployment operations",
    )])
}

fn detail_tool(parameters: Value, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: EXTRACT_PRIVATE_STUDY_ROOM_DETAILS.to_string(),
        description: description.to_string(),
        parameters,
    }
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
                &inline_schema_value::<ExtractPrivateStudyRoomDetailsWireV1>(),
            )
        })?;
    normalize_private_study_room_details(&mut input.copy, &mut input.naming, &mut input.controls)?;
    let covered_facets = validate_facets(&input, required_facets)?;
    Ok(PrivateStudyRoomDetailsV1 {
        expected_revision,
        core_semantic_digest: expected_core_semantic_digest.to_string(),
        copy: input.copy,
        naming: input.naming,
        controls: input.controls,
        covered_facets,
    })
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
) -> Result<Vec<IntentRecipeDetailFacetV3>, StructuredError> {
    let required = exact_facet_set(required_facets, "intent.details.required_facets")?;
    if required.is_empty() {
        return Err(detail_error(
            "EMPTY_RECIPE_DETAIL_REQUEST",
            "intent.details.required_facets",
            "The detail extractor has no active facet",
            "Use the deterministic default path when no detail facet is requested",
        ));
    }
    let unmapped = exact_facet_set(&input.unmapped_facets, "intent.details.unmapped_facets")?;
    if !unmapped.is_empty() {
        return Err(detail_error(
            "UNMAPPED_RECIPE_DETAIL_FACET",
            "intent.details.unmapped_facets",
            "At least one requested recipe detail facet was not mapped",
            "Map every selected copy, naming, or controls facet",
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
    Ok(required.into_iter().collect())
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
