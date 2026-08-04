use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;

use crate::adapter::{AdapterError, AdapterErrorKind};
use crate::event::{EventKind, RunningRuleSetIdentity, RuntimeContext, RuntimeEvent};
use crate::interpret::interpret;
use crate::modal_input::normalize_modal_submit_inputs;
use crate::plan::{ActionPlan, PlannedAction};

pub struct PreparedEventExecution {
    context: RuntimeContext,
    plan: ActionPlan,
    leading_defer_ephemeral: bool,
}

impl PreparedEventExecution {
    pub fn context(&self) -> &RuntimeContext {
        &self.context
    }

    pub fn plan(&self) -> &ActionPlan {
        &self.plan
    }

    pub fn leading_defer_ephemeral(&self) -> bool {
        self.leading_defer_ephemeral
    }

    pub fn into_parts(self) -> (RuntimeContext, ActionPlan, bool) {
        (self.context, self.plan, self.leading_defer_ephemeral)
    }
}

pub fn prepare_event_execution(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    identity: &RunningRuleSetIdentity,
) -> Result<Option<PreparedEventExecution>, AdapterError> {
    if let EventKind::InstanceAction { .. } = &event.kind {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidEventRoute,
            "InstanceAction must be dispatched via automation-ruleset-dispatch",
        ));
    }
    let normalized_inputs = normalize_modal_submit_inputs(event, ruleset)
        .map_err(|error| AdapterError::new(AdapterErrorKind::BadRequest, error.to_string()))?;
    let mut context = RuntimeContext::from_event(event, identity);
    if let Some(inputs) = normalized_inputs {
        context.inputs = inputs;
    }
    let Some(plan) = interpret(event, ruleset, bindings) else {
        return Ok(None);
    };
    let mut steps = plan.steps;
    let defer_ephemeral = if matches!(steps.first(), Some(PlannedAction::DeferEphemeral)) {
        steps.remove(0);
        true
    } else {
        false
    };
    Ok(Some(PreparedEventExecution {
        context,
        plan: ActionPlan { steps },
        leading_defer_ephemeral: defer_ephemeral,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use automation_instance::InstanceRuleSetVersion;
    use automation_state::{
        ActionSpec, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle,
        ModalInputPolicy, ModalSpec, TriggerSpec,
    };
    use discord_model::{GuildId, UserId};

    use super::*;

    fn identity() -> RunningRuleSetIdentity {
        RunningRuleSetIdentity {
            key: "study".to_string(),
            version: InstanceRuleSetVersion::new(3).unwrap(),
        }
    }

    fn submit() -> RuntimeEvent {
        RuntimeEvent {
            guild_id: GuildId(7),
            actor: UserId(9),
            kind: EventKind::ModalSubmit {
                modal: "room".to_string(),
                inputs: BTreeMap::from([("room_name".to_string(), "cozy".to_string())]),
            },
        }
    }

    fn ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![ModalSpec {
                key: "room".to_string(),
                title: "Room".to_string(),
                fields: vec![ModalFieldSpec {
                    key: "room_name".to_string(),
                    label: "Room name".to_string(),
                    style: ModalFieldStyle::Short,
                    required: true,
                    min_length: None,
                    max_length: None,
                    input_policy: ModalInputPolicy::Preserve,
                }],
            }],
            rules: vec![InteractionRule {
                key: "submit".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: "room".to_string(),
                },
                actions,
            }],
        }
    }

    #[test]
    fn preparation_extracts_leading_defer_and_preserves_remaining_order() {
        let prepared = prepare_event_execution(
            &submit(),
            &ruleset(vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::CreateRole {
                    key: "member".to_string(),
                    name: "${input.room_name} member".to_string(),
                },
                ActionSpec::EditResponse {
                    content: "ready".to_string(),
                },
            ]),
            &ResourceBindingMap::default(),
            &identity(),
        )
        .unwrap()
        .unwrap();

        assert!(prepared.leading_defer_ephemeral());
        assert_eq!(prepared.context().guild_id, GuildId(7));
        assert_eq!(prepared.context().actor, UserId(9));
        assert_eq!(prepared.context().ruleset_key, "study");
        assert_eq!(prepared.context().ruleset_version.get(), 3);
        assert_eq!(
            prepared.plan().steps,
            vec![
                PlannedAction::CreateRole {
                    key: "member".to_string(),
                    name: "${input.room_name} member".to_string(),
                },
                PlannedAction::EditResponse {
                    content: "ready".to_string(),
                },
            ]
        );
    }

    #[test]
    fn preparation_keeps_non_deferred_plan_unchanged() {
        let prepared = prepare_event_execution(
            &submit(),
            &ruleset(vec![ActionSpec::RespondEphemeral {
                content: "ready".to_string(),
            }]),
            &ResourceBindingMap::default(),
            &identity(),
        )
        .unwrap()
        .unwrap();

        assert!(!prepared.leading_defer_ephemeral());
        assert_eq!(
            prepared.plan().steps,
            vec![PlannedAction::RespondEphemeral {
                content: "ready".to_string(),
            }]
        );
    }

    #[test]
    fn preparation_returns_none_for_unmatched_event() {
        let mut event = submit();
        event.kind = EventKind::ButtonClick {
            component: "other".to_string(),
        };
        let prepared = prepare_event_execution(
            &event,
            &ruleset(vec![ActionSpec::RespondEphemeral {
                content: "ready".to_string(),
            }]),
            &ResourceBindingMap::default(),
            &identity(),
        )
        .unwrap();

        assert!(prepared.is_none());
    }

    #[test]
    fn prepared_execution_can_be_inspected_then_consumed_without_reinterpretation() {
        let prepared = prepare_event_execution(
            &submit(),
            &ruleset(vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::RespondEphemeral {
                    content: "ready".to_string(),
                },
            ]),
            &ResourceBindingMap::default(),
            &identity(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(prepared.context().ruleset_key, "study");
        assert_eq!(prepared.plan().steps.len(), 1);
        assert!(prepared.leading_defer_ephemeral());

        let (context, plan, leading_defer_ephemeral) = prepared.into_parts();
        assert_eq!(context.ruleset_key, "study");
        assert_eq!(plan.steps.len(), 1);
        assert!(leading_defer_ephemeral);
    }
}
