use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use automation_core::{
    AutomationServices, EventKind, HandleOutcome, MockMutationAdapter, RuntimeEvent,
};
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceRegistrarV1,
    InstanceResources, InstanceRuleSetVersion, InstanceStatus, InstanceStore, InstanceStoreError,
    SequenceInstanceIdGenerator,
};
use automation_ruleset::{
    InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetActivation, RuleSetKey,
    RuleSetStore, RuleSetStoreError, RuleSetVersion, RuleSetVersionId,
};
use automation_ruleset_dispatch::{
    dispatch_instance_action, dispatch_instance_action_with_resolver_v1, DispatchError,
    FailureResponseOutcome, GuildRoleSnapshot, GuildRoleSnapshotProvider,
    PinnedInstanceResolverErrorV1, PinnedInstanceResolverV1, ResolvedPinnedInstanceV1,
    SnapshotError,
};
use automation_state::{
    ActionSpec, ActionTarget, InstanceRef, InteractionRule, InteractionRuleSet, RoleRef,
    TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions, RoleId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

const GUILD: GuildId = GuildId(7);
const ACTOR: UserId = UserId(42);
const MEMBER_ROLE: RoleId = RoleId(500);

type Trace = Arc<Mutex<Vec<&'static str>>>;

struct TracingResponder {
    trace: Trace,
    fail_defer: bool,
    fail_edit: bool,
}

impl automation_core::InteractionResponder for TracingResponder {
    async fn respond_ephemeral(
        &self,
        _content: String,
    ) -> Result<(), automation_core::AdapterError> {
        Ok(())
    }

    async fn defer_ephemeral(&self) -> Result<(), automation_core::AdapterError> {
        self.trace.lock().unwrap().push("defer");
        if self.fail_defer {
            return Err(automation_core::AdapterError::new(
                automation_core::AdapterErrorKind::Network,
                "defer failed",
            ));
        }
        Ok(())
    }

    async fn edit_response(&self, _content: String) -> Result<(), automation_core::AdapterError> {
        self.trace.lock().unwrap().push("edit");
        if self.fail_edit {
            return Err(automation_core::AdapterError::new(
                automation_core::AdapterErrorKind::Network,
                "edit failed",
            ));
        }
        Ok(())
    }
}

struct TracingInstances {
    inner: InMemoryInstanceStore,
    trace: Trace,
    fail: Option<InstanceStoreError>,
}

impl InstanceStore for TracingInstances {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
        self.inner.register(instance).await
    }

    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        self.trace.lock().unwrap().push("instance.get");
        if let Some(error) = &self.fail {
            return Err(error.clone());
        }
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

impl InstanceRegistrarV1 for TracingInstances {
    async fn register_instance_v1(
        &self,
        instance: AutomationInstance,
    ) -> Result<(), InstanceStoreError> {
        InstanceStore::register(self, instance).await
    }
}

struct TracingRulesets {
    inner: InMemoryRuleSetStore,
    trace: Trace,
}

impl RuleSetStore for TracingRulesets {
    async fn publish(
        &self,
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError> {
        self.inner.publish(request).await
    }

    async fn get_version(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        self.trace.lock().unwrap().push("ruleset.get_version");
        self.inner.get_version(guild_id, key, version).await
    }

    async fn list_versions(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError> {
        self.inner.list_versions(guild_id, key).await
    }

    async fn activate(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError> {
        self.inner.activate(guild_id, key, version).await
    }

    async fn active(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        self.inner.active(guild_id, key).await
    }
}

struct StubSnapshot {
    trace: Trace,
    result: Result<GuildRoleSnapshot, SnapshotError>,
}

struct CountingPinnedResolver {
    calls: AtomicUsize,
    resolved: ResolvedPinnedInstanceV1,
}

impl PinnedInstanceResolverV1 for CountingPinnedResolver {
    async fn resolve_pinned_instance_v1(
        &self,
        _guild_id: GuildId,
        _instance_id: &InstanceId,
    ) -> Result<ResolvedPinnedInstanceV1, PinnedInstanceResolverErrorV1> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.resolved.clone())
    }
}

impl GuildRoleSnapshotProvider for StubSnapshot {
    async fn snapshot(&self, _guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError> {
        self.trace.lock().unwrap().push("snapshot");
        self.result.clone()
    }
}

fn key() -> RuleSetKey {
    RuleSetKey::parse("studyroom_demo").unwrap()
}

fn join_rule(tag: &str) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "join".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::GrantRole {
                    role: RoleRef::Instance {
                        instance: InstanceRef::Event,
                        alias: "member_role".to_string(),
                    },
                    target: ActionTarget::Actor,
                },
                ActionSpec::EditResponse {
                    content: format!("joined {tag}"),
                },
            ],
        }],
    }
}

