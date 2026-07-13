use automation_state::{
    ActionSpec, ButtonRoute, ChannelRef, InstanceRef, ModalFieldStyle, OverwriteTargetSpec,
    RoleRef, TriggerSpec,
};
use design_harness::{dispatch_tool, tool_definitions, Draft, ToolResult};
use futures::executor::block_on;
use serde_json::{json, Value};

fn call(draft: &mut Draft, name: &str, arguments: Value) -> ToolResult {
    block_on(dispatch_tool(draft, name, &arguments.to_string()))
}

fn schema_has_kind_variant(schema: &Value, variant: &str) -> bool {
    match schema {
        Value::Array(values) => values
            .iter()
            .any(|value| schema_has_kind_variant(value, variant)),
        Value::Object(values) => {
            let matches = values
                .get("properties")
                .and_then(Value::as_object)
                .and_then(|properties| properties.get("kind"))
                .and_then(|kind| kind.get("const"))
                .and_then(Value::as_str)
                == Some(variant);
            matches
                || values
                    .values()
                    .any(|value| schema_has_kind_variant(value, variant))
        }
        _ => false,
    }
}

#[test]
fn tool_registry_contains_the_locked_design_tools() {
    let definitions = tool_definitions();
    let names: Vec<&str> = definitions
        .iter()
        .map(|definition| definition.name.as_str())
        .collect();

    assert_eq!(
        names,
        vec![
            "add_panel",
            "add_button",
            "add_modal",
            "begin_rule",
            "add_resource_action",
            "add_grant_role_action",
            "add_upsert_overwrite_action",
            "add_interaction_action",
            "add_post_panel_action",
            "set_register_instance",
            "update_panel",
            "remove_panel",
            "update_button",
            "remove_button",
            "update_modal",
            "remove_modal",
            "update_rule",
            "remove_rule",
            "update_action",
            "remove_action",
            "validate_draft",
            "simulate_draft",
        ]
    );
    assert!(definitions
        .iter()
        .all(|definition| definition.parameters.is_object()));
    assert!(names
        .iter()
        .all(|name| !name.contains("activate") && !name.contains("publish")));
}

#[test]
fn model_facing_reference_schemas_are_tagged() {
    let definitions = tool_definitions();
    let expected = [
        ("add_button", &["static", "instance_action"][..]),
        ("add_grant_role_action", &["created", "existing"][..]),
        ("add_upsert_overwrite_action", &["created", "existing"][..]),
        (
            "add_post_panel_action",
            &["created", "existing", "static", "instance_action"][..],
        ),
    ];

    for (name, variants) in expected {
        let definition = definitions
            .iter()
            .find(|definition| definition.name == name)
            .unwrap();
        for variant in variants {
            assert!(schema_has_kind_variant(&definition.parameters, variant));
        }
    }
}

#[test]
fn begin_rule_schema_is_flat() {
    let definitions = tool_definitions();
    let begin_rule = definitions
        .iter()
        .find(|definition| definition.name == "begin_rule")
        .unwrap();
    let properties = begin_rule.parameters["properties"].as_object().unwrap();

    assert_eq!(properties.len(), 3);
    assert!(properties.contains_key("key"));
    assert!(properties.contains_key("trigger_kind"));
    assert!(properties.contains_key("trigger_ref"));
    assert!(!properties.contains_key("trigger"));
    assert_eq!(
        begin_rule.parameters["$defs"]["TriggerKindInput"]["enum"],
        json!(["button_click", "modal_submit", "instance_action"])
    );
    assert_eq!(begin_rule.parameters["additionalProperties"], false);
}

#[test]
fn permission_tool_schemas_are_flat_and_share_reference_shape() {
    let definitions = tool_definitions();
    let grant = definitions
        .iter()
        .find(|definition| definition.name == "add_grant_role_action")
        .unwrap();
    let overwrite = definitions
        .iter()
        .find(|definition| definition.name == "add_upsert_overwrite_action")
        .unwrap();
    let grant_properties = grant.parameters["properties"].as_object().unwrap();
    let overwrite_properties = overwrite.parameters["properties"].as_object().unwrap();

    assert_eq!(grant_properties.len(), 3);
    assert!(grant_properties.contains_key("rule_key"));
    assert!(grant_properties.contains_key("role"));
    assert!(grant_properties.contains_key("target"));
    assert!(!grant_properties.contains_key("kind"));
    assert_eq!(overwrite_properties.len(), 6);
    assert!(overwrite_properties.contains_key("channel"));
    assert!(overwrite_properties.contains_key("target_kind"));
    assert!(overwrite_properties.contains_key("role"));
    assert!(!overwrite_properties.contains_key("kind"));

    let schema = format!("{}{}", grant.parameters, overwrite.parameters);
    assert!(schema.contains("\"name\""));
    assert!(schema.contains("\"everyone\""));
    assert!(schema.contains("\"role\""));
    assert!(!schema.contains("\"alias\""));
    assert!(!schema.contains("\"binding\""));
    assert!(!schema.contains("OverwriteTargetInput"));
}

