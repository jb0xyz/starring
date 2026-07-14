use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::intent::ExistingChannelKey;

use super::{
    interpret_intent_turn_frontier, parse_interpret_intent_turn, IntentBoundaryRequestV2,
    INTERPRET_INTENT_TURN,
};

fn valid_build() -> String {
    json!({
        "expected_revision": 0,
        "request_mode": "build",
        "automation_kind": "managed_private_study_room",
        "objective": "Create private study rooms",
        "requested_outcome": "validated_preview",
        "hub_channel": "community_hub",
        "locale": "en",
        "close_authorization": "disabled",
        "runtime_requirements": {
            "persistence": "none",
            "timers": "none",
            "economy": "none",
            "event_time_llm": false
        },
        "boundary_requests": [],
        "unclassified_requirements": [],
        "response": "",
        "response_locale": "en"
    })
    .to_string()
}

#[test]
fn frontier_is_one_uniform_strict_tool() {
    let [tool] = interpret_intent_turn_frontier();
    assert_eq!(tool.name, INTERPRET_INTENT_TURN);
    assert_eq!(
        required_names(&tool.parameters),
        BTreeSet::from([
            "automation_kind".to_string(),
            "boundary_requests".to_string(),
            "close_authorization".to_string(),
            "expected_revision".to_string(),
            "hub_channel".to_string(),
            "locale".to_string(),
            "objective".to_string(),
            "request_mode".to_string(),
            "requested_outcome".to_string(),
            "response".to_string(),
            "response_locale".to_string(),
            "runtime_requirements".to_string(),
            "unclassified_requirements".to_string(),
        ])
    );
    let properties = property_names(&tool.parameters);
    assert!(properties.contains("copy"));
    assert!(properties.contains("naming"));
    assert!(properties.contains("controls"));
    for forbidden in [
        "route",
        "proposal",
        "reason",
        "capabilities",
        "schema_version",
        "feature_id",
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
}

#[test]
fn frontier_locks_closed_enums_nested_shape_and_bounds() {
    let [tool] = interpret_intent_turn_frontier();
    let root = &tool.parameters;
    assert_eq!(
        enum_strings(top_property(root, "request_mode"), root),
        strings(["build", "discussion"])
    );
    assert_eq!(
        enum_strings(top_property(root, "automation_kind"), root),
        strings(["custom_automation", "managed_private_study_room", "none",])
    );
    assert_eq!(
        enum_strings(top_property(root, "requested_outcome"), root),
        strings(["discussion", "validated_preview", "working_draft"])
    );
    assert_eq!(
        enum_strings(top_property(root, "locale"), root),
        strings(["en", "ko", "unspecified"])
    );
    assert_eq!(
        enum_strings(top_property(root, "response_locale"), root),
        strings(["en", "ko", "unspecified"])
    );
    assert_eq!(
        enum_strings(top_property(root, "close_authorization"), root),
        strings(["any_member", "creator_only", "disabled", "not_requested"])
    );
    assert_eq!(
        enum_strings(
            top_property(root, "boundary_requests")
                .get("items")
                .unwrap(),
            root,
        ),
        strings([
            "bypass_validation_preview_approval",
            "direct_live_mutation",
            "secret_disclosure",
        ])
    );

    let runtime = resolve_ref(top_property(root, "runtime_requirements"), root);
    assert_eq!(
        required_names(runtime),
        strings(["economy", "event_time_llm", "persistence", "timers"])
    );
    assert_eq!(
        runtime.get("additionalProperties"),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        enum_strings(top_property(runtime, "persistence"), root),
        strings(["none", "restart_persistent"])
    );
    assert_eq!(
        enum_strings(top_property(runtime, "timers"), root),
        strings(["durable", "none"])
    );
    assert_eq!(
        enum_strings(top_property(runtime, "economy"), root),
        strings(["none", "persistent_ledger"])
    );

    let mut hub_types = BTreeSet::new();
    collect_schema_types(
        top_property(root, "hub_channel"),
        root,
        &mut BTreeSet::new(),
        &mut hub_types,
    );
    assert_eq!(hub_types, strings(["null", "string"]));
    assert_eq!(
        top_property(root, "objective").get("maxLength"),
        Some(&Value::from(2_048))
    );
    assert_eq!(
        top_property(root, "boundary_requests").get("maxItems"),
        Some(&Value::from(3))
    );
    let unclassified = top_property(root, "unclassified_requirements");
    assert_eq!(unclassified.get("maxItems"), Some(&Value::from(8)));
    assert_eq!(
        unclassified
            .get("items")
            .and_then(|items| items.get("maxLength")),
        Some(&Value::from(160))
    );
    assert_eq!(
        top_property(root, "response").get("maxLength"),
        Some(&Value::from(2_000))
    );
}

#[test]
fn parser_accepts_complete_build_and_required_null_hub() {
    let parsed = parse_interpret_intent_turn(&valid_build()).unwrap();
    assert_eq!(parsed.expected_revision(), 0);
    assert_eq!(
        parsed.hub_channel(),
        Some(&ExistingChannelKey("community_hub".to_string()))
    );
    assert_eq!(parsed.response(), "");

    let mut value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["hub_channel"] = Value::Null;
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap()
            .hub_channel(),
        None
    );
    value.as_object_mut().unwrap().remove("hub_channel");
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "MISSING_REQUIRED_FIELD"
    );
}

