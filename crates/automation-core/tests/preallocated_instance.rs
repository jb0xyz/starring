use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use automation_core::adapter::{
    AdapterError, AdapterErrorKind, AutomationServices, PostPanelButtonSpec, ResolvedButtonRoute,
};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{ActionPlan, CreatedResource, PlannedAction, PlannedChannel};
use automation_core::run::run;
use automation_core::validate::{validate_structural, CreatedKind, ValidationError};
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceIdGenerationError,
    InstanceIdGenerator, InstanceKind, InstanceResources, InstanceRuleSetVersion, InstanceStatus,
    InstanceStore, InstanceStoreError,
};
use automation_state::{
    ActionSpec, ButtonRoute, ButtonSpec, ChannelRef, CreatedRef, InstanceRef, InstanceResourceRefs,
    InteractionRule, InteractionRuleSet, PanelSpec, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, MessageId, RoleId, UserId};
use futures::executor::block_on;

fn created(key: &str) -> CreatedRef {
    CreatedRef {
        created: key.to_string(),
    }
}

fn context() -> automation_core::RuntimeContext {
    automation_core::RuntimeContext {
        guild_id: GuildId(7),
        actor: UserId(42),
        ruleset_key: "studyroom_demo".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        inputs: BTreeMap::new(),
        instance: None,
    }
}

fn complete_manifest() -> InstanceResourceRefs {
    InstanceResourceRefs {
        roles: BTreeMap::from([("member_role".to_string(), created("study_member_role"))]),
        channels: BTreeMap::from([("room_channel".to_string(), created("study_channel"))]),
        messages: BTreeMap::from([
            ("welcome_panel".to_string(), created("study_welcome_panel")),
            ("hub_panel".to_string(), created("study_hub_entry")),
        ]),
    }
}

fn complete_plan() -> ActionPlan {
    ActionPlan {
        steps: vec![
            PlannedAction::CreateRole {
                key: "study_member_role".to_string(),
                name: "member".to_string(),
            },
            PlannedAction::CreateChannel {
                key: "study_channel".to_string(),
                name: "study-room".to_string(),
            },
            PlannedAction::PostPanel {
                key: "study_welcome_panel".to_string(),
                channel: PlannedChannel::Created("study_channel".to_string()),
                content: "welcome".to_string(),
                buttons: vec![],
            },
            PlannedAction::PostPanel {
                key: "study_hub_entry".to_string(),
                channel: PlannedChannel::Resolved(ChannelId(99)),
                content: "join".to_string(),
                buttons: vec![ButtonSpec {
                    label: "Join".to_string(),
                    route: ButtonRoute::InstanceAction {
                        instance: InstanceRef::Created(created("study_room_instance")),
                        action: "join".to_string(),
                    },
                }],
            },
            PlannedAction::RegisterInstance {
                key: "study_room_instance".to_string(),
                kind: InstanceKind("study_room".to_string()),
                resources: complete_manifest(),
            },
        ],
    }
}

struct CountingGenerator {
    calls: AtomicUsize,
    next: AtomicUsize,
    fail: bool,
}

impl CountingGenerator {
    fn sequence() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            next: AtomicUsize::new(1),
            fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            next: AtomicUsize::new(1),
            fail: true,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl InstanceIdGenerator for CountingGenerator {
    fn generate(&self) -> Result<InstanceId, InstanceIdGenerationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(InstanceIdGenerationError::Entropy);
        }
        let value = self.next.fetch_add(1, Ordering::SeqCst);
        InstanceId::parse(&format!("room_{value:03}")).map_err(InstanceIdGenerationError::Invalid)
    }
}

#[derive(Default)]
struct CountingStore {
    inner: InMemoryInstanceStore,
    register_calls: AtomicUsize,
}

impl CountingStore {
    fn calls(&self) -> usize {
        self.register_calls.load(Ordering::SeqCst)
    }
}

