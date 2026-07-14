use std::collections::BTreeMap;

use automation_core::adapter::AdapterErrorKind;
use automation_core::event::{EventKind, RuntimeContext, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, ResponderCall};
use automation_core::plan::{ActionPlan, PlannedAction};
use automation_core::run::{handle_event, run, HandleOutcome};
use automation_core::validate::{validate, ValidationError};
use automation_core::AutomationServices;
use automation_instance::{InMemoryInstanceStore, SequenceInstanceIdGenerator};
use automation_state::{
    ActionSpec, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle, ModalSpec,
    TriggerSpec,
};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn identity(key: &str) -> automation_core::RunningRuleSetIdentity {
    automation_core::RunningRuleSetIdentity {
        key: key.to_string(),
        version: automation_instance::InstanceRuleSetVersion::new(1).unwrap(),
    }
}

fn modal() -> ModalSpec {
    ModalSpec {
        key: "m".to_string(),
        title: "M".to_string(),
        fields: vec![ModalFieldSpec {
            key: "room_name".to_string(),
            label: "R".to_string(),
            style: ModalFieldStyle::Short,
            required: true,
            min_length: None,
            max_length: None,
            input_policy: automation_state::ModalInputPolicy::Preserve,
        }],
    }
}

fn rule(actions: Vec<ActionSpec>) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::ModalSubmit {
                modal: "m".to_string(),
            },
            actions,
        }],
    }
}

fn submit(room: &str) -> RuntimeEvent {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), room.to_string());
    RuntimeEvent {
        guild_id: GuildId(7),
        actor: UserId(3),
        kind: EventKind::ModalSubmit {
            modal: "m".to_string(),
            inputs,
        },
    }
}

fn defer_rule() -> InteractionRuleSet {
    rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        ActionSpec::EditResponse {
            content: "스터디룸 '${input.room_name}' 완료".to_string(),
        },
    ])
}

#[test]
fn run_executes_defer_and_edit() {
    let context = RuntimeContext::from_event(&submit("cozy"), &identity("test"));
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let steps = vec![
        PlannedAction::DeferEphemeral,
        PlannedAction::EditResponse {
            content: "완료".to_string(),
        },
    ];
    block_on(run(
        &context,
        &ActionPlan { steps },
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("test", 1),
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
    ))
    .unwrap();
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "완료".to_string(),
            },
        ]
    );
}

#[test]
fn handle_event_defer_success_edits_completion() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let outcome = block_on(handle_event(
        &submit("cozy"),
        &defer_rule(),
        &ResourceBindingMap::default(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("test", 1),
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
        "실패",
        &identity("test"),
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "스터디룸 'cozy' 완료".to_string(),
            },
        ]
    );
}

#[test]
fn handle_event_failure_edits_failure_message() {
    let mutation = MockMutationAdapter::failing(automation_core::adapter::AdapterError::new(
        AdapterErrorKind::Forbidden,
        "no",
    ));
    let responder = MockInteractionResponder::new();
    let result = block_on(handle_event(
        &submit("cozy"),
        &defer_rule(),
        &ResourceBindingMap::default(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("test", 1),
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
        "스터디룸 '${input.room_name}' 실패",
        &identity("test"),
    ));
    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "스터디룸 'cozy' 실패".to_string(),
            },
        ]
    );
}

#[test]
fn valid_defer_rule_passes() {
    assert!(validate(&defer_rule(), &ResourceBindingMap::default()).is_ok());
}

#[test]
fn defer_not_first_fails() {
    let set = rule(vec![
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "x".to_string(),
        },
        ActionSpec::DeferEphemeral,
        ActionSpec::EditResponse {
            content: "완료".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::DeferNotFirst {
            rule: "r".to_string(),
        }));
}

#[test]
fn conflicting_initial_response_fails() {
    let set = rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::RespondEphemeral {
            content: "hi".to_string(),
        },
        ActionSpec::EditResponse {
            content: "완료".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::ConflictingInitialResponse {
            rule: "r".to_string(),
        }));
}

#[test]
fn edit_without_defer_fails() {
    let set = rule(vec![ActionSpec::EditResponse {
        content: "완료".to_string(),
    }]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::EditResponseWithoutDefer {
            rule: "r".to_string(),
        }));
}

#[test]
fn deferred_missing_edit_fails() {
    let set = rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "x".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::DeferredMissingEditResponse {
            rule: "r".to_string(),
        }));
}

#[test]
fn multiple_edit_fails() {
    let set = rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::EditResponse {
            content: "a".to_string(),
        },
        ActionSpec::EditResponse {
            content: "b".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::MultipleEditResponse {
            rule: "r".to_string(),
        }));
}

#[test]
fn edit_not_last_fails() {
    let set = rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::EditResponse {
            content: "완료".to_string(),
        },
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "x".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::EditResponseNotLast {
            rule: "r".to_string(),
        }));
}
