use automation_state::{ActionSpec, ActionTarget, InteractionRuleSet, RoleRef, TriggerSpec};
use resource_resolution::ResourceBindingMap;

use crate::event::{EventKind, RuntimeEvent};
use crate::plan::{ActionPlan, ModalPresentation, PlannedAction, PlannedRole};

pub fn interpret(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
) -> Option<ActionPlan> {
    let rule = ruleset
        .rules
        .iter()
        .find(|rule| trigger_matches(&rule.trigger, &event.kind))?;

    let mut steps = Vec::new();
    for action in &rule.actions {
        match action {
            ActionSpec::GrantRole { role, target } => {
                let planned_role = match role {
                    RoleRef::Existing(key) => {
                        PlannedRole::Resolved(*bindings.role_bindings.get(key)?)
                    }
                    RoleRef::Created { created } => PlannedRole::Created(created.clone()),
                };
                let target_id = match target {
                    ActionTarget::Actor => event.actor,
                };
                steps.push(PlannedAction::GrantRole {
                    role: planned_role,
                    target: target_id,
                });
            }
            ActionSpec::RespondEphemeral { content } => {
                steps.push(PlannedAction::RespondEphemeral {
                    content: content.clone(),
                });
            }
            ActionSpec::OpenModal { modal } => {
                let spec = ruleset
                    .modals
                    .iter()
                    .find(|candidate| candidate.key == *modal)?;
                steps.push(PlannedAction::OpenModal(ModalPresentation {
                    key: spec.key.clone(),
                    title: spec.title.clone(),
                    fields: spec.fields.clone(),
                }));
            }
            ActionSpec::CreateChannel { key, name } => {
                steps.push(PlannedAction::CreateChannel {
                    key: key.clone(),
                    name: name.clone(),
                });
            }
            ActionSpec::CreateRole { key, name } => {
                steps.push(PlannedAction::CreateRole {
                    key: key.clone(),
                    name: name.clone(),
                });
            }
        }
    }

    Some(ActionPlan { steps })
}

fn trigger_matches(trigger: &TriggerSpec, kind: &EventKind) -> bool {
    match (trigger, kind) {
        (TriggerSpec::ButtonClick { component }, EventKind::ButtonClick { component: clicked }) => {
            component == clicked
        }
        (
            TriggerSpec::ModalSubmit { modal },
            EventKind::ModalSubmit {
                modal: submitted, ..
            },
        ) => modal == submitted,
        _ => false,
    }
}
