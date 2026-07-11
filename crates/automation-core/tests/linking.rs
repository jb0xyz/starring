use std::collections::BTreeMap;

use automation_core::adapter::{AdapterError, AdapterErrorKind, DiscordMutationAdapter};
use automation_core::event::{EventKind, RuntimeContext, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{ActionPlan, CreatedResource, PlannedAction, PlannedRole};
use automation_core::policy::{analyze, PolicyFinding};
use automation_core::run::run;
use automation_core::validate::{validate, ValidationError};
use automation_instance::{InMemoryInstanceStore, SequenceInstanceIdGenerator};
use automation_state::{
    ActionSpec, ActionTarget, CreatedRef, InteractionRule, InteractionRuleSet, ModalFieldSpec,
    ModalFieldStyle, ModalSpec, RoleRef, TriggerSpec,
};
use discord_model::{ChannelId, GuildId, RoleId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

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

fn submit_rule(actions: Vec<ActionSpec>) -> InteractionRuleSet {
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

fn study_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        ActionSpec::CreateChannel {
            key: "channel".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        ActionSpec::GrantRole {
            role: RoleRef::Created(CreatedRef {
                created: "member".to_string(),
            }),
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

fn plan(steps: Vec<PlannedAction>) -> ActionPlan {
    ActionPlan { steps }
}

#[test]
fn created_role_granted_to_actor() {
    let context = RuntimeContext::from_event(&submit("코딩"), "test");
    let steps = vec![
        PlannedAction::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::GrantRole {
            role: PlannedRole::Created("member".to_string()),
            target: UserId(3),
        },
    ];
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    block_on(run(
        &context,
        &plan(steps),
        &mutation,
        &responder,
        &InMemoryInstanceStore::new(),
        &SequenceInstanceIdGenerator::new("test", 1),
    ))
    .unwrap();
    assert_eq!(
        mutation.calls(),
        vec![
            MutationCall::CreateRole {
                guild: GuildId(7),
                name: "코딩 멤버".to_string(),
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
fn full_study_run_creates_then_grants() {
    let context = RuntimeContext::from_event(&submit("cozy"), "test");
    let steps = vec![
        PlannedAction::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::CreateChannel {
            key: "channel".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        PlannedAction::GrantRole {
            role: PlannedRole::Created("member".to_string()),
            target: UserId(3),
        },
    ];
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let created = block_on(run(
        &context,
        &plan(steps),
        &mutation,
        &responder,
        &InMemoryInstanceStore::new(),
        &SequenceInstanceIdGenerator::new("test", 1),
    ))
    .unwrap();
    assert_eq!(
        created,
        vec![
            CreatedResource::Role {
                action_index: 0,
                key: "member".to_string(),
                name: "cozy 멤버".to_string(),
                id: RoleId(800_000),
            },
            CreatedResource::Channel {
                action_index: 1,
                key: "channel".to_string(),
                name: "study-cozy".to_string(),
                id: ChannelId(800_001),
            },
        ]
    );
    assert!(matches!(
        mutation.calls().as_slice(),
        [
            MutationCall::CreateRole { .. },
            MutationCall::CreateChannel { .. },
            MutationCall::GrantRole {
                role: RoleId(800_000),
                ..
            }
        ]
    ));
}

#[test]
fn duplicate_action_key_fails_validate() {
    let set = submit_rule(vec![
        ActionSpec::CreateRole {
            key: "dup".to_string(),
            name: "a".to_string(),
        },
        ActionSpec::CreateChannel {
            key: "dup".to_string(),
            name: "b".to_string(),
        },
    ]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::DuplicateActionKey {
        rule: "r".to_string(),
        key: "dup".to_string(),
    }));
}

#[test]
fn unknown_created_role_ref_fails_validate() {
    let set = submit_rule(vec![ActionSpec::GrantRole {
        role: RoleRef::Created(CreatedRef {
            created: "ghost".to_string(),
        }),
        target: ActionTarget::Actor,
    }]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownCreatedRoleRef {
        rule: "r".to_string(),
        key: "ghost".to_string(),
    }));
}

#[test]
fn created_role_ref_to_channel_fails_validate() {
    let set = submit_rule(vec![
        ActionSpec::CreateChannel {
            key: "channel".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::GrantRole {
            role: RoleRef::Created(CreatedRef {
                created: "channel".to_string(),
            }),
            target: ActionTarget::Actor,
        },
    ]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(
        errors.contains(&ValidationError::CreatedRoleRefTypeMismatch {
            rule: "r".to_string(),
            key: "channel".to_string(),
        })
    );
}

#[test]
fn forward_created_ref_fails_validate() {
    let set = submit_rule(vec![
        ActionSpec::GrantRole {
            role: RoleRef::Created(CreatedRef {
                created: "member".to_string(),
            }),
            target: ActionTarget::Actor,
        },
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "x".to_string(),
        },
    ]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownCreatedRoleRef {
        rule: "r".to_string(),
        key: "member".to_string(),
    }));
}

#[test]
fn valid_study_ruleset_passes() {
    assert!(validate(
        &submit_rule(study_actions()),
        &ResourceBindingMap::default()
    )
    .is_ok());
}

#[test]
fn create_role_failure_skips_grant() {
    struct FailCreate;
    impl DiscordMutationAdapter for FailCreate {
        async fn grant_role(
            &self,
            _g: GuildId,
            _m: UserId,
            _r: RoleId,
        ) -> Result<(), AdapterError> {
            panic!("grant_role must not run")
        }
        async fn create_role(
            &self,
            _g: GuildId,
            _s: automation_core::adapter::CreateRoleSpec,
        ) -> Result<RoleId, AdapterError> {
            Err(AdapterError::new(AdapterErrorKind::Forbidden, "no"))
        }
    }
    let context = RuntimeContext::from_event(&submit("x"), "test");
    let steps = vec![
        PlannedAction::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::GrantRole {
            role: PlannedRole::Created("member".to_string()),
            target: UserId(3),
        },
    ];
    let responder = MockInteractionResponder::new();
    let result = block_on(run(
        &context,
        &plan(steps),
        &FailCreate,
        &responder,
        &InMemoryInstanceStore::new(),
        &SequenceInstanceIdGenerator::new("test", 1),
    ));
    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
}

#[test]
fn created_template_variable_is_unsupported() {
    let set = submit_rule(vec![ActionSpec::CreateRole {
        key: "member".to_string(),
        name: "${created.member.id}".to_string(),
    }]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::BadTemplate {
        rule: "r".to_string(),
    }));
}

#[test]
fn created_reference_flagged_by_policy() {
    let findings = analyze(&submit_rule(study_actions()), &BTreeMap::new());
    assert!(findings.contains(&PolicyFinding::CreatedResourceReference {
        rule: "r".to_string(),
    }));
}
