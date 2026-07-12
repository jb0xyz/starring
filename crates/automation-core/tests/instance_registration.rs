use std::collections::BTreeMap;

use automation_core::adapter::{AdapterErrorKind, PostPanelButtonSpec, ResolvedButtonRoute};
use automation_core::event::{EventKind, RunningRuleSetIdentity, RuntimeContext, RuntimeEvent};
use automation_core::interpret::interpret;
use automation_core::mock::{
    MockInteractionResponder, MockMutationAdapter, MutationCall, ResponderCall,
};
use automation_core::plan::{ActionPlan, CreatedResource, PlannedAction, PlannedChannel};
use automation_core::run::{handle_event, run, HandleOutcome};
use automation_core::validate::{validate, ValidationError};
use automation_core::AutomationServices;
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceResources,
    InstanceRuleSetVersion, InstanceStatus, InstanceStore, SequenceInstanceIdGenerator,
};
use automation_state::{
    ActionSpec, ActionTarget, ButtonRoute, ButtonSpec, ChannelRef, CreatedRef,
    InstanceResourceRefs, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle,
    ModalSpec, OverwriteTargetSpec, RoleRef, TriggerSpec,
};
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn created(key: &str) -> CreatedRef {
    CreatedRef {
        created: key.to_string(),
    }
}

fn context(guild_id: GuildId) -> RuntimeContext {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), "cozy".to_string());
    RuntimeContext {
        guild_id,
        actor: UserId(3),
        ruleset_key: "studyroom_demo".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        inputs,
        instance: None,
    }
}

fn event(guild_id: GuildId) -> RuntimeEvent {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), "cozy".to_string());
    RuntimeEvent {
        guild_id,
        actor: UserId(3),
        kind: EventKind::ModalSubmit {
            modal: "study_modal".to_string(),
            inputs,
        },
    }
}

fn full_manifest() -> InstanceResourceRefs {
    InstanceResourceRefs {
        roles: BTreeMap::from([("member_role".to_string(), created("study_member_role"))]),
        channels: BTreeMap::from([("room_channel".to_string(), created("study_channel"))]),
        messages: BTreeMap::from([("welcome_panel".to_string(), created("study_welcome_panel"))]),
    }
}

fn registration_plan() -> ActionPlan {
    ActionPlan {
        steps: vec![
            PlannedAction::CreateRole {
                key: "study_member_role".to_string(),
                name: "${input.room_name} 멤버".to_string(),
            },
            PlannedAction::CreateChannel {
                key: "study_channel".to_string(),
                name: "study-${input.room_name}".to_string(),
            },
            PlannedAction::PostPanel {
                key: "study_welcome_panel".to_string(),
                channel: PlannedChannel::Created("study_channel".to_string()),
                content: "환영합니다.".to_string(),
                buttons: vec![],
            },
            PlannedAction::RegisterInstance {
                key: "study_room_instance".to_string(),
                kind: InstanceKind("study_room".to_string()),
                resources: full_manifest(),
            },
        ],
    }
}

fn validation_ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![ModalSpec {
            key: "study_modal".to_string(),
            title: "Study".to_string(),
            fields: vec![ModalFieldSpec {
                key: "room_name".to_string(),
                label: "Room".to_string(),
                style: ModalFieldStyle::Short,
                required: true,
            }],
        }],
        rules: vec![InteractionRule {
            key: "study_rule".to_string(),
            trigger: TriggerSpec::ModalSubmit {
                modal: "study_modal".to_string(),
            },
            actions,
        }],
    }
}

fn register_action(key: &str, resources: InstanceResourceRefs) -> ActionSpec {
    ActionSpec::RegisterInstance {
        key: key.to_string(),
        kind: InstanceKind("study_room".to_string()),
        resources,
    }
}

fn stored_instance(guild_id: GuildId, id: &str) -> AutomationInstance {
    AutomationInstance {
        id: InstanceId::parse(id).unwrap(),
        guild_id,
        ruleset_key: "existing".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        kind: InstanceKind("existing".to_string()),
        created_by: UserId(99),
        resources: InstanceResources::default(),
        status: InstanceStatus::Active,
    }
}

