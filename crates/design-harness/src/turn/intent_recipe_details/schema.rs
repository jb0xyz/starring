use std::collections::BTreeSet;

use serde_json::Value;

use crate::errors::StructuredError;
use crate::tools::ToolDefinition;

use super::super::schema::inline_schema_value;
use super::super::IntentRecipeDetailFieldV4;
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

#[cfg(test)]
pub(crate) fn private_study_room_details_frontier_for(
    required_facets: &[IntentRecipeDetailFacetV3],
) -> Result<[ToolDefinition; 1], StructuredError> {
    let parameters = private_study_room_details_serving_schema(required_facets)?;
    Ok([detail_tool(
        parameters,
        "Extract one nonempty object for every exposed private StudyRoom detail facet using only exact literals from the original human turn. Pattern affixes use flat fields ending in _prefix and _suffix. Never author bindings, routes, authorization, actions, permissions, recipe identity, or deployment operations",
    )])
}

pub(crate) fn private_study_room_details_frontier_for_fields(
    required_facets: &[IntentRecipeDetailFacetV3],
    required_fields: &[IntentRecipeDetailFieldV4],
) -> Result<[ToolDefinition; 1], StructuredError> {
    let parameters =
        private_study_room_details_serving_schema_for_fields(required_facets, required_fields)?;
    Ok([detail_tool(
        parameters,
        "Extract every exposed private StudyRoom detail leaf exactly once using only exact literals from the original human turn. Every exposed facet object and leaf is required. Pattern affixes use flat fields ending in _prefix and _suffix. Never author bindings, routes, authorization, actions, permissions, recipe identity, or deployment operations",
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

pub(super) fn private_study_room_details_serving_schema_for_fields(
    required_facets: &[IntentRecipeDetailFacetV3],
    required_fields: &[IntentRecipeDetailFieldV4],
) -> Result<Value, StructuredError> {
    let mut parameters = private_study_room_details_serving_schema(required_facets)?;
    let fields = exact_field_set(required_fields)?;
    let facets_from_fields = fields
        .iter()
        .map(|field| field.facet())
        .collect::<BTreeSet<_>>();
    let facets = exact_facet_set(required_facets, "intent.details.required_facets")?;
    if fields.is_empty() || facets_from_fields != facets {
        return Err(detail_error(
            "INVALID_RECIPE_DETAIL_FIELD_FRONTIER",
            "intent.details.required_fields",
            "The active detail fields do not cover exactly the selected facets",
            "Derive every active material detail field from the same grounded human turn",
        ));
    }
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
    for facet in facets {
        let facet_name = facet_name(facet);
        let schema = properties
            .get_mut(facet_name)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                detail_error(
                    "INVALID_RECIPE_DETAIL_SCHEMA",
                    format!("intent.details.schema.properties.{facet_name}"),
                    "The generated recipe detail facet schema is not an object",
                    "Keep every selected detail facet object-shaped",
                )
            })?;
        let leaf_names = fields
            .iter()
            .filter(|field| field.facet() == facet)
            .map(|field| field.as_str())
            .collect::<BTreeSet<_>>();
        let leaf_properties = schema
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                detail_error(
                    "INVALID_RECIPE_DETAIL_SCHEMA",
                    format!("intent.details.schema.properties.{facet_name}.properties"),
                    "The generated recipe detail facet has no leaf properties",
                    "Keep every selected material detail leaf in its facet schema",
                )
            })?;
        leaf_properties.retain(|name, _| leaf_names.contains(name.as_str()));
        if leaf_properties.len() != leaf_names.len() {
            return Err(detail_error(
                "INVALID_RECIPE_DETAIL_SCHEMA",
                format!("intent.details.schema.properties.{facet_name}.properties"),
                "The routed recipe detail schema is missing an active leaf",
                "Keep every grounded material detail field aligned with its serving wire field",
            ));
        }
        for value in leaf_properties.values_mut() {
            *value = serde_json::json!({"type": "string", "minLength": 1});
        }
        schema.insert(
            "required".to_string(),
            Value::Array(
                leaf_names
                    .into_iter()
                    .map(|name| Value::String(name.to_string()))
                    .collect(),
            ),
        );
    }
    Ok(parameters)
}

pub(super) fn validate_serving_root_keys(
    value: &Value,
    required_facets: &[IntentRecipeDetailFacetV3],
) -> Result<(), StructuredError> {
    let Some(object) = value.as_object() else {
        return Err(detail_error(
            "RECIPE_DETAIL_FRONTIER_MISMATCH",
            "intent.details.arguments",
            "The recipe detail arguments root is not an object",
            "Provide every exposed facet object exactly once and no unexposed facet object",
        ));
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

pub(super) fn validate_serving_leaf_keys(
    value: &Value,
    required_facets: &[IntentRecipeDetailFacetV3],
    required_fields: &[IntentRecipeDetailFieldV4],
) -> Result<(), StructuredError> {
    let fields = exact_field_set(required_fields)?;
    for facet in exact_facet_set(required_facets, "intent.details.required_facets")? {
        let name = facet_name(facet);
        let expected = fields
            .iter()
            .filter(|field| field.facet() == facet)
            .map(|field| field.as_str())
            .collect::<BTreeSet<_>>();
        let object = value.get(name).and_then(Value::as_object);
        let actual = object
            .map(|object| object.keys().map(String::as_str).collect::<BTreeSet<_>>())
            .unwrap_or_default();
        if actual != expected {
            return Err(detail_error(
                "RECIPE_DETAIL_FRONTIER_MISMATCH",
                format!("intent.details.arguments.{name}"),
                "The recipe detail arguments do not match the active leaf frontier",
                "Provide every exposed material leaf exactly once and no unexposed leaf",
            ));
        }
        if expected.iter().any(|field| {
            !object
                .and_then(|object| object.get(*field))
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        }) {
            return Err(detail_error(
                "RECIPE_DETAIL_FRONTIER_MISMATCH",
                format!("intent.details.arguments.{name}"),
                "The recipe detail arguments contain an empty or non-string active leaf",
                "Fill every exposed material leaf with one exact nonempty human literal",
            ));
        }
    }
    Ok(())
}

fn exact_field_set(
    values: &[IntentRecipeDetailFieldV4],
) -> Result<BTreeSet<IntentRecipeDetailFieldV4>, StructuredError> {
    if values.len() > 20 {
        return Err(detail_error(
            "TOO_MANY_RECIPE_DETAIL_FIELDS",
            "intent.details.required_fields",
            "More than twenty recipe detail fields were supplied",
            "Use each closed recipe detail field at most once",
        ));
    }
    let set = values.iter().copied().collect::<BTreeSet<_>>();
    if set.len() != values.len() {
        return Err(detail_error(
            "DUPLICATE_RECIPE_DETAIL_FIELD",
            "intent.details.required_fields",
            "A recipe detail field appears more than once",
            "Provide each active material detail field exactly once",
        ));
    }
    Ok(set)
}

fn detail_tool(parameters: Value, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: EXTRACT_PRIVATE_STUDY_ROOM_DETAILS.to_string(),
        description: description.to_string(),
        parameters,
    }
}
