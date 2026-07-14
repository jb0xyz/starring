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
    ActionSpec, ButtonRoute, ButtonSpec, InteractionRule, InteractionRuleSet, ModalFieldSpec,
    ModalFieldStyle, ModalSpec, PanelSpec, TriggerSpec,
};
use desired_state::ResourceKey;
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
        key: "study_room_modal".to_string(),
        title: "Create study room".to_string(),
        fields: vec![ModalFieldSpec {
            key: "room_name".to_string(),
            label: "Room name".to_string(),
            style: ModalFieldStyle::Short,
            required: true,
            min_length: None,
            max_length: None,
            input_policy: automation_state::ModalInputPolicy::Preserve,
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
                label: "Create study room".to_string(),
                route: ButtonRoute::Static {
                    key: "create_study_button".to_string(),
                },
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
fn modal_field_min_length_above_discord_limit_fails() {
    let mut set = ruleset();
    set.modals[0].fields[0].min_length = Some(4_001);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(
        errors.contains(&ValidationError::InvalidModalFieldMinLength {
            modal: "study_room_modal".to_string(),
            field: "room_name".to_string(),
            min_length: 4_001,
        })
    );
}

#[test]
fn modal_field_discord_length_boundaries_pass() {
    let mut set = ruleset();
    set.modals[0].fields[0].min_length = Some(0);
    set.modals[0].fields[0].max_length = Some(4_000);

    assert!(validate(&set, &ResourceBindingMap::default()).is_ok());
}

#[test]
fn modal_field_max_length_outside_discord_range_fails() {
    for max_length in [0, 4_001] {
        let mut set = ruleset();
        set.modals[0].fields[0].max_length = Some(max_length);
        let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
        assert!(
            errors.contains(&ValidationError::InvalidModalFieldMaxLength {
                modal: "study_room_modal".to_string(),
                field: "room_name".to_string(),
                max_length,
            })
        );
    }
}

#[test]
fn modal_field_min_length_must_not_exceed_max_length() {
    let mut set = ruleset();
    set.modals[0].fields[0].min_length = Some(5);
    set.modals[0].fields[0].max_length = Some(4);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(
        errors.contains(&ValidationError::InvalidModalFieldLengthRange {
            modal: "study_room_modal".to_string(),
            field: "room_name".to_string(),
            min_length: 5,
            max_length: 4,
        })
    );
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
        min_length: None,
        max_length: None,
        input_policy: automation_state::ModalInputPolicy::Preserve,
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
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
        "",
        &identity("test"),
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
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
        "",
        &identity("test"),
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

#[test]
fn invalid_modal_inputs_stop_before_any_runtime_effect() {
    let cases = [
        (BTreeMap::new(), "MODAL_INPUT_MISSING"),
        (
            BTreeMap::from([("room_name".to_string(), String::new())]),
            "MODAL_INPUT_MISSING",
        ),
        (
            BTreeMap::from([
                ("room_name".to_string(), "ok".to_string()),
                ("admin".to_string(), "true".to_string()),
            ]),
            "MODAL_INPUT_UNEXPECTED",
        ),
        (
            BTreeMap::from([("room_name".to_string(), "a".to_string())]),
            "MODAL_INPUT_TOO_SHORT",
        ),
        (
            BTreeMap::from([("room_name".to_string(), "abcde".to_string())]),
            "MODAL_INPUT_TOO_LONG",
        ),
        (
            BTreeMap::from([("room_name".to_string(), "😀😀😀".to_string())]),
            "MODAL_INPUT_TOO_LONG",
        ),
    ];

    for (inputs, code) in cases {
        let mut set = ruleset();
        set.modals[0].fields[0].min_length = Some(2);
        set.modals[0].fields[0].max_length = Some(4);
        set.rules[1].actions = vec![
            ActionSpec::DeferEphemeral,
            ActionSpec::CreateRole {
                key: "room_role".to_string(),
                name: "${input.room_name}".to_string(),
            },
            ActionSpec::EditResponse {
                content: "ready".to_string(),
            },
        ];
        let mutation = MockMutationAdapter::new();
        let responder = MockInteractionResponder::new();
        let event = RuntimeEvent {
            guild_id: GuildId(1),
            actor: UserId(42),
            kind: EventKind::ModalSubmit {
                modal: "study_room_modal".to_string(),
                inputs,
            },
        };
        let error = block_on(handle_event(
            &event,
            &set,
            &ResourceBindingMap::default(),
            &AutomationServices {
                mutation: &mutation,
                responder: &responder,
                instances: &InMemoryInstanceStore::new(),
                instance_ids: &SequenceInstanceIdGenerator::new("test", 1),
                teardown: &automation_core::MockInstanceTeardownService::new(),
            },
            "failed",
            &identity("test"),
        ))
        .unwrap_err();

        assert_eq!(error.kind, AdapterErrorKind::BadRequest);
        assert!(error.message.starts_with(code));
        assert!(mutation.calls().is_empty());
        assert!(responder.calls().is_empty());
    }
}

#[test]
fn unicode_input_at_utf16_boundary_reaches_mutation() {
    let mut set = ruleset();
    set.modals[0].fields[0].min_length = Some(4);
    set.modals[0].fields[0].max_length = Some(4);
    set.rules[1].actions = vec![ActionSpec::CreateRole {
        key: "room_role".to_string(),
        name: "${input.room_name}".to_string(),
    }];
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();

    let outcome = block_on(handle_event(
        &modal_event("study_room_modal", "😀😀"),
        &set,
        &ResourceBindingMap::default(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("test", 1),
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
        "failed",
        &identity("test"),
    ))
    .unwrap();

    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(mutation.calls().len(), 1);
    assert!(responder.calls().is_empty());
}

#[test]
fn explicit_trim_policy_updates_runtime_context_before_rendering() {
    let mut set = ruleset();
    set.modals[0].fields[0].input_policy =
        automation_state::ModalInputPolicy::TrimUnicodeWhitespace;
    set.modals[0].fields[0].min_length = Some(2);
    set.rules[1].actions = vec![ActionSpec::RespondEphemeral {
        content: "${input.room_name}".to_string(),
    }];
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();

    block_on(handle_event(
        &modal_event("study_room_modal", "  방a  "),
        &set,
        &ResourceBindingMap::default(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("test", 1),
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
        "failed",
        &identity("test"),
    ))
    .unwrap();

    assert!(mutation.calls().is_empty());
    assert_eq!(
        responder.calls(),
        vec![ResponderCall::RespondEphemeral {
            content: "방a".to_string(),
        }]
    );
}
