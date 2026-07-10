use std::collections::BTreeMap;

use automation_core::policy::analyze;
use automation_state::{
    ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::Permissions;

fn rule(key: &str, role: &str) -> InteractionRule {
    InteractionRule {
        key: key.to_string(),
        trigger: TriggerSpec::ButtonClick {
            component: "b".to_string(),
        },
        actions: vec![ActionSpec::GrantRole {
            role: ResourceKey(role.to_string()),
            target: ActionTarget::Actor,
        }],
    }
}

fn roles() -> BTreeMap<ResourceKey, Permissions> {
    let mut roles = BTreeMap::new();
    roles.insert(ResourceKey("admin".to_string()), Permissions::ADMINISTRATOR);
    roles.insert(
        ResourceKey("verified".to_string()),
        Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
    );
    roles
}

#[test]
fn granting_privileged_role_is_flagged() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![rule("r1", "admin")],
    };
    let findings = analyze(&set, &roles());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, "r1");
    assert_eq!(findings[0].role, ResourceKey("admin".to_string()));
}

#[test]
fn granting_ordinary_role_is_allowed() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![rule("r1", "verified")],
    };
    assert!(analyze(&set, &roles()).is_empty());
}
