use design_harness::{dispatch_tool, Draft};
use serde_json::{json, Value};

pub fn golden_calls() -> Vec<(&'static str, Value)> {
    vec![
        (
            "add_panel",
            json!({
                "key":"study_panel",
                "channel":"study_hub",
                "content":"Create a study room"
            }),
        ),
        (
            "add_button",
            json!({
                "panel_key":"study_panel",
                "label":"Create room",
                "route":{"static":"create_study_room"}
            }),
        ),
        (
            "add_modal",
            json!({
                "key":"study_modal",
                "title":"Create study room",
                "fields":[
                    {"key":"room_name","label":"Room name","style":"short","required":true}
                ]
            }),
        ),
        (
            "begin_rule",
            json!({"key":"open_modal","trigger":{"button_click":"create_study_room"}}),
        ),
        (
            "add_interaction_action",
            json!({"rule_key":"open_modal","kind":"open_modal","modal":"study_modal"}),
        ),
        (
            "begin_rule",
            json!({"key":"submit_room","trigger":{"modal_submit":"study_modal"}}),
        ),
        (
            "add_interaction_action",
            json!({"rule_key":"submit_room","kind":"defer_ephemeral"}),
        ),
        (
            "add_resource_action",
            json!({
                "rule_key":"submit_room",
                "kind":"create_role",
                "key":"member_role",
                "name":"${input.room_name} members"
            }),
        ),
        (
            "add_resource_action",
            json!({
                "rule_key":"submit_room",
                "kind":"create_channel",
                "key":"room_channel",
                "name":"study-${input.room_name}"
            }),
        ),
        (
            "add_permission_action",
            json!({
                "rule_key":"submit_room",
                "kind":"upsert_overwrite",
                "channel":{"created":"room_channel"},
                "target":"everyone",
                "allow":[],
                "deny":["view_channel"]
            }),
        ),
        (
            "add_permission_action",
            json!({
                "rule_key":"submit_room",
                "kind":"upsert_overwrite",
                "channel":{"created":"room_channel"},
                "target":{"role":{"created":"member_role"}},
                "allow":["view_channel"],
                "deny":[]
            }),
        ),
        (
            "add_permission_action",
            json!({
                "rule_key":"submit_room",
                "kind":"grant_role",
                "role":{"created":"member_role"},
                "target":"actor"
            }),
        ),
        (
            "add_post_panel_action",
            json!({
                "rule_key":"submit_room",
                "key":"welcome_panel",
                "channel":{"created":"room_channel"},
                "content":"Welcome to ${input.room_name}",
                "buttons":[
                    {"label":"Help","route":{"static":"study_help"}},
                    {"label":"Close","route":{"instance_action":"close"}}
                ]
            }),
        ),
        (
            "add_post_panel_action",
            json!({
                "rule_key":"submit_room",
                "key":"hub_panel",
                "channel":{"existing":"study_hub"},
                "content":"${input.room_name} is open",
                "buttons":[
                    {"label":"Join","route":{"instance_action":"join"}}
                ]
            }),
        ),
        (
            "add_interaction_action",
            json!({
                "rule_key":"submit_room",
                "kind":"edit_response",
                "content":"Created ${input.room_name}"
            }),
        ),
        (
            "set_register_instance",
            json!({
                "rule_key":"submit_room",
                "instance_key":"study_instance",
                "kind":"study_room",
                "roles":[{"alias":"member_role","created":"member_role"}],
                "channels":[{"alias":"room_channel","created":"room_channel"}],
                "messages":[
                    {"alias":"welcome_panel","created":"welcome_panel"},
                    {"alias":"hub_panel","created":"hub_panel"}
                ]
            }),
        ),
    ]
}

pub async fn golden_draft() -> Draft {
    let mut draft = Draft::new();
    for (name, arguments) in golden_calls() {
        let result = dispatch_tool(&mut draft, name, &arguments.to_string()).await;
        assert!(result.is_ok(), "{name}: {}", result.as_json());
    }
    draft
}
