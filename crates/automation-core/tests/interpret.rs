use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::interpret::interpret;
use automation_core::plan::{PlannedAction, PlannedRole};
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, InteractionRule, InteractionRuleSet, PanelSpec, RoleRef,
    TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, RoleId, UserId};
use resource_resolution::ResourceBindingMap;

fn fixture() -> (InteractionRuleSet, ResourceBindingMap) {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "verify_panel".to_string(),
            channel: ResourceKey("verify_channel".to_string()),
            content: "click".to_string(),
            buttons: vec![ButtonSpec {
                key: "verify_button".to_string(),
                label: "Verify".to_string(),
            }],
        }],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "verify_rule".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "verify_button".to_string(),
            },
            actions: vec![
                ActionSpec::GrantRole {
                    role: RoleRef::Existing(ResourceKey("verified_member".to_string())),
                    target: ActionTarget::Actor,
                },
                ActionSpec::RespondEphemeral {
                    content: "welcome".to_string(),
                },
            ],
        }],
    };
    let mut bindings = ResourceBindingMap::default();
    bindings
        .role_bindings
        .insert(ResourceKey("verified_member".to_string()), RoleId(555));
    (set, bindings)
}

fn click(component: &str) -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(42),
        kind: EventKind::ButtonClick {
            component: component.to_string(),
        },
    }
}

#[test]
fn matching_button_click_finds_rule() {
    let (set, bindings) = fixture();
    assert!(interpret(&click("verify_button"), &set, &bindings).is_some());
}

#[test]
fn plan_grants_role_to_actor_and_responds() {
    let (set, bindings) = fixture();
    let plan = interpret(&click("verify_button"), &set, &bindings).unwrap();
    assert_eq!(
        plan.steps,
        vec![
            PlannedAction::GrantRole {
                role: PlannedRole::Resolved(RoleId(555)),
                target: UserId(42),
            },
            PlannedAction::RespondEphemeral {
                content: "welcome".to_string(),
            },
        ]
    );
}

#[test]
fn unmatched_button_click_is_none() {
    let (set, bindings) = fixture();
    assert!(interpret(&click("other_button"), &set, &bindings).is_none());
}
