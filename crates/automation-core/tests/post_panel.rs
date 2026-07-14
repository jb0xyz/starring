use std::collections::BTreeMap;

use automation_core::adapter::{PostPanelButtonSpec, ResolvedButtonRoute};
use automation_core::event::{EventKind, RuntimeContext, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{
    ActionPlan, CreatedResource, PlannedAction, PlannedChannel, PlannedOverwriteTarget, PlannedRole,
};
use automation_core::policy::{analyze, PolicyFinding};
use automation_core::run::run;
use automation_core::validate::{validate, ValidationError};
use automation_core::AutomationServices;
use automation_instance::{InMemoryInstanceStore, SequenceInstanceIdGenerator};
use automation_state::{
    ActionSpec, ButtonRoute, ButtonSpec, ChannelRef, CreatedRef, InteractionRule,
    InteractionRuleSet, ModalFieldSpec, ModalFieldStyle, ModalSpec, PanelSpec, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};
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
        }],
    }
}

fn post_panel_rule(actions: Vec<ActionSpec>) -> InteractionRuleSet {
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

fn channel_created(key: &str) -> ChannelRef {
    ChannelRef::Created(CreatedRef {
        created: key.to_string(),
    })
}

fn button(key: &str, label: &str) -> ButtonSpec {
    ButtonSpec {
        label: label.to_string(),
        route: ButtonRoute::Static {
            key: key.to_string(),
        },
    }
}

fn resolved_button(key: &str, label: &str) -> PostPanelButtonSpec {
    PostPanelButtonSpec {
        label: label.to_string(),
        route: ResolvedButtonRoute::Static {
            key: key.to_string(),
        },
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

fn run_calls(steps: Vec<PlannedAction>) -> Vec<MutationCall> {
    let context = RuntimeContext::from_event(&submit("cozy"), &identity("test"));
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
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
    mutation.calls()
}

fn run_created(steps: Vec<PlannedAction>) -> Vec<CreatedResource> {
    let context = RuntimeContext::from_event(&submit("cozy"), &identity("test"));
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
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
    .unwrap()
    .created
}

#[test]
fn post_panel_into_created_channel() {
    let calls = run_calls(vec![
        PlannedAction::CreateChannel {
            key: "c".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        PlannedAction::PostPanel {
            key: "panel".to_string(),
            channel: PlannedChannel::Created("c".to_string()),
            content: "환영 ${input.room_name}".to_string(),
            buttons: vec![button("study_help", "도움말")],
        },
    ]);
    assert_eq!(
        calls,
        vec![
            MutationCall::CreateChannel {
                guild: GuildId(7),
                name: "study-cozy".to_string(),
            },
            MutationCall::PostPanel {
                guild: GuildId(7),
                channel: ChannelId(800_000),
                content: "환영 cozy".to_string(),
                buttons: vec![resolved_button("study_help", "도움말")],
            },
        ]
    );
}

#[test]
fn full_study_room_call_sequence() {
    let calls = run_calls(vec![
        PlannedAction::CreateRole {
            key: "study_member_role".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::CreateChannel {
            key: "study_channel".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        PlannedAction::UpsertOverwrite {
            channel: PlannedChannel::Created("study_channel".to_string()),
            target: PlannedOverwriteTarget::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        },
        PlannedAction::UpsertOverwrite {
            channel: PlannedChannel::Created("study_channel".to_string()),
            target: PlannedOverwriteTarget::Role(PlannedRole::Created(
                "study_member_role".to_string(),
            )),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
        PlannedAction::GrantRole {
            role: PlannedRole::Created("study_member_role".to_string()),
            target: UserId(3),
        },
        PlannedAction::PostPanel {
            key: "study_welcome_panel".to_string(),
            channel: PlannedChannel::Created("study_channel".to_string()),
            content: "스터디룸 개설 완료".to_string(),
            buttons: vec![button("study_help", "도움말")],
        },
    ]);
    assert_eq!(
        calls,
        vec![
            MutationCall::CreateRole {
                guild: GuildId(7),
                name: "cozy 멤버".to_string(),
            },
            MutationCall::CreateChannel {
                guild: GuildId(7),
                name: "study-cozy".to_string(),
            },
            MutationCall::UpsertOverwrite {
                guild: GuildId(7),
                channel: ChannelId(800_001),
                target: OverwriteTarget::Role(RoleId(7)),
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            },
            MutationCall::UpsertOverwrite {
                guild: GuildId(7),
                channel: ChannelId(800_001),
                target: OverwriteTarget::Role(RoleId(800_000)),
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::empty(),
            },
            MutationCall::GrantRole {
                guild: GuildId(7),
                member: UserId(3),
                role: RoleId(800_000),
            },
            MutationCall::PostPanel {
                guild: GuildId(7),
                channel: ChannelId(800_001),
                content: "스터디룸 개설 완료".to_string(),
                buttons: vec![resolved_button("study_help", "도움말")],
            },
        ]
    );
}

#[test]
fn message_id_recorded_in_result() {
    let created = run_created(vec![
        PlannedAction::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        PlannedAction::PostPanel {
            key: "panel".to_string(),
            channel: PlannedChannel::Created("c".to_string()),
            content: "hi".to_string(),
            buttons: vec![],
        },
    ]);
    assert_eq!(
        created.last().unwrap(),
        &CreatedResource::Message {
            action_index: 1,
            key: "panel".to_string(),
            channel: ChannelId(800_000),
            id: MessageId(800_001),
        }
    );
}

#[test]
fn missing_input_fails_post_panel() {
    let context = RuntimeContext::from_event(&submit("cozy"), &identity("test"));
    let steps = vec![
        PlannedAction::CreateRole {
            key: "member".to_string(),
            name: "member".to_string(),
        },
        PlannedAction::PostPanel {
            key: "panel".to_string(),
            channel: PlannedChannel::Resolved(ChannelId(999)),
            content: "${input.missing}".to_string(),
            buttons: vec![],
        },
    ];
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    assert!(block_on(run(
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
    .is_err());
    assert_eq!(
        mutation.calls(),
        vec![MutationCall::CreateRole {
            guild: GuildId(7),
            name: "member".to_string(),
        }]
    );
}

#[test]
fn unknown_created_channel_ref_fails() {
    let set = post_panel_rule(vec![ActionSpec::PostPanel {
        key: "panel".to_string(),
        channel: channel_created("ghost"),
        content: "hi".to_string(),
        buttons: vec![],
    }]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::UnknownCreatedChannelRef {
            rule: "r".to_string(),
            key: "ghost".to_string(),
        }));
}

#[test]
fn channel_ref_to_role_key_fails() {
    let set = post_panel_rule(vec![
        ActionSpec::CreateRole {
            key: "somerole".to_string(),
            name: "x".to_string(),
        },
        ActionSpec::PostPanel {
            key: "panel".to_string(),
            channel: channel_created("somerole"),
            content: "hi".to_string(),
            buttons: vec![],
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::CreatedChannelRefTypeMismatch {
            rule: "r".to_string(),
            key: "somerole".to_string(),
        }));
}

#[test]
fn forward_channel_ref_fails() {
    let set = post_panel_rule(vec![
        ActionSpec::PostPanel {
            key: "panel".to_string(),
            channel: channel_created("c"),
            content: "hi".to_string(),
            buttons: vec![],
        },
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::UnknownCreatedChannelRef {
            rule: "r".to_string(),
            key: "c".to_string(),
        }));
}

#[test]
fn empty_button_label_fails() {
    let set = post_panel_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::PostPanel {
            key: "panel".to_string(),
            channel: channel_created("c"),
            content: "hi".to_string(),
            buttons: vec![button("b", "  ")],
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::EmptyButtonLabel {
            rule: "r".to_string(),
            button: "b".to_string(),
        }));
}

#[test]
fn too_many_buttons_fails() {
    let buttons: Vec<ButtonSpec> = (0..6).map(|i| button(&format!("b{i}"), "x")).collect();
    let set = post_panel_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::PostPanel {
            key: "panel".to_string(),
            channel: channel_created("c"),
            content: "hi".to_string(),
            buttons,
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::TooManyPanelButtons {
            rule: "r".to_string(),
            count: 6,
        }));
}

#[test]
fn duplicate_button_key_within_panel_fails() {
    let set = post_panel_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::PostPanel {
            key: "panel".to_string(),
            channel: channel_created("c"),
            content: "hi".to_string(),
            buttons: vec![button("dup", "x"), button("dup", "y")],
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::DuplicateButtonKey("dup".to_string())));
}

#[test]
fn panel_button_collides_with_post_panel_button_fails() {
    let mut set = post_panel_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::PostPanel {
            key: "panel".to_string(),
            channel: channel_created("c"),
            content: "hi".to_string(),
            buttons: vec![button("shared", "x")],
        },
    ]);
    set.panels = vec![PanelSpec {
        key: "p".to_string(),
        channel: ResourceKey("chan".to_string()),
        content: "c".to_string(),
        buttons: vec![button("shared", "y")],
    }];
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::DuplicateButtonKey("shared".to_string())));
}

#[test]
fn two_post_panels_same_button_key_fails() {
    let set = post_panel_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::PostPanel {
            key: "panel_one".to_string(),
            channel: channel_created("c"),
            content: "hi".to_string(),
            buttons: vec![button("shared", "x")],
        },
        ActionSpec::PostPanel {
            key: "panel_two".to_string(),
            channel: channel_created("c"),
            content: "bye".to_string(),
            buttons: vec![button("shared", "y")],
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::DuplicateButtonKey("shared".to_string())));
}

#[test]
fn post_panel_button_referenced_by_button_click_ok() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![
            InteractionRule {
                key: "create".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: "m".to_string(),
                },
                actions: vec![
                    ActionSpec::CreateChannel {
                        key: "c".to_string(),
                        name: "study".to_string(),
                    },
                    ActionSpec::PostPanel {
                        key: "panel".to_string(),
                        channel: channel_created("c"),
                        content: "hi".to_string(),
                        buttons: vec![button("study_help", "도움말")],
                    },
                ],
            },
            InteractionRule {
                key: "help".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: "study_help".to_string(),
                },
                actions: vec![ActionSpec::RespondEphemeral {
                    content: "help".to_string(),
                }],
            },
        ],
    };
    assert!(validate(&set, &ResourceBindingMap::default()).is_ok());
}

#[test]
fn post_panel_flagged_by_policy() {
    let findings = analyze(
        &post_panel_rule(vec![
            ActionSpec::CreateChannel {
                key: "c".to_string(),
                name: "study".to_string(),
            },
            ActionSpec::PostPanel {
                key: "panel".to_string(),
                channel: channel_created("c"),
                content: "hi".to_string(),
                buttons: vec![button("study_help", "도움말")],
            },
        ]),
        &BTreeMap::new(),
    );
    assert!(findings.contains(&PolicyFinding::RuntimeMessagePost {
        rule: "r".to_string(),
    }));
    assert!(findings.contains(&PolicyFinding::RuntimeInteractivePanel {
        rule: "r".to_string(),
    }));
}
