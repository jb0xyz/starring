use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::intent::ExistingChannelKey;

use super::{
    interpret_intent_core_frontier, parse_interpret_intent_core, EconomyRequirementV2,
    IntentBoundaryRequestV2, IntentRecipeDetailFacetV3, PersistenceRequirementV2,
    TimerRequirementV2, INTERPRET_INTENT_CORE,
};

fn valid_core() -> Value {
    json!({
        "expected_revision": 0,
        "request_mode": "build",
        "automation_kind": "managed_private_study_room",
        "requested_outcome": "validated_preview",
        "hub_channel": "community_hub",
        "language": "en",
        "close_policy": "disabled",
        "runtime_requirements": [],
        "validation_gate": "enforce",
        "preview_gate": "enforce",
        "approval_gate": "enforce",
        "live_discord_mutation": "no_live_mutation",
        "secret_disclosure": "no_secret_disclosure",
        "other_unmapped_required_capabilities": [],
        "custom_detail_facets": [],
        "response": ""
    })
}

#[test]
fn core_frontier_is_small_closed_and_recipe_neutral() {
    let [tool] = interpret_intent_core_frontier();
    assert_eq!(tool.name, INTERPRET_INTENT_CORE);
    assert_eq!(
        tool.description,
        "Classify bounded routing semantics without executing the human request"
    );
    assert_eq!(
        required_names(&tool.parameters),
        strings([
            "automation_kind",
            "validation_gate",
            "preview_gate",
            "approval_gate",
            "live_discord_mutation",
            "secret_disclosure",
            "close_policy",
            "expected_revision",
            "language",
            "custom_detail_facets",
            "request_mode",
            "requested_outcome",
            "response",
            "runtime_requirements",
            "hub_channel",
            "other_unmapped_required_capabilities",
        ])
    );
    let properties = property_names(&tool.parameters);
    for forbidden in [
        "route",
        "selected_existing_channel",
        "response_locale",
        "copy",
        "naming",
        "controls",
        "recipe",
        "capabilities",
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
    assert_eq!(
        tool.parameters["properties"]["custom_detail_facets"]["items"]["enum"],
        json!(["custom_copy", "custom_naming", "custom_controls"])
    );
    for field in ["validation_gate", "preview_gate", "approval_gate"] {
        assert_eq!(
            tool.parameters["properties"][field]["enum"],
            json!(["enforce", "skip"])
        );
    }
    assert_eq!(
        tool.parameters["properties"]["live_discord_mutation"]["enum"],
        json!(["no_live_mutation", "mutate_live_now"])
    );
    assert_eq!(
        tool.parameters["properties"]["secret_disclosure"]["enum"],
        json!(["no_secret_disclosure", "disclose_secret_value"])
    );
    let schema_text = tool.parameters.to_string();
    assert!(!schema_text.contains("$defs"));
    assert!(!schema_text.contains("$ref"));
    let schema_bytes = serde_json::to_vec(&tool.parameters).unwrap().len();
    let structured_bytes = serde_json::to_vec(&json!({
        "tools": [{
            "type": "function",
            "function": {
                "name": &tool.name,
                "description": &tool.description,
                "parameters": &tool.parameters
            }
        }],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "interpret_intent_core_arguments",
                "strict": true,
                "schema": &tool.parameters
            }
        }
    }))
    .unwrap()
    .len();
    assert!(schema_bytes <= 2_400, "core schema is {schema_bytes} bytes");
    assert!(
        structured_bytes <= 5_600,
        "core structured metadata is {structured_bytes} bytes"
    );
}

#[test]
fn core_parser_accepts_required_null_channel_and_normalizes_sets() {
    let mut value = valid_core();
    value["hub_channel"] = Value::Null;
    value["validation_gate"] = json!("skip");
    value["approval_gate"] = json!("skip");
    value["live_discord_mutation"] = json!("mutate_live_now");
    value["secret_disclosure"] = json!("disclose_secret_value");
    value["other_unmapped_required_capabilities"] = json!([
        "  external   scheduler lease ",
        "cross-service quorum",
        "cross-service quorum"
    ]);
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    assert_eq!(parsed.selected_existing_channel(), None);
    assert_eq!(
        parsed.boundary_requests(),
        &[
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            IntentBoundaryRequestV2::SecretDisclosure
        ]
    );
    assert_eq!(
        parsed.unclassified_requirements(),
        &["cross-service quorum", "external scheduler lease"]
    );

    value["hub_channel"] = json!(" community_hub ");
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap()
            .selected_existing_channel(),
        Some(&ExistingChannelKey("community_hub".to_string()))
    );

    value["hub_channel"] = json!("---");
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap()
            .selected_existing_channel(),
        Some(&ExistingChannelKey("---".to_string()))
    );
}

