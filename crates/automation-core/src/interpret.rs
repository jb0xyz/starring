use automation_state::{
    ActionSpec, ActionTarget, ChannelRef, InstanceRef, InteractionRuleSet, OverwriteTargetSpec,
    RoleRef, TriggerSpec,
};
use resource_resolution::ResourceBindingMap;

use crate::event::{EventKind, RuntimeEvent};
use crate::plan::{
    ActionPlan, ModalPresentation, PlannedAction, PlannedChannel, PlannedOverwriteTarget,
    PlannedRole,
};

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
                let planned_role = resolve_role(role, bindings)?;
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
            ActionSpec::UpsertOverwrite {
                channel,
                target,
                allow,
                deny,
            } => {
                let planned_channel = resolve_channel(channel, bindings)?;
                let planned_target = match target {
                    OverwriteTargetSpec::Everyone => PlannedOverwriteTarget::Everyone,
                    OverwriteTargetSpec::Role(role) => {
                        PlannedOverwriteTarget::Role(resolve_role(role, bindings)?)
                    }
                };
                steps.push(PlannedAction::UpsertOverwrite {
                    channel: planned_channel,
                    target: planned_target,
                    allow: *allow,
                    deny: *deny,
                });
            }
            ActionSpec::PostPanel {
                key,
                channel,
                content,
                buttons,
            } => {
                let planned_channel = resolve_channel(channel, bindings)?;
                steps.push(PlannedAction::PostPanel {
                    key: key.clone(),
                    channel: planned_channel,
                    content: content.clone(),
                    buttons: buttons.clone(),
                });
            }
            ActionSpec::DeferEphemeral => {
                steps.push(PlannedAction::DeferEphemeral);
            }
            ActionSpec::EditResponse { content } => {
                steps.push(PlannedAction::EditResponse {
                    content: content.clone(),
                });
            }
            ActionSpec::RegisterInstance {
                key,
                kind,
                resources,
            } => {
                steps.push(PlannedAction::RegisterInstance {
                    key: key.clone(),
                    kind: kind.clone(),
                    resources: resources.clone(),
                });
            }
        }
    }

    Some(ActionPlan { steps })
}

fn resolve_role(role: &RoleRef, bindings: &ResourceBindingMap) -> Option<PlannedRole> {
    match role {
        RoleRef::Existing(key) => Some(PlannedRole::Resolved(*bindings.role_bindings.get(key)?)),
        RoleRef::Created(inner) => Some(PlannedRole::Created(inner.created.clone())),
        RoleRef::Instance {
            instance: InstanceRef::Event,
            alias,
        } => Some(PlannedRole::Instance {
            alias: alias.clone(),
        }),
        RoleRef::Instance {
            instance: InstanceRef::Created(_),
            ..
        } => None,
    }
}

fn resolve_channel(channel: &ChannelRef, bindings: &ResourceBindingMap) -> Option<PlannedChannel> {
    match channel {
        ChannelRef::Existing(key) => Some(PlannedChannel::Resolved(
            *bindings.channel_bindings.get(key)?,
        )),
        ChannelRef::Created(inner) => Some(PlannedChannel::Created(inner.created.clone())),
    }
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
        (
            TriggerSpec::InstanceAction { action },
            EventKind::InstanceAction {
                action: received, ..
            },
        ) => action == received,
        _ => false,
    }
}
