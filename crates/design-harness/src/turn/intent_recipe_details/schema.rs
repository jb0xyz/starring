use std::collections::BTreeSet;

use serde_json::Value;

use crate::errors::StructuredError;
use crate::tools::ToolDefinition;

use super::super::schema::inline_schema_value;
use super::validation::{detail_error, exact_facet_set, facet_name};
use super::{
    ExtractPrivateStudyRoomDetailsServingWireV2, ExtractPrivateStudyRoomDetailsWireV1,
    IntentRecipeDetailFacetV3, EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
};

pub fn private_study_room_details_frontier() -> [ToolDefinition; 1] {
    [detail_tool(
        inline_schema_value::<ExtractPrivateStudyRoomDetailsWireV1>(),
        "Extract only the requested private StudyRoom copy, naming, and control literals from the original human turn. Omit unrequested objects, report a facet as unmapped instead of inventing a value, and never author bindings, routes, authorization, actions, permissions, recipe identity, or deployment operations",
    )]
}

pub(crate) fn private_study_room_details_frontier_for(
    required_facets: &[IntentRecipeDetailFacetV3],
) -> Result<[ToolDefinition; 1], StructuredError> {
    let parameters = private_study_room_details_serving_schema(required_facets)?;
    Ok([detail_tool(
        parameters,
        "Extract one nonempty object for every exposed private StudyRoom detail facet using only exact literals from the original human turn. Pattern affixes use flat fields ending in _prefix and _suffix. Never author bindings, routes, authorization, actions, permissions, recipe identity, or deployment operations",
    )])
}

pub(super) fn private_study_room_details_serving_schema(
    required_facets: &[IntentRecipeDetailFacetV3],
) -> Result<Value, StructuredError> {
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
    let mut parameters = inline_schema_value::<ExtractPrivateStudyRoomDetailsServingWireV2>();
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
    if properties.len() != required_names.len() {
        return Err(detail_error(
            "INVALID_RECIPE_DETAIL_SCHEMA",
            "intent.details.schema.properties",
            "The routed recipe detail schema is missing an active facet",
            "Keep every closed detail facet aligned with its serving wire field",
        ));
    }
    root.insert(
        "required".to_string(),
        Value::Array(
            required_names
                .into_iter()
                .map(|name| Value::String(name.to_string()))
                .collect(),
        ),
    );
    Ok(parameters)
}

pub(super) fn validate_serving_root_keys(
    value: &Value,
    required_facets: &[IntentRecipeDetailFacetV3],
) -> Result<(), StructuredError> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    let expected = exact_facet_set(required_facets, "intent.details.required_facets")?
        .into_iter()
        .map(facet_name)
        .collect::<BTreeSet<_>>();
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(detail_error(
            "RECIPE_DETAIL_FRONTIER_MISMATCH",
            "intent.details.arguments",
            "The recipe detail arguments do not match the active facet frontier",
            "Provide every exposed facet object exactly once and no unexposed facet object",
        ));
    }
    Ok(())
}

fn detail_tool(parameters: Value, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: EXTRACT_PRIVATE_STUDY_ROOM_DETAILS.to_string(),
        description: description.to_string(),
        parameters,
    }
}
