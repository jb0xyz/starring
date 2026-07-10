use automation_core::validate::{validate, ValidationError};
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, InteractionRule, InteractionRuleSet, PanelSpec, RoleRef,
    TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::RoleId;
use resource_resolution::ResourceBindingMap;

fn bindings_with(role: &str, id: u64) -> ResourceBindingMap {
    let mut map = ResourceBindingMap::default();
    map.role_bindings
        .insert(ResourceKey(role.to_string()), RoleId(id));
    map
}

fn rule(key: &str, component: &str, role: &str) -> InteractionRule {
    InteractionRule {
        key: key.to_string(),
        trigger: TriggerSpec::ButtonClick {
            component: component.to_string(),
        },
        actions: vec![ActionSpec::GrantRole {
            role: RoleRef::Existing(ResourceKey(role.to_string())),
            target: ActionTarget::Actor,
        }],
    }
}

fn panel(button: &str) -> PanelSpec {
    PanelSpec {
        key: "p".to_string(),
        channel: ResourceKey("c".to_string()),
        content: "x".to_string(),
        buttons: vec![ButtonSpec {
            key: button.to_string(),
            label: "b".to_string(),
        }],
    }
}

#[test]
fn valid_ruleset_passes() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        modals: vec![],
        rules: vec![rule("r1", "verify_button", "verified")],
    };
    assert!(validate(&set, &bindings_with("verified", 100)).is_ok());
}

#[test]
fn missing_role_ref_fails() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        modals: vec![],
        rules: vec![rule("r1", "verify_button", "ghost_role")],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownRoleRef {
        rule: "r1".to_string(),
        role: ResourceKey("ghost_role".to_string()),
    }));
}

#[test]
fn unknown_button_ref_fails() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        modals: vec![],
        rules: vec![rule("r1", "ghost_button", "verified")],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownButtonRef {
        rule: "r1".to_string(),
        component: "ghost_button".to_string(),
    }));
}

#[test]
fn duplicate_rule_and_button_keys_fail() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "p".to_string(),
            channel: ResourceKey("c".to_string()),
            content: "x".to_string(),
            buttons: vec![
                ButtonSpec {
                    key: "b".to_string(),
                    label: "one".to_string(),
                },
                ButtonSpec {
                    key: "b".to_string(),
                    label: "two".to_string(),
                },
            ],
        }],
        modals: vec![],
        rules: vec![rule("dup", "b", "verified"), rule("dup", "b", "verified")],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::DuplicateButtonKey("b".to_string())));
    assert!(errors.contains(&ValidationError::DuplicateRuleKey("dup".to_string())));
}

#[test]
fn duplicate_button_trigger_fails() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        modals: vec![],
        rules: vec![
            rule("r1", "verify_button", "verified"),
            rule("r2", "verify_button", "verified"),
        ],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::ConflictingTrigger {
        component: "verify_button".to_string(),
    }));
}

#[test]
fn empty_respond_content_fails() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "r1".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "verify_button".to_string(),
            },
            actions: vec![ActionSpec::RespondEphemeral {
                content: "   ".to_string(),
            }],
        }],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::EmptyResponseContent {
        rule: "r1".to_string(),
    }));
}
