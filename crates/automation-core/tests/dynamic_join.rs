use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use automation_core::adapter::InteractionResponder;
use automation_core::adapter::{
    AdapterError, AdapterErrorKind, AutomationServices, PostPanelButtonSpec, ResolvedButtonRoute,
};
use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::interpret::interpret;
use automation_core::mock::{
    MockInteractionResponder, MockMutationAdapter, MutationCall, ResponderCall,
};
use automation_core::plan::{
    ActionPlan, ModalPresentation, PlannedAction, PlannedChannel, PlannedRole,
};
use automation_core::run::{handle_event, run, HandleOutcome};
use automation_core::validate::{validate, ValidationError};
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceResources,
    InstanceStatus, InstanceStore, InstanceStoreError, SequenceInstanceIdGenerator,
};
use automation_state::{
    ActionSpec, ActionTarget, ButtonRoute, ButtonSpec, CreatedRef, InstanceRef,
    InstanceResourceRefs, InteractionRule, InteractionRuleSet, PanelSpec, RoleRef, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, MessageId, RoleId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

impl Trace {
    fn record(&self, entry: String) {
        self.0.lock().unwrap().push(entry);
    }
    fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct TracingStore {
    trace: Trace,
    inner: InMemoryInstanceStore,
}

impl InstanceStore for TracingStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
        self.inner.register(instance).await
    }
    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        self.trace.record("instance_store.get".to_string());
        self.inner.get(guild_id, instance_id).await
    }
    async fn list_by_guild(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        self.inner.list_by_guild(guild_id).await
    }
    async fn update_status(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        status: InstanceStatus,
    ) -> Result<(), InstanceStoreError> {
        self.inner
            .update_status(guild_id, instance_id, status)
            .await
    }
}

struct TracingResponder {
    trace: Trace,
}

impl InteractionResponder for TracingResponder {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError> {
        self.trace.record(format!("respond_ephemeral:{content}"));
        Ok(())
    }
    async fn open_modal(&self, _modal: &ModalPresentation) -> Result<(), AdapterError> {
        self.trace.record("open_modal".to_string());
        Ok(())
    }
    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        self.trace.record("defer_ephemeral".to_string());
        Ok(())
    }
    async fn edit_response(&self, content: String) -> Result<(), AdapterError> {
        self.trace.record(format!("edit_response:{content}"));
        Ok(())
    }
}

struct FailingDeferResponder {
    trace: Trace,
}

impl InteractionResponder for FailingDeferResponder {
    async fn respond_ephemeral(&self, _content: String) -> Result<(), AdapterError> {
        Ok(())
    }
    async fn open_modal(&self, _modal: &ModalPresentation) -> Result<(), AdapterError> {
        Ok(())
    }
    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        self.trace.record("defer_ephemeral".to_string());
        Err(AdapterError::new(AdapterErrorKind::Unknown, "defer failed"))
    }
    async fn edit_response(&self, content: String) -> Result<(), AdapterError> {
        self.trace.record(format!("edit_response:{content}"));
        Ok(())
    }
}

fn instance_id() -> InstanceId {
    InstanceId::parse("room_001").unwrap()
}

fn instance(
    guild_id: GuildId,
    status: InstanceStatus,
    ruleset_key: &str,
    resources: InstanceResources,
) -> AutomationInstance {
    AutomationInstance {
        id: instance_id(),
        guild_id,
        ruleset_key: ruleset_key.to_string(),
        kind: InstanceKind("study_room".to_string()),
        created_by: UserId(1),
        resources,
        status,
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
                ActionSpec::GrantRole {
                    role,
                    target: ActionTarget::Actor,
                },
                ActionSpec::RespondEphemeral {
                    content: "joined".to_string(),
                },
            ],
        }],
    }
}

