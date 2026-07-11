use automation_core::{validate, validate_bindings, validate_structural, ValidationError};
use automation_state::{
    ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, PanelSpec, RoleRef, TriggerSpec,
};
use automation_state::{ButtonRoute, ButtonSpec};
use desired_state::ResourceKey;
use resource_resolution::ResourceBindingMap;

fn verify_rule(role_key: &str) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "p".to_string(),
            channel: ResourceKey("c".to_string()),
            content: "x".to_string(),
            buttons: vec![ButtonSpec {
                label: "V".to_string(),
                route: ButtonRoute::Static {
                    key: "b".to_string(),
                },
            }],
        }],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "b".to_string(),
            },
            actions: vec![ActionSpec::GrantRole {
                role: RoleRef::Existing(ResourceKey(role_key.to_string())),
                target: ActionTarget::Actor,
            }],
        }],
    }
}

#[test]
fn structural_passes_without_bindings_binding_layer_flags_missing() {
    let ruleset = verify_rule("member");
    assert!(validate_structural(&ruleset).is_ok());
    let empty = ResourceBindingMap::default();
    let errors = validate_bindings(&ruleset, &empty).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownRoleRef {
        rule: "r".to_string(),
        role: ResourceKey("member".to_string()),
    }));
    assert!(validate(&ruleset, &empty).is_err());
    let mut bound = ResourceBindingMap::default();
    bound
        .role_bindings
        .insert(ResourceKey("member".to_string()), discord_model::RoleId(9));
    assert!(validate(&ruleset, &bound).is_ok());
}