fn admin_snapshot() -> GuildRoleSnapshot {
    let mut roles = BTreeMap::new();
    roles.insert(RoleId(GUILD.0), Permissions::ADMINISTRATOR);
    GuildRoleSnapshot {
        roles,
        bot_role_ids: BTreeSet::new(),
    }
}

fn publish(store: &InMemoryRuleSetStore, def: InteractionRuleSet) -> RuleSetVersionId {
    let outcome = block_on(store.publish(PublishRuleSetRequest {
        guild_id: GUILD,
        ruleset_key: key(),
        definition: def,
        created_by: UserId(1),
    }))
    .unwrap();
    match outcome {
        PublishOutcome::Created(version) | PublishOutcome::Reused(version) => version.version,
    }
}

fn instance(id: &str, pin: u32, status: InstanceStatus) -> AutomationInstance {
    let mut resources = InstanceResources::default();
    resources
        .roles
        .insert("member_role".to_string(), MEMBER_ROLE);
    AutomationInstance {
        id: InstanceId::parse(id).unwrap(),
        guild_id: GUILD,
        ruleset_key: "studyroom_demo".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(pin).unwrap(),
        kind: InstanceKind("study_room".to_string()),
        created_by: ACTOR,
        resources,
        status,
    }
}

fn join_event(id: &str) -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GUILD,
        actor: ACTOR,
        kind: EventKind::InstanceAction {
            instance_id: InstanceId::parse(id).unwrap(),
            action: "join".to_string(),
        },
    }
}

struct Fixture {
    trace: Trace,
    instances: TracingInstances,
    rulesets: TracingRulesets,
    mutation: MockMutationAdapter,
    ids: SequenceInstanceIdGenerator,
    responder: TracingResponder,
    teardown: automation_core::MockInstanceTeardownService,
}

fn fixture(
    fail_defer: bool,
    fail_edit: bool,
    instance_fail: Option<InstanceStoreError>,
) -> Fixture {
    let trace: Trace = Arc::new(Mutex::new(Vec::new()));
    Fixture {
        trace: trace.clone(),
        instances: TracingInstances {
            inner: InMemoryInstanceStore::new(),
            trace: trace.clone(),
            fail: instance_fail,
        },
        rulesets: TracingRulesets {
            inner: InMemoryRuleSetStore::default(),
            trace: trace.clone(),
        },
        mutation: MockMutationAdapter::new(),
        ids: SequenceInstanceIdGenerator::new("room", 1),
        responder: TracingResponder {
            trace,
            fail_defer,
            fail_edit,
        },
        teardown: automation_core::MockInstanceTeardownService::new(),
    }
}

fn services(
    fixture: &Fixture,
) -> AutomationServices<
    '_,
    MockMutationAdapter,
    TracingResponder,
    TracingInstances,
    SequenceInstanceIdGenerator,
    automation_core::MockInstanceTeardownService,
> {
    AutomationServices {
        mutation: &fixture.mutation,
        responder: &fixture.responder,
        instances: &fixture.instances,
        instance_ids: &fixture.ids,
        teardown: &fixture.teardown,
    }
}

#[test]
fn pinned_v1_runs_while_active_is_v2() {
    let fixture = fixture(false, false, None);
    let v1 = publish(&fixture.rulesets.inner, join_rule("v1"));
    let v2 = publish(&fixture.rulesets.inner, join_rule("v2"));
    block_on(fixture.rulesets.inner.activate(GUILD, &key(), v2)).unwrap();
    block_on(fixture.instances.inner.register(instance(
        "room_a",
        v1.get(),
        InstanceStatus::Active,
    )))
    .unwrap();
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let outcome = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &fixture.rulesets,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(fixture.mutation.calls().len(), 1);
    assert_eq!(
        fixture.trace.lock().unwrap()[..4],
        ["defer", "instance.get", "ruleset.get_version", "snapshot"]
    );
    assert!(fixture.trace.lock().unwrap().contains(&"edit"));
    assert_eq!(
        fixture
            .trace
            .lock()
            .unwrap()
            .iter()
            .filter(|entry| **entry == "defer")
            .count(),
        1
    );
}

