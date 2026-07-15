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
            "close_policy",
            "expected_revision",
            "language",
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
        "validation_gate",
        "preview_gate",
        "approval_gate",
        "live_discord_mutation",
        "secret_disclosure",
        "custom_detail_facets",
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
    assert!(schema_bytes <= 1_600, "core schema is {schema_bytes} bytes");
    assert!(
        structured_bytes <= 3_800,
        "core structured metadata is {structured_bytes} bytes"
    );
}

#[test]
fn core_parser_defaults_hidden_model_fields_to_safe_empty_semantics() {
    let mut value = valid_core();
    for field in [
        "validation_gate",
        "preview_gate",
        "approval_gate",
        "live_discord_mutation",
        "secret_disclosure",
        "custom_detail_facets",
    ] {
        value.as_object_mut().unwrap().remove(field);
    }
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    assert!(parsed.boundary_requests().is_empty());
    assert!(parsed.recipe_detail_facets().is_empty());
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
fn core_parser_keeps_behaviors_separate_from_runtime_infrastructure() {
    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    value["runtime_requirements"] = json!([
        "persistent_economy",
        "event_time_llm",
        "restart_persistent",
        "durable_timer"
    ]);
    value["other_unmapped_required_capabilities"] = json!([
        "timers advance quests",
        "every message earns XP",
        "an LLM decides rewards at event time",
        "levels unlock an economy",
        "persistent_economy",
        "event_time_llm",
        "restart_persistent",
        "durable_timer"
    ]);
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    assert_eq!(
        parsed.unclassified_requirements(),
        &[
            "an LLM decides rewards at event time",
            "every message earns XP",
            "levels unlock an economy",
            "timers advance quests"
        ]
    );
}

#[test]
fn human_grounding_canonicalizes_live_stateful_evidence() {
    let human = "Build a persistent Discord game where every message earns XP, levels unlock an economy, timers advance quests, and an LLM decides rewards at event time. Quest timers must be durable, and the economy ledger must be persistent. Preserve state across restarts and do not reduce the request to static responses.";
    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    value["other_unmapped_required_capabilities"] = json!([
        "LLM decides rewards at event time",
        "do not reduce the request to static responses",
        "every message earns XP",
        "levels unlock an economy",
        "timers advance quests"
    ]);
    let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    parsed.apply_human_grounding(human, None).unwrap();
    assert_eq!(
        parsed.unclassified_requirements(),
        &[
            "an LLM decides rewards at event time",
            "every message earns XP",
            "levels unlock an economy",
            "timers advance quests"
        ]
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

    value["other_unmapped_required_capabilities"] = json!(["LLM decides rewards at event time"]);
    let parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    assert_eq!(
        parsed
            .validate_human_evidence("An XLLM decides rewards at event time.")
            .unwrap_err()
            .code,
        "UNGROUNDED_INTENT_CAPABILITY_EVIDENCE"
    );
}

#[test]
fn failed_human_grounding_leaves_the_parsed_core_unchanged() {
    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    value["other_unmapped_required_capabilities"] = json!(["fabricated capability"]);
    let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    let before = parsed.clone();
    assert_eq!(
        parsed
            .apply_human_grounding(
                "Build in community_hub with only the requested behavior.",
                Some(&ExistingChannelKey("community_hub".to_string())),
            )
            .unwrap_err()
            .code,
        "UNGROUNDED_INTENT_CAPABILITY_EVIDENCE"
    );
    assert_eq!(parsed, before);
}

#[test]
fn human_grounding_reclassifies_exact_supported_recipe_details() {
    let human = "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults except for these exact overrides: the launcher create-button label is 'Start focus room'; the created channel name uses prefix 'focus-' and an empty suffix; the room Help button label is 'Guide' and its ephemeral response is 'Read this first'. Leave room closing disabled.";
    let mut value = valid_core();
    value["other_unmapped_required_capabilities"] = json!([
        "created channel name uses prefix 'focus-' and an empty suffix",
        "ephemeral response is 'Read this first'",
        "launcher create-button label is 'Start focus room'",
        "room Help button label is 'Guide'"
    ]);
    let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();

    parsed
        .apply_human_grounding(
            human,
            Some(&ExistingChannelKey("community_hub".to_string())),
        )
        .unwrap();

    assert!(parsed.unclassified_requirements().is_empty());
    assert_eq!(
        parsed.recipe_detail_facets(),
        &[
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls
        ]
    );
}

#[test]
fn human_grounding_preserves_external_capability_next_to_recipe_details() {
    let human = "Build a managed private study-room automation in community_hub. Use these exact overrides: the room Help button label is 'Guide' and its ephemeral response is 'Read this first'. Acquire an external consensus lease before responding.";
    let mut value = valid_core();
    value["other_unmapped_required_capabilities"] = json!([
        "ephemeral response is 'Read this first'",
        "external consensus lease",
        "room Help button label is 'Guide'"
    ]);
    let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();

    parsed
        .apply_human_grounding(
            human,
            Some(&ExistingChannelKey("community_hub".to_string())),
        )
        .unwrap();

    assert_eq!(
        parsed.unclassified_requirements(),
        &["external consensus lease"]
    );
    assert_eq!(
        parsed.recipe_detail_facets(),
        &[IntentRecipeDetailFacetV3::Controls]
    );
}

#[test]
fn human_grounding_never_reduces_dynamic_behavior_to_recipe_details() {
    let human = "Build a managed private study-room automation. When the Close button is clicked, change the channel name to 'closed'.";
    let mut value = valid_core();
    value["other_unmapped_required_capabilities"] = json!(["channel name to 'closed'"]);
    let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();

    parsed.apply_human_grounding(human, None).unwrap();

    assert_eq!(
        parsed.unclassified_requirements(),
        &["channel name to 'closed'"]
    );
    assert!(parsed.recipe_detail_facets().is_empty());
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