impl InstanceStore for CountingStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
        self.register_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.register(instance).await
    }

    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
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

    async fn transition_to_deleting(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<(), InstanceStoreError> {
        self.inner
            .transition_to_deleting(guild_id, instance_id)
            .await
    }

    async fn mark_deleted(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<(), InstanceStoreError> {
        self.inner.mark_deleted(guild_id, instance_id).await
    }

    async fn list_deleting(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        self.inner.list_deleting(guild_id).await
    }
}

#[test]
fn planned_id_routes_hub_and_registration_persists_complete_manifest() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let instances = CountingStore::default();
    let instance_ids = CountingGenerator::sequence();
    let created_resources = block_on(run(
        &context(),
        &complete_plan(),
        &AutomationServices {
            mutation: &mutation,
            responder: &responder,
            instances: &instances,
            instance_ids: &instance_ids,
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
    ))
    .unwrap();

    let planned_id = InstanceId::parse("room_001").unwrap();
    assert_eq!(instance_ids.calls(), 1);
    assert_eq!(instances.calls(), 1);
    assert_eq!(
        mutation.calls().last(),
        Some(&MutationCall::PostPanel {
            guild: GuildId(7),
            channel: ChannelId(99),
            content: "join".to_string(),
            buttons: vec![PostPanelButtonSpec {
                label: "Join".to_string(),
                route: ResolvedButtonRoute::InstanceAction {
                    instance_id: planned_id.clone(),
                    action: "join".to_string(),
                },
            }],
        })
    );
    let instance = block_on(instances.get(GuildId(7), &planned_id))
        .unwrap()
        .unwrap();
    assert_eq!(instance.id, planned_id);
    assert_eq!(instance.resources.roles["member_role"], RoleId(800_000));
    assert_eq!(
        instance.resources.channels["room_channel"],
        ChannelId(800_001)
    );
    assert_eq!(
        instance.resources.messages["welcome_panel"].id,
        MessageId(800_002)
    );
    assert_eq!(
        instance.resources.messages["welcome_panel"].channel,
        ChannelId(800_001)
    );
    assert_eq!(
        instance.resources.messages["hub_panel"].id,
        MessageId(800_003)
    );
    assert_eq!(
        instance.resources.messages["hub_panel"].channel,
        ChannelId(99)
    );
    assert_eq!(
        created_resources.created.last(),
        Some(&CreatedResource::Instance {
            action_index: 4,
            key: "study_room_instance".to_string(),
            id: InstanceId::parse("room_001").unwrap(),
        })
    );
}

#[test]
fn allocator_failure_prevents_mutations_and_store_calls() {
    let mutation = MockMutationAdapter::new();
    let instances = CountingStore::default();
    let instance_ids = CountingGenerator::failing();
    let result = block_on(run(
        &context(),
        &complete_plan(),
        &AutomationServices {
            mutation: &mutation,
            responder: &MockInteractionResponder::new(),
            instances: &instances,
            instance_ids: &instance_ids,
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::BadRequest);
    assert_eq!(instance_ids.calls(), 1);
    assert!(mutation.calls().is_empty());
    assert_eq!(instances.calls(), 0);
}

#[test]
fn mutation_failure_after_preallocation_does_not_register() {
    let mutation = MockMutationAdapter::failing(AdapterError::new(
        AdapterErrorKind::Forbidden,
        "create failed",
    ));
    let instances = CountingStore::default();
    let instance_ids = CountingGenerator::sequence();
    let result = block_on(run(
        &context(),
        &complete_plan(),
        &AutomationServices {
            mutation: &mutation,
            responder: &MockInteractionResponder::new(),
            instances: &instances,
            instance_ids: &instance_ids,
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
    assert_eq!(instance_ids.calls(), 1);
    assert_eq!(mutation.calls().len(), 1);
    assert_eq!(instances.calls(), 0);
}

#[test]
fn duplicate_store_id_does_not_reallocate() {
    let instances = CountingStore::default();
    let planned_id = InstanceId::parse("room_001").unwrap();
    let existing = AutomationInstance {
        id: planned_id.clone(),
        guild_id: GuildId(7),
        ruleset_key: "existing".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        kind: InstanceKind("existing".to_string()),
        created_by: UserId(1),
        resources: InstanceResources::default(),
        status: InstanceStatus::Active,
    };
    block_on(instances.register(existing.clone())).unwrap();
    let instance_ids = CountingGenerator::sequence();
    let result = block_on(run(
        &context(),
        &complete_plan(),
        &AutomationServices {
            mutation: &MockMutationAdapter::new(),
            responder: &MockInteractionResponder::new(),
            instances: &instances,
            instance_ids: &instance_ids,
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::BadRequest);
    assert_eq!(instance_ids.calls(), 1);
    assert_eq!(instances.calls(), 2);
    assert_eq!(
        block_on(instances.get(GuildId(7), &planned_id)).unwrap(),
        Some(existing)
    );
}

#[test]
fn duplicate_logical_register_key_allocates_once() {
    let instances = CountingStore::default();
    let instance_ids = CountingGenerator::sequence();
    let register = PlannedAction::RegisterInstance {
        key: "instance".to_string(),
        kind: InstanceKind("study_room".to_string()),
        resources: InstanceResourceRefs::default(),
    };
    let result = block_on(run(
        &context(),
        &ActionPlan {
            steps: vec![register.clone(), register],
        },
        &AutomationServices {
            mutation: &MockMutationAdapter::new(),
            responder: &MockInteractionResponder::new(),
            instances: &instances,
            instance_ids: &instance_ids,
            teardown: &automation_core::MockInstanceTeardownService::new(),
        },
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::BadRequest);
    assert_eq!(instance_ids.calls(), 1);
    assert_eq!(instances.calls(), 2);
}

fn validation_ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "create_panel".to_string(),
            channel: ResourceKey("hub".to_string()),
            content: "create".to_string(),
            buttons: vec![ButtonSpec {
                label: "Create".to_string(),
                route: ButtonRoute::Static {
                    key: "create".to_string(),
                },
            }],
        }],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "create_rule".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "create".to_string(),
            },
            actions,
        }],
    }
}

fn create_role() -> ActionSpec {
    ActionSpec::CreateRole {
        key: "role".to_string(),
        name: "member".to_string(),
    }
}

fn create_channel() -> ActionSpec {
    ActionSpec::CreateChannel {
        key: "channel".to_string(),
        name: "room".to_string(),
    }
}

fn post_panel() -> ActionSpec {
    ActionSpec::PostPanel {
        key: "panel".to_string(),
        channel: ChannelRef::Created(created("channel")),
        content: "welcome".to_string(),
        buttons: vec![],
    }
}

fn register(resources: InstanceResourceRefs) -> ActionSpec {
    ActionSpec::RegisterInstance {
        key: "instance".to_string(),
        kind: InstanceKind("study_room".to_string()),
        resources,
    }
}

fn validation_manifest() -> InstanceResourceRefs {
    InstanceResourceRefs {
        roles: BTreeMap::from([("member_role".to_string(), created("role"))]),
        channels: BTreeMap::from([("room_channel".to_string(), created("channel"))]),
        messages: BTreeMap::from([("welcome_panel".to_string(), created("panel"))]),
    }
}

#[test]
fn complete_instance_footprint_passes_validation() {
    let ruleset = validation_ruleset(vec![
        create_role(),
        create_channel(),
        post_panel(),
        register(validation_manifest()),
    ]);

    assert!(validate_structural(&ruleset).is_ok());
}

#[test]
fn missing_created_resource_from_manifest_fails_validation() {
    let mut manifest = validation_manifest();
    manifest.roles.clear();
    let ruleset = validation_ruleset(vec![
        create_role(),
        create_channel(),
        post_panel(),
        register(manifest),
    ]);

    assert!(validate_structural(&ruleset).unwrap_err().contains(
        &ValidationError::InstanceResourceMissingFromManifest {
            key: "role".to_string(),
            kind: CreatedKind::Role,
        }
    ));
}

#[test]
fn duplicate_manifest_resource_fails_validation() {
    let mut manifest = validation_manifest();
    manifest
        .roles
        .insert("second_role_alias".to_string(), created("role"));
    let ruleset = validation_ruleset(vec![
        create_role(),
        create_channel(),
        post_panel(),
        register(manifest),
    ]);

    assert!(validate_structural(&ruleset).unwrap_err().contains(
        &ValidationError::InstanceResourceDeclaredMultipleTimes {
            key: "role".to_string(),
        }
    ));
}

#[test]
fn resource_produced_after_register_fails_validation() {
    let ruleset = validation_ruleset(vec![
        create_role(),
        register(InstanceResourceRefs {
            roles: BTreeMap::from([("member_role".to_string(), created("role"))]),
            channels: BTreeMap::from([("room_channel".to_string(), created("channel"))]),
            ..InstanceResourceRefs::default()
        }),
        create_channel(),
    ]);

    assert!(validate_structural(&ruleset).unwrap_err().contains(
        &ValidationError::InstanceResourceProducedAfterRegister {
            key: "channel".to_string(),
            kind: CreatedKind::Channel,
        }
    ));
}

#[test]
fn multiple_register_instance_fails_validation() {
    let ruleset = validation_ruleset(vec![
        create_role(),
        register(InstanceResourceRefs {
            roles: BTreeMap::from([("member_role".to_string(), created("role"))]),
            ..InstanceResourceRefs::default()
        }),
        ActionSpec::RegisterInstance {
            key: "second_instance".to_string(),
            kind: InstanceKind("study_room".to_string()),
            resources: InstanceResourceRefs {
                roles: BTreeMap::from([("member_role".to_string(), created("role"))]),
                ..InstanceResourceRefs::default()
            },
        },
    ]);

    assert!(validate_structural(&ruleset).unwrap_err().contains(
        &ValidationError::MultipleRegisterInstance {
            rule: "create_rule".to_string(),
        }
    ));
}