#[test]
fn register_instance_resolves_and_stores_manifest() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let instances = InMemoryInstanceStore::new();
    let instance_ids = SequenceInstanceIdGenerator::new("room", 1);
    let created_resources = block_on(run(
        &context(GuildId(7)),
        &registration_plan(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &instances,
            instance_ids: &instance_ids,
        },
    ))
    .unwrap();
    let instance_id = InstanceId::parse("room_001").unwrap();
    let instance = block_on(instances.get(GuildId(7), &instance_id))
        .unwrap()
        .unwrap();

    assert_eq!(instance.id, instance_id);
    assert_eq!(instance.guild_id, GuildId(7));
    assert_eq!(instance.ruleset_key, "studyroom_demo");
    assert_eq!(instance.kind, InstanceKind("study_room".to_string()));
    assert_eq!(instance.created_by, UserId(3));
    assert_eq!(instance.status, InstanceStatus::Active);
    assert_eq!(instance.resources.roles["member_role"], RoleId(800_000));
    assert_eq!(
        instance.resources.channels["room_channel"],
        ChannelId(800_001)
    );
    assert_eq!(
        instance.resources.messages["welcome_panel"],
        MessageId(800_002)
    );
    assert_eq!(
        created_resources,
        vec![
            CreatedResource::Role {
                action_index: 0,
                key: "study_member_role".to_string(),
                name: "cozy 멤버".to_string(),
                id: RoleId(800_000),
            },
            CreatedResource::Channel {
                action_index: 1,
                key: "study_channel".to_string(),
                name: "study-cozy".to_string(),
                id: ChannelId(800_001),
            },
            CreatedResource::Message {
                action_index: 2,
                key: "study_welcome_panel".to_string(),
                channel: ChannelId(800_001),
                id: MessageId(800_002),
            },
            CreatedResource::Instance {
                action_index: 3,
                key: "study_room_instance".to_string(),
                id: InstanceId::parse("room_001").unwrap(),
            },
        ]
    );
}

