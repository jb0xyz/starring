use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::{
    parse_private_study_room_details, private_study_room_details_frontier,
    private_study_room_details_frontier_for, IntentRecipeDetailFacetV3,
    EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
};

const CORE_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn valid_copy_details() -> Value {
    json!({
        "copy": {
            "create_button_label": "Start focus room",
            "welcome_content": {"prefix": "Welcome to ", "suffix": ""}
        }
    })
}

#[test]
fn active_detail_frontier_exposes_and_requires_only_selected_facets() {
    let [tool] = private_study_room_details_frontier_for(&[
        IntentRecipeDetailFacetV3::Controls,
        IntentRecipeDetailFacetV3::Copy,
    ])
    .unwrap();
    assert_eq!(
        property_names(&tool.parameters),
        ["controls", "copy"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    assert_eq!(
        required_names(&tool.parameters),
        ["controls", "copy"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    assert!(!tool.parameters.to_string().contains("unmapped_facets"));
    let schema_bytes = serde_json::to_vec(&tool.parameters).unwrap().len();
    assert!(
        schema_bytes < 1_800,
        "routed detail schema is {schema_bytes} bytes"
    );
}

#[test]
fn active_detail_frontier_rejects_empty_and_duplicate_tickets() {
    assert_eq!(
        private_study_room_details_frontier_for(&[])
            .unwrap_err()
            .code,
        "EMPTY_RECIPE_DETAIL_REQUEST"
    );
    assert_eq!(
        private_study_room_details_frontier_for(&[
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Copy,
        ])
        .unwrap_err()
        .code,
        "DUPLICATE_RECIPE_DETAIL_FACET"
    );
}

#[test]
fn detail_frontier_is_one_closed_recipe_specific_tool() {
    let [tool] = private_study_room_details_frontier();
    assert_eq!(tool.name, EXTRACT_PRIVATE_STUDY_ROOM_DETAILS);
    assert_eq!(required_names(&tool.parameters), BTreeSet::new());
    let properties = property_names(&tool.parameters);
    for forbidden in [
        "route",
        "objective",
        "hub_channel",
        "locale",
        "close_authorization",
        "runtime_requirements",
        "expected_revision",
        "core_semantic_digest",
        "covered_facets",
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
    let schema_text = tool.parameters.to_string();
    assert!(!schema_text.contains("$defs"));
    assert!(!schema_text.contains("$ref"));
    let schema_bytes = serde_json::to_vec(&tool.parameters).unwrap().len();
    assert!(
        schema_bytes <= 2_100,
        "detail schema is {schema_bytes} bytes"
    );
}

#[test]
fn detail_parser_accepts_exact_selected_facets_and_null_unselected_objects() {
    let parsed = parse_private_study_room_details(
        &valid_copy_details().to_string(),
        &[IntentRecipeDetailFacetV3::Copy],
        0,
        CORE_DIGEST,
    )
    .unwrap();
    assert_eq!(parsed.expected_revision(), 0);
    assert_eq!(parsed.core_semantic_digest(), CORE_DIGEST);
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
        &[IntentRecipeDetailFacetV3::Copy],
        0,
        CORE_DIGEST
    )
    .is_ok());
}

#[test]
fn detail_parser_requires_nonempty_selected_values_and_no_unmapped_facets() {
    let mut value = valid_copy_details();
    value["copy"] = json!({});
    assert_eq!(
        parse_private_study_room_details(
            &value.to_string(),
            &[IntentRecipeDetailFacetV3::Copy],
            0,
            CORE_DIGEST,
        )
        .unwrap_err()
        .code,
        "EMPTY_REQUIRED_RECIPE_DETAIL"
    );

    value = valid_copy_details();
    value["unmapped_facets"] = json!(["copy"]);
    assert_eq!(
        parse_private_study_room_details(
            &value.to_string(),
            &[IntentRecipeDetailFacetV3::Copy],
            0,
            CORE_DIGEST,
        )
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
        parse_private_study_room_details(
            &value.to_string(),
            &[IntentRecipeDetailFacetV3::Copy],
            0,
            CORE_DIGEST,
        )
        .unwrap_err()
        .code,
        "UNREQUESTED_RECIPE_DETAIL"
    );

    assert_eq!(
        parse_private_study_room_details(
            &valid_copy_details().to_string(),
            &[
                IntentRecipeDetailFacetV3::Copy,
                IntentRecipeDetailFacetV3::Copy,
            ],
            0,
            CORE_DIGEST,
        )
        .unwrap_err()
        .code,
        "DUPLICATE_RECIPE_DETAIL_FACET"
    );

    assert_eq!(
        parse_private_study_room_details(&valid_copy_details().to_string(), &[], 0, CORE_DIGEST)
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
        parse_private_study_room_details(
            &value.to_string(),
            &[IntentRecipeDetailFacetV3::Copy],
            0,
            CORE_DIGEST,
        )
        .unwrap_err()
        .code,
        "RAW_INTENT_TEMPLATE_FORBIDDEN"
    );

    value = valid_copy_details();
    value["controls"] = json!({"help_label": " Guide "});
    let parsed = parse_private_study_room_details(
        &value.to_string(),
        &[
            IntentRecipeDetailFacetV3::Controls,
            IntentRecipeDetailFacetV3::Copy,
        ],
        0,
        CORE_DIGEST,
    )
    .unwrap();
    assert_eq!(parsed.controls().help_label.as_deref(), Some("Guide"));
}

#[test]
fn detail_parser_stamps_harness_binding_and_rejects_model_authored_metadata() {
    let digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let parsed = parse_private_study_room_details(
        &valid_copy_details().to_string(),
        &[IntentRecipeDetailFacetV3::Copy],
        7,
        digest,
    )
    .unwrap();
    assert_eq!(parsed.expected_revision(), 7);
    assert_eq!(parsed.core_semantic_digest(), digest);
    assert_eq!(parsed.covered_facets(), &[IntentRecipeDetailFacetV3::Copy]);

    for field in [
        ("expected_revision", json!(7)),
        ("core_semantic_digest", json!(digest)),
        ("covered_facets", json!(["copy"])),
    ] {
        let mut value = valid_copy_details();
        value[field.0] = field.1;
        assert_eq!(
            parse_private_study_room_details(
                &value.to_string(),
                &[IntentRecipeDetailFacetV3::Copy],
                7,
                digest,
            )
            .unwrap_err()
            .code,
            "UNKNOWN_FIELD"
        );
    }
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