#[test]
fn parser_rejects_v1_unknown_and_duplicate_fields() {
    let mut legacy = serde_json::from_str::<Value>(&valid_build()).unwrap();
    legacy["route"] = Value::String("private_study_room".to_string());
    assert_eq!(
        parse_interpret_intent_turn(&legacy.to_string())
            .unwrap_err()
            .code,
        "UNKNOWN_FIELD"
    );
    for arguments in [
        valid_build().replacen(
            "\"expected_revision\":0",
            "\"expected_revision\":0,\"expected_revision\":1",
            1,
        ),
        valid_build().replacen(
            "\"persistence\":\"none\"",
            "\"persistence\":\"none\",\"persistence\":\"restart_persistent\"",
            1,
        ),
        valid_build().replacen(
            "\"response_locale\":\"en\"",
            "\"response_locale\":\"en\",\"response_locale\":\"ko\"",
            1,
        ),
    ] {
        assert_eq!(
            parse_interpret_intent_turn(&arguments).unwrap_err().code,
            "INVALID_TOOL_ARGUMENTS"
        );
    }
}

#[test]
fn parser_rejects_unknown_and_duplicate_nested_fields() {
    let mut runtime = serde_json::from_str::<Value>(&valid_build()).unwrap();
    runtime["runtime_requirements"]["lease"] = Value::Bool(true);
    assert_eq!(
        parse_interpret_intent_turn(&runtime.to_string())
            .unwrap_err()
            .code,
        "UNKNOWN_FIELD"
    );

    for (field, payload) in [
        ("copy", json!({"source": "model"})),
        ("naming", json!({"recipe": "hidden"})),
        ("controls", json!({"close_policy": "disabled"})),
        (
            "copy",
            json!({"welcome_content": {"prefix": "Welcome ", "suffix": "", "template": "raw"}}),
        ),
    ] {
        let mut value = serde_json::from_str::<Value>(&valid_build()).unwrap();
        value[field] = payload;
        assert_eq!(
            parse_interpret_intent_turn(&value.to_string())
                .unwrap_err()
                .code,
            "UNKNOWN_FIELD"
        );
    }

    let mut value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["controls"] = json!({"help_label": "Help"});
    let duplicate_control = value.to_string().replacen(
        "\"help_label\":\"Help\"",
        "\"help_label\":\"Help\",\"help_label\":\"Other\"",
        1,
    );
    assert_eq!(
        parse_interpret_intent_turn(&duplicate_control)
            .unwrap_err()
            .code,
        "INVALID_TOOL_ARGUMENTS"
    );

    value["copy"] = json!({"welcome_content": {"prefix": "Welcome ", "suffix": ""}});
    let duplicate_affix = value.to_string().replacen(
        "\"prefix\":\"Welcome \"",
        "\"prefix\":\"Welcome \",\"prefix\":\"Hello \"",
        1,
    );
    assert_eq!(
        parse_interpret_intent_turn(&duplicate_affix)
            .unwrap_err()
            .code,
        "INVALID_TOOL_ARGUMENTS"
    );
}

#[test]
fn mode_outcome_and_discussion_response_are_consistent() {
    let mut value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["request_mode"] = Value::String("discussion".to_string());
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "INCONSISTENT_INTENT_INTERPRETATION"
    );
    value["requested_outcome"] = Value::String("discussion".to_string());
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .location,
        "intent.interpretation.automation_kind"
    );
    value["automation_kind"] = Value::String("none".to_string());
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "EMPTY_INTENT_TEXT"
    );
    value["response"] = Value::String("Let us compare the options.".to_string());
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap()
            .response(),
        "Let us compare the options."
    );
}

