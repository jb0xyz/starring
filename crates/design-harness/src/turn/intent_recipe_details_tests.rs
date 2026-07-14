use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::{
    parse_private_study_room_details, private_study_room_details_frontier,
    IntentRecipeDetailFacetV3, EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
};

fn valid_copy_details() -> Value {
    json!({
        "expected_revision": 0,
        "copy": {
            "create_button_label": "Start focus room",
            "welcome_content": {"prefix": "Welcome to ", "suffix": ""}
        },
        "naming": {},
        "controls": {},
        "covered_facets": ["copy"],
        "unmapped_facets": []
    })
}

#[test]
fn detail_frontier_is_one_closed_recipe_specific_tool() {
    let [tool] = private_study_room_details_frontier();
    assert_eq!(tool.name, EXTRACT_PRIVATE_STUDY_ROOM_DETAILS);
    assert_eq!(
        required_names(&tool.parameters),
        strings([
            "controls",
            "copy",
            "covered_facets",
            "expected_revision",
            "naming",
            "unmapped_facets",
        ])
    );
    let properties = property_names(&tool.parameters);
    for forbidden in [
        "route",
        "objective",
        "hub_channel",
        "locale",
        "close_authorization",
        "runtime_requirements",
        "capabilities",
        "recipe",
        "actions",
        "permissions",
        "ruleset",
    ] {
        assert!(!properties.contains(forbidden));
    }
    assert_eq!(
        tool.parameters.get("additionalProperties"),
        Some(&Value::Bool(false))
    );
    let schema_bytes = serde_json::to_vec(&tool.parameters).unwrap().len();
    assert!(
        schema_bytes <= 2_400,
        "detail schema is {schema_bytes} bytes"
    );
}

#[test]
fn detail_parser_accepts_exact_selected_facets_and_null_unselected_objects() {
    let parsed = parse_private_study_room_details(
        &valid_copy_details().to_string(),
        &[IntentRecipeDetailFacetV3::Copy],
    )
    .unwrap();
    assert_eq!(parsed.expected_revision(), 0);
    assert_eq!(
        parsed.copy().create_button_label.as_deref(),
        Some("Start focus room")
    );
    assert_eq!(parsed.covered_facets(), &[IntentRecipeDetailFacetV3::Copy]);

    let mut value = valid_copy_details();
    value["naming"] = Value::Null;
    value["controls"] = Value::Null;
    assert!(parse_private_study_room_details(
        &value.to_string(),
        &[IntentRecipeDetailFacetV3::Copy]
    )
    .is_ok());
}

#[test]
fn detail_parser_requires_exact_coverage_and_nonempty_selected_values() {
    let mut value = valid_copy_details();
    value["covered_facets"] = json!([]);
    assert_eq!(
        parse_private_study_room_details(&value.to_string(), &[IntentRecipeDetailFacetV3::Copy])
            .unwrap_err()
            .code,
        "RECIPE_DETAIL_COVERAGE_MISMATCH"
    );

    value = valid_copy_details();
    value["copy"] = json!({});
    assert_eq!(
        parse_private_study_room_details(&value.to_string(), &[IntentRecipeDetailFacetV3::Copy])
            .unwrap_err()
            .code,
        "EMPTY_REQUIRED_RECIPE_DETAIL"
    );

    value = valid_copy_details();
    value["unmapped_facets"] = json!(["copy"]);
    assert_eq!(
        parse_private_study_room_details(&value.to_string(), &[IntentRecipeDetailFacetV3::Copy])
            .unwrap_err()
            .code,
        "UNMAPPED_RECIPE_DETAIL_FACET"
    );
}

#[test]
fn detail_parser_rejects_unrequested_values_duplicates_and_empty_ticket() {
    let mut value = valid_copy_details();
    value["naming"] = json!({
        "channel_name": {"prefix": "focus-", "suffix": ""}
    });
    assert_eq!(
        parse_private_study_room_details(&value.to_string(), &[IntentRecipeDetailFacetV3::Copy])
            .unwrap_err()
            .code,
        "UNREQUESTED_RECIPE_DETAIL"
    );

    value = valid_copy_details();
    value["covered_facets"] = json!(["copy", "copy"]);
    assert_eq!(
        parse_private_study_room_details(&value.to_string(), &[IntentRecipeDetailFacetV3::Copy])
            .unwrap_err()
            .code,
        "DUPLICATE_RECIPE_DETAIL_FACET"
    );

    assert_eq!(
        parse_private_study_room_details(&valid_copy_details().to_string(), &[])
            .unwrap_err()
            .code,
        "EMPTY_RECIPE_DETAIL_REQUEST"
    );
}

#[test]
fn detail_parser_reuses_recipe_text_and_template_guards() {
    let mut value = valid_copy_details();
    value["copy"]["create_button_label"] = json!("${raw}");
    assert_eq!(
        parse_private_study_room_details(&value.to_string(), &[IntentRecipeDetailFacetV3::Copy])
            .unwrap_err()
            .code,
        "RAW_INTENT_TEMPLATE_FORBIDDEN"
    );

    value = valid_copy_details();
    value["controls"] = json!({"help_label": " Guide "});
    value["covered_facets"] = json!(["copy", "controls"]);
    let parsed = parse_private_study_room_details(
        &value.to_string(),
        &[
            IntentRecipeDetailFacetV3::Controls,
            IntentRecipeDetailFacetV3::Copy,
        ],
    )
    .unwrap();
    assert_eq!(parsed.controls().help_label.as_deref(), Some("Guide"));
}

fn property_names(schema: &Value) -> BTreeSet<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

fn required_names(schema: &Value) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn strings<const N: usize>(values: [&str; N]) -> BTreeSet<String> {
    values.into_iter().map(str::to_string).collect()
}