#[test]
fn draft_mutation_increments_revision_and_invalidates_gates() {
    let mut draft = Draft::new();
    draft.validated_revision = Some(0);
    draft.simulated_revision = Some(0);

    let result = call(
        &mut draft,
        "add_panel",
        json!({
            "key": "study_panel",
            "channel": "study_hub",
            "content": "Create a study room"
        }),
    );

    assert!(result.is_ok());
    assert_eq!(draft.draft_revision, 1);
    assert_eq!(draft.validated_revision, None);
    assert_eq!(draft.simulated_revision, None);
    assert_eq!(draft.summary().panels, 1);
    assert_eq!(draft.summary().actions, 0);
}

#[test]
fn structure_dtos_normalize_to_state_types() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "add_panel",
        json!({"key":"study_panel","channel":"study_hub","content":"Create"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_button",
        json!({
            "panel_key":"study_panel",
            "label":"Create room",
            "route":{"kind":"static","key":"create_study_room"}
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_modal",
        json!({
            "key":"study_modal",
            "title":"Create study room",
            "fields":[
                {"key":"room_name","label":"Room name","style":"short","required":true},
                {"key":"topic","label":"Topic","style":"paragraph","required":false}
            ]
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({
            "key":"open_modal",
            "trigger_kind":"button_click",
            "trigger_ref":"create_study_room"
        }),
    )
    .is_ok());

    assert_eq!(draft.ruleset.panels[0].key, "study_panel");
    assert!(matches!(
        draft.ruleset.panels[0].buttons[0].route,
        ButtonRoute::Static { ref key } if key == "create_study_room"
    ));
    assert_eq!(
        draft.ruleset.modals[0].fields[0].style,
        ModalFieldStyle::Short
    );
    assert_eq!(
        draft.ruleset.modals[0].fields[1].style,
        ModalFieldStyle::Paragraph
    );
    assert!(matches!(
        draft.ruleset.rules[0].trigger,
        TriggerSpec::ButtonClick { ref component } if component == "create_study_room"
    ));
}

#[test]
fn grouped_action_dtos_normalize_and_register_finalizes_footprint() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "add_modal",
        json!({
            "key":"study_modal",
            "title":"Create study room",
            "fields":[{"key":"room_name","label":"Room name","style":"short","required":true}]
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({
            "key":"submit_room",
            "trigger_kind":"modal_submit",
            "trigger_ref":"study_modal"
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_interaction_action",
        json!({"rule_key":"submit_room","kind":"defer_ephemeral"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_resource_action",
        json!({
            "rule_key":"submit_room",
            "kind":"create_role",
            "key":"member_role",
            "name":"${input.room_name} members"
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_resource_action",
        json!({
            "rule_key":"submit_room",
            "kind":"create_channel",
            "key":"room_channel",
            "name":"study-${input.room_name}"
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_upsert_overwrite_action",
        json!({
            "rule_key":"submit_room",
            "channel":{"kind":"created","name":"room_channel"},
            "target_kind":"everyone",
            "allow":[],
            "deny":["view_channel"]
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_grant_role_action",
        json!({
            "rule_key":"submit_room",
            "role":{"kind":"created","name":"member_role"},
            "target":"actor"
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_post_panel_action",
        json!({
            "rule_key":"submit_room",
            "key":"hub_panel",
            "channel":{"kind":"existing","name":"study_hub"},
            "content":"Room ${input.room_name} is open",
            "buttons":[
                {"label":"Help","route":{"kind":"static","key":"study_help"}},
                {"label":"Join","route":{"kind":"instance_action","action":"join"}}
            ]
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_interaction_action",
        json!({
            "rule_key":"submit_room",
            "kind":"edit_response",
            "content":"Created ${input.room_name}"
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "set_register_instance",
        json!({
            "rule_key":"submit_room",
            "instance_key":"study_instance",
            "kind":"study_room",
            "roles":[{"alias":"member_role","created":"member_role"}],
            "channels":[{"alias":"room_channel","created":"room_channel"}],
            "messages":[{"alias":"hub_panel","created":"hub_panel"}]
        }),
    )
    .is_ok());

    let actions = &draft.ruleset.rules[0].actions;
    assert!(matches!(actions[0], ActionSpec::DeferEphemeral));
    assert!(matches!(actions[1], ActionSpec::CreateRole { .. }));
    assert!(matches!(actions[2], ActionSpec::CreateChannel { .. }));
    assert!(matches!(
        actions[3],
        ActionSpec::UpsertOverwrite {
            channel: ChannelRef::Created(_),
            target: OverwriteTargetSpec::Everyone,
            ..
        }
    ));
    assert!(matches!(
        actions[4],
        ActionSpec::GrantRole {
            role: RoleRef::Created(_),
            ..
        }
    ));
    let ActionSpec::PostPanel { buttons, .. } = &actions[5] else {
        panic!("expected post panel")
    };
    assert!(matches!(
        buttons[1].route,
        ButtonRoute::InstanceAction {
            instance: InstanceRef::Created(ref created),
            ref action,
        } if created.created == "study_instance" && action == "join"
    ));
    assert!(matches!(actions[6], ActionSpec::RegisterInstance { .. }));
    assert!(matches!(actions[7], ActionSpec::EditResponse { .. }));
}

#[test]
fn overwrite_reference_error_hint_has_concrete_flat_shape() {
    let mut draft = Draft::new();
    let result = call(
        &mut draft,
        "add_upsert_overwrite_action",
        json!({
            "rule_key":"submit_room",
            "channel":{"kind":"created"},
            "target_kind":"role",
            "role":{"kind":"created","name":"member_role"},
            "allow":["view_channel"],
            "deny":[]
        }),
    );

    let failure = result.failure().unwrap();
    assert_eq!(failure.code, "MISSING_REQUIRED_FIELD");
    assert!(failure
        .hint
        .contains("channel: { kind: created(required), name: string(required) }"));
    assert!(failure
        .hint
        .contains("target_kind: everyone|role(required)"));
    assert!(!failure.hint.contains("value"));
}

#[test]
fn overwrite_role_reference_matches_target_kind() {
    let mut draft = Draft::new();
    let missing = call(
        &mut draft,
        "add_upsert_overwrite_action",
        json!({
            "rule_key":"submit_room",
            "channel":{"kind":"created","name":"room_channel"},
            "target_kind":"role",
            "allow":["view_channel"],
            "deny":[]
        }),
    );
    let unexpected = call(
        &mut draft,
        "add_upsert_overwrite_action",
        json!({
            "rule_key":"submit_room",
            "channel":{"kind":"created","name":"room_channel"},
            "target_kind":"everyone",
            "role":{"kind":"created","name":"member_role"},
            "allow":[],
            "deny":["view_channel"]
        }),
    );

    assert_eq!(missing.failure().unwrap().code, "MISSING_OVERWRITE_ROLE");
    assert_eq!(
        unexpected.failure().unwrap().code,
        "UNEXPECTED_OVERWRITE_ROLE"
    );
}

#[test]
fn open_modal_and_instance_trigger_dtos_normalize() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "add_modal",
        json!({"key":"m","title":"Modal","fields":[]}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({
            "key":"open",
            "trigger_kind":"button_click",
            "trigger_ref":"open_button"
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_interaction_action",
        json!({"rule_key":"open","kind":"open_modal","modal":"m"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_interaction_action",
        json!({"rule_key":"open","kind":"respond_ephemeral","content":"Done"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({
            "key":"join",
            "trigger_kind":"instance_action",
            "trigger_ref":"join"
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_interaction_action",
        json!({"rule_key":"join","kind":"teardown_instance"}),
    )
    .is_ok());

    assert!(matches!(
        draft.ruleset.rules[0].actions[0],
        ActionSpec::OpenModal { ref modal } if modal == "m"
    ));
    assert!(matches!(
        draft.ruleset.rules[0].actions[1],
        ActionSpec::RespondEphemeral { ref content } if content == "Done"
    ));
    assert!(matches!(
        draft.ruleset.rules[1].trigger,
        TriggerSpec::InstanceAction { ref action } if action == "join"
    ));
    assert!(matches!(
        draft.ruleset.rules[1].actions[0],
        ActionSpec::TeardownInstance {
            instance: InstanceRef::Event
        }
    ));
}

#[test]
fn incomplete_register_manifest_is_structured_and_non_mutating() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({
            "key":"submit",
            "trigger_kind":"button_click",
            "trigger_ref":"submit"
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_resource_action",
        json!({"rule_key":"submit","kind":"create_role","key":"member","name":"Member"}),
    )
    .is_ok());
    let revision = draft.draft_revision;

    let result = call(
        &mut draft,
        "set_register_instance",
        json!({
            "rule_key":"submit",
            "instance_key":"instance",
            "kind":"study_room",
            "roles":[],
            "channels":[],
            "messages":[]
        }),
    );

    let failure = result.failure().unwrap();
    assert_eq!(failure.code, "INSTANCE_RESOURCE_MISSING");
    assert_eq!(failure.location, "rule.submit.actions");
    assert!(failure.hint.contains("member"));
    assert_eq!(draft.draft_revision, revision);
    assert_eq!(draft.ruleset.rules[0].actions.len(), 1);
}

#[test]
fn malformed_and_unknown_tool_calls_return_structured_errors() {
    let mut draft = Draft::new();
    let malformed = block_on(dispatch_tool(&mut draft, "add_panel", "{"));
    assert_eq!(malformed.failure().unwrap().code, "INVALID_TOOL_ARGUMENTS");

    let unknown = call(&mut draft, "activate", json!({}));
    assert_eq!(unknown.failure().unwrap().code, "UNKNOWN_TOOL");
    assert_eq!(draft.draft_revision, 0);
}

#[test]
fn missing_required_field_returns_expected_shape() {
    let mut draft = Draft::new();
    let result = call(
        &mut draft,
        "add_modal",
        json!({"title":"Study room","fields":[]}),
    );

    let failure = result.failure().unwrap();
    assert_eq!(failure.code, "MISSING_REQUIRED_FIELD");
    assert_eq!(failure.location, "tool.add_modal.arguments.key");
    assert_eq!(failure.message, "missing required field key");
    assert!(failure.hint.contains("add_modal expects"));
    assert!(failure.hint.contains("key: string(required)"));
    assert!(failure.hint.contains("fields: array(required) of"));
    assert!(failure.hint.contains("style: short|paragraph(required)"));
    assert!(!failure.hint.contains("line 1 column"));
}

#[test]
fn invalid_flat_trigger_kind_returns_schema_choices() {
    let mut draft = Draft::new();
    let result = call(
        &mut draft,
        "begin_rule",
        json!({
            "key":"join",
            "trigger_kind":"join_button",
            "trigger_ref":"join"
        }),
    );

    let failure = result.failure().unwrap();
    assert_eq!(failure.code, "INVALID_TOOL_ARGUMENTS");
    assert_eq!(failure.location, "tool.begin_rule.arguments");
    assert_eq!(failure.message, "value join_button is not allowed");
    assert!(failure.hint.contains("button_click"));
    assert!(failure.hint.contains("modal_submit"));
    assert!(failure.hint.contains("instance_action"));
    assert!(!failure.hint.contains("kind must be one of"));
    assert!(!failure.hint.contains("unknown variant"));
}

#[test]
fn begin_rule_rejects_the_legacy_nested_trigger_shape() {
    let mut draft = Draft::new();
    let result = call(
        &mut draft,
        "begin_rule",
        json!({
            "key":"join",
            "trigger":{"kind":"instance_action","action":"join"}
        }),
    );

    let failure = result.failure().unwrap();
    assert_eq!(failure.code, "UNKNOWN_FIELD");
    assert_eq!(failure.location, "tool.begin_rule.arguments.trigger");
    assert_eq!(draft.draft_revision, 0);
    assert!(draft.ruleset.rules.is_empty());
}

#[test]
fn invalid_plain_enum_value_is_not_reported_as_kind() {
    let mut draft = Draft::new();
    let result = call(
        &mut draft,
        "add_modal",
        json!({
            "key":"study_modal",
            "title":"Study room",
            "fields":[{
                "key":"room_name",
                "label":"Room name",
                "style":"long",
                "required":true
            }]
        }),
    );

    let failure = result.failure().unwrap();
    assert_eq!(failure.code, "INVALID_TOOL_ARGUMENTS");
    assert_eq!(failure.message, "value long is not allowed");
    assert!(failure.hint.contains("style: short|paragraph(required)"));
    assert!(!failure.hint.contains("kind must be one of"));
}

#[test]
fn invalid_field_type_returns_expected_nested_shape() {
    let mut draft = Draft::new();
    let result = call(
        &mut draft,
        "add_modal",
        json!({"key":"study_modal","title":"Study room","fields":"room_name"}),
    );

    let failure = result.failure().unwrap();
    assert_eq!(failure.code, "INVALID_FIELD_TYPE");
    assert_eq!(failure.location, "tool.add_modal.arguments");
    assert_eq!(
        failure.message,
        "a field value has a type that does not match the schema"
    );
    assert!(failure.hint.contains("fields: array(required) of"));
    assert!(failure.hint.contains("required: boolean(required)"));
    assert!(!failure.hint.contains("invalid type"));
}

#[test]
fn summary_reports_unknown_fixed_bindings() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "add_panel",
        json!({"key":"p","channel":"unknown_hub","content":"Panel"}),
    )
    .is_ok());

    assert_eq!(
        draft.summary().unresolved_references,
        vec!["unknown_hub".to_string()]
    );
}

#[test]
fn register_rejects_an_empty_ownable_footprint() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({
            "key":"empty",
            "trigger_kind":"button_click",
            "trigger_ref":"empty"
        }),
    )
    .is_ok());
    let revision = draft.draft_revision;

    let result = call(
        &mut draft,
        "set_register_instance",
        json!({
            "rule_key":"empty",
            "instance_key":"empty_instance",
            "kind":"empty",
            "roles":[],
            "channels":[],
            "messages":[]
        }),
    );

    assert_eq!(result.failure().unwrap().code, "EMPTY_INSTANCE_RESOURCES");
    assert_eq!(draft.draft_revision, revision);
}

#[test]
fn structure_updates_keep_stable_keys_and_invalidate_gates() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "add_panel",
        json!({"key":"panel","channel":"study_hub","content":"Before"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_modal",
        json!({"key":"modal","title":"Before","fields":[]}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({"key":"rule","trigger_kind":"instance_action","trigger_ref":"join"}),
    )
    .is_ok());
    draft.validated_revision = Some(draft.draft_revision);
    draft.simulated_revision = Some(draft.draft_revision);
    let revision = draft.draft_revision;

    assert!(call(
        &mut draft,
        "update_panel",
        json!({"key":"panel","content":"After"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "update_modal",
        json!({
            "key":"modal",
            "title":"After",
            "fields":[{"key":"name","label":"Name","style":"short","required":true}]
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "update_rule",
        json!({"key":"rule","trigger":{"kind":"instance_action","action":"leave"}}),
    )
    .is_ok());

    assert_eq!(draft.draft_revision, revision + 3);
    assert_eq!(draft.validated_revision, None);
    assert_eq!(draft.simulated_revision, None);
    assert_eq!(draft.ruleset.panels[0].key, "panel");
    assert_eq!(draft.ruleset.panels[0].content, "After");
    assert_eq!(draft.ruleset.modals[0].key, "modal");
    assert_eq!(draft.ruleset.modals[0].title, "After");
    assert!(matches!(
        draft.ruleset.rules[0].trigger,
        TriggerSpec::InstanceAction { ref action } if action == "leave"
    ));
}

#[test]
fn button_edits_use_route_selector_and_reject_dangling_trigger() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "add_panel",
        json!({"key":"panel","channel":"study_hub","content":"Panel"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_button",
        json!({
            "panel_key":"panel",
            "label":"Before",
            "route":{"kind":"static","key":"open"}
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({"key":"open_rule","trigger_kind":"button_click","trigger_ref":"open"}),
    )
    .is_ok());

    assert!(call(
        &mut draft,
        "update_button",
        json!({
            "panel_key":"panel",
            "selector":{"kind":"static","key":"open"},
            "label":"After"
        }),
    )
    .is_ok());
    assert_eq!(draft.ruleset.panels[0].buttons[0].label, "After");
    let revision = draft.draft_revision;
    draft.validated_revision = Some(revision);

    let result = call(
        &mut draft,
        "remove_button",
        json!({
            "panel_key":"panel",
            "selector":{"kind":"static","key":"open"}
        }),
    );

    assert_eq!(result.failure().unwrap().code, "UNKNOWN_BUTTON_REFERENCE");
    assert_eq!(draft.draft_revision, revision);
    assert_eq!(draft.validated_revision, Some(revision));
    assert_eq!(draft.ruleset.panels[0].buttons.len(), 1);
}

#[test]
fn modal_field_update_rejects_new_template_dangling_reference() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "add_modal",
        json!({
            "key":"modal",
            "title":"Modal",
            "fields":[{"key":"name","label":"Name","style":"short","required":true}]
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({"key":"submit","trigger_kind":"modal_submit","trigger_ref":"modal"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_resource_action",
        json!({
            "rule_key":"submit",
            "kind":"create_role",
            "key":"role",
            "name":"${input.name}"
        }),
    )
    .is_ok());
    let revision = draft.draft_revision;

    let result = call(
        &mut draft,
        "update_modal",
        json!({"key":"modal","fields":[]}),
    );

    assert_eq!(result.failure().unwrap().code, "UNKNOWN_TEMPLATE_INPUT");
    assert_eq!(draft.draft_revision, revision);
    assert_eq!(draft.ruleset.modals[0].fields.len(), 1);
}

#[test]
fn action_updates_support_stable_keys_and_deterministic_occurrences() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({"key":"rule","trigger_kind":"instance_action","trigger_ref":"join"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_resource_action",
        json!({"rule_key":"rule","kind":"create_role","key":"role","name":"Before"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_interaction_action",
        json!({"rule_key":"rule","kind":"respond_ephemeral","content":"First"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_interaction_action",
        json!({"rule_key":"rule","kind":"respond_ephemeral","content":"Second"}),
    )
    .is_ok());

    assert!(call(
        &mut draft,
        "update_action",
        json!({
            "rule_key":"rule",
            "selector":{"kind":"by_key","key":"role"},
            "patch":{"kind":"create_role","name":"After"}
        }),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "update_action",
        json!({
            "rule_key":"rule",
            "selector":{"kind":"by_kind","action":"respond_ephemeral","occurrence":1},
            "patch":{"kind":"respond_ephemeral","content":"Updated second"}
        }),
    )
    .is_ok());

    assert!(matches!(
        draft.ruleset.rules[0].actions[0],
        ActionSpec::CreateRole { ref name, .. } if name == "After"
    ));
    assert!(matches!(
        draft.ruleset.rules[0].actions[2],
        ActionSpec::RespondEphemeral { ref content } if content == "Updated second"
    ));
}

#[test]
fn action_remove_rejects_dangling_created_reference_without_mutation() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({"key":"rule","trigger_kind":"instance_action","trigger_ref":"join"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_resource_action",
        json!({"rule_key":"rule","kind":"create_role","key":"role","name":"Role"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_grant_role_action",
        json!({
            "rule_key":"rule",
            "role":{"kind":"created","name":"role"},
            "target":"actor"
        }),
    )
    .is_ok());
    let revision = draft.draft_revision;

    let result = call(
        &mut draft,
        "remove_action",
        json!({
            "rule_key":"rule",
            "selector":{"kind":"by_key","key":"role"}
        }),
    );

    assert_eq!(
        result.failure().unwrap().code,
        "UNRESOLVED_CREATED_REFERENCE"
    );
    assert_eq!(draft.draft_revision, revision);
    assert_eq!(draft.ruleset.rules[0].actions.len(), 2);
}

#[test]
fn unreferenced_structures_and_rules_can_be_removed() {
    let mut draft = Draft::new();
    assert!(call(
        &mut draft,
        "add_panel",
        json!({"key":"panel","channel":"study_hub","content":"Panel"}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "add_modal",
        json!({"key":"modal","title":"Modal","fields":[]}),
    )
    .is_ok());
    assert!(call(
        &mut draft,
        "begin_rule",
        json!({"key":"rule","trigger_kind":"instance_action","trigger_ref":"join"}),
    )
    .is_ok());

    assert!(call(&mut draft, "remove_panel", json!({"key":"panel"})).is_ok());
    assert!(call(&mut draft, "remove_modal", json!({"key":"modal"})).is_ok());
    assert!(call(&mut draft, "remove_rule", json!({"key":"rule"})).is_ok());
    assert_eq!(draft.summary().panels, 0);
    assert_eq!(draft.summary().modals, 0);
    assert_eq!(draft.summary().rules, 0);
}
