use std::collections::{BTreeMap, BTreeSet};

use automation_state::{
    ActionSpec, ButtonRoute, ChannelRef, InstanceRef, InstanceResourceRefs, InteractionRule,
    InteractionRuleSet, OverwriteTargetSpec, RoleRef, TriggerSpec,
};
use desired_state::ResourceKey;
use resource_resolution::ResourceBindingMap;

use crate::template::TemplateString;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    DuplicatePanelKey(String),
    DuplicateButtonKey(String),
    DuplicateRuleKey(String),
    UnknownButtonRef {
        rule: String,
        component: String,
    },
    UnknownRoleRef {
        rule: String,
        role: ResourceKey,
    },
    ConflictingTrigger {
        component: String,
    },
    EmptyResponseContent {
        rule: String,
    },
    DuplicateModalKey(String),
    DuplicateModalFieldKey {
        modal: String,
        field: String,
    },
    UnknownModalRef {
        rule: String,
        modal: String,
    },
    ConflictingModalTrigger {
        modal: String,
    },
    BadTemplate {
        rule: String,
    },
    InputTemplateInButtonRule {
        rule: String,
        input: String,
    },
    UnknownTemplateInput {
        rule: String,
        modal: String,
        input: String,
    },
    DuplicateActionKey {
        rule: String,
        key: String,
    },
    UnknownCreatedRoleRef {
        rule: String,
        key: String,
    },
    CreatedRoleRefTypeMismatch {
        rule: String,
        key: String,
    },
    UnknownChannelRef {
        rule: String,
        channel: ResourceKey,
    },
    UnknownCreatedChannelRef {
        rule: String,
        key: String,
    },
    CreatedChannelRefTypeMismatch {
        rule: String,
        key: String,
    },
    UnknownCreatedMessageRef {
        rule: String,
        key: String,
    },
    CreatedMessageRefTypeMismatch {
        rule: String,
        key: String,
    },
    EmptyInstanceResources {
        rule: String,
    },
    InvalidResourceAlias {
        rule: String,
        alias: String,
    },
    OverlappingOverwrite {
        rule: String,
    },
    EmptyOverwrite {
        rule: String,
    },
    EmptyButtonLabel {
        rule: String,
        button: String,
    },
    TooManyPanelButtons {
        rule: String,
        count: usize,
    },
    DeferNotFirst {
        rule: String,
    },
    ConflictingInitialResponse {
        rule: String,
    },
    EditResponseWithoutDefer {
        rule: String,
    },
    DeferredMissingEditResponse {
        rule: String,
    },
    MultipleEditResponse {
        rule: String,
    },
    EditResponseNotLast {
        rule: String,
    },
    InstanceRoleOutsideInstanceRule {
        rule: String,
    },
    InstanceRoleMustUseEvent {
        rule: String,
    },
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
            if let ButtonRoute::Static { key } = &button.route {
                if !button_keys.insert(key.clone()) {
                    errors.push(ValidationError::DuplicateButtonKey(key.clone()));
                }
            }
        }
    }

    for rule in &ruleset.rules {
        for action in &rule.actions {
            if let ActionSpec::PostPanel { buttons, .. } = action {
                if buttons.len() > MAX_PANEL_BUTTONS {
                    errors.push(ValidationError::TooManyPanelButtons {
                        rule: rule.key.clone(),
                        count: buttons.len(),
                    });
                }
                for button in buttons {
                    let button_name = match &button.route {
                        ButtonRoute::Static { key } => key,
                        ButtonRoute::InstanceAction { action, .. } => action,
                    };
                    if button.label.trim().is_empty() {
                        errors.push(ValidationError::EmptyButtonLabel {
                            rule: rule.key.clone(),
                            button: button_name.clone(),
                        });
                    }
                    if let ButtonRoute::Static { key } = &button.route {
                        if !button_keys.insert(key.clone()) {
                            errors.push(ValidationError::DuplicateButtonKey(key.clone()));
                        }
                    }
                }
            }
        }
    }

    let mut modal_keys: BTreeSet<String> = BTreeSet::new();
    let mut modal_fields: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for modal in &ruleset.modals {
        if !modal_keys.insert(modal.key.clone()) {
            errors.push(ValidationError::DuplicateModalKey(modal.key.clone()));
        }
        let mut field_keys: BTreeSet<String> = BTreeSet::new();
        for field in &modal.fields {
            if !field_keys.insert(field.key.clone()) {
                errors.push(ValidationError::DuplicateModalFieldKey {
                    modal: modal.key.clone(),
                    field: field.key.clone(),
                });
            }
        }
        modal_fields.insert(modal.key.clone(), field_keys);
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
            TriggerSpec::InstanceAction { .. } => {}
        }
        let mut created: BTreeMap<String, CreatedKind> = BTreeMap::new();
        for action in &rule.actions {
            match action {
                ActionSpec::GrantRole { role, .. } => {
                    check_role_ref(&mut errors, rule, bindings, &created, role);
                }
                ActionSpec::RespondEphemeral { content } => {
                    if content.trim().is_empty() {
                        errors.push(ValidationError::EmptyResponseContent {
                            rule: rule.key.clone(),
                        });
                    }
                    check_template(&mut errors, rule, &modal_fields, content);
                }
                ActionSpec::OpenModal { modal } => {
                    if !modal_keys.contains(modal) {
                        errors.push(ValidationError::UnknownModalRef {
                            rule: rule.key.clone(),
                            modal: modal.clone(),
                        });
                    }
                }
                ActionSpec::CreateChannel { key, name } => {
                    if created.insert(key.clone(), CreatedKind::Channel).is_some() {
                        errors.push(ValidationError::DuplicateActionKey {
                            rule: rule.key.clone(),
                            key: key.clone(),
                        });
                    }
                    check_template(&mut errors, rule, &modal_fields, name);
                }
                ActionSpec::CreateRole { key, name } => {
                    if created.insert(key.clone(), CreatedKind::Role).is_some() {
                        errors.push(ValidationError::DuplicateActionKey {
                            rule: rule.key.clone(),
                            key: key.clone(),
                        });
                    }
                    check_template(&mut errors, rule, &modal_fields, name);
                }
                ActionSpec::UpsertOverwrite {
                    channel,
                    target,
                    allow,
                    deny,
                } => {
                    check_channel_ref(&mut errors, rule, bindings, &created, channel);
                    if let OverwriteTargetSpec::Role(role) = target {
                        check_role_ref(&mut errors, rule, bindings, &created, role);
                    }
                    if allow.intersects(*deny) {
                        errors.push(ValidationError::OverlappingOverwrite {
                            rule: rule.key.clone(),
                        });
                    }
                    if allow.is_empty() && deny.is_empty() {
                        errors.push(ValidationError::EmptyOverwrite {
                            rule: rule.key.clone(),
                        });
                    }
                }
                ActionSpec::PostPanel {
                    key,
                    channel,
                    content,
                    ..
                } => {
                    if created.insert(key.clone(), CreatedKind::Message).is_some() {
                        errors.push(ValidationError::DuplicateActionKey {
                            rule: rule.key.clone(),
                            key: key.clone(),
                        });
                    }
                    check_channel_ref(&mut errors, rule, bindings, &created, channel);
                    check_template(&mut errors, rule, &modal_fields, content);
                }
                ActionSpec::DeferEphemeral => {}
                ActionSpec::EditResponse { content } => {
                    check_template(&mut errors, rule, &modal_fields, content);
                }
                ActionSpec::RegisterInstance {
                    key,
                    kind: _,
                    resources,
                } => {
                    if created.insert(key.clone(), CreatedKind::Instance).is_some() {
                        errors.push(ValidationError::DuplicateActionKey {
                            rule: rule.key.clone(),
                            key: key.clone(),
                        });
                    }
                    check_manifest(&mut errors, rule, &created, resources);
                }
            }
        }
        let defer_positions: Vec<usize> = rule
            .actions
            .iter()
            .enumerate()
            .filter(|(_, action)| matches!(action, ActionSpec::DeferEphemeral))
            .map(|(index, _)| index)
            .collect();
        let edit_positions: Vec<usize> = rule
            .actions
            .iter()
            .enumerate()
            .filter(|(_, action)| matches!(action, ActionSpec::EditResponse { .. }))
            .map(|(index, _)| index)
            .collect();
        let has_other_initial = rule.actions.iter().any(|action| {
            matches!(
                action,
                ActionSpec::RespondEphemeral { .. } | ActionSpec::OpenModal { .. }
            )
        });
        let last_index = rule.actions.len().saturating_sub(1);
        if defer_positions.iter().any(|&index| index != 0) {
            errors.push(ValidationError::DeferNotFirst {
                rule: rule.key.clone(),
            });
        }
        if !defer_positions.is_empty() {
            if has_other_initial {
                errors.push(ValidationError::ConflictingInitialResponse {
                    rule: rule.key.clone(),
                });
            }
            if edit_positions.is_empty() {
                errors.push(ValidationError::DeferredMissingEditResponse {
                    rule: rule.key.clone(),
                });
            }
        }
        if !edit_positions.is_empty() && defer_positions.is_empty() {
            errors.push(ValidationError::EditResponseWithoutDefer {
                rule: rule.key.clone(),
            });
        }
        if edit_positions.len() > 1 {
            errors.push(ValidationError::MultipleEditResponse {
                rule: rule.key.clone(),
            });
        }
        if edit_positions.iter().any(|&index| index != last_index) {
            errors.push(ValidationError::EditResponseNotLast {
                rule: rule.key.clone(),
            });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

const MAX_PANEL_BUTTONS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreatedKind {
    Role,
    Channel,
    Message,
    Instance,
}

fn check_template(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    modal_fields: &BTreeMap<String, BTreeSet<String>>,
    content: &str,
) {
    let template = match TemplateString::parse(content) {
        Ok(template) => template,
        Err(_) => {
            errors.push(ValidationError::BadTemplate {
                rule: rule.key.clone(),
            });
            return;
        }
    };
    for key in template.input_keys() {
        match &rule.trigger {
            TriggerSpec::ButtonClick { .. } | TriggerSpec::InstanceAction { .. } => {
                errors.push(ValidationError::InputTemplateInButtonRule {
                    rule: rule.key.clone(),
                    input: key.to_string(),
                });
            }
            TriggerSpec::ModalSubmit { modal } => {
                if !modal_fields
                    .get(modal)
                    .is_some_and(|fields| fields.contains(key))
                {
                    errors.push(ValidationError::UnknownTemplateInput {
                        rule: rule.key.clone(),
                        modal: modal.clone(),
                        input: key.to_string(),
                    });
                }
            }
        }
    }
}

fn check_role_ref(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    bindings: &ResourceBindingMap,
    created: &BTreeMap<String, CreatedKind>,
    role: &RoleRef,
) {
    match role {
        RoleRef::Existing(key) => {
            if !bindings.role_bindings.contains_key(key) {
                errors.push(ValidationError::UnknownRoleRef {
                    rule: rule.key.clone(),
                    role: key.clone(),
                });
            }
        }
        RoleRef::Created(inner) => match created.get(&inner.created) {
            None => errors.push(ValidationError::UnknownCreatedRoleRef {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
            Some(CreatedKind::Role) => {}
            Some(_) => errors.push(ValidationError::CreatedRoleRefTypeMismatch {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
        },
        RoleRef::Instance { instance, alias } => {
            if !matches!(rule.trigger, TriggerSpec::InstanceAction { .. }) {
                errors.push(ValidationError::InstanceRoleOutsideInstanceRule {
                    rule: rule.key.clone(),
                });
            }
            if !matches!(instance, InstanceRef::Event) {
                errors.push(ValidationError::InstanceRoleMustUseEvent {
                    rule: rule.key.clone(),
                });
            }
            check_resource_alias(errors, rule, alias);
        }
    }
}

fn check_channel_ref(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    bindings: &ResourceBindingMap,
    created: &BTreeMap<String, CreatedKind>,
    channel: &ChannelRef,
) {
    match channel {
        ChannelRef::Existing(key) => {
            if !bindings.channel_bindings.contains_key(key) {
                errors.push(ValidationError::UnknownChannelRef {
                    rule: rule.key.clone(),
                    channel: key.clone(),
                });
            }
        }
        ChannelRef::Created(inner) => match created.get(&inner.created) {
            None => errors.push(ValidationError::UnknownCreatedChannelRef {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
            Some(CreatedKind::Channel) => {}
            Some(_) => errors.push(ValidationError::CreatedChannelRefTypeMismatch {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
        },
    }
}

fn check_manifest(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    created: &BTreeMap<String, CreatedKind>,
    resources: &InstanceResourceRefs,
) {
    if resources.roles.is_empty() && resources.channels.is_empty() && resources.messages.is_empty()
    {
        errors.push(ValidationError::EmptyInstanceResources {
            rule: rule.key.clone(),
        });
    }

    for (alias, reference) in &resources.roles {
        check_resource_alias(errors, rule, alias);
        match created.get(&reference.created) {
            None => errors.push(ValidationError::UnknownCreatedRoleRef {
                rule: rule.key.clone(),
                key: reference.created.clone(),
            }),
            Some(CreatedKind::Role) => {}
            Some(_) => {
                errors.push(ValidationError::CreatedRoleRefTypeMismatch {
                    rule: rule.key.clone(),
                    key: reference.created.clone(),
                });
            }
        }
    }

    for (alias, reference) in &resources.channels {
        check_resource_alias(errors, rule, alias);
        match created.get(&reference.created) {
            None => errors.push(ValidationError::UnknownCreatedChannelRef {
                rule: rule.key.clone(),
                key: reference.created.clone(),
            }),
            Some(CreatedKind::Channel) => {}
            Some(_) => {
                errors.push(ValidationError::CreatedChannelRefTypeMismatch {
                    rule: rule.key.clone(),
                    key: reference.created.clone(),
                });
            }
        }
    }

    for (alias, reference) in &resources.messages {
        check_resource_alias(errors, rule, alias);
        match created.get(&reference.created) {
            None => errors.push(ValidationError::UnknownCreatedMessageRef {
                rule: rule.key.clone(),
                key: reference.created.clone(),
            }),
            Some(CreatedKind::Message) => {}
            Some(_) => {
                errors.push(ValidationError::CreatedMessageRefTypeMismatch {
                    rule: rule.key.clone(),
                    key: reference.created.clone(),
                });
            }
        }
    }
}

fn check_resource_alias(errors: &mut Vec<ValidationError>, rule: &InteractionRule, alias: &str) {
    if alias.is_empty()
        || alias.len() > 32
        || !alias.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        errors.push(ValidationError::InvalidResourceAlias {
            rule: rule.key.clone(),
            alias: alias.to_string(),
        });
    }
}