#[test]
fn narrow_dispatch_resolves_the_exact_pin_once() {
    let fixture = fixture(false, false, None);
    let version = publish(&fixture.rulesets.inner, join_rule("v1"));
    let artifact = block_on(fixture.rulesets.inner.get_version(GUILD, &key(), version))
        .unwrap()
        .unwrap();
    let resolver = CountingPinnedResolver {
        calls: AtomicUsize::new(0),
        resolved: ResolvedPinnedInstanceV1 {
            instance: instance("room_a", version.get(), InstanceStatus::Active),
            artifact,
        },
    };
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let outcome = block_on(dispatch_instance_action_with_resolver_v1(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        "studyroom_demo",
        &resolver,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.trace.lock().unwrap()[..2], ["defer", "snapshot"]);
}

#[test]
fn narrow_dispatch_binds_the_admitted_ruleset_key() {
    let fixture = fixture(false, false, None);
    let version = publish(&fixture.rulesets.inner, join_rule("v1"));
    let artifact = block_on(fixture.rulesets.inner.get_version(GUILD, &key(), version))
        .unwrap()
        .unwrap();
    let resolver = CountingPinnedResolver {
        calls: AtomicUsize::new(0),
        resolved: ResolvedPinnedInstanceV1 {
            instance: instance("room_a", version.get(), InstanceStatus::Active),
            artifact,
        },
    };
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action_with_resolver_v1(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        "different_route",
        &resolver,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert_eq!(failure.cause, DispatchError::PinnedVersionMissing);
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert!(!fixture.trace.lock().unwrap().contains(&"snapshot"));
    assert!(fixture.mutation.calls().is_empty());
}

#[test]
fn narrow_dispatch_binds_the_requested_instance_id() {
    let fixture = fixture(false, false, None);
    let version = publish(&fixture.rulesets.inner, join_rule("v1"));
    let artifact = block_on(fixture.rulesets.inner.get_version(GUILD, &key(), version))
        .unwrap()
        .unwrap();
    let resolver = CountingPinnedResolver {
        calls: AtomicUsize::new(0),
        resolved: ResolvedPinnedInstanceV1 {
            instance: instance("room_b", version.get(), InstanceStatus::Active),
            artifact,
        },
    };
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action_with_resolver_v1(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        "studyroom_demo",
        &resolver,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert_eq!(failure.cause, DispatchError::PinnedVersionMissing);
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert!(!fixture.trace.lock().unwrap().contains(&"snapshot"));
    assert!(fixture.mutation.calls().is_empty());
}

#[test]
fn defer_failure_stops_before_any_lookup() {
    let fixture = fixture(true, false, None);
    publish(&fixture.rulesets.inner, join_rule("v1"));
    block_on(
        fixture
            .instances
            .inner
            .register(instance("room_a", 1, InstanceStatus::Active)),
    )
    .unwrap();
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &fixture.rulesets,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert!(matches!(failure.cause, DispatchError::DeferFailed(_)));
    assert_eq!(
        failure.failure_response,
        FailureResponseOutcome::NotAttempted
    );
    assert_eq!(*fixture.trace.lock().unwrap(), vec!["defer"]);
    assert!(fixture.mutation.calls().is_empty());
}

#[test]
fn snapshot_failure_is_fail_closed() {
    let fixture = fixture(false, false, None);
    publish(&fixture.rulesets.inner, join_rule("v1"));
    block_on(
        fixture
            .instances
            .inner
            .register(instance("room_a", 1, InstanceStatus::Active)),
    )
    .unwrap();
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Err(SnapshotError::new("discord down")),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &fixture.rulesets,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert!(matches!(failure.cause, DispatchError::SnapshotFailed(_)));
    assert_eq!(failure.failure_response, FailureResponseOutcome::Sent);
    assert!(fixture.mutation.calls().is_empty());
}

#[test]
fn missing_pinned_version_has_no_active_fallback() {
    let fixture = fixture(false, false, None);
    let v1 = publish(&fixture.rulesets.inner, join_rule("v1"));
    block_on(fixture.rulesets.inner.activate(GUILD, &key(), v1)).unwrap();
    block_on(
        fixture
            .instances
            .inner
            .register(instance("room_a", 99, InstanceStatus::Active)),
    )
    .unwrap();
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &fixture.rulesets,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert_eq!(failure.cause, DispatchError::PinnedVersionMissing);
    assert!(!fixture.trace.lock().unwrap().contains(&"snapshot"));
    assert!(fixture.mutation.calls().is_empty());
}

#[test]
fn unknown_action_is_no_matching_rule() {
    let fixture = fixture(false, false, None);
    let v1 = publish(&fixture.rulesets.inner, join_rule("v1"));
    block_on(fixture.instances.inner.register(instance(
        "room_a",
        v1.get(),
        InstanceStatus::Active,
    )))
    .unwrap();
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let event = RuntimeEvent {
        guild_id: GUILD,
        actor: ACTOR,
        kind: EventKind::InstanceAction {
            instance_id: InstanceId::parse("room_a").unwrap(),
            action: "leave".to_string(),
        },
    };
    let failure = block_on(dispatch_instance_action(
        &event,
        &InstanceId::parse("room_a").unwrap(),
        "leave",
        &fixture.rulesets,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert_eq!(
        failure.cause,
        DispatchError::NoMatchingRule {
            action: "leave".to_string(),
        }
    );
    assert!(fixture.mutation.calls().is_empty());
}

#[test]
fn inactive_instance_is_rejected() {
    for status in [
        InstanceStatus::Deleting,
        InstanceStatus::Disabled,
        InstanceStatus::Deleted,
    ] {
        let fixture = fixture(false, false, None);
        let v1 = publish(&fixture.rulesets.inner, join_rule("v1"));
        block_on(
            fixture
                .instances
                .inner
                .register(instance("room_a", v1.get(), status)),
        )
        .unwrap();
        let snapshot = StubSnapshot {
            trace: fixture.trace.clone(),
            result: Ok(admin_snapshot()),
        };
        let failure = block_on(dispatch_instance_action(
            &join_event("room_a"),
            &InstanceId::parse("room_a").unwrap(),
            "join",
            &fixture.rulesets,
            &snapshot,
            &ResourceBindingMap::default(),
            &services(&fixture),
            "failed",
        ))
        .unwrap_err();
        assert_eq!(failure.cause, DispatchError::InstanceInactive(status));
        assert!(fixture.mutation.calls().is_empty());
    }
}

#[test]
fn inactive_instance_precedes_missing_pinned_version() {
    let fixture = fixture(false, false, None);
    block_on(
        fixture
            .instances
            .inner
            .register(instance("room_a", 99, InstanceStatus::Disabled)),
    )
    .unwrap();
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &fixture.rulesets,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert_eq!(
        failure.cause,
        DispatchError::InstanceInactive(InstanceStatus::Disabled)
    );
    assert!(!fixture
        .trace
        .lock()
        .unwrap()
        .contains(&"ruleset.get_version"));
    assert!(!fixture.trace.lock().unwrap().contains(&"snapshot"));
    assert!(fixture.mutation.calls().is_empty());
}

#[test]
fn privilege_escalation_in_fresh_snapshot_blocks() {
    let fixture = fixture(false, false, None);
    let definition = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "join".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::GrantRole {
                    role: RoleRef::Existing(ResourceKey("admin".to_string())),
                    target: ActionTarget::Actor,
                },
                ActionSpec::EditResponse {
                    content: "joined".to_string(),
                },
            ],
        }],
    };
    let v1 = publish(&fixture.rulesets.inner, definition);
    block_on(fixture.instances.inner.register(instance(
        "room_a",
        v1.get(),
        InstanceStatus::Active,
    )))
    .unwrap();

    let mut bindings = ResourceBindingMap::default();
    bindings
        .role_bindings
        .insert(ResourceKey("admin".to_string()), RoleId(10));
    let mut roles = BTreeMap::new();
    roles.insert(RoleId(GUILD.0), Permissions::ADMINISTRATOR);
    roles.insert(RoleId(10), Permissions::ADMINISTRATOR);
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(GuildRoleSnapshot {
            roles,
            bot_role_ids: BTreeSet::new(),
        }),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &fixture.rulesets,
        &snapshot,
        &bindings,
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert!(matches!(failure.cause, DispatchError::NotReady(_)));
    assert!(fixture.mutation.calls().is_empty());
}

#[test]
fn instance_lookup_failure_stops_before_version() {
    let fixture = fixture(
        false,
        false,
        Some(InstanceStoreError::Backend("db down".to_string())),
    );
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &fixture.rulesets,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert!(matches!(failure.cause, DispatchError::InstanceLookup(_)));
    assert!(!fixture
        .trace
        .lock()
        .unwrap()
        .contains(&"ruleset.get_version"));
    assert!(!fixture.trace.lock().unwrap().contains(&"snapshot"));
}

#[test]
fn failure_edit_failure_preserves_primary_cause() {
    let fixture = fixture(false, true, None);
    let v1 = publish(&fixture.rulesets.inner, join_rule("v1"));
    block_on(fixture.rulesets.inner.activate(GUILD, &key(), v1)).unwrap();
    block_on(
        fixture
            .instances
            .inner
            .register(instance("room_a", 99, InstanceStatus::Active)),
    )
    .unwrap();
    let snapshot = StubSnapshot {
        trace: fixture.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &fixture.rulesets,
        &snapshot,
        &ResourceBindingMap::default(),
        &services(&fixture),
        "failed",
    ))
    .unwrap_err();
    assert_eq!(failure.cause, DispatchError::PinnedVersionMissing);
    assert!(matches!(
        failure.failure_response,
        FailureResponseOutcome::Failed(_)
    ));
}
