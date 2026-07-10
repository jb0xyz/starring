use std::collections::BTreeSet;

use automation_state::{ActionSpec, InteractionRuleSet, TriggerSpec};
use desired_state::ResourceKey;
use resource_resolution::ResourceBindingMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    DuplicatePanelKey(String),
    DuplicateButtonKey(String),
    DuplicateRuleKey(String),
    UnknownButtonRef { rule: String, component: String },
    UnknownRoleRef { rule: String, role: ResourceKey },
    ConflictingTrigger { component: String },
    EmptyResponseContent { rule: String },
}

pub fn validate(
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    let mut panel_keys: BTreeSet<&str> = BTreeSet::new();
    let mut button_keys: BTreeSet<String> = BTreeSet::new();
    for panel in &ruleset.panels {
        if !panel_keys.insert(panel.key.as_str()) {
            errors.push(ValidationError::DuplicatePanelKey(panel.key.clone()));
        }
        for button in &panel.buttons {
            if !button_keys.insert(button.key.clone()) {
                errors.push(ValidationError::DuplicateButtonKey(button.key.clone()));
            }
        }
    }

    let mut rule_keys: BTreeSet<&str> = BTreeSet::new();
    let mut trigger_components: BTreeSet<String> = BTreeSet::new();
    for rule in &ruleset.rules {
        if !rule_keys.insert(rule.key.as_str()) {
            errors.push(ValidationError::DuplicateRuleKey(rule.key.clone()));
        }
        match &rule.trigger {
            TriggerSpec::ButtonClick { component } => {
                if !button_keys.contains(component) {
                    errors.push(ValidationError::UnknownButtonRef {
                        rule: rule.key.clone(),
                        component: component.clone(),
                    });
                }
                if !trigger_components.insert(component.clone()) {
                    errors.push(ValidationError::ConflictingTrigger {
                        component: component.clone(),
                    });
                }
            }
        }
        for action in &rule.actions {
            match action {
                ActionSpec::GrantRole { role, .. } => {
                    if !bindings.role_bindings.contains_key(role) {
                        errors.push(ValidationError::UnknownRoleRef {
                            rule: rule.key.clone(),
                            role: role.clone(),
                        });
                    }
                }
                ActionSpec::RespondEphemeral { content } => {
                    if content.trim().is_empty() {
                        errors.push(ValidationError::EmptyResponseContent {
                            rule: rule.key.clone(),
                        });
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
