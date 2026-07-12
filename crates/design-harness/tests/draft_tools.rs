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
fn tool_registry_contains_the_locked_twelve_tools() {
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
        (
            "begin_rule",
            &["button_click", "modal_submit", "instance_action"][..],
        ),
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
            "trigger":{"kind":"button_click","component":"create_study_room"}
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
            "trigger":{"kind":"modal_submit","modal":"study_modal"}
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
            "trigger":{"kind":"button_click","component":"open_button"}
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
        "begin_rule",
        json!({
            "key":"join",
            "trigger":{"kind":"instance_action","action":"join"}
        }),
    )
    .is_ok());

    assert!(matches!(
        draft.ruleset.rules[0].actions[0],
        ActionSpec::OpenModal { ref modal } if modal == "m"
    ));
    assert!(matches!(
        draft.ruleset.rules[1].trigger,
        TriggerSpec::InstanceAction { ref action } if action == "join"
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
            "trigger":{"kind":"button_click","component":"submit"}
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
fn invalid_kind_returns_schema_kind_choices() {
    let mut draft = Draft::new();
    let result = call(
        &mut draft,
        "begin_rule",
        json!({
            "key":"join",
            "trigger":{"kind":"join_button","action":"join"}
        }),
    );

    let failure = result.failure().unwrap();
    assert_eq!(failure.code, "INVALID_KIND");
    assert_eq!(failure.location, "tool.begin_rule.arguments");
    assert_eq!(failure.message, "kind join_button is not valid");
    assert!(failure.hint.contains("button_click"));
    assert!(failure.hint.contains("modal_submit"));
    assert!(failure.hint.contains("instance_action"));
    assert!(failure.hint.contains("kind must be one of"));
    assert!(!failure.hint.contains("unknown variant"));
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
            "trigger":{"kind":"button_click","component":"empty"}
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
