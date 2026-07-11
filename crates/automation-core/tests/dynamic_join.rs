use automation_core::adapter::{AutomationServices, PostPanelButtonSpec, ResolvedButtonRoute};
use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::interpret::interpret;
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{ActionPlan, PlannedAction, PlannedChannel, PlannedRole};
use automation_core::run::run;
use automation_core::validate::{validate, ValidationError};
use automation_instance::{
    InMemoryInstanceStore, InstanceId, InstanceKind, InstanceRuleSetVersion,
    SequenceInstanceIdGenerator,
};
use automation_state::{
    ActionSpec, ActionTarget, ButtonRoute, ButtonSpec, CreatedRef, InstanceRef,
    InstanceResourceRefs, InteractionRule, InteractionRuleSet, PanelSpec, RoleRef, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn instance_id() -> InstanceId {
    InstanceId::parse("room_001").unwrap()
}

fn identity(key: &str) -> automation_core::RunningRuleSetIdentity {
    automation_core::RunningRuleSetIdentity {
        key: key.to_string(),
        version: InstanceRuleSetVersion::new(1).unwrap(),
    }
}

fn event(guild_id: GuildId, action: &str) -> RuntimeEvent {
    RuntimeEvent {
        guild_id,
        actor: UserId(42),
        kind: EventKind::InstanceAction {
            instance_id: instance_id(),
            action: action.to_string(),
        },
    }
}

fn instance_role(instance: InstanceRef, alias: &str) -> RoleRef {
    RoleRef::Instance {
        instance,
        alias: alias.to_string(),
    }
}

fn join_ruleset(role: RoleRef) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "join_rule".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::GrantRole {
                    role,
                    target: ActionTarget::Actor,
                },
                ActionSpec::EditResponse {
                    content: "joined".to_string(),
                },
            ],
        }],
    }
}

#[test]
fn instance_action_matches_configured_trigger_and_role() {
    let ruleset = join_ruleset(instance_role(InstanceRef::Event, "member_role"));
    let plan = interpret(
        &event(GuildId(7), "join"),
        &ruleset,
        &ResourceBindingMap::default(),
    )
    .unwrap();
    assert!(matches!(
        plan.steps.get(1),
        Some(PlannedAction::GrantRole {
            role: PlannedRole::Instance { alias },
            target: UserId(42),
        }) if alias == "member_role"
    ));
    assert!(interpret(
        &event(GuildId(7), "leave"),
        &ruleset,
        &ResourceBindingMap::default(),
    )
    .is_none());
}

#[test]
fn static_button_route_still_validates_and_interprets() {
    let ruleset = InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "panel".to_string(),
            channel: ResourceKey("channel".to_string()),
            content: "content".to_string(),
            buttons: vec![ButtonSpec {
                label: "Help".to_string(),
                route: ButtonRoute::Static {
                    key: "help".to_string(),
                },
            }],
        }],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "help_rule".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "help".to_string(),
            },
            actions: vec![ActionSpec::RespondEphemeral {
                content: "help".to_string(),
            }],
        }],
    };
    assert!(validate(&ruleset, &ResourceBindingMap::default()).is_ok());
    let button_event = RuntimeEvent {
        guild_id: GuildId(7),
        actor: UserId(42),
        kind: EventKind::ButtonClick {
            component: "help".to_string(),
        },
    };
    assert!(interpret(&button_event, &ruleset, &ResourceBindingMap::default()).is_some());
}

#[test]
fn valid_instance_role_ref_passes_validation() {
    let ruleset = join_ruleset(instance_role(InstanceRef::Event, "member_role"));
    assert!(validate(&ruleset, &ResourceBindingMap::default()).is_ok());
}

#[test]
fn created_instance_role_ref_fails_validation() {
    let ruleset = join_ruleset(instance_role(
        InstanceRef::Created(CreatedRef {
            created: "room".to_string(),
        }),
        "member_role",
    ));
    let errors = validate(&ruleset, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::InstanceRoleMustUseEvent {
        rule: "join_rule".to_string(),
    }));
}

#[test]
fn instance_role_ref_outside_instance_rule_fails_validation() {
    let mut ruleset = join_ruleset(instance_role(InstanceRef::Event, "member_role"));
    ruleset.panels.push(PanelSpec {
        key: "panel".to_string(),
        channel: ResourceKey("channel".to_string()),
        content: "content".to_string(),
        buttons: vec![ButtonSpec {
            label: "Join".to_string(),
            route: ButtonRoute::Static {
                key: "join".to_string(),
            },
        }],
    });
    ruleset.rules[0].trigger = TriggerSpec::ButtonClick {
        component: "join".to_string(),
    };
    let errors = validate(&ruleset, &ResourceBindingMap::default()).unwrap_err();
    assert!(
        errors.contains(&ValidationError::InstanceRoleOutsideInstanceRule {
            rule: "join_rule".to_string(),
        })
    );
}

#[test]
fn invalid_instance_role_alias_fails_validation() {
    let ruleset = join_ruleset(instance_role(InstanceRef::Event, "bad alias"));
    let errors = validate(&ruleset, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::InvalidResourceAlias {
        rule: "join_rule".to_string(),
        alias: "bad alias".to_string(),
    }));
}

#[test]
fn created_instance_button_route_resolves_before_posting() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let instances = InMemoryInstanceStore::new();
    let generator = SequenceInstanceIdGenerator::new("room", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &generator,
    };
    let context = automation_core::RuntimeContext::from_event(
        &RuntimeEvent {
            guild_id: GuildId(7),
            actor: UserId(42),
            kind: EventKind::ButtonClick {
                component: "create".to_string(),
            },
        },
        &identity("studyroom"),
    );
    let plan = ActionPlan {
        steps: vec![
            PlannedAction::RegisterInstance {
                key: "study_room_instance".to_string(),
                kind: InstanceKind("study_room".to_string()),
                resources: InstanceResourceRefs::default(),
            },
            PlannedAction::PostPanel {
                key: "hub_entry".to_string(),
                channel: PlannedChannel::Resolved(ChannelId(99)),
                content: "join".to_string(),
                buttons: vec![ButtonSpec {
                    label: "Join".to_string(),
                    route: ButtonRoute::InstanceAction {
                        instance: InstanceRef::Created(CreatedRef {
                            created: "study_room_instance".to_string(),
                        }),
                        action: "join".to_string(),
                    },
                }],
            },
        ],
    };
    block_on(run(&context, &plan, &services)).unwrap();
    assert_eq!(
        mutation.calls(),
        vec![MutationCall::PostPanel {
            guild: GuildId(7),
            channel: ChannelId(99),
            content: "join".to_string(),
            buttons: vec![PostPanelButtonSpec {
                label: "Join".to_string(),
                route: ResolvedButtonRoute::InstanceAction {
                    instance_id: instance_id(),
                    action: "join".to_string(),
                },
            }],
        }]
    );
}
