use std::collections::BTreeMap;

use automation_core::adapter::{AdapterError, AdapterErrorKind, InteractionResponder};
use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::interpret::interpret;
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, ResponderCall};
use automation_core::plan::PlannedAction;
use automation_core::run::{handle_event, HandleOutcome};
use automation_core::validate::{validate, ValidationError};
use automation_core::AutomationServices;
use automation_instance::{InMemoryInstanceStore, SequenceInstanceIdGenerator};
use automation_state::{
    ActionSpec, ButtonSpec, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle,
    ModalSpec, PanelSpec, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn modal() -> ModalSpec {
    ModalSpec {
        key: "study_room_modal".to_string(),
        title: "Create study room".to_string(),
        fields: vec![ModalFieldSpec {
            key: "room_name".to_string(),
            label: "Room name".to_string(),
            style: ModalFieldStyle::Short,
            required: true,
        }],
    }
}

fn ruleset() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "study_panel".to_string(),
            channel: ResourceKey("study_channel".to_string()),
            content: "Create a study room".to_string(),
            buttons: vec![ButtonSpec {
                key: "create_study_button".to_string(),
                label: "Create study room".to_string(),
            }],
        }],
        modals: vec![modal()],
        rules: vec![
            InteractionRule {
                key: "open_study_modal".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: "create_study_button".to_string(),
                },
                actions: vec![ActionSpec::OpenModal {
                    modal: "study_room_modal".to_string(),
                }],
            },
            InteractionRule {
                key: "submit_study_modal".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: "study_room_modal".to_string(),
                },
                actions: vec![ActionSpec::RespondEphemeral {
                    content: "요청이 접수되었습니다.".to_string(),
                }],
            },
        ],
    }
}

fn button_event(component: &str) -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(42),
        kind: EventKind::ButtonClick {
            component: component.to_string(),
        },
    }
}

fn modal_event(modal_key: &str, room: &str) -> RuntimeEvent {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), room.to_string());
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(42),
        kind: EventKind::ModalSubmit {
            modal: modal_key.to_string(),
            inputs,
        },
    }
}

#[test]
fn valid_modal_ruleset_passes() {
    assert!(validate(&ruleset(), &ResourceBindingMap::default()).is_ok());
}

#[test]
fn duplicate_modal_key_fails() {
    let mut set = ruleset();
    set.modals.push(modal());
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::DuplicateModalKey(
        "study_room_modal".to_string()
    )));
}

#[test]
fn duplicate_modal_field_key_fails() {
    let mut set = ruleset();
    set.modals[0].fields.push(ModalFieldSpec {
        key: "room_name".to_string(),
        label: "again".to_string(),
        style: ModalFieldStyle::Short,
        required: false,
    });
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::DuplicateModalFieldKey {
        modal: "study_room_modal".to_string(),
        field: "room_name".to_string(),
    }));
}

#[test]
fn open_modal_unknown_ref_fails() {
    let mut set = ruleset();
    set.rules[0].actions = vec![ActionSpec::OpenModal {
        modal: "ghost_modal".to_string(),
    }];
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownModalRef {
        rule: "open_study_modal".to_string(),
        modal: "ghost_modal".to_string(),
    }));
}

#[test]
fn modal_submit_unknown_ref_fails() {
    let mut set = ruleset();
    set.rules[1].trigger = TriggerSpec::ModalSubmit {
        modal: "ghost_modal".to_string(),
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownModalRef {
        rule: "submit_study_modal".to_string(),
        modal: "ghost_modal".to_string(),
    }));
}

#[test]
fn duplicate_modal_trigger_fails() {
    let mut set = ruleset();
    set.rules.push(InteractionRule {
        key: "submit_again".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "study_room_modal".to_string(),
        },
        actions: vec![ActionSpec::RespondEphemeral {
            content: "dup".to_string(),
        }],
    });
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::ConflictingModalTrigger {
        modal: "study_room_modal".to_string(),
    }));
}

#[test]
fn button_click_produces_open_modal_plan() {
    let plan = interpret(
        &button_event("create_study_button"),
        &ruleset(),
        &ResourceBindingMap::default(),
    )
    .unwrap();
    match &plan.steps[..] {
        [PlannedAction::OpenModal(presentation)] => {
            assert_eq!(presentation.key, "study_room_modal");
            assert_eq!(presentation.title, "Create study room");
            assert_eq!(presentation.fields.len(), 1);
            assert_eq!(presentation.fields[0].key, "room_name");
        }
        other => panic!("expected single OpenModal, got {other:?}"),
    }
}

#[test]
fn modal_submit_event_captures_inputs() {
    let event = modal_event("study_room_modal", "cozy corner");
    match &event.kind {
        EventKind::ModalSubmit { modal, inputs } => {
            assert_eq!(modal, "study_room_modal");
            assert_eq!(inputs.get("room_name"), Some(&"cozy corner".to_string()));
        }
        other => panic!("expected ModalSubmit, got {other:?}"),
    }
    assert!(interpret(&event, &ruleset(), &ResourceBindingMap::default()).is_some());
}

#[test]
fn modal_submit_produces_static_plan() {
    let plan = interpret(
        &modal_event("study_room_modal", "cozy corner"),
        &ruleset(),
        &ResourceBindingMap::default(),
    )
    .unwrap();
    assert_eq!(
        plan.steps,
        vec![PlannedAction::RespondEphemeral {
            content: "요청이 접수되었습니다.".to_string(),
        }]
    );
}

#[test]
fn unknown_modal_submit_is_none() {
    assert!(interpret(
        &modal_event("ghost_modal", "x"),
        &ruleset(),
        &ResourceBindingMap::default()
    )
    .is_none());
}

#[test]
fn default_responder_open_modal_is_unsupported() {
    struct DefaultResponder;
    impl InteractionResponder for DefaultResponder {
        async fn respond_ephemeral(&self, _content: String) -> Result<(), AdapterError> {
            Ok(())
        }
    }

    let mutation = MockMutationAdapter::new();
    let responder = DefaultResponder;
    let result = block_on(handle_event(
        &button_event("create_study_button"),
        &ruleset(),
        &ResourceBindingMap::default(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("test", 1),
        },
        "",
        "test",
    ));
    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Unsupported);
}

#[test]
fn mock_responder_runs_open_modal() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let outcome = block_on(handle_event(
        &button_event("create_study_button"),
        &ruleset(),
        &ResourceBindingMap::default(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("test", 1),
        },
        "",
        "test",
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        responder.calls(),
        vec![ResponderCall::OpenModal {
            modal: "study_room_modal".to_string(),
        }]
    );
}
