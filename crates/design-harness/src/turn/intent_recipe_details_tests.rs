use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::{
    analyze_private_study_room_details, parse_private_study_room_details,
    parse_private_study_room_details_for_active_serving,
    parse_private_study_room_details_for_serving, private_study_room_details_frontier,
    private_study_room_details_frontier_for, private_study_room_details_frontier_for_fields,
    IntentRecipeDetailFacetV3, IntentRecipeDetailFieldV4, EXTRACT_PRIVATE_STUDY_ROOM_DETAILS,
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
    let copy_properties = property_names(&tool.parameters["properties"]["copy"]);
    assert!(copy_properties.contains("welcome_content_prefix"));
    assert!(copy_properties.contains("welcome_content_suffix"));
    assert!(!copy_properties.contains("welcome_content"));
    let schema_bytes = serde_json::to_vec(&tool.parameters).unwrap().len();
    assert!(
        schema_bytes < 1_800,
        "routed detail schema is {schema_bytes} bytes"
    );
}

#[test]
fn active_field_frontier_exposes_and_requires_only_grounded_material_leaves() {
    let human = "Use English defaults except for these exact overrides: the launcher create-button label is 'Start focus room'; the created channel name uses prefix 'focus-' and an empty suffix; the room Help button label is 'Guide' and its ephemeral response is 'Read this first'.";
    let analysis = analyze_private_study_room_details(human);
    let fields = analysis.fields();
    let [tool] = private_study_room_details_frontier_for_fields(
        &[
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ],
        fields,
    )
    .unwrap();

    assert_eq!(
        property_names(&tool.parameters["properties"]["copy"]),
        ["create_button_label"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    assert_eq!(
        property_names(&tool.parameters["properties"]["naming"]),
        ["channel_name_prefix"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    assert_eq!(
        property_names(&tool.parameters["properties"]["controls"]),
        ["help_label", "help_response"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    for facet in ["copy", "naming", "controls"] {
        assert_eq!(
            required_names(&tool.parameters["properties"][facet]),
            property_names(&tool.parameters["properties"][facet])
        );
    }
    assert!(!tool.parameters.to_string().contains("channel_name_suffix"));
    assert!(serde_json::to_vec(&tool.parameters).unwrap().len() < 900);
}

#[test]
fn every_detail_field_name_matches_serde_and_its_single_leaf_schema() {
    let fields = [
        IntentRecipeDetailFieldV4::LauncherContent,
        IntentRecipeDetailFieldV4::CreateButtonLabel,
        IntentRecipeDetailFieldV4::ModalTitle,
        IntentRecipeDetailFieldV4::RoomNameLabel,
        IntentRecipeDetailFieldV4::WelcomeContentPrefix,
        IntentRecipeDetailFieldV4::WelcomeContentSuffix,
        IntentRecipeDetailFieldV4::HubAnnouncementPrefix,
        IntentRecipeDetailFieldV4::HubAnnouncementSuffix,
        IntentRecipeDetailFieldV4::CompletedResponsePrefix,
        IntentRecipeDetailFieldV4::CompletedResponseSuffix,
        IntentRecipeDetailFieldV4::ChannelNamePrefix,
        IntentRecipeDetailFieldV4::ChannelNameSuffix,
        IntentRecipeDetailFieldV4::MemberRoleNamePrefix,
        IntentRecipeDetailFieldV4::MemberRoleNameSuffix,
        IntentRecipeDetailFieldV4::HelpLabel,
        IntentRecipeDetailFieldV4::HelpResponse,
        IntentRecipeDetailFieldV4::JoinLabel,
        IntentRecipeDetailFieldV4::JoinedResponse,
        IntentRecipeDetailFieldV4::CloseLabel,
        IntentRecipeDetailFieldV4::ClosedResponse,
    ];
    for field in fields {
        assert_eq!(serde_json::to_value(field).unwrap(), json!(field.as_str()));
        let facet = field.facet();
        let facet_name = match facet {
            IntentRecipeDetailFacetV3::Copy => "copy",
            IntentRecipeDetailFacetV3::Naming => "naming",
            IntentRecipeDetailFacetV3::Controls => "controls",
        };
        let [tool] = private_study_room_details_frontier_for_fields(&[facet], &[field]).unwrap();
        assert_eq!(
            property_names(&tool.parameters),
            [facet_name.to_string()].into()
        );
        assert_eq!(
            required_names(&tool.parameters),
            [facet_name.to_string()].into()
        );
        assert_eq!(
            property_names(&tool.parameters["properties"][facet_name]),
            [field.as_str().to_string()].into()
        );
        assert_eq!(
            required_names(&tool.parameters["properties"][facet_name]),
            [field.as_str().to_string()].into()
        );
    }
    let facets = [
        IntentRecipeDetailFacetV3::Copy,
        IntentRecipeDetailFacetV3::Naming,
        IntentRecipeDetailFacetV3::Controls,
    ];
    let [tool] = private_study_room_details_frontier_for_fields(&facets, &fields).unwrap();
    let schema_bytes = serde_json::to_vec(&tool.parameters).unwrap().len();
    assert!(
        schema_bytes < 2_100,
        "all-field schema is {schema_bytes} bytes"
    );
}

#[test]
fn active_field_parser_requires_exact_leaf_shape_and_restores_empty_affix_counterpart() {
    let human = "Use English defaults except for these exact overrides: the launcher create-button label is 'Start focus room'; the created channel name uses prefix 'focus-' and an empty suffix; the room Help button label is 'Guide' and its ephemeral response is 'Read this first'.";
    let analysis = analyze_private_study_room_details(human);
    let fields = [
        IntentRecipeDetailFieldV4::CreateButtonLabel,
        IntentRecipeDetailFieldV4::ChannelNamePrefix,
        IntentRecipeDetailFieldV4::HelpLabel,
        IntentRecipeDetailFieldV4::HelpResponse,
    ];
    let facets = [
        IntentRecipeDetailFacetV3::Copy,
        IntentRecipeDetailFacetV3::Naming,
        IntentRecipeDetailFacetV3::Controls,
    ];
    assert_eq!(analysis.fields(), &fields);
    let arguments = json!({
        "copy": {"create_button_label": "Start focus room"},
        "naming": {"channel_name_prefix": "focus-"},
        "controls": {"help_label": "Guide", "help_response": "Read this first"}
    });
    let parsed = parse_private_study_room_details_for_active_serving(
        &arguments.to_string(),
        &facets,
        analysis.expectations(),
        4,
        CORE_DIGEST,
        human,
    )
    .unwrap();
    assert_eq!(parsed.naming().channel_name.as_ref().unwrap().suffix, "");

    for invalid in [
        json!({
            "copy": {"create_button_label": "Start focus room"},
            "naming": {"channel_name_prefix": "focus-", "channel_name_suffix": ""},
            "controls": {"help_label": "Guide", "help_response": "Read this first"}
        }),
        json!({
            "copy": {"create_button_label": "Start focus room"},
            "naming": {"channel_name_prefix": "focus-"},
            "controls": {"help_label": "Guide"}
        }),
        json!({
            "copy": {"create_button_label": "Start focus room"},
            "naming": {"channel_name_prefix": "focus-"},
            "controls": {"help_label": "Guide", "help_response": null}
        }),
    ] {
        let error = parse_private_study_room_details_for_active_serving(
            &invalid.to_string(),
            &facets,
            analysis.expectations(),
            4,
            CORE_DIGEST,
            human,
        )
        .unwrap_err();
        assert_eq!(error.code, "RECIPE_DETAIL_FRONTIER_MISMATCH");
    }
}

#[test]
fn active_field_parser_rejects_duplicate_root_and_leaf_keys_before_deserialization() {
    let human = "Set the Help button label to 'Guide' and its response to 'Read this first'.";
    let analysis = analyze_private_study_room_details(human);
    let facets = [IntentRecipeDetailFacetV3::Controls];
    let fields = [
        IntentRecipeDetailFieldV4::HelpLabel,
        IntentRecipeDetailFieldV4::HelpResponse,
    ];
    assert_eq!(analysis.fields(), &fields);
    for arguments in [
        r#"{"controls":{"help_label":"Guide","help_response":"Read this first"},"controls":{"help_label":"Guide","help_response":"Read this first"}}"#,
        r#"{"controls":{"help_label":"Guide","help_label":"Guide","help_response":"Read this first"}}"#,
    ] {
        let error = parse_private_study_room_details_for_active_serving(
            arguments,
            &facets,
            analysis.expectations(),
            4,
            CORE_DIGEST,
            human,
        )
        .unwrap_err();
        assert_eq!(error.code, "RECIPE_DETAIL_FRONTIER_MISMATCH");
        assert!(error
            .location
            .starts_with("intent.details.arguments.controls"));
    }
}

#[test]
fn active_field_parser_reports_non_object_frontier_locations_deterministically() {
    let human = "Set the Help button label to 'Guide'.";
    let analysis = analyze_private_study_room_details(human);
    for (arguments, location) in [
        ("null", "intent.details.arguments"),
        ("[]", "intent.details.arguments"),
        (r#""scalar""#, "intent.details.arguments"),
        (r#"{"controls":null}"#, "intent.details.arguments.controls"),
        (r#"{"controls":[]}"#, "intent.details.arguments.controls"),
        (
            r#"{"controls":"scalar"}"#,
            "intent.details.arguments.controls",
        ),
    ] {
        let error = parse_private_study_room_details_for_active_serving(
            arguments,
            &[IntentRecipeDetailFacetV3::Controls],
            analysis.expectations(),
            4,
            CORE_DIGEST,
            human,
        )
        .unwrap_err();
        assert_eq!(error.code, "RECIPE_DETAIL_FRONTIER_MISMATCH");
        assert_eq!(error.location, location);
    }
}

#[test]
fn active_field_parser_binds_each_literal_to_its_exact_grounded_slot() {
    let human = "Exact overrides: the launcher create-button label is 'Start focus room'; the Help button label is 'Guide' and its response is 'Read this first'.";
    let analysis = analyze_private_study_room_details(human);
    let facets = [
        IntentRecipeDetailFacetV3::Copy,
        IntentRecipeDetailFacetV3::Controls,
    ];
    for arguments in [
        json!({
            "copy": {"create_button_label": "Start focus room"},
            "controls": {"help_label": "Read this first", "help_response": "Guide"}
        }),
        json!({
            "copy": {"create_button_label": "Guide"},
            "controls": {"help_label": "Start focus room", "help_response": "Read this first"}
        }),
        json!({
            "copy": {"create_button_label": "a"},
            "controls": {"help_label": "Guide", "help_response": "Read this first"}
        }),
    ] {
        let error = parse_private_study_room_details_for_active_serving(
            &arguments.to_string(),
            &facets,
            analysis.expectations(),
            4,
            CORE_DIGEST,
            human,
        )
        .unwrap_err();
        assert_eq!(error.code, "RECIPE_DETAIL_LITERAL_MISMATCH");
        assert!(!error.message.contains("Guide"));
        assert!(!error.message.contains("Read this first"));
    }
}

#[test]
fn active_field_parser_rejects_swaps_between_raw_literals_with_equal_normalized_text() {
    for (human, facets, valid, swapped) in [
        (
            "Set the Help button label to 'A  B' and its response to 'A B'.",
            vec![IntentRecipeDetailFacetV3::Controls],
            json!({"controls": {"help_label": "A  B", "help_response": "A B"}}),
            json!({"controls": {"help_label": "A B", "help_response": "A  B"}}),
        ),
        (
            "Set the launcher create-button label to 'A  B' and the Help button label to 'A B'.",
            vec![
                IntentRecipeDetailFacetV3::Copy,
                IntentRecipeDetailFacetV3::Controls,
            ],
            json!({
                "copy": {"create_button_label": "A  B"},
                "controls": {"help_label": "A B"}
            }),
            json!({
                "copy": {"create_button_label": "A B"},
                "controls": {"help_label": "A  B"}
            }),
        ),
    ] {
        let analysis = analyze_private_study_room_details(human);
        parse_private_study_room_details_for_active_serving(
            &valid.to_string(),
            &facets,
            analysis.expectations(),
            4,
            CORE_DIGEST,
            human,
        )
        .unwrap();
        let error = parse_private_study_room_details_for_active_serving(
            &swapped.to_string(),
            &facets,
            analysis.expectations(),
            4,
            CORE_DIGEST,
            human,
        )
        .unwrap_err();
        assert_eq!(error.code, "RECIPE_DETAIL_LITERAL_MISMATCH");
    }
}

#[test]
fn active_field_parser_accepts_one_exact_literal_intentionally_shared_by_two_slots() {
    let human = "Set the Help button label to 'Same text' and its response to 'Same text'.";
    let analysis = analyze_private_study_room_details(human);
    let parsed = parse_private_study_room_details_for_active_serving(
        &json!({
            "controls": {"help_label": "Same text", "help_response": "Same text"}
        })
        .to_string(),
        &[IntentRecipeDetailFacetV3::Controls],
        analysis.expectations(),
        4,
        CORE_DIGEST,
        human,
    )
    .unwrap();
    assert_eq!(parsed.controls().help_label.as_deref(), Some("Same text"));
    assert_eq!(
        parsed.controls().help_response.as_deref(),
        Some("Same text")
    );
}

#[test]
fn active_field_parser_preserves_exact_human_whitespace_after_slot_matching() {
    let human = "Set the modal title to 'Deep   Focus'.";
    let analysis = analyze_private_study_room_details(human);
    let parsed = parse_private_study_room_details_for_active_serving(
        &json!({"copy": {"modal_title": "Deep   Focus"}}).to_string(),
        &[IntentRecipeDetailFacetV3::Copy],
        analysis.expectations(),
        4,
        CORE_DIGEST,
        human,
    )
    .unwrap();
    assert_eq!(parsed.copy().modal_title.as_deref(), Some("Deep   Focus"));

    let error = parse_private_study_room_details_for_active_serving(
        &json!({"copy": {"modal_title": "Deep Focus"}}).to_string(),
        &[IntentRecipeDetailFacetV3::Copy],
        analysis.expectations(),
        4,
        CORE_DIGEST,
        human,
    )
    .unwrap_err();
    assert_eq!(error.code, "UNGROUNDED_RECIPE_DETAIL_LITERAL");
}

#[test]
fn active_field_frontier_rejects_duplicate_or_cross_facet_tickets() {
    assert_eq!(
        private_study_room_details_frontier_for_fields(
            &[IntentRecipeDetailFacetV3::Copy],
            &[
                IntentRecipeDetailFieldV4::CreateButtonLabel,
                IntentRecipeDetailFieldV4::CreateButtonLabel,
            ],
        )
        .unwrap_err()
        .code,
        "DUPLICATE_RECIPE_DETAIL_FIELD"
    );
    assert_eq!(
        private_study_room_details_frontier_for_fields(
            &[IntentRecipeDetailFacetV3::Copy],
            &[IntentRecipeDetailFieldV4::HelpLabel],
        )
        .unwrap_err()
        .code,
        "INVALID_RECIPE_DETAIL_FIELD_FRONTIER"
    );
}

#[test]
fn active_detail_frontier_is_canonical_for_every_nonempty_facet_subset() {
    let all = [
        IntentRecipeDetailFacetV3::Copy,
        IntentRecipeDetailFacetV3::Naming,
        IntentRecipeDetailFacetV3::Controls,
    ];
    let mut largest_schema = 0;
    for mask in 1..8 {
        let canonical = all
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, facet)| *facet)
            .collect::<Vec<_>>();
        let reversed = canonical.iter().rev().copied().collect::<Vec<_>>();
        let [canonical_tool] = private_study_room_details_frontier_for(&canonical).unwrap();
        let [reversed_tool] = private_study_room_details_frontier_for(&reversed).unwrap();
        assert_eq!(canonical_tool.parameters, reversed_tool.parameters);

        let names = canonical
            .iter()
            .map(|facet| match facet {
                IntentRecipeDetailFacetV3::Copy => "copy",
                IntentRecipeDetailFacetV3::Naming => "naming",
                IntentRecipeDetailFacetV3::Controls => "controls",
            })
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(property_names(&canonical_tool.parameters), names);
        assert_eq!(required_names(&canonical_tool.parameters), names);
        assert_eq!(
            canonical_tool.parameters.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
        for name in property_names(&canonical_tool.parameters) {
            assert_eq!(
                canonical_tool.parameters["properties"][&name].get("additionalProperties"),
                Some(&Value::Bool(false))
            );
        }
        let schema_text = canonical_tool.parameters.to_string();
        assert!(!schema_text.contains("$defs"));
        assert!(!schema_text.contains("$ref"));
        largest_schema = largest_schema.max(
            serde_json::to_vec(&canonical_tool.parameters)
                .unwrap()
                .len(),
        );
    }
    assert!(
        largest_schema < 1_800,
        "largest routed detail schema is {largest_schema} bytes"
    );
}

#[test]
fn serving_detail_parser_flattens_patterns_and_stamps_all_facets() {
    let arguments = json!({
        "copy": {"create_button_label": "Start focus room"},
        "naming": {"channel_name_prefix": "focus-", "channel_name_suffix": ""},
        "controls": {"help_label": "Guide", "help_response": "Read this first"}
    });
    let parsed = parse_private_study_room_details_for_serving(
        &arguments.to_string(),
        &[
            IntentRecipeDetailFacetV3::Controls,
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Copy,
        ],
        9,
        CORE_DIGEST,
        "Start focus room focus- Guide Read this first",
    )
    .unwrap();
    assert_eq!(parsed.expected_revision(), 9);
    assert_eq!(
        parsed.copy().create_button_label.as_deref(),
        Some("Start focus room")
    );
    let channel_name = parsed.naming().channel_name.as_ref().unwrap();
    assert_eq!(channel_name.prefix, "focus-");
    assert_eq!(channel_name.suffix, "");
    assert_eq!(parsed.controls().help_label.as_deref(), Some("Guide"));
    assert_eq!(
        parsed.controls().help_response.as_deref(),
        Some("Read this first")
    );
    assert_eq!(
        parsed.covered_facets(),
        &[
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ]
    );
}

#[test]
fn serving_detail_parser_uses_empty_counterpart_and_rejects_nested_patterns() {
    let parsed = parse_private_study_room_details_for_serving(
        &json!({"naming": {"channel_name_prefix": "focus-"}}).to_string(),
        &[IntentRecipeDetailFacetV3::Naming],
        0,
        CORE_DIGEST,
        "focus-",
    )
    .unwrap();
    assert_eq!(parsed.naming().channel_name.as_ref().unwrap().suffix, "");
    assert_eq!(
        parse_private_study_room_details_for_serving(
            &json!({"naming": {"channel_name": {"prefix": "focus-", "suffix": ""}}}).to_string(),
            &[IntentRecipeDetailFacetV3::Naming],
            0,
            CORE_DIGEST,
            "focus-",
        )
        .unwrap_err()
        .code,
        "UNKNOWN_FIELD"
    );
}

#[test]
fn serving_detail_parser_grounds_every_literal_family_in_the_current_human_turn() {
    let arguments = json!({
        "copy": {
            "launcher_content": "Launch text",
            "create_button_label": "Create label",
            "modal_title": "Modal title",
            "room_name_label": "Room label",
            "welcome_content_prefix": "Welcome prefix",
            "welcome_content_suffix": "Welcome suffix",
            "hub_announcement_prefix": "Hub prefix",
            "hub_announcement_suffix": "Hub suffix",
            "completed_response_prefix": "Done prefix",
            "completed_response_suffix": "Done suffix"
        },
        "naming": {
            "channel_name_prefix": "channel-",
            "channel_name_suffix": "-room",
            "member_role_name_prefix": "member-",
            "member_role_name_suffix": "-role"
        },
        "controls": {
            "help_label": "Help label",
            "help_response": "Help response",
            "join_label": "Join label",
            "joined_response": "Joined response",
            "close_label": "Close label",
            "closed_response": "Closed response"
        }
    });
    let human = "Launch text Create label Modal title Room label Welcome prefix Welcome suffix Hub prefix Hub suffix Done prefix Done suffix channel- -room member- -role Help label Help response Join label Joined response Close label Closed response";
    assert!(parse_private_study_room_details_for_serving(
        &arguments.to_string(),
        &[
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ],
        0,
        CORE_DIGEST,
        human,
    )
    .is_ok());
}

#[test]
fn serving_detail_parser_rejects_ungrounded_literals_without_echoing_them() {
    let cases = [
        (
            json!({"copy": {"create_button_label": "Invented scalar"}}),
            IntentRecipeDetailFacetV3::Copy,
            "intent.details.copy.create_button_label",
        ),
        (
            json!({"copy": {"welcome_content_prefix": "Invented prefix"}}),
            IntentRecipeDetailFacetV3::Copy,
            "intent.details.copy.welcome_content.prefix",
        ),
        (
            json!({"naming": {"channel_name_suffix": "Invented suffix"}}),
            IntentRecipeDetailFacetV3::Naming,
            "intent.details.naming.channel_name.suffix",
        ),
        (
            json!({"controls": {"help_response": "Invented response"}}),
            IntentRecipeDetailFacetV3::Controls,
            "intent.details.controls.help_response",
        ),
    ];
    for (arguments, facet, location) in cases {
        let error = parse_private_study_room_details_for_serving(
            &arguments.to_string(),
            &[facet],
            0,
            CORE_DIGEST,
            "The current turn contains none of those values",
        )
        .unwrap_err();
        assert_eq!(error.code, "UNGROUNDED_RECIPE_DETAIL_LITERAL");
        assert_eq!(error.location, location);
        assert!(!error.message.contains("Invented"));
        assert!(!error.hint.contains("Invented"));
    }
}

#[test]
fn serving_detail_grounding_is_exact_and_normalizes_only_crlf_pairs() {
    let trimmed = parse_private_study_room_details_for_serving(
        &json!({"controls": {"help_label": " Guide "}}).to_string(),
        &[IntentRecipeDetailFacetV3::Controls],
        0,
        CORE_DIGEST,
        "Use Guide as the label",
    )
    .unwrap();
    assert_eq!(trimmed.controls().help_label.as_deref(), Some("Guide"));

    let crlf = parse_private_study_room_details_for_serving(
        &json!({"controls": {"help_response": "Line one\nLine two"}}).to_string(),
        &[IntentRecipeDetailFacetV3::Controls],
        0,
        CORE_DIGEST,
        "Reply with Line one\r\nLine two exactly",
    )
    .unwrap();
    assert_eq!(
        crlf.controls().help_response.as_deref(),
        Some("Line one\nLine two")
    );

    for (candidate, human) in [("Guide", "guide"), ("Two spaces", "Two  spaces")] {
        assert_eq!(
            parse_private_study_room_details_for_serving(
                &json!({"controls": {"help_label": candidate}}).to_string(),
                &[IntentRecipeDetailFacetV3::Controls],
                0,
                CORE_DIGEST,
                human,
            )
            .unwrap_err()
            .code,
            "UNGROUNDED_RECIPE_DETAIL_LITERAL"
        );
    }
}

#[test]
fn serving_detail_parser_rejects_a_present_empty_pattern() {
    let error = parse_private_study_room_details_for_serving(
        &json!({"naming": {"channel_name_prefix": "", "channel_name_suffix": ""}}).to_string(),
        &[IntentRecipeDetailFacetV3::Naming],
        0,
        CORE_DIGEST,
        "Use an empty name pattern",
    )
    .unwrap_err();
    assert_eq!(error.code, "EMPTY_RECIPE_DETAIL_PATTERN");
    assert_eq!(error.location, "intent.details.naming.channel_name");
}

#[test]
fn serving_detail_parser_enforces_the_active_root_frontier() {
    for arguments in [
        json!({}),
        json!({
            "copy": {"create_button_label": "Start focus room"},
            "controls": {}
        }),
    ] {
        let error = parse_private_study_room_details_for_serving(
            &arguments.to_string(),
            &[IntentRecipeDetailFacetV3::Copy],
            0,
            CORE_DIGEST,
            "Use Start focus room",
        )
        .unwrap_err();
        assert_eq!(error.code, "RECIPE_DETAIL_FRONTIER_MISMATCH");
        assert_eq!(error.location, "intent.details.arguments");
    }

    let error = parse_private_study_room_details_for_serving(
        &json!({"copy": null}).to_string(),
        &[IntentRecipeDetailFacetV3::Copy],
        0,
        CORE_DIGEST,
        "Use Start focus room",
    )
    .unwrap_err();
    assert_eq!(error.code, "EMPTY_REQUIRED_RECIPE_DETAIL");
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
        schema_bytes < 2_100,
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