#[test]
fn unresolved_manifest_fails_after_id_preallocation() {
    let plan = ActionPlan {
        steps: vec![PlannedAction::RegisterInstance {
            key: "instance".to_string(),
            kind: InstanceKind("study_room".to_string()),
            resources: InstanceResourceRefs {
                roles: BTreeMap::from([("member_role".to_string(), created("missing"))]),
                ..InstanceResourceRefs::default()
            },
        }],
    };
    let result = block_on(run(
        &context(GuildId(7)),
        &plan,
        &AutomationServices {
            mutation: &MockMutationAdapter::new(),
            responder: &MockInteractionResponder::new(),
            instances: &InMemoryInstanceStore::new(),
            instance_ids: &SequenceInstanceIdGenerator::new("room", 1),
        },
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::BadRequest);
}

#[test]
fn duplicate_store_registration_is_fail_fast() {
    let instances = InMemoryInstanceStore::new();
    let existing = stored_instance(GuildId(7), "room_001");
    block_on(instances.register(existing.clone())).unwrap();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let plan = ActionPlan {
        steps: vec![
            PlannedAction::CreateRole {
                key: "member".to_string(),
                name: "member".to_string(),
            },
            PlannedAction::RegisterInstance {
                key: "instance".to_string(),
                kind: InstanceKind("study_room".to_string()),
                resources: InstanceResourceRefs {
                    roles: BTreeMap::from([("member_role".to_string(), created("member"))]),
                    ..InstanceResourceRefs::default()
                },
            },
            PlannedAction::EditResponse {
                content: "must not run".to_string(),
            },
        ],
    };
    let result = block_on(run(
        &context(GuildId(7)),
        &plan,
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &instances,
            instance_ids: &SequenceInstanceIdGenerator::new("room", 1),
        },
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::BadRequest);
    assert!(responder.calls().is_empty());
    assert_eq!(
        block_on(instances.get(GuildId(7), &existing.id)).unwrap(),
        Some(existing)
    );
}

#[test]
fn duplicate_registration_key_fails_validate() {
    let ruleset = validation_ruleset(vec![
        ActionSpec::CreateRole {
            key: "duplicate".to_string(),
            name: "member".to_string(),
        },
        register_action(
            "duplicate",
            InstanceResourceRefs {
                roles: BTreeMap::from([("member_role".to_string(), created("duplicate"))]),
                ..InstanceResourceRefs::default()
            },
        ),
    ]);

    assert!(validate(&ruleset, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::DuplicateActionKey {
            rule: "study_rule".to_string(),
            key: "duplicate".to_string(),
        }));
}

#[test]
fn manifest_type_mismatches_fail_validate() {
    let ruleset = validation_ruleset(vec![
        ActionSpec::CreateRole {
            key: "role".to_string(),
            name: "member".to_string(),
        },
        ActionSpec::CreateChannel {
            key: "channel".to_string(),
            name: "study".to_string(),
        },
        register_action(
            "instance",
            InstanceResourceRefs {
                roles: BTreeMap::from([("member_role".to_string(), created("channel"))]),
                messages: BTreeMap::from([("welcome_panel".to_string(), created("role"))]),
                ..InstanceResourceRefs::default()
            },
        ),
    ]);
    let errors = validate(&ruleset, &ResourceBindingMap::default()).unwrap_err();

    assert!(
        errors.contains(&ValidationError::CreatedRoleRefTypeMismatch {
            rule: "study_rule".to_string(),
            key: "channel".to_string(),
        })
    );
    assert!(
        errors.contains(&ValidationError::CreatedMessageRefTypeMismatch {
            rule: "study_rule".to_string(),
            key: "role".to_string(),
        })
    );
}

#[test]
fn missing_and_forward_manifest_refs_fail_validate() {
    let ruleset = validation_ruleset(vec![
        register_action(
            "instance",
            InstanceResourceRefs {
                roles: BTreeMap::from([("member_role".to_string(), created("missing"))]),
                channels: BTreeMap::from([("room_channel".to_string(), created("later"))]),
                messages: BTreeMap::from([(
                    "welcome_panel".to_string(),
                    created("missing_message"),
                )]),
            },
        ),
        ActionSpec::CreateChannel {
            key: "later".to_string(),
            name: "study".to_string(),
        },
    ]);
    let errors = validate(&ruleset, &ResourceBindingMap::default()).unwrap_err();

    assert!(errors.contains(&ValidationError::UnknownCreatedRoleRef {
        rule: "study_rule".to_string(),
        key: "missing".to_string(),
    }));
    assert!(errors.contains(&ValidationError::UnknownCreatedChannelRef {
        rule: "study_rule".to_string(),
        key: "later".to_string(),
    }));
    assert!(errors.contains(&ValidationError::UnknownCreatedMessageRef {
        rule: "study_rule".to_string(),
        key: "missing_message".to_string(),
    }));
}

#[test]
fn empty_instance_resources_fail_validate() {
    let ruleset = validation_ruleset(vec![register_action(
        "instance",
        InstanceResourceRefs::default(),
    )]);

    assert!(validate(&ruleset, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::EmptyInstanceResources {
            rule: "study_rule".to_string(),
        }));
}

#[test]
fn invalid_resource_alias_fails_validate() {
    let ruleset = validation_ruleset(vec![
        ActionSpec::CreateRole {
            key: "role".to_string(),
            name: "member".to_string(),
        },
        register_action(
            "instance",
            InstanceResourceRefs {
                roles: BTreeMap::from([
                    ("".to_string(), created("role")),
                    ("a".repeat(33), created("role")),
                    ("member role".to_string(), created("role")),
                    ("../role".to_string(), created("role")),
                    ("member:role".to_string(), created("role")),
                ]),
                ..InstanceResourceRefs::default()
            },
        ),
    ]);
    let errors = validate(&ruleset, &ResourceBindingMap::default()).unwrap_err();
    let long_alias = "a".repeat(33);

    for alias in [
        "",
        long_alias.as_str(),
        "member role",
        "../role",
        "member:role",
    ] {
        assert!(errors.contains(&ValidationError::InvalidResourceAlias {
            rule: "study_rule".to_string(),
            alias: alias.to_string(),
        }));
    }
}

fn study_ruleset() -> InteractionRuleSet {
    validation_ruleset(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::CreateRole {
            key: "study_member_role".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        ActionSpec::CreateChannel {
            key: "study_channel".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: ChannelRef::Created(created("study_channel")),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        },
        ActionSpec::UpsertOverwrite {
            channel: ChannelRef::Created(created("study_channel")),
            target: OverwriteTargetSpec::Role(RoleRef::Created(created("study_member_role"))),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
        ActionSpec::GrantRole {
            role: RoleRef::Created(created("study_member_role")),
            target: ActionTarget::Actor,
        },
        ActionSpec::PostPanel {
            key: "study_welcome_panel".to_string(),
            channel: ChannelRef::Created(created("study_channel")),
            content: "환영합니다.".to_string(),
            buttons: vec![ButtonSpec {
                label: "도움말".to_string(),
                route: ButtonRoute::Static {
                    key: "study_help".to_string(),
                },
            }],
        },
        register_action("study_room_instance", full_manifest()),
        ActionSpec::EditResponse {
            content: "스터디룸 '${input.room_name}' 생성 완료!".to_string(),
        },
    ])
}

#[test]
fn full_study_room_run_registers_instance() {
    let ruleset = study_ruleset();
    assert!(validate(&ruleset, &ResourceBindingMap::default()).is_ok());
    let event = event(GuildId(7));
    let plan = interpret(&event, &ruleset, &ResourceBindingMap::default()).unwrap();
    assert!(matches!(
        plan.steps.get(7),
        Some(PlannedAction::RegisterInstance { .. })
    ));
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let instances = InMemoryInstanceStore::new();
    let outcome = block_on(handle_event(
        &event,
        &ruleset,
        &ResourceBindingMap::default(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &instances,
            instance_ids: &SequenceInstanceIdGenerator::new("room", 1),
        },
        "실패",
        &RunningRuleSetIdentity {
            key: "studyroom_demo".to_string(),
            version: InstanceRuleSetVersion::new(1).unwrap(),
        },
    ))
    .unwrap();

    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "스터디룸 'cozy' 생성 완료!".to_string(),
            },
        ]
    );
    assert_eq!(
        mutation.calls(),
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
                content: "환영합니다.".to_string(),
                buttons: vec![PostPanelButtonSpec {
                    label: "도움말".to_string(),
                    route: ResolvedButtonRoute::Static {
                        key: "study_help".to_string(),
                    },
                }],
            },
        ]
    );
    let instance = block_on(instances.get(GuildId(7), &InstanceId::parse("room_001").unwrap()))
        .unwrap()
        .unwrap();
    assert_eq!(instance.resources.roles["member_role"], RoleId(800_000));
    assert_eq!(
        instance.resources.channels["room_channel"],
        ChannelId(800_001)
    );
    assert_eq!(
        instance.resources.messages["welcome_panel"],
        MessageId(800_002)
    );
}