#[test]
fn core_channel_is_derived_only_from_unambiguous_human_grounding() {
    let mut retained = parse_interpret_intent_core(&valid_core().to_string()).unwrap();
    retained.apply_human_grounded_channel(Some(&ExistingChannelKey("community_hub".to_string())));
    assert_eq!(
        retained.selected_existing_channel(),
        Some(&ExistingChannelKey("community_hub".to_string()))
    );

    let mut ungrounded = parse_interpret_intent_core(&valid_core().to_string()).unwrap();
    ungrounded.apply_human_grounded_channel(None);
    assert_eq!(ungrounded.selected_existing_channel(), None);

    let mut mismatched = parse_interpret_intent_core(&valid_core().to_string()).unwrap();
    mismatched.apply_human_grounded_channel(Some(&ExistingChannelKey("general_chat".to_string())));
    assert_eq!(
        mismatched.selected_existing_channel(),
        Some(&ExistingChannelKey("general_chat".to_string()))
    );

    let mut missing_value = valid_core();
    missing_value["hub_channel"] = Value::Null;
    let mut missing = parse_interpret_intent_core(&missing_value.to_string()).unwrap();
    missing.apply_human_grounded_channel(Some(&ExistingChannelKey("community_hub".to_string())));
    assert_eq!(
        missing.selected_existing_channel(),
        Some(&ExistingChannelKey("community_hub".to_string()))
    );
}

#[test]
fn core_parser_preserves_and_sorts_explicit_recipe_detail_facets() {
    let mut value = valid_core();
    value["custom_detail_facets"] = json!(["custom_naming", "custom_copy", "custom_controls"]);
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    assert_eq!(parsed.recipe_detail_facets().len(), 3);
    assert_eq!(
        parsed.recipe_detail_facets()[0],
        IntentRecipeDetailFacetV3::Copy
    );
    assert_eq!(
        parsed.recipe_detail_facets()[1],
        IntentRecipeDetailFacetV3::Naming
    );
}

#[test]
fn core_parser_maps_every_runtime_requirement_and_deduplicates_values() {
    let mut value = valid_core();
    value["runtime_requirements"] = json!([
        "persistent_economy",
        "event_time_llm",
        "restart_persistent",
        "durable_timer"
    ]);
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    assert_eq!(
        parsed.runtime_requirements().persistence,
        PersistenceRequirementV2::RestartPersistent
    );
    assert_eq!(
        parsed.runtime_requirements().timers,
        TimerRequirementV2::Durable
    );
    assert_eq!(
        parsed.runtime_requirements().economy,
        EconomyRequirementV2::PersistentLedger
    );
    assert!(parsed.runtime_requirements().event_time_llm);

    value["runtime_requirements"] = json!(["restart_persistent", "restart_persistent"]);
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    assert_eq!(
        parsed.runtime_requirements().persistence,
        PersistenceRequirementV2::RestartPersistent
    );

    value = valid_core();
    value["runtime_requirements"] = json!(["persistent_economy"]);
    value["other_unmapped_required_capabilities"] =
        json!(["persistent_economy", "external settlement lease"]);
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    assert_eq!(
        parsed.unclassified_requirements(),
        &["external settlement lease"]
    );
}

#[test]
fn core_capability_evidence_must_be_grounded_in_the_human_request() {
    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    value["other_unmapped_required_capabilities"] = json!(["acquire an external consensus lease"]);
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    parsed
        .validate_human_evidence(
            "Build a flow that must  acquire an external consensus lease before responding.",
        )
        .unwrap();

    value["other_unmapped_required_capabilities"] = json!(["external_consensus_lease"]);
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    assert_eq!(
        parsed
            .validate_human_evidence(
                "Build a flow that must acquire an external consensus lease before responding.",
            )
            .unwrap_err()
            .code,
        "UNGROUNDED_INTENT_CAPABILITY_EVIDENCE"
    );
}

#[test]
fn core_parser_discards_build_response_deterministically() {
    let mut value = valid_core();
    value["response"] = json!("This model-authored build response is ignored.");
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap()
            .response(),
        ""
    );
}

