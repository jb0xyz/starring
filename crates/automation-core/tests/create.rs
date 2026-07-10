use std::collections::BTreeMap;

use automation_core::adapter::AdapterErrorKind;
use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{ActionPlan, CreatedResource, PlannedAction};
use automation_core::run::{handle_event, run};
use automation_core::validate::{validate, ValidationError};
use automation_core::RuntimeContext;
use automation_state::{
    ActionSpec, ButtonSpec, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle,
    ModalSpec, PanelSpec, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, RoleId, UserId};
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

fn submit_rule(actions: Vec<ActionSpec>) -> InteractionRule {
    InteractionRule {
        key: "submit".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "study_modal".to_string(),
        },
        actions,
    }
}

fn ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![submit_rule(actions)],
    }
}

fn submit(room: &str) -> RuntimeEvent {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), room.to_string());
    RuntimeEvent {
        guild_id: GuildId(5),
        actor: UserId(9),
        kind: EventKind::ModalSubmit {
            modal: "study_modal".to_string(),
            inputs,
        },
    }
}

fn run_calls(event: &RuntimeEvent, actions: Vec<ActionSpec>) -> Vec<MutationCall> {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    block_on(handle_event(
        event,
        &ruleset(actions),
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
    ))
    .unwrap();
    mutation.calls()
}

#[test]
fn create_channel_renders_name() {
    let calls = run_calls(
        &submit("cozy corner"),
        vec![ActionSpec::CreateChannel {
            name: "study-${input.room_name}".to_string(),
        }],
    );
    assert_eq!(
        calls,
        vec![MutationCall::CreateChannel {
            guild: GuildId(5),
            name: "study-cozy-corner".to_string(),
        }]
    );
}

#[test]
fn create_role_renders_name() {
    let calls = run_calls(
        &submit("코딩"),
        vec![ActionSpec::CreateRole {
            name: "${input.room_name} 멤버".to_string(),
        }],
    );
    assert_eq!(
        calls,
        vec![MutationCall::CreateRole {
            guild: GuildId(5),
            name: "코딩 멤버".to_string(),
        }]
    );
}

#[test]
fn create_channel_missing_input_errors() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let mut inputs = BTreeMap::new();
    inputs.insert("other".to_string(), "x".to_string());
    let event = RuntimeEvent {
        guild_id: GuildId(5),
        actor: UserId(9),
        kind: EventKind::ModalSubmit {
            modal: "study_modal".to_string(),
            inputs,
        },
    };
    let result = block_on(handle_event(
        &event,
        &ruleset(vec![ActionSpec::CreateChannel {
            name: "study-${input.room_name}".to_string(),
        }]),
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
    ));
    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::BadRequest);
}

#[test]
fn created_ids_recorded_in_run_result() {
    let context = RuntimeContext::from_event(&submit("cozy"));
    let plan = ActionPlan {
        steps: vec![
            PlannedAction::CreateChannel {
                name: "study-${input.room_name}".to_string(),
            },
            PlannedAction::CreateRole {
                name: "${input.room_name} 멤버".to_string(),
            },
        ],
    };
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let created = block_on(run(&context, &plan, &mutation, &responder)).unwrap();
    assert_eq!(
        created,
        vec![
            CreatedResource::Channel {
                action_index: 0,
                name: "study-cozy".to_string(),
                id: ChannelId(800_000),
            },
            CreatedResource::Role {
                action_index: 1,
                name: "cozy 멤버".to_string(),
                id: RoleId(800_001),
            },
        ]
    );
}

#[test]
fn button_rule_create_input_template_fails_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "p".to_string(),
            channel: ResourceKey("c".to_string()),
            content: "x".to_string(),
            buttons: vec![ButtonSpec {
                key: "b".to_string(),
                label: "B".to_string(),
            }],
        }],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "click".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "b".to_string(),
            },
            actions: vec![ActionSpec::CreateChannel {
                name: "study-${input.room_name}".to_string(),
            }],
        }],
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(
        errors.contains(&ValidationError::InputTemplateInButtonRule {
            rule: "click".to_string(),
            input: "room_name".to_string(),
        })
    );
}
