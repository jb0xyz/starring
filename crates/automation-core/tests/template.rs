use std::collections::BTreeMap;

use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, ResponderCall};
use automation_core::run::handle_event;
use automation_core::validate::{validate, ValidationError};
use automation_instance::{InMemoryInstanceStore, SequenceInstanceIdGenerator};
use automation_state::{
    ActionSpec, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle, ModalSpec,
    TriggerSpec,
};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn modal() -> ModalSpec {
    ModalSpec {
        key: "study_modal".to_string(),
        title: "Study".to_string(),
        fields: vec![ModalFieldSpec {
            key: "room_name".to_string(),
            label: "Room".to_string(),
            style: ModalFieldStyle::Short,
            required: true,
        }],
    }
}

fn modal_rule(content: &str) -> InteractionRule {
    InteractionRule {
        key: "submit_rule".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "study_modal".to_string(),
        },
        actions: vec![ActionSpec::RespondEphemeral {
            content: content.to_string(),
        }],
    }
}

fn button_rule(content: &str) -> InteractionRule {
    InteractionRule {
        key: "click_rule".to_string(),
        trigger: TriggerSpec::ButtonClick {
            component: "b".to_string(),
        },
        actions: vec![ActionSpec::RespondEphemeral {
            content: content.to_string(),
        }],
    }
}

fn modal_submit(room: &str) -> RuntimeEvent {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), room.to_string());
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(9),
        kind: EventKind::ModalSubmit {
            modal: "study_modal".to_string(),
            inputs,
        },
    }
}

fn button_click() -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(9),
        kind: EventKind::ButtonClick {
            component: "b".to_string(),
        },
    }
}

fn responded(event: &RuntimeEvent, rule: InteractionRule) -> String {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![rule],
    };
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    block_on(handle_event(
        event,
        &set,
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
        "",
        "test",
        &InMemoryInstanceStore::new(),
        &SequenceInstanceIdGenerator::new("test", 1),
    ))
    .unwrap();
    match responder.calls().into_iter().next().unwrap() {
        ResponderCall::RespondEphemeral { content } => content,
        other => panic!("expected RespondEphemeral, got {other:?}"),
    }
}

#[test]
fn button_input_template_fails_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![automation_state::PanelSpec {
            key: "p".to_string(),
            channel: desired_state::ResourceKey("c".to_string()),
            content: "x".to_string(),
            buttons: vec![automation_state::ButtonSpec {
                key: "b".to_string(),
                label: "B".to_string(),
            }],
        }],
        modals: vec![],
        rules: vec![button_rule("${input.room_name}")],
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(
        errors.contains(&ValidationError::InputTemplateInButtonRule {
            rule: "click_rule".to_string(),
            input: "room_name".to_string(),
        })
    );
}

#[test]
fn modal_unknown_input_fails_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![modal_rule("${input.ghost}")],
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownTemplateInput {
        rule: "submit_rule".to_string(),
        modal: "study_modal".to_string(),
        input: "ghost".to_string(),
    }));
}

#[test]
fn modal_known_input_passes_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![modal_rule("room: ${input.room_name}")],
    };
    assert!(validate(&set, &ResourceBindingMap::default()).is_ok());
}

#[test]
fn bad_template_syntax_fails_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![modal_rule("oops ${input.")],
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::BadTemplate {
        rule: "submit_rule".to_string(),
    }));
}

#[test]
fn static_content_renders_unchanged() {
    assert_eq!(
        responded(&button_click(), button_rule("welcome")),
        "welcome"
    );
}

#[test]
fn modal_input_rendered_into_response() {
    assert_eq!(
        responded(
            &modal_submit("cozy"),
            modal_rule("room: ${input.room_name}")
        ),
        "room: cozy"
    );
}

#[test]
fn injected_mention_is_sanitized_in_response() {
    let out = responded(&modal_submit("@everyone"), modal_rule("${input.room_name}"));
    assert!(!out.contains("@everyone"));
}