#[test]
fn same_instance_id_is_allowed_in_different_guilds() {
    let instances = InMemoryInstanceStore::new();
    let first = block_on(run(
        &context(GuildId(7)),
        &registration_plan(),
        &AutomationServices {
            mutation: &MockMutationAdapter::new(),
            responder: &MockInteractionResponder::new(),
            instances: &instances,
            instance_ids: &SequenceInstanceIdGenerator::new("room", 1),
        },
    ))
    .unwrap();
    let second = block_on(run(
        &context(GuildId(8)),
        &registration_plan(),
        &AutomationServices {
            mutation: &MockMutationAdapter::new(),
            responder: &MockInteractionResponder::new(),
            instances: &instances,
            instance_ids: &SequenceInstanceIdGenerator::new("room", 1),
        },
    ))
    .unwrap();

    assert_eq!(
        first.last(),
        Some(&CreatedResource::Instance {
            action_index: 3,
            key: "study_room_instance".to_string(),
            id: InstanceId::parse("room_001").unwrap(),
        })
    );
    assert_eq!(first.last(), second.last());
    assert_eq!(
        block_on(instances.list_by_guild(GuildId(7))).unwrap().len(),
        1
    );
    assert_eq!(
        block_on(instances.list_by_guild(GuildId(8))).unwrap().len(),
        1
    );
}

#[test]
fn running_ruleset_v7_is_pinned_on_registered_instance() {
    let ruleset = study_ruleset();
    let event = event(GuildId(7));
    let instances = InMemoryInstanceStore::new();
    let identity = RunningRuleSetIdentity {
        key: "studyroom_demo".to_string(),
        version: InstanceRuleSetVersion::new(7).unwrap(),
    };

    block_on(handle_event(
        &event,
        &ruleset,
        &ResourceBindingMap::default(),
        &AutomationServices {
            mutation: &MockMutationAdapter::new(),
            responder: &MockInteractionResponder::new(),
            instances: &instances,
            instance_ids: &SequenceInstanceIdGenerator::new("room", 1),
        },
        "실패",
        &identity,
    ))
    .unwrap();

    let instance = block_on(instances.get(GuildId(7), &InstanceId::parse("room_001").unwrap()))
        .unwrap()
        .unwrap();
    assert_eq!(instance.ruleset_version, identity.version);
}

#[test]
fn register_instance_action_has_no_ruleset_version() {
    let value = serde_json::to_value(register_action("instance", full_manifest())).unwrap();
    assert!(value.get("ruleset_version").is_none());
}

#[test]
fn non_instance_event_context_has_ruleset_version() {
    let identity = RunningRuleSetIdentity {
        key: "studyroom_demo".to_string(),
        version: InstanceRuleSetVersion::new(7).unwrap(),
    };
    let context = RuntimeContext::from_event(&event(GuildId(7)), &identity);
    assert_eq!(context.ruleset_key, identity.key);
    assert_eq!(context.ruleset_version, identity.version);
}