#[test]
fn parser_enforces_text_channel_and_evidence_bounds() {
    let mut value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["objective"] = Value::String("x".repeat(2_048 + 1));
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "INTENT_TEXT_TOO_LONG"
    );

    value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["response"] = Value::String("x".repeat(2_000 + 1));
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "INTENT_TEXT_TOO_LONG"
    );

    value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["hub_channel"] = Value::String("bad channel".to_string());
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "INVALID_INTENT_CHANNEL_BINDING"
    );

    value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["unclassified_requirements"] = json!([" "]);
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "EMPTY_INTENT_TEXT"
    );

    value["unclassified_requirements"] = json!(["x".repeat(160 + 1)]);
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "INTENT_TEXT_TOO_LONG"
    );

    value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["objective"] = Value::String("unsafe\u{202e}text".to_string());
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "INVALID_INTENT_TEXT_CONTROL"
    );
}

#[test]
fn parser_normalizes_closed_sets_and_bounded_evidence() {
    let mut value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["boundary_requests"] = json!([
        "secret_disclosure",
        "direct_live_mutation",
        "secret_disclosure"
    ]);
    value["unclassified_requirements"] = json!([
        "  external   scheduler lease ",
        "cross-service quorum",
        "cross-service quorum"
    ]);
    let parsed = parse_interpret_intent_turn(&value.to_string()).unwrap();
    assert_eq!(
        parsed.boundary_requests(),
        &[
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::SecretDisclosure
        ]
    );
    assert_eq!(
        parsed.unclassified_requirements(),
        &["cross-service quorum", "external scheduler lease"]
    );

    value["unclassified_requirements"] = json!(vec!["x"; 9]);
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "TOO_MANY_UNCLASSIFIED_INTENT_REQUIREMENTS"
    );
}

#[test]
fn parser_preserves_and_validates_recipe_overrides_on_every_route() {
    let mut value = serde_json::from_str::<Value>(&valid_build()).unwrap();
    value["automation_kind"] = Value::String("custom_automation".to_string());
    value["copy"] = json!({
        "create_button_label": "  Create now  ",
        "welcome_content": {"prefix": "Welcome to ", "suffix": ""}
    });
    value["naming"] = json!({
        "channel_name": {"prefix": "study-", "suffix": ""}
    });
    value["controls"] = json!({
        "help_label": "  Help  ",
        "help_response": "  Private room help  "
    });
    let parsed = parse_interpret_intent_turn(&value.to_string()).unwrap();
    assert_eq!(
        parsed.copy().create_button_label.as_deref(),
        Some("Create now")
    );
    assert_eq!(parsed.controls().help_label.as_deref(), Some("Help"));
    assert_eq!(
        parsed.naming().channel_name.as_ref().unwrap().prefix,
        "study-"
    );

    value["controls"] = json!({"help_label": "${raw}"});
    assert_eq!(
        parse_interpret_intent_turn(&value.to_string())
            .unwrap_err()
            .code,
        "RAW_INTENT_TEMPLATE_FORBIDDEN"
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

fn top_property<'a>(schema: &'a Value, name: &str) -> &'a Value {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(name))
        .unwrap()
}

fn resolve_ref<'a>(value: &'a Value, root: &'a Value) -> &'a Value {
    value
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .and_then(|name| {
            root.get("$defs")
                .and_then(|definitions| definitions.get(name))
        })
        .unwrap_or(value)
}

fn enum_strings(value: &Value, root: &Value) -> BTreeSet<String> {
    resolve_ref(value, root)
        .get("enum")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn strings<const N: usize>(values: [&str; N]) -> BTreeSet<String> {
    values.into_iter().map(str::to_string).collect()
}

fn collect_schema_types(
    value: &Value,
    root: &Value,
    visited: &mut BTreeSet<String>,
    output: &mut BTreeSet<String>,
) {
    if let Some(name) = value
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        if visited.insert(name.to_string()) {
            if let Some(definition) = root.get("$defs").and_then(|defs| defs.get(name)) {
                collect_schema_types(definition, root, visited, output);
            }
        }
        return;
    }
    if let Some(value_type) = value.get("type") {
        match value_type {
            Value::String(value_type) => {
                output.insert(value_type.clone());
            }
            Value::Array(value_types) => {
                output.extend(
                    value_types
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string),
                );
            }
            _ => {}
        }
    }
    match value {
        Value::Array(values) => {
            for value in values {
                collect_schema_types(value, root, visited, output);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_schema_types(value, root, visited, output);
            }
        }
        _ => {}
    }
}
