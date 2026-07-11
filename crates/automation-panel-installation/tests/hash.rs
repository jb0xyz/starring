use automation_panel_installation::spec_hash;
use automation_state::{ButtonRoute, ButtonSpec, InstanceRef, PanelSpec};
use desired_state::ResourceKey;

fn panel(key: &str, channel: &str, buttons: Vec<ButtonSpec>) -> PanelSpec {
    PanelSpec {
        key: key.to_string(),
        channel: ResourceKey(channel.to_string()),
        content: "Panel content".to_string(),
        buttons,
    }
}

fn static_button(label: &str, key: &str) -> ButtonSpec {
    ButtonSpec {
        label: label.to_string(),
        route: ButtonRoute::Static {
            key: key.to_string(),
        },
    }
}

#[test]
fn panel_key_and_channel_are_excluded() {
    let first = panel("first", "channel_a", vec![static_button("Join", "join")]);
    let second = panel("second", "channel_b", vec![static_button("Join", "join")]);
    assert_eq!(spec_hash(1, &first), spec_hash(1, &second));
}

#[test]
fn button_order_is_significant() {
    let first = panel(
        "panel",
        "channel",
        vec![static_button("A", "a"), static_button("B", "b")],
    );
    let second = panel(
        "panel",
        "channel",
        vec![static_button("B", "b"), static_button("A", "a")],
    );
    assert_ne!(spec_hash(1, &first), spec_hash(1, &second));
}

#[test]
fn render_revision_is_significant() {
    let spec = panel("panel", "channel", vec![static_button("Join", "join")]);
    assert_ne!(spec_hash(1, &spec), spec_hash(2, &spec));
}

#[test]
fn instance_action_route_uses_action_token() {
    let first = panel(
        "panel",
        "channel",
        vec![ButtonSpec {
            label: "Join".to_string(),
            route: ButtonRoute::InstanceAction {
                instance: InstanceRef::Event,
                action: "join".to_string(),
            },
        }],
    );
    let second = panel(
        "panel",
        "channel",
        vec![ButtonSpec {
            label: "Join".to_string(),
            route: ButtonRoute::InstanceAction {
                instance: InstanceRef::Event,
                action: "leave".to_string(),
            },
        }],
    );
    assert_ne!(spec_hash(1, &first), spec_hash(1, &second));
}