fn deferred_join_ruleset(role: RoleRef) -> InteractionRuleSet {
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

fn rejected(
    stored: Option<AutomationInstance>,
    event_guild: GuildId,
    ruleset_key: &str,
) -> (AdapterError, Vec<MutationCall>, Vec<ResponderCall>) {
    let instances = InMemoryInstanceStore::new();
    if let Some(instance) = stored {
        block_on(instances.register(instance)).unwrap();
    }
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let generator = SequenceInstanceIdGenerator::new("unused", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &generator,
    };
    let error = block_on(handle_event(
        &event(event_guild, "join"),
        &join_ruleset(instance_role(InstanceRef::Event, "member_role")),
        &ResourceBindingMap::default(),
        &services,
        "",
        ruleset_key,
    ))
    .unwrap_err();
    (error, mutation.calls(), responder.calls())
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
        plan.steps.first(),
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
fn active_instance_grants_resolved_role_and_responds() {
    let instances = InMemoryInstanceStore::new();
    let stored = instance(
        GuildId(7),
        InstanceStatus::Active,
        "studyroom",
        InstanceResources {
            roles: BTreeMap::from([("member_role".to_string(), RoleId(55))]),
            ..InstanceResources::default()
        },
    );
    block_on(instances.register(stored)).unwrap();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let generator = SequenceInstanceIdGenerator::new("unused", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &generator,
    };
    let outcome = block_on(handle_event(
        &event(GuildId(7), "join"),
        &join_ruleset(instance_role(InstanceRef::Event, "member_role")),
        &ResourceBindingMap::default(),
        &services,
        "",
        "studyroom",
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        mutation.calls(),
        vec![MutationCall::GrantRole {
            guild: GuildId(7),
            member: UserId(42),
            role: RoleId(55),
        }]
    );
    assert_eq!(
        responder.calls(),
        vec![ResponderCall::RespondEphemeral {
            content: "joined".to_string(),
        }]
    );
}

#[test]
fn missing_instance_is_rejected() {
    let (error, mutations, responses) = rejected(None, GuildId(7), "studyroom");
    assert_eq!(error.kind, AdapterErrorKind::NotFound);
    assert!(error.message.contains("InstanceNotFound"));
    assert!(mutations.is_empty());
    assert!(responses.is_empty());
}

#[test]
fn disabled_instance_is_rejected() {
    let stored = instance(
        GuildId(7),
        InstanceStatus::Disabled,
        "studyroom",
        InstanceResources::default(),
    );
    let (error, mutations, responses) = rejected(Some(stored), GuildId(7), "studyroom");
    assert_eq!(error.kind, AdapterErrorKind::Forbidden);
    assert!(error.message.contains("InstanceInactive"));
    assert!(mutations.is_empty());
    assert!(responses.is_empty());
}

#[test]
fn deleted_instance_is_rejected() {
    let stored = instance(
        GuildId(7),
        InstanceStatus::Deleted,
        "studyroom",
        InstanceResources::default(),
    );
    let (error, mutations, responses) = rejected(Some(stored), GuildId(7), "studyroom");
    assert_eq!(error.kind, AdapterErrorKind::Forbidden);
    assert!(error.message.contains("InstanceInactive"));
    assert!(mutations.is_empty());
    assert!(responses.is_empty());
}

#[test]
fn ruleset_mismatch_is_rejected() {
    let stored = instance(
        GuildId(7),
        InstanceStatus::Active,
        "other",
        InstanceResources::default(),
    );
    let (error, mutations, responses) = rejected(Some(stored), GuildId(7), "studyroom");
    assert_eq!(error.kind, AdapterErrorKind::Forbidden);
    assert!(error.message.contains("InstanceRulesetMismatch"));
    assert!(mutations.is_empty());
    assert!(responses.is_empty());
}

#[test]
fn same_instance_id_in_other_guild_is_not_visible() {
    let stored = instance(
        GuildId(8),
        InstanceStatus::Active,
        "studyroom",
        InstanceResources::default(),
    );
    let (error, mutations, responses) = rejected(Some(stored), GuildId(7), "studyroom");
    assert_eq!(error.kind, AdapterErrorKind::NotFound);
    assert!(error.message.contains("InstanceNotFound"));
    assert!(mutations.is_empty());
    assert!(responses.is_empty());
}

#[test]
fn missing_role_alias_is_rejected() {
    let stored = instance(
        GuildId(7),
        InstanceStatus::Active,
        "studyroom",
        InstanceResources::default(),
    );
    let (error, mutations, responses) = rejected(Some(stored), GuildId(7), "studyroom");
    assert_eq!(error.kind, AdapterErrorKind::BadRequest);
    assert!(error.message.contains("InstanceResourceNotFound"));
    assert!(mutations.is_empty());
    assert!(responses.is_empty());
}

#[test]
fn channel_and_message_aliases_do_not_resolve_as_roles() {
    let stored = instance(
        GuildId(7),
        InstanceStatus::Active,
        "studyroom",
        InstanceResources {
            channels: BTreeMap::from([("member_role".to_string(), ChannelId(55))]),
            messages: BTreeMap::from([("member_role".to_string(), MessageId(56))]),
            ..InstanceResources::default()
        },
    );
    let (error, mutations, responses) = rejected(Some(stored), GuildId(7), "studyroom");
    assert_eq!(error.kind, AdapterErrorKind::BadRequest);
    assert!(error.message.contains("InstanceResourceNotFound"));
    assert!(mutations.is_empty());
    assert!(responses.is_empty());
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
        "studyroom",
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

#[test]
fn deferred_join_defers_before_resolution_and_grants() {
    let instances = InMemoryInstanceStore::new();
    let stored = instance(
        GuildId(7),
        InstanceStatus::Active,
        "studyroom",
        InstanceResources {
            roles: BTreeMap::from([("member_role".to_string(), RoleId(55))]),
            ..InstanceResources::default()
        },
    );
    block_on(instances.register(stored)).unwrap();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let generator = SequenceInstanceIdGenerator::new("unused", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &generator,
    };
    let outcome = block_on(handle_event(
        &event(GuildId(7), "join"),
        &deferred_join_ruleset(instance_role(InstanceRef::Event, "member_role")),
        &ResourceBindingMap::default(),
        &services,
        "could not join",
        "studyroom",
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        mutation.calls(),
        vec![MutationCall::GrantRole {
            guild: GuildId(7),
            member: UserId(42),
            role: RoleId(55),
        }]
    );
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "joined".to_string(),
            },
        ]
    );
    assert_eq!(
        responder
            .calls()
            .iter()
            .filter(|call| matches!(call, ResponderCall::DeferEphemeral))
            .count(),
        1
    );
}

#[test]
fn deferred_join_missing_instance_traces_defer_then_lookup_then_edit() {
    let trace = Trace::default();
    let instances = TracingStore {
        trace: trace.clone(),
        inner: InMemoryInstanceStore::new(),
    };
    let mutation = MockMutationAdapter::new();
    let responder = TracingResponder {
        trace: trace.clone(),
    };
    let generator = SequenceInstanceIdGenerator::new("unused", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &generator,
    };
    let error = block_on(handle_event(
        &event(GuildId(7), "join"),
        &deferred_join_ruleset(instance_role(InstanceRef::Event, "member_role")),
        &ResourceBindingMap::default(),
        &services,
        "could not join",
        "studyroom",
    ))
    .unwrap_err();
    assert_eq!(error.kind, AdapterErrorKind::NotFound);
    assert!(error.message.contains("InstanceNotFound"));
    assert_eq!(
        trace.entries(),
        vec![
            "defer_ephemeral".to_string(),
            "instance_store.get".to_string(),
            "edit_response:could not join".to_string(),
        ]
    );
    assert!(mutation.calls().is_empty());
}

#[test]
fn deferred_join_defer_failure_skips_lookup_and_edit() {
    let trace = Trace::default();
    let instances = TracingStore {
        trace: trace.clone(),
        inner: InMemoryInstanceStore::new(),
    };
    let mutation = MockMutationAdapter::new();
    let responder = FailingDeferResponder {
        trace: trace.clone(),
    };
    let generator = SequenceInstanceIdGenerator::new("unused", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &generator,
    };
    let error = block_on(handle_event(
        &event(GuildId(7), "join"),
        &deferred_join_ruleset(instance_role(InstanceRef::Event, "member_role")),
        &ResourceBindingMap::default(),
        &services,
        "could not join",
        "studyroom",
    ))
    .unwrap_err();
    assert_eq!(error.kind, AdapterErrorKind::Unknown);
    assert_eq!(trace.entries(), vec!["defer_ephemeral".to_string()]);
    assert!(mutation.calls().is_empty());
}
