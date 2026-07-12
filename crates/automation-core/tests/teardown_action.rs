use std::collections::BTreeMap;

use automation_core::{
    interpret, run, ActionPlan, AdapterError, AdapterErrorKind, AutomationServices, EventKind,
    InteractionResponder, MockInstanceTeardownService, MockMutationAdapter, PlannedAction,
    ResolvedInstanceContext, ResponseDeliveryOutcome, RuntimeContext, RuntimeEvent,
    TeardownActionResult, ValidationError,
};
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceResources,
    InstanceRuleSetVersion, InstanceStatus, SequenceInstanceIdGenerator,
};
use automation_instance_teardown::TeardownOutcome;
use automation_state::{
    ActionSpec, ButtonRoute, ButtonSpec, CreatedRef, InstanceRef, InstanceResourceRefs,
    InteractionRule, InteractionRuleSet, PanelSpec, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn instance_id() -> InstanceId {
    InstanceId::parse("room_001").unwrap()
}

fn event() -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GuildId(7),
        actor: UserId(42),
        kind: EventKind::InstanceAction {
            instance_id: instance_id(),
            action: "close".to_string(),
        },
    }
}

fn instance() -> AutomationInstance {
    AutomationInstance {
        id: instance_id(),
        guild_id: GuildId(7),
        ruleset_key: "studyroom_demo".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        kind: InstanceKind("study_room".to_string()),
        created_by: UserId(42),
        resources: InstanceResources::default(),
        status: InstanceStatus::Active,
    }
}

fn context() -> RuntimeContext {
    RuntimeContext {
        guild_id: GuildId(7),
        actor: UserId(42),
        ruleset_key: "studyroom_demo".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        inputs: BTreeMap::new(),
        instance: Some(ResolvedInstanceContext {
            instance: instance(),
            action: "close".to_string(),
        }),
    }
}

fn close_rule(instance: InstanceRef) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "close_rule".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "close".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::TeardownInstance { instance },
                ActionSpec::EditResponse {
                    content: "closed".to_string(),
                },
            ],
        }],
    }
}

#[test]
fn interpret_plans_teardown_instance() {
    let plan = interpret(
        &event(),
        &close_rule(InstanceRef::Event),
        &ResourceBindingMap::default(),
    )
    .unwrap();

    assert!(matches!(
        plan.steps.get(1),
        Some(PlannedAction::TeardownInstance {
            instance: InstanceRef::Event,
        })
    ));
}

#[test]
fn run_resolves_event_and_records_teardown_outcome() {
    let teardown = MockInstanceTeardownService::with_outcome(TeardownOutcome::Completed);
    let result = block_on(run(
        &context(),
        &ActionPlan {
            steps: vec![
                PlannedAction::TeardownInstance {
                    instance: InstanceRef::Event,
                },
                PlannedAction::EditResponse {
                    content: "closed".to_string(),
                },
            ],
        },
        &AutomationServices {
            mutation: &MockMutationAdapter::new(),
            responder: &automation_core::MockInteractionResponder::new(),
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("room", 1),
            teardown: &teardown,
        },
    ))
    .unwrap();

    assert_eq!(teardown.calls(), vec![(GuildId(7), instance_id())]);
    assert_eq!(
        result.teardowns,
        vec![TeardownActionResult {
            action_index: 0,
            instance_id: instance_id(),
            teardown: TeardownOutcome::Completed,
            response: ResponseDeliveryOutcome::Sent,
        }]
    );
}

struct FailingEditResponder;

impl InteractionResponder for FailingEditResponder {
    async fn respond_ephemeral(&self, _: String) -> Result<(), AdapterError> {
        Ok(())
    }

    async fn edit_response(&self, _: String) -> Result<(), AdapterError> {
        Err(AdapterError::new(AdapterErrorKind::Network, "edit failed"))
    }
}

#[test]
fn edit_failure_after_teardown_is_recorded_without_retry() {
    let teardown = MockInstanceTeardownService::with_outcome(TeardownOutcome::Completed);
    let result = block_on(run(
        &context(),
        &ActionPlan {
            steps: vec![
                PlannedAction::TeardownInstance {
                    instance: InstanceRef::Event,
                },
                PlannedAction::EditResponse {
                    content: "closed".to_string(),
                },
            ],
        },
        &AutomationServices {
            mutation: &MockMutationAdapter::new(),
            responder: &FailingEditResponder,
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("room", 1),
            teardown: &teardown,
        },
    ))
    .unwrap();

    assert_eq!(teardown.calls().len(), 1);
    assert_eq!(
        result.teardowns[0].response,
        ResponseDeliveryOutcome::Failed
    );
}

#[test]
fn in_progress_is_a_successful_teardown_outcome() {
    let teardown = MockInstanceTeardownService::with_outcome(TeardownOutcome::InProgress);
    let result = block_on(run(
        &context(),
        &ActionPlan {
            steps: vec![PlannedAction::TeardownInstance {
                instance: InstanceRef::Event,
            }],
        },
        &AutomationServices {
            mutation: &MockMutationAdapter::new(),
            responder: &automation_core::MockInteractionResponder::new(),
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("room", 1),
            teardown: &teardown,
        },
    ))
    .unwrap();

    assert_eq!(result.teardowns[0].teardown, TeardownOutcome::InProgress);
}

#[test]
fn teardown_event_ref_requires_instance_action_rule() {
    let mut ruleset = close_rule(InstanceRef::Event);
    ruleset.panels.push(PanelSpec {
        key: "panel".to_string(),
        channel: ResourceKey("channel".to_string()),
        content: "close".to_string(),
        buttons: vec![ButtonSpec {
            label: "Close".to_string(),
            route: ButtonRoute::Static {
                key: "close".to_string(),
            },
        }],
    });
    ruleset.rules[0].trigger = TriggerSpec::ButtonClick {
        component: "close".to_string(),
    };
    let errors = automation_core::validate_structural(&ruleset).unwrap_err();

    assert!(
        errors.contains(&ValidationError::TeardownInstanceOutsideInstanceRule {
            rule: "close_rule".to_string(),
        })
    );
}

#[test]
fn unknown_created_instance_ref_fails_validation() {
    let ruleset = close_rule(InstanceRef::Created(CreatedRef {
        created: "missing".to_string(),
    }));
    let errors = automation_core::validate_structural(&ruleset).unwrap_err();

    assert!(
        errors.contains(&ValidationError::UnknownCreatedInstanceRef {
            rule: "close_rule".to_string(),
            key: "missing".to_string(),
        })
    );
}

#[test]
fn register_and_teardown_cannot_mix() {
    let mut ruleset = close_rule(InstanceRef::Created(CreatedRef {
        created: "instance".to_string(),
    }));
    ruleset.rules[0].actions.insert(
        1,
        ActionSpec::RegisterInstance {
            key: "instance".to_string(),
            kind: InstanceKind("study_room".to_string()),
            resources: InstanceResourceRefs::default(),
        },
    );
    let errors = automation_core::validate_structural(&ruleset).unwrap_err();

    assert!(errors.contains(&ValidationError::TeardownRegisterConflict {
        rule: "close_rule".to_string(),
    }));
}

#[test]
fn valid_close_rule_passes_validation() {
    assert!(automation_core::validate_structural(&close_rule(InstanceRef::Event)).is_ok());
}
