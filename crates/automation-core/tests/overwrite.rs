use std::collections::BTreeMap;

use automation_core::event::{EventKind, RuntimeContext, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{
    ActionPlan, PlannedAction, PlannedChannel, PlannedOverwriteTarget, PlannedRole,
};
use automation_core::policy::{analyze, PolicyFinding};
use automation_core::run::run;
use automation_core::validate::{validate, ValidationError};
use automation_core::AutomationServices;
use automation_instance::{InMemoryInstanceStore, SequenceInstanceIdGenerator};
use automation_state::{
    ActionSpec, ActionTarget, ChannelRef, CreatedRef, InteractionRule, InteractionRuleSet,
    ModalFieldSpec, ModalFieldStyle, ModalSpec, OverwriteTargetSpec, RoleRef, TriggerSpec,
};
use discord_model::{ChannelId, GuildId, OverwriteTarget, Permissions, RoleId, UserId};
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

fn overwrite_rule(actions: Vec<ActionSpec>) -> InteractionRuleSet {
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

fn role_created(key: &str) -> RoleRef {
    RoleRef::Created(CreatedRef {
        created: key.to_string(),
    })
}

fn private_study_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec::CreateRole {
            key: "study_member_role".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        ActionSpec::CreateChannel {
            key: "study_channel".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("study_channel"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("study_channel"),
            target: OverwriteTargetSpec::Role(role_created("study_member_role")),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
        ActionSpec::GrantRole {
            role: role_created("study_member_role"),
            target: ActionTarget::Actor,
        },
    ]
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

fn run_plan(steps: Vec<PlannedAction>) -> Vec<MutationCall> {
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

#[test]
fn everyone_overwrite_on_created_channel_resolves() {
    let calls = run_plan(vec![
        PlannedAction::CreateChannel {
            key: "c".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        PlannedAction::UpsertOverwrite {
            channel: PlannedChannel::Created("c".to_string()),
            target: PlannedOverwriteTarget::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        },
    ]);
    assert_eq!(
        calls,
        vec![
            MutationCall::CreateChannel {
                guild: GuildId(7),
                name: "study-cozy".to_string(),
            },
            MutationCall::UpsertOverwrite {
                guild: GuildId(7),
                channel: ChannelId(800_000),
                target: OverwriteTarget::Role(RoleId(7)),
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            },
        ]
    );
}

#[test]
fn created_role_target_resolves() {
    let calls = run_plan(vec![
        PlannedAction::CreateRole {
            key: "r".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::CreateChannel {
            key: "c".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        PlannedAction::UpsertOverwrite {
            channel: PlannedChannel::Created("c".to_string()),
            target: PlannedOverwriteTarget::Role(PlannedRole::Created("r".to_string())),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
    ]);
    assert_eq!(
        calls.last().unwrap(),
        &MutationCall::UpsertOverwrite {
            guild: GuildId(7),
            channel: ChannelId(800_001),
            target: OverwriteTarget::Role(RoleId(800_000)),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        }
    );
}

#[test]
fn private_study_room_call_sequence() {
    let calls = run_plan(vec![
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
        ]
    );
}

#[test]
fn channel_ref_missing_created_key_fails() {
    let set = overwrite_rule(vec![ActionSpec::UpsertOverwrite {
        channel: channel_created("ghost"),
        target: OverwriteTargetSpec::Everyone,
        allow: Permissions::empty(),
        deny: Permissions::VIEW_CHANNEL,
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
    let set = overwrite_rule(vec![
        ActionSpec::CreateRole {
            key: "somerole".to_string(),
            name: "x".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("somerole"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
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
fn role_target_missing_created_key_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Role(role_created("ghost")),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::UnknownCreatedRoleRef {
            rule: "r".to_string(),
            key: "ghost".to_string(),
        }));
}

#[test]
fn role_target_to_channel_key_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Role(role_created("c")),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::CreatedRoleRefTypeMismatch {
            rule: "r".to_string(),
            key: "c".to_string(),
        }));
}

#[test]
fn forward_channel_ref_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
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
fn allow_deny_overlap_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::VIEW_CHANNEL,
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::OverlappingOverwrite {
            rule: "r".to_string(),
        }));
}

#[test]
fn allow_deny_both_empty_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::empty(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::EmptyOverwrite {
            rule: "r".to_string(),
        }));
}

#[test]
fn valid_private_study_passes() {
    assert!(validate(
        &overwrite_rule(private_study_actions()),
        &ResourceBindingMap::default()
    )
    .is_ok());
}

#[test]
fn everyone_overwrite_flagged_by_policy() {
    let findings = analyze(&overwrite_rule(private_study_actions()), &BTreeMap::new());
    assert!(findings.contains(&PolicyFinding::EveryoneOverwrite {
        rule: "r".to_string(),
    }));
}

#[test]
fn privileged_allow_flagged_by_policy() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::ADMINISTRATOR,
            deny: Permissions::empty(),
        },
    ]);
    let findings = analyze(&set, &BTreeMap::new());
    assert!(findings.contains(&PolicyFinding::PrivilegedOverwriteAllow {
        rule: "r".to_string(),
    }));
}
