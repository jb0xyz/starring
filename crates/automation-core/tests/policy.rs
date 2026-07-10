use std::collections::BTreeMap;

use automation_core::policy::{analyze, DynamicAction, PolicyFinding};
use automation_state::{
    ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::Permissions;

fn grant_rule(key: &str, role: &str) -> InteractionRule {
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

fn set(rule: InteractionRule) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![rule],
    }
}

#[test]
fn granting_privileged_role_is_flagged() {
    let findings = analyze(&set(grant_rule("r1", "admin")), &roles());
    assert_eq!(
        findings,
        vec![PolicyFinding::PrivilegedRoleGrant {
            rule: "r1".to_string(),
            role: ResourceKey("admin".to_string()),
        }]
    );
}

#[test]
fn granting_ordinary_role_is_allowed() {
    assert!(analyze(&set(grant_rule("r1", "verified")), &roles()).is_empty());
}

#[test]
fn create_channel_is_flagged() {
    let rule = InteractionRule {
        key: "r1".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "m".to_string(),
        },
        actions: vec![ActionSpec::CreateChannel {
            name: "study-x".to_string(),
        }],
    };
    assert_eq!(
        analyze(&set(rule), &roles()),
        vec![PolicyFinding::DynamicResourceCreation {
            rule: "r1".to_string(),
            action: DynamicAction::CreateChannel,
        }]
    );
}

#[test]
fn create_role_is_flagged() {
    let rule = InteractionRule {
        key: "r1".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "m".to_string(),
        },
        actions: vec![ActionSpec::CreateRole {
            name: "member".to_string(),
        }],
    };
    assert_eq!(
        analyze(&set(rule), &roles()),
        vec![PolicyFinding::DynamicResourceCreation {
            rule: "r1".to_string(),
            action: DynamicAction::CreateRole,
        }]
    );
}
