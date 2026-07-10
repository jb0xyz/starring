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
    DuplicateModalKey(String),
    DuplicateModalFieldKey { modal: String, field: String },
    UnknownModalRef { rule: String, modal: String },
    ConflictingModalTrigger { modal: String },
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

    let mut modal_keys: BTreeSet<String> = BTreeSet::new();
    for modal in &ruleset.modals {
        if !modal_keys.insert(modal.key.clone()) {
            errors.push(ValidationError::DuplicateModalKey(modal.key.clone()));
        }
        let mut field_keys: BTreeSet<&str> = BTreeSet::new();
        for field in &modal.fields {
            if !field_keys.insert(field.key.as_str()) {
                errors.push(ValidationError::DuplicateModalFieldKey {
                    modal: modal.key.clone(),
                    field: field.key.clone(),
                });
            }
        }
    }

    let mut rule_keys: BTreeSet<&str> = BTreeSet::new();
    let mut trigger_components: BTreeSet<String> = BTreeSet::new();
    let mut modal_triggers: BTreeSet<String> = BTreeSet::new();
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
            TriggerSpec::ModalSubmit { modal } => {
                if !modal_keys.contains(modal) {
                    errors.push(ValidationError::UnknownModalRef {
                        rule: rule.key.clone(),
                        modal: modal.clone(),
                    });
                }
                if !modal_triggers.insert(modal.clone()) {
                    errors.push(ValidationError::ConflictingModalTrigger {
                        modal: modal.clone(),
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
                ActionSpec::OpenModal { modal } => {
                    if !modal_keys.contains(modal) {
                        errors.push(ValidationError::UnknownModalRef {
                            rule: rule.key.clone(),
                            modal: modal.clone(),
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