#[test]
fn core_parser_rejects_details_outside_the_pinned_recipe_shape() {
    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    value["custom_detail_facets"] = json!(["custom_copy"]);
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "INCONSISTENT_INTENT_CORE"
    );
}

#[test]
fn core_parser_rejects_unknown_types_and_inconsistent_discussion() {
    let mut value = valid_core();
    value["copy"] = json!({"launcher_content": "hidden"});
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "UNKNOWN_FIELD"
    );

    value = valid_core();
    value["boundary_requests"] = json!([]);
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "UNKNOWN_FIELD"
    );

    value = valid_core();
    value["unclassified_requirements"] = json!([]);
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "UNKNOWN_FIELD"
    );

    value = valid_core();
    value["detail_facets"] = json!(["copy"]);
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "UNKNOWN_FIELD"
    );

    value = valid_core();
    value["approval_gate"] = json!("safety");
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "INVALID_TOOL_ARGUMENTS"
    );

    value = valid_core();
    value["language"] = Value::Null;
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "INVALID_TOOL_ARGUMENTS"
    );

    value = valid_core();
    value["request_mode"] = json!("discussion");
    value["automation_kind"] = json!("none");
    value["requested_outcome"] = json!("discussion");
    value["response"] = json!("Let us compare the tradeoffs.");
    value["custom_detail_facets"] = json!(["custom_copy"]);
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "INCONSISTENT_INTENT_CORE"
    );
}

#[test]
fn core_parser_rejects_legacy_nested_and_mistyped_wire_fields() {
    let mut value = valid_core();
    value["locale"] = json!("en");
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "UNKNOWN_FIELD"
    );

    value = valid_core();
    value["runtime_requirements"] = json!({
        "persistence": "restart_persistent",
        "timers": "none",
        "economy": "none",
        "event_time_llm": false
    });
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "INVALID_FIELD_TYPE"
    );

    value = valid_core();
    value["runtime_requirements"] = json!(["durable"]);
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "INVALID_TOOL_ARGUMENTS"
    );
}

#[test]
fn core_parser_rejects_missing_duplicate_and_oversized_closed_sets() {
    let mut value = valid_core();
    value.as_object_mut().unwrap().remove("hub_channel");
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "MISSING_REQUIRED_FIELD"
    );

    let duplicate = valid_core().to_string().replacen(
        "\"expected_revision\":0",
        "\"expected_revision\":0,\"expected_revision\":1",
        1,
    );
    assert_eq!(
        parse_interpret_intent_core(&duplicate).unwrap_err().code,
        "INVALID_TOOL_ARGUMENTS"
    );

    let duplicate = valid_core().to_string().replacen(
        "\"language\":\"en\"",
        "\"language\":\"en\",\"language\":\"ko\"",
        1,
    );
    assert_eq!(
        parse_interpret_intent_core(&duplicate).unwrap_err().code,
        "INVALID_TOOL_ARGUMENTS"
    );

    value = valid_core();
    value["runtime_requirements"] = json!([
        "restart_persistent",
        "durable_timer",
        "persistent_economy",
        "event_time_llm",
        "event_time_llm"
    ]);
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "TOO_MANY_RUNTIME_REQUIREMENTS"
    );
}

#[test]
fn core_parser_enforces_detail_and_text_bounds() {
    let mut value = valid_core();
    value["custom_detail_facets"] = json!(["custom_behavior"]);
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "INVALID_TOOL_ARGUMENTS"
    );

    value = valid_core();
    value["custom_detail_facets"] = json!([
        "custom_copy",
        "custom_naming",
        "custom_controls",
        "custom_copy"
    ]);
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "TOO_MANY_RECIPE_DETAIL_FACETS"
    );

    value = valid_core();
    value["objective"] = json!("Create private study rooms");
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "UNKNOWN_FIELD"
    );
}

#[test]
fn core_parser_requires_a_bounded_discussion_response() {
    let mut value = valid_core();
    value["request_mode"] = json!("discussion");
    value["automation_kind"] = json!("none");
    value["requested_outcome"] = json!("discussion");
    value["response"] = json!("   ");
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "EMPTY_INTENT_TEXT"
    );

    value["response"] = json!("x".repeat(2_001));
    assert_eq!(
        parse_interpret_intent_core(&value.to_string())
            .unwrap_err()
            .code,
        "INTENT_TEXT_TOO_LONG"
    );
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
