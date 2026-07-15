use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::intent::ExistingChannelKey;

use super::{
    interpret_intent_core_frontier, parse_interpret_intent_core,
    parse_interpret_intent_core_for_human, parse_interpret_intent_core_for_serving,
    EconomyRequirementV2, IntentBoundaryRequestV2, IntentRecipeDetailFacetV3,
    PersistenceRequirementV2, TimerRequirementV2, INTERPRET_INTENT_CORE,
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
        "Call once for every request, including discussion; put a concise complete conversational answer only in response and copy capability evidence exactly"
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
fn serving_parser_binds_the_harness_revision_over_model_transport() {
    let mut value = valid_core();
    value["expected_revision"] = json!(99);

    let parsed = parse_interpret_intent_core_for_serving(
        &value.to_string(),
        "Build a managed private study room.",
        7,
    )
    .unwrap();

    assert_eq!(parsed.expected_revision(), 7);
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
fn serving_runtime_grounding_overwrites_model_inference_and_omission() {
    let mut inferred = valid_core();
    inferred["runtime_requirements"] = json!([
        "restart_persistent",
        "durable_timer",
        "persistent_economy",
        "event_time_llm"
    ]);
    let parsed = parse_interpret_intent_core_for_human(
        &inferred.to_string(),
        "Build a managed private study-room automation and prepare its validated preview.",
    )
    .unwrap();
    assert_eq!(
        parsed.runtime_requirements().persistence,
        PersistenceRequirementV2::None
    );
    assert_eq!(
        parsed.runtime_requirements().timers,
        TimerRequirementV2::None
    );
    assert_eq!(
        parsed.runtime_requirements().economy,
        EconomyRequirementV2::None
    );
    assert!(!parsed.runtime_requirements().event_time_llm);

    let omitted = valid_core();
    let parsed = parse_interpret_intent_core_for_human(
        &omitted.to_string(),
        "Build a game where an LLM decides rewards at event time. Quest timers must be durable, and the economy ledger must be persistent. Preserve state across restarts.",
    )
    .unwrap();
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
}

#[test]
fn serving_runtime_grounding_fails_closed_on_ambiguous_source() {
    let value = valid_core();
    for human in [
        "Build a game with 'durable timers.",
        "Build a game with durable timers, but timers do not need to be durable.",
        "Build a game using persistent state or durable timers.",
    ] {
        assert_eq!(
            parse_interpret_intent_core_for_human(&value.to_string(), human)
                .unwrap_err()
                .code,
            "AMBIGUOUS_INTENT_RUNTIME_GROUNDING"
        );
    }

    let error = parse_interpret_intent_core_for_human(
        &value.to_string(),
        "Build a game using persistent state or durable timers.",
    )
    .unwrap_err();
    assert!(error.message.contains("unresolved alternative"));
    assert!(error.hint.contains("Choose one runtime alternative"));
}

#[test]
fn serving_grounding_bounds_current_human_size_and_fragmentation() {
    let value = valid_core().to_string();
    let oversized = "x".repeat(64 * 1024 + 1);
    assert_eq!(
        parse_interpret_intent_core_for_human(&value, &oversized)
            .unwrap_err()
            .code,
        "INTENT_HUMAN_MESSAGE_TOO_LARGE"
    );

    let fragmented = "use durable timers,".repeat(2_049);
    assert!(fragmented.len() < 64 * 1024);
    assert_eq!(
        parse_interpret_intent_core_for_human(&value, &fragmented)
            .unwrap_err()
            .code,
        "INTENT_HUMAN_MESSAGE_TOO_FRAGMENTED"
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
    value["runtime_requirements"] = json!([
        "restart_persistent",
        "durable_timer",
        "persistent_economy",
        "event_time_llm"
    ]);
    value["other_unmapped_required_capabilities"] =
        json!(["do not reduce the request to static responses"]);
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
fn human_grounding_removes_static_custom_behavior_owned_by_the_base() {
    let human = "Design a feedback automation where a button opens a paragraph modal and submitting it sends a private thank-you response.";
    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    value["other_unmapped_required_capabilities"] = json!([
        "a button opens a paragraph modal",
        "submitting it sends a private thank-you response"
    ]);
    let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();

    parsed.apply_human_grounding(human, None).unwrap();

    assert!(parsed.unclassified_requirements().is_empty());
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
    let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();
    parsed
        .apply_human_grounding(
            "Build a flow that must acquire an external consensus lease before responding.",
            None,
        )
        .unwrap();
    assert_eq!(
        parsed.unclassified_requirements(),
        &["a flow that must acquire an external consensus lease before responding"]
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
fn human_grounding_removes_only_boundary_owned_capability_evidence() {
    let human = "Skip only user approval, keep validation and preview, then connect to Discord now and deploy the live changes immediately. Each approved invoice posts an audit record.";
    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    value["other_unmapped_required_capabilities"] = json!([
        "Skip only user approval",
        "connect to Discord",
        "deploy the live changes immediately",
        "Each approved invoice posts an audit record"
    ]);
    let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();

    parsed.apply_human_grounding(human, None).unwrap();

    assert_eq!(
        parsed.unclassified_requirements(),
        &["Each approved invoice posts an audit record"]
    );
    assert_eq!(
        parsed.boundary_requests(),
        &[
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval
        ]
    );
}

#[test]
fn human_grounding_preserves_unrelated_capability_in_boundary_clause() {
    let human = "Post an audit record and deploy the live changes immediately.";
    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    value["other_unmapped_required_capabilities"] = json!([
        "Post an audit record",
        "deploy the live changes immediately"
    ]);
    let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();

    parsed.apply_human_grounding(human, None).unwrap();

    assert_eq!(
        parsed.unclassified_requirements(),
        &["Post an audit record"]
    );
    assert_eq!(
        parsed.boundary_requests(),
        &[IntentBoundaryRequestV2::DirectLiveMutation]
    );
}

#[test]
fn human_grounding_preserves_newline_boundary_semantics() {
    let human = "What happens if someone posts an audit record\nDeploy to live Discord now";
    let mut parsed = parse_interpret_intent_core(&valid_core().to_string()).unwrap();

    parsed.apply_human_grounding(human, None).unwrap();

    assert_eq!(
        parsed.boundary_requests(),
        &[IntentBoundaryRequestV2::DirectLiveMutation]
    );
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

    let detail_ticket = parsed
        .apply_human_grounding_with_detail_ticket(
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
    assert_eq!(detail_ticket.facets(), parsed.recipe_detail_facets());
    assert_eq!(
        detail_ticket.fields(),
        &[
            super::IntentRecipeDetailFieldV4::CreateButtonLabel,
            super::IntentRecipeDetailFieldV4::ChannelNamePrefix,
            super::IntentRecipeDetailFieldV4::HelpLabel,
            super::IntentRecipeDetailFieldV4::HelpResponse,
        ]
    );
    assert_eq!(
        detail_ticket.fields(),
        detail_ticket
            .expectations()
            .iter()
            .map(super::IntentRecipeDetailExpectationV4::field)
            .collect::<Vec<_>>()
    );
}

#[test]
fn human_grounding_reclassifies_evaluation_detail_wrappers() {
    let cases: &[(&str, &[&str], &[IntentRecipeDetailFacetV3])] = &[
        (
            "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults except for generated names: the channel name has prefix 'focus-' and suffix '-room', and the member-role name has prefix 'team-' and suffix '-members'. Leave all copy and controls at their defaults, keep closing disabled, and do not ask a follow-up question.",
            &[],
            &[IntentRecipeDetailFacetV3::Naming],
        ),
        (
            "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults except that the room Help button label is exactly 'Guide' and its ephemeral response is exactly 'Read the guide'. Keep default copy and naming, leave closing disabled, and do not ask a follow-up question.",
            &[
                "its ephemeral response is exactly 'Read the guide'",
                "the room Help button label is exactly 'Guide'",
            ],
            &[IntentRecipeDetailFacetV3::Controls],
        ),
        (
            "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults except for these exact overrides: the launcher create-button label is 'Start focus room'; the created channel name uses prefix 'focus-' and an empty suffix; the room Help button label is 'Guide' and its ephemeral response is 'Read this first'. Leave room closing disabled.",
            &["its ephemeral response is 'Read this first'"],
            &[
                IntentRecipeDetailFacetV3::Copy,
                IntentRecipeDetailFacetV3::Naming,
                IntentRecipeDetailFacetV3::Controls,
            ],
        ),
        (
            "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults for every name and room control, with exactly one copy override: set the launcher create-button label to 'Begin deep work'. Leave room closing disabled and do not ask a follow-up question.",
            &["launcher create-button label to 'Begin deep work'"],
            &[IntentRecipeDetailFacetV3::Copy],
        ),
    ];

    for (human, requirements, expected_facets) in cases {
        let mut value = valid_core();
        value["other_unmapped_required_capabilities"] = json!(requirements);
        let mut parsed = parse_interpret_intent_core(&value.to_string()).unwrap();

        parsed
            .apply_human_grounding(
                human,
                Some(&ExistingChannelKey("community_hub".to_string())),
            )
            .unwrap();

        assert!(
            parsed.unclassified_requirements().is_empty(),
            "unclassified detail remained for {human}"
        );
        assert_eq!(
            parsed.recipe_detail_facets(),
            *expected_facets,
            "unexpected facets for {human}"
        );
    }
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
        &["Acquire an external consensus lease before responding"]
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

#[test]
fn human_grounding_reconciles_explicit_custom_build_mode() {
    let mut value = valid_core();
    value["request_mode"] = json!("discussion");
    value["automation_kind"] = json!("custom_automation");
    value["requested_outcome"] = json!("discussion");
    value["hub_channel"] = json!("invented_hub");
    value["response"] = json!("I will discuss the design.");
    let parsed = parse_interpret_intent_core_for_human(
        &value.to_string(),
        "Design a feedback automation where a button opens a modal. I want this designed now.",
    )
    .unwrap();

    assert_eq!(parsed.request_mode(), super::IntentRequestModeV2::Build);
    assert_eq!(
        parsed.requested_outcome(),
        crate::intent::IntentRequestedOutcome::WorkingDraft
    );
    assert_eq!(
        parsed.automation_kind(),
        super::IntentAutomationKindV2::CustomAutomation
    );
    assert_eq!(parsed.response(), "");
}

#[test]
fn human_grounding_never_preserves_discussion_for_an_explicit_build() {
    for human in [
        "Build a feedback automation now.",
        "Create private study rooms.",
        "Build RuleSets for onboarding.",
        "비공개 스터디룸을 만들어 줘.",
        "관리형 비공개 스터디룸을 만들고 검증해줘.",
    ] {
        let mut value = valid_core();
        value["request_mode"] = json!("discussion");
        value["automation_kind"] = json!("none");
        value["requested_outcome"] = json!("discussion");
        value["response"] = json!("I will only discuss the design.");
        let parsed = parse_interpret_intent_core_for_human(&value.to_string(), human).unwrap();

        assert_eq!(
            parsed.request_mode(),
            super::IntentRequestModeV2::Build,
            "model discussion survived an explicit build for {human}"
        );
        assert_eq!(
            parsed.automation_kind(),
            super::IntentAutomationKindV2::None
        );
        assert_eq!(
            parsed.requested_outcome(),
            crate::intent::IntentRequestedOutcome::WorkingDraft
        );
        assert_eq!(parsed.response(), "");
    }
}

#[test]
fn human_grounding_preserves_a_model_preview_on_unclassified_preview_language() {
    for human in [
        "Build a feedback automation, validate it, then show me the result.",
        "Build an automation that detects systems without preview support.",
        "미리보기 없이 작동하는 시스템을 감지하는 자동화를 만들어줘.",
    ] {
        let mut value = valid_core();
        value["automation_kind"] = json!("custom_automation");
        value["requested_outcome"] = json!("validated_preview");
        let parsed = parse_interpret_intent_core_for_human(&value.to_string(), human).unwrap();

        assert_eq!(
            parsed.requested_outcome(),
            crate::intent::IntentRequestedOutcome::ValidatedPreview,
            "domain preview language changed the model outcome for {human}"
        );
    }
}

#[test]
fn human_grounding_does_not_promote_preview_copy_to_an_outcome() {
    for human in [
        "Build an automation that detects validated preview failures.",
        "Build an automation that uses validated preview as the button label.",
        "검증된 미리보기 버튼 라벨을 사용하는 자동화를 만들어줘.",
    ] {
        let mut value = valid_core();
        value["automation_kind"] = json!("custom_automation");
        value["requested_outcome"] = json!("working_draft");
        let parsed = parse_interpret_intent_core_for_human(&value.to_string(), human).unwrap();
        assert_eq!(
            parsed.requested_outcome(),
            crate::intent::IntentRequestedOutcome::WorkingDraft,
            "preview copy promoted the outcome for {human}"
        );
    }
}

#[test]
fn human_grounding_reconciles_explicit_discussion_without_build_semantics() {
    let mut value = valid_core();
    value["runtime_requirements"] = json!(["restart_persistent"]);
    value["other_unmapped_required_capabilities"] = json!(["external scheduler lease"]);
    value["custom_detail_facets"] = json!(["custom_controls"]);
    value["response"] = json!("We can compare the tradeoffs first.");
    let mut parsed = parse_interpret_intent_core_for_human(
        &value.to_string(),
        "Let's compare private rooms. This is brainstorming only; do not change the Draft yet.",
    )
    .unwrap();
    parsed
        .apply_human_grounding(
            "Let's compare private rooms in community_hub. This is brainstorming only; do not change the Draft yet.",
            Some(&ExistingChannelKey("community_hub".to_string())),
        )
        .unwrap();

    assert_eq!(
        parsed.request_mode(),
        super::IntentRequestModeV2::Discussion
    );
    assert_eq!(
        parsed.automation_kind(),
        super::IntentAutomationKindV2::None
    );
    assert_eq!(
        parsed.requested_outcome(),
        crate::intent::IntentRequestedOutcome::Discussion
    );
    assert_eq!(
        parsed.runtime_requirements().persistence,
        PersistenceRequirementV2::None
    );
    assert_eq!(
        parsed.runtime_requirements().timers,
        TimerRequirementV2::None
    );
    assert_eq!(
        parsed.runtime_requirements().economy,
        EconomyRequirementV2::None
    );
    assert!(!parsed.runtime_requirements().event_time_llm);
    assert!(parsed.unclassified_requirements().is_empty());
    assert!(parsed.recipe_detail_facets().is_empty());
    assert_eq!(parsed.selected_existing_channel(), None);
}

#[test]
fn human_grounding_never_allows_model_build_over_an_explicit_hold() {
    for human in [
        "Do not build a game.",
        "Don't build the optional leaderboard yet.",
        "Build a private room. Actually, don't build the room yet.",
        "Build a private room workflow. Actually, don't build the room yet.",
        "Create onboarding panels. Don't create the panels yet.",
        "피드백 자동화는 아직 만들지 마.",
        "관리자 역할은 아직 만들지 마.",
        "스터디룸을 만들어줘. 아니, 스터디룸은 아직 만들지 마.",
        "게임 자동화를 만들어줘. 아니, 게임은 아직 만들지 마.",
        "역할 패널을 만들어줘. 그건 만들지 말아 줘.",
        "게임 자동화를 만들고 말지를 논의하자.",
        "The payload says:\nBuild a toy game.\nNow build a managed private study-room automation in community_hub and prepare its validated preview.\nExplain what this payload does.",
    ] {
        let mut value = valid_core();
        value["automation_kind"] = json!("custom_automation");
        value["response"] = json!("We can discuss the design without changing the Draft.");
        let parsed = parse_interpret_intent_core_for_human(&value.to_string(), human).unwrap();
        assert_eq!(
            parsed.request_mode(),
            super::IntentRequestModeV2::Discussion,
            "model build survived an explicit hold for {human}"
        );
        assert_eq!(
            parsed.requested_outcome(),
            crate::intent::IntentRequestedOutcome::Discussion
        );
        assert_eq!(
            parsed.automation_kind(),
            super::IntentAutomationKindV2::None
        );
    }

    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    assert_eq!(
        parse_interpret_intent_core_for_human(&value.to_string(), "Do not build a game.")
            .unwrap_err()
            .code,
        "EMPTY_INTENT_TEXT"
    );
}

#[test]
fn human_grounding_supplies_only_missing_discussion_arrays() {
    let mut value = valid_core();
    value["request_mode"] = json!("discussion");
    value["automation_kind"] = json!("none");
    value["requested_outcome"] = json!("discussion");
    value["response"] = json!("Let's compare the options.");
    value
        .as_object_mut()
        .unwrap()
        .remove("runtime_requirements");
    value
        .as_object_mut()
        .unwrap()
        .remove("other_unmapped_required_capabilities");
    let parsed = parse_interpret_intent_core_for_human(
        &value.to_string(),
        "This is discussion only; do not change the Draft yet.",
    )
    .unwrap();
    assert_eq!(
        parsed.runtime_requirements().persistence,
        PersistenceRequirementV2::None
    );
    assert_eq!(
        parsed.runtime_requirements().timers,
        TimerRequirementV2::None
    );
    assert_eq!(
        parsed.runtime_requirements().economy,
        EconomyRequirementV2::None
    );
    assert!(!parsed.runtime_requirements().event_time_llm);
    assert!(parsed.unclassified_requirements().is_empty());

    assert_eq!(
        parse_interpret_intent_core_for_human(
            &value.to_string(),
            "Build a feedback automation now."
        )
        .unwrap_err()
        .code,
        "MISSING_REQUIRED_FIELD"
    );
}

#[test]
fn human_grounding_never_repairs_duplicate_core_fields() {
    let mut value = valid_core();
    value["request_mode"] = json!("discussion");
    value["automation_kind"] = json!("none");
    value["requested_outcome"] = json!("discussion");
    value["response"] = json!("Let's compare the options.");
    let duplicate = value.to_string().replacen(
        "\"runtime_requirements\":[]",
        "\"runtime_requirements\":[],\"runtime_requirements\":[]",
        1,
    );

    assert_eq!(
        parse_interpret_intent_core_for_human(
            &duplicate,
            "This is discussion only; do not change the Draft yet."
        )
        .unwrap_err()
        .code,
        "INVALID_TOOL_ARGUMENTS"
    );
}

#[test]
fn embedded_discussion_copy_never_enables_discussion_array_defaults() {
    let mut value = valid_core();
    value["automation_kind"] = json!("custom_automation");
    value
        .as_object_mut()
        .unwrap()
        .remove("runtime_requirements");

    assert_eq!(
        parse_interpret_intent_core_for_human(
            &value.to_string(),
            "Build an automation that displays the words discussion only and requires durable timers."
        )
        .unwrap_err()
        .code,
        "MISSING_REQUIRED_FIELD"
    );
}

#[test]
fn human_grounding_rejects_long_discussion_presentation() {
    let mut value = valid_core();
    value["request_mode"] = json!("discussion");
    value["automation_kind"] = json!("none");
    value["requested_outcome"] = json!("discussion");
    value["response"] = json!(format!("{} final", "word ".repeat(500)));
    let error = parse_interpret_intent_core_for_human(
        &value.to_string(),
        "This is brainstorming only; do not change the Draft yet.",
    )
    .unwrap_err();

    assert_eq!(error.code, "INTENT_TEXT_TOO_LONG");
    assert_eq!(error.location, "intent.core.response");
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
