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
                "route":{"kind":"static","key":"create_study_room"}
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
            json!({
                "key":"open_modal",
                "trigger":{"kind":"button_click","component":"create_study_room"}
            }),
        ),
        (
            "add_interaction_action",
            json!({"rule_key":"open_modal","kind":"open_modal","modal":"study_modal"}),
        ),
        (
            "begin_rule",
            json!({
                "key":"submit_room",
                "trigger":{"kind":"modal_submit","modal":"study_modal"}
            }),
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
            "add_upsert_overwrite_action",
            json!({
                "rule_key":"submit_room",
                "channel":{"kind":"created","name":"room_channel"},
                "target_kind":"everyone",
                "allow":[],
                "deny":["view_channel"]
            }),
        ),
        (
            "add_upsert_overwrite_action",
            json!({
                "rule_key":"submit_room",
                "channel":{"kind":"created","name":"room_channel"},
                "target_kind":"role",
                "role":{"kind":"created","name":"member_role"},
                "allow":["view_channel"],
                "deny":[]
            }),
        ),
        (
            "add_grant_role_action",
            json!({
                "rule_key":"submit_room",
                "role":{"kind":"created","name":"member_role"},
                "target":"actor"
            }),
        ),
        (
            "add_post_panel_action",
            json!({
                "rule_key":"submit_room",
                "key":"welcome_panel",
                "channel":{"kind":"created","name":"room_channel"},
                "content":"Welcome to ${input.room_name}",
                "buttons":[
                    {"label":"Help","route":{"kind":"static","key":"study_help"}},
                    {"label":"Close","route":{"kind":"instance_action","action":"close"}}
                ]
            }),
        ),
        (
            "add_post_panel_action",
            json!({
                "rule_key":"submit_room",
                "key":"hub_panel",
                "channel":{"kind":"existing","name":"study_hub"},
                "content":"${input.room_name} is open",
                "buttons":[
                    {"label":"Join","route":{"kind":"instance_action","action":"join"}}
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
