# Pinned Version Dispatch (18c-3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `AutomationInstance.ruleset_version` an executed semantic — every `InstanceAction` runs against the instance's pinned RuleSet version, re-validated against the current Discord environment, fail-closed — while `ButtonClick`/`ModalSubmit` keep using the boot-hydrated active RuleSet.

**Architecture:** A new pure crate `automation-ruleset-dispatch` owns the entire `InstanceAction` path (defer → load instance → load pinned version → fresh Discord snapshot → readiness → interpret → run). `automation-core::handle_event` is reduced to static events and rejects `InstanceAction` with a typed error. The edge (`automation-runtime`) routes by event kind and provides a twilight snapshot provider.

**Tech Stack:** Rust 2021, native `async fn` in trait + `#[allow(async_fn_in_trait)]`, generic static dispatch, `futures::executor::block_on` tests, reuse of `automation-core` (`interpret`/`run`/`from_event`), `automation-ruleset` (`get_version`), `automation-ruleset-readiness` (`build_readiness_context`/`check_readiness`).

## Global Constraints

- No code comments anywhere (`//`, `///`, `//!` all forbidden).
- New crate `automation-ruleset-dispatch` must NOT depend on `sqlx`, `twilight-*`, or `ai-gateway`. Stores and the snapshot source are injected generics.
- Crate layering unchanged; no dependency cycle (the new crate is a leaf consumer).
- Fail-closed: no fallback to boot snapshot or active RuleSet on any failure.
- Defer is called exactly once; the failure EditResponse and the success EditResponse are mutually exclusive.
- Gates: `$HOME/.cargo/bin/cargo test` (workspace), `clippy --all-targets -- -D warnings`, `fmt --check`. Postgres integration tests are `#[ignore]`.

## File Structure

- Create `crates/automation-ruleset-dispatch/Cargo.toml`
- Create `crates/automation-ruleset-dispatch/src/lib.rs` — module wiring + re-exports
- Create `crates/automation-ruleset-dispatch/src/snapshot.rs` — `GuildRoleSnapshot`, `SnapshotError`, `GuildRoleSnapshotProvider`
- Create `crates/automation-ruleset-dispatch/src/error.rs` — `DispatchError`, `FailureResponseOutcome`, `DispatchFailure`
- Create `crates/automation-ruleset-dispatch/src/dispatch.rs` — `dispatch_instance_action`, `ensure_active`
- Create `crates/automation-ruleset-dispatch/tests/dispatch.rs` — in-memory harness + behavior tests
- Create `crates/automation-ruleset-dispatch/tests/no_ai_gateway.rs` — event-time-LLM guard
- Create `crates/automation-ruleset-dispatch/tests/postgres_dispatch.rs` — ignored Postgres integration
- Modify `Cargo.toml` (workspace `members`) — register the crate
- Modify `crates/automation-core/src/adapter.rs` — `AdapterErrorKind::InvalidEventRoute`
- Modify `crates/automation-core/src/run.rs` — `handle_event` guard; remove `InstanceAction` execution
- Modify `crates/automation-core/src/validate.rs` — `ValidationError::InstanceActionRuleMustDefer` + check
- Modify `crates/automation-ruleset-readiness/src/gate.rs` + `src/hydrate.rs` — defer-wrap InstanceAction test fixtures
- Modify `crates/automation-runtime/` — twilight snapshot provider + gateway/runner routing
- Modify `tools/interaction-smoke/src/main.rs` — construct store + snapshot provider, thread into gateway

---

## Task 1: New crate `automation-ruleset-dispatch` — seam, errors, dispatcher, tests

**Files:**
- Create: `crates/automation-ruleset-dispatch/Cargo.toml`, `src/lib.rs`, `src/snapshot.rs`, `src/error.rs`, `src/dispatch.rs`, `tests/dispatch.rs`, `tests/no_ai_gateway.rs`
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**
- Produces: `dispatch_instance_action(event, instance_id, action, ruleset_store, snapshot_provider, bindings, services, failure_message) -> Result<HandleOutcome, DispatchFailure>`; `GuildRoleSnapshot { roles, bot_role_ids }`; `GuildRoleSnapshotProvider::snapshot(guild) -> Result<GuildRoleSnapshot, SnapshotError>`; `DispatchError`, `DispatchFailure { cause, failure_response }`, `FailureResponseOutcome`.
- Consumes: `automation_core::{interpret, run, RuntimeContext::from_event, AutomationServices, InteractionResponder, HandleOutcome, ActionPlan, PlannedAction, ResolvedInstanceContext, RunningRuleSetIdentity, TemplateString, SanitizeContext, AdapterError}`; `automation_instance::{InstanceStore, InstanceStatus, InstanceRuleSetVersion, InstanceId, AutomationInstance}`; `automation_ruleset::{RuleSetStore, RuleSetKey, RuleSetVersionId}`; `automation_ruleset_readiness::{build_readiness_context, check_readiness, RuleSetReadinessInput, ReadinessError, ReadinessContextError}`.

- [ ] **Step 1: Scaffold the crate manifest and register it in the workspace**

Create `crates/automation-ruleset-dispatch/Cargo.toml`:

```toml
[package]
name = "automation-ruleset-dispatch"
version = "0.1.0"
edition.workspace = true

[dependencies]
automation-core = { path = "../automation-core" }
automation-state = { path = "../automation-state" }
automation-instance = { path = "../automation-instance" }
automation-ruleset = { path = "../automation-ruleset" }
automation-ruleset-readiness = { path = "../automation-ruleset-readiness" }
discord-model = { path = "../discord-model" }
resource-resolution = { path = "../resource-resolution" }
desired-state = { path = "../desired-state" }

[dev-dependencies]
futures = "0.3"
```

In the workspace root `Cargo.toml`, add `"crates/automation-ruleset-dispatch",` to `members` immediately after `"crates/automation-ruleset-readiness",`.

- [ ] **Step 2: Write the snapshot seam**

Create `crates/automation-ruleset-dispatch/src/snapshot.rs`:

```rust
use std::collections::{BTreeMap, BTreeSet};

use discord_model::{GuildId, Permissions, RoleId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildRoleSnapshot {
    pub roles: BTreeMap<RoleId, Permissions>,
    pub bot_role_ids: BTreeSet<RoleId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotError(String);

impl SnapshotError {
    pub fn new(detail: impl Into<String>) -> Self {
        SnapshotError(detail.into())
    }

    pub fn detail(&self) -> &str {
        &self.0
    }
}

#[allow(async_fn_in_trait)]
pub trait GuildRoleSnapshotProvider {
    async fn snapshot(&self, guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError>;
}
```

- [ ] **Step 3: Write the error and failure-response types**

Create `crates/automation-ruleset-dispatch/src/error.rs`:

```rust
use automation_core::AdapterError;
use automation_instance::{InstanceStatus, InstanceStoreError};
use automation_ruleset::RuleSetStoreError;
use automation_ruleset_readiness::{ReadinessContextError, ReadinessError};

use crate::snapshot::SnapshotError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchError {
    DeferFailed(AdapterError),
    InstanceLookup(InstanceStoreError),
    InstanceNotFound,
    InstanceInactive(InstanceStatus),
    PinnedKeyInvalid,
    VersionLookup(RuleSetStoreError),
    PinnedVersionMissing,
    SnapshotFailed(SnapshotError),
    ContextInvalid(ReadinessContextError),
    NotReady(ReadinessError),
    NoMatchingRule { action: String },
    Execution(AdapterError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailureResponseOutcome {
    NotAttempted,
    Sent,
    Failed(AdapterError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchFailure {
    pub cause: DispatchError,
    pub failure_response: FailureResponseOutcome,
}
```

- [ ] **Step 4: Write the dispatcher**

Create `crates/automation-ruleset-dispatch/src/dispatch.rs`:

```rust
use std::collections::BTreeMap;

use automation_core::{
    interpret, run, ActionPlan, AutomationServices, DiscordMutationAdapter, HandleOutcome,
    InteractionResponder, PlannedAction, ResolvedInstanceContext, RunningRuleSetIdentity,
    RuntimeContext, RuntimeEvent, SanitizeContext, TemplateString,
};
use automation_instance::{
    AutomationInstance, InstanceId, InstanceIdGenerator, InstanceStatus, InstanceStore,
};
use automation_ruleset::{RuleSetKey, RuleSetStore, RuleSetVersionId};
use automation_ruleset_readiness::{build_readiness_context, check_readiness, RuleSetReadinessInput};
use discord_model::RoleId;
use resource_resolution::ResourceBindingMap;

use crate::error::{DispatchError, DispatchFailure, FailureResponseOutcome};
use crate::snapshot::GuildRoleSnapshotProvider;

pub async fn dispatch_instance_action<M, R, S, G, RS, P>(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    ruleset_store: &RS,
    snapshot_provider: &P,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G>,
    failure_message: &str,
) -> Result<HandleOutcome, DispatchFailure>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
    RS: RuleSetStore,
    P: GuildRoleSnapshotProvider,
{
    if let Err(error) = services.responder.defer_ephemeral().await {
        return Err(DispatchFailure {
            cause: DispatchError::DeferFailed(error),
            failure_response: FailureResponseOutcome::NotAttempted,
        });
    }
    match run_pinned(
        event,
        instance_id,
        action,
        ruleset_store,
        snapshot_provider,
        bindings,
        services,
    )
    .await
    {
        Ok(outcome) => Ok(outcome),
        Err(cause) => {
            let failure_response = emit_failure(services, failure_message).await;
            Err(DispatchFailure {
                cause,
                failure_response,
            })
        }
    }
}

async fn run_pinned<M, R, S, G, RS, P>(
    event: &RuntimeEvent,
    instance_id: &InstanceId,
    action: &str,
    ruleset_store: &RS,
    snapshot_provider: &P,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G>,
) -> Result<HandleOutcome, DispatchError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
    RS: RuleSetStore,
    P: GuildRoleSnapshotProvider,
{
    let instance = services
        .instances
        .get(event.guild_id, instance_id)
        .await
        .map_err(DispatchError::InstanceLookup)?
        .ok_or(DispatchError::InstanceNotFound)?;
    ensure_active(&instance)?;

    let key = RuleSetKey::parse(&instance.ruleset_key).map_err(|_| DispatchError::PinnedKeyInvalid)?;
    let version_id = RuleSetVersionId::new(instance.ruleset_version.get())
        .map_err(|_| DispatchError::PinnedKeyInvalid)?;
    let identity = RunningRuleSetIdentity {
        key: instance.ruleset_key.clone(),
        version: instance.ruleset_version,
    };

    let artifact = ruleset_store
        .get_version(event.guild_id, &key, version_id)
        .await
        .map_err(DispatchError::VersionLookup)?
        .ok_or(DispatchError::PinnedVersionMissing)?;
    if artifact.guild_id != event.guild_id
        || artifact.ruleset_key != key
        || artifact.version != version_id
    {
        return Err(DispatchError::PinnedVersionMissing);
    }

    let snapshot = snapshot_provider
        .snapshot(event.guild_id)
        .await
        .map_err(DispatchError::SnapshotFailed)?;
    let bot_roles: Vec<RoleId> = snapshot.bot_role_ids.iter().copied().collect();
    let (guild_capabilities, role_permissions) =
        build_readiness_context(event.guild_id, bindings, &snapshot.roles, &bot_roles)
            .map_err(DispatchError::ContextInvalid)?;

    let runtime_ruleset = check_readiness(RuleSetReadinessInput {
        artifact: &artifact,
        bindings,
        guild_capabilities: &guild_capabilities,
        role_permissions: &role_permissions,
    })
    .map_err(DispatchError::NotReady)?;

    let plan = interpret(event, &runtime_ruleset.definition, bindings)
        .ok_or_else(|| DispatchError::NoMatchingRule {
            action: action.to_string(),
        })?;
    let mut steps = plan.steps;
    if matches!(steps.first(), Some(PlannedAction::DeferEphemeral)) {
        steps.remove(0);
    }

    let mut context = RuntimeContext::from_event(event, &identity);
    context.instance = Some(ResolvedInstanceContext {
        instance,
        action: action.to_string(),
    });
    run(&context, &ActionPlan { steps }, services)
        .await
        .map_err(DispatchError::Execution)?;
    Ok(HandleOutcome::Executed)
}

fn ensure_active(instance: &AutomationInstance) -> Result<(), DispatchError> {
    if instance.status != InstanceStatus::Active {
        return Err(DispatchError::InstanceInactive(instance.status));
    }
    Ok(())
}

async fn emit_failure<M, R, S, G>(
    services: &AutomationServices<'_, M, R, S, G>,
    failure_message: &str,
) -> FailureResponseOutcome
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
{
    let inputs: BTreeMap<String, String> = BTreeMap::new();
    let rendered = match TemplateString::parse(failure_message)
        .and_then(|template| template.render(&inputs, SanitizeContext::EphemeralMessageContent))
    {
        Ok(text) => text,
        Err(_) => return FailureResponseOutcome::NotAttempted,
    };
    match services.responder.edit_response(rendered).await {
        Ok(()) => FailureResponseOutcome::Sent,
        Err(error) => FailureResponseOutcome::Failed(error),
    }
}
```

Note: after `check_readiness` passes, the matched `InstanceAction` rule is guaranteed defer-first (Task 2 adds `InstanceActionRuleMustDefer` to `validate_structural`, which `check_readiness` re-runs), so stripping the leading `DeferEphemeral` is always safe.

**Locked invariants (pre-defer preservation):**
- `defer_ephemeral` is called exactly once (the pre-defer at step 1); the leading `DeferEphemeral` is stripped from the tail before `run` exactly as `handle_event` does (same `matches!(steps.first(), Some(PlannedAction::DeferEphemeral))` + `remove(0)` pattern), so the two paths never diverge.
- `run()` does not record `DeferEphemeral` as a `CreatedResource` (it produces no output), so stripping it changes no created-resource set. The Defer is recorded only as the single responder call.
- Created-resource bindings are keyed by each action's `key` field (`created_roles`/`created_channels`/`created_messages`/`created_instances`), never by position, so the one-position `action_index` shift from stripping the Defer does not change any binding or output-key resolution. `action_index` on a `CreatedResource` is audit-only and post-strip, identical to `handle_event`'s existing behavior.
- `InvalidEventRoute` (and every `DispatchError`) is non-retryable by absence: there is no retry-classification method in the codebase; do NOT introduce one that would retry `InvalidEventRoute` or a `DispatchError`.

- [ ] **Step 5: Wire the library root**

Create `crates/automation-ruleset-dispatch/src/lib.rs`:

```rust
pub mod dispatch;
pub mod error;
pub mod snapshot;

pub use dispatch::dispatch_instance_action;
pub use error::{DispatchError, DispatchFailure, FailureResponseOutcome};
pub use snapshot::{GuildRoleSnapshot, GuildRoleSnapshotProvider, SnapshotError};
```

- [ ] **Step 6: Run to verify it compiles**

Run: `$HOME/.cargo/bin/cargo build -p automation-ruleset-dispatch`
Expected: builds clean.

- [ ] **Step 7: Write the test harness + behavior tests**

Create `crates/automation-ruleset-dispatch/tests/dispatch.rs`:

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use automation_core::{AutomationServices, EventKind, HandleOutcome, MockMutationAdapter, RuntimeEvent};
use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceResources,
    InstanceRuleSetVersion, InstanceStatus, InstanceStore, InstanceStoreError,
    SequenceInstanceIdGenerator,
};
use automation_ruleset::{
    InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetActivation, RuleSetKey,
    RuleSetStore, RuleSetStoreError, RuleSetVersion, RuleSetVersionId,
};
use automation_ruleset_dispatch::{
    dispatch_instance_action, DispatchError, FailureResponseOutcome, GuildRoleSnapshot,
    GuildRoleSnapshotProvider, SnapshotError,
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
    async fn respond_ephemeral(&self, _content: String) -> Result<(), automation_core::AdapterError> {
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
        self.inner.update_status(guild_id, instance_id, status).await
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
        PublishOutcome::Created(v) => v.version,
        PublishOutcome::Reused(v) => v.version,
    }
}

fn instance(id: &str, pin: u32, status: InstanceStatus) -> AutomationInstance {
    let mut resources = InstanceResources::default();
    resources.roles.insert("member_role".to_string(), MEMBER_ROLE);
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
}

fn fixture(fail_defer: bool, fail_edit: bool, instance_fail: Option<InstanceStoreError>) -> Fixture {
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
    }
}

fn services<'a>(f: &'a Fixture) -> AutomationServices<'a, MockMutationAdapter, TracingResponder, TracingInstances, SequenceInstanceIdGenerator> {
    AutomationServices {
        mutation: &f.mutation,
        responder: &f.responder,
        instances: &f.instances,
        instance_ids: &f.ids,
    }
}

#[test]
fn pinned_v1_runs_while_active_is_v2() {
    let f = fixture(false, false, None);
    let v1 = publish(&f.rulesets.inner, join_rule("v1"));
    let v2 = publish(&f.rulesets.inner, join_rule("v2"));
    block_on(f.rulesets.inner.activate(GUILD, &key(), v2)).unwrap();
    block_on(f.instances.inner.register(instance("room_a", v1.get(), InstanceStatus::Active))).unwrap();
    let snap = StubSnapshot {
        trace: f.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let outcome = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &f.rulesets,
        &snap,
        &ResourceBindingMap::default(),
        &services(&f),
        "failed",
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        f.mutation.calls().len(),
        1,
        "the pinned v1 join grants member_role exactly once"
    );
    assert_eq!(
        f.trace.lock().unwrap()[..4],
        ["defer", "instance.get", "ruleset.get_version", "snapshot"]
    );
    assert!(f.trace.lock().unwrap().contains(&"edit"));
    assert_eq!(
        f.trace.lock().unwrap().iter().filter(|s| **s == "defer").count(),
        1
    );
}

#[test]
fn defer_failure_stops_before_any_lookup() {
    let f = fixture(true, false, None);
    publish(&f.rulesets.inner, join_rule("v1"));
    block_on(f.instances.inner.register(instance("room_a", 1, InstanceStatus::Active))).unwrap();
    let snap = StubSnapshot {
        trace: f.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &f.rulesets,
        &snap,
        &ResourceBindingMap::default(),
        &services(&f),
        "failed",
    ))
    .unwrap_err();
    assert!(matches!(failure.cause, DispatchError::DeferFailed(_)));
    assert_eq!(failure.failure_response, FailureResponseOutcome::NotAttempted);
    assert_eq!(*f.trace.lock().unwrap(), vec!["defer"]);
    assert!(f.mutation.calls().is_empty());
}

#[test]
fn snapshot_failure_is_fail_closed() {
    let f = fixture(false, false, None);
    publish(&f.rulesets.inner, join_rule("v1"));
    block_on(f.instances.inner.register(instance("room_a", 1, InstanceStatus::Active))).unwrap();
    let snap = StubSnapshot {
        trace: f.trace.clone(),
        result: Err(SnapshotError::new("discord down")),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &f.rulesets,
        &snap,
        &ResourceBindingMap::default(),
        &services(&f),
        "failed",
    ))
    .unwrap_err();
    assert!(matches!(failure.cause, DispatchError::SnapshotFailed(_)));
    assert_eq!(failure.failure_response, FailureResponseOutcome::Sent);
    assert!(f.mutation.calls().is_empty());
}

#[test]
fn missing_pinned_version_has_no_active_fallback() {
    let f = fixture(false, false, None);
    let v1 = publish(&f.rulesets.inner, join_rule("v1"));
    block_on(f.rulesets.inner.activate(GUILD, &key(), v1)).unwrap();
    block_on(f.instances.inner.register(instance("room_a", 99, InstanceStatus::Active))).unwrap();
    let snap = StubSnapshot {
        trace: f.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &f.rulesets,
        &snap,
        &ResourceBindingMap::default(),
        &services(&f),
        "failed",
    ))
    .unwrap_err();
    assert_eq!(failure.cause, DispatchError::PinnedVersionMissing);
    assert!(!f.trace.lock().unwrap().contains(&"snapshot"));
    assert!(f.mutation.calls().is_empty());
}

#[test]
fn unknown_action_is_no_matching_rule() {
    let f = fixture(false, false, None);
    let v1 = publish(&f.rulesets.inner, join_rule("v1"));
    block_on(f.instances.inner.register(instance("room_a", v1.get(), InstanceStatus::Active))).unwrap();
    let snap = StubSnapshot {
        trace: f.trace.clone(),
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
        &f.rulesets,
        &snap,
        &ResourceBindingMap::default(),
        &services(&f),
        "failed",
    ))
    .unwrap_err();
    assert_eq!(
        failure.cause,
        DispatchError::NoMatchingRule {
            action: "leave".to_string()
        }
    );
    assert!(f.mutation.calls().is_empty());
}

#[test]
fn inactive_instance_is_rejected() {
    for status in [InstanceStatus::Disabled, InstanceStatus::Deleted] {
        let f = fixture(false, false, None);
        let v1 = publish(&f.rulesets.inner, join_rule("v1"));
        block_on(f.instances.inner.register(instance("room_a", v1.get(), status))).unwrap();
        let snap = StubSnapshot {
            trace: f.trace.clone(),
            result: Ok(admin_snapshot()),
        };
        let failure = block_on(dispatch_instance_action(
            &join_event("room_a"),
            &InstanceId::parse("room_a").unwrap(),
            "join",
            &f.rulesets,
            &snap,
            &ResourceBindingMap::default(),
            &services(&f),
            "failed",
        ))
        .unwrap_err();
        assert_eq!(failure.cause, DispatchError::InstanceInactive(status));
        assert!(f.mutation.calls().is_empty());
    }
}

#[test]
fn privilege_escalation_in_fresh_snapshot_blocks() {
    let f = fixture(false, false, None);
    let def = InteractionRuleSet {
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
    let v1 = publish(&f.rulesets.inner, def);
    block_on(f.instances.inner.register(instance("room_a", v1.get(), InstanceStatus::Active))).unwrap();

    let mut bindings = ResourceBindingMap::default();
    bindings
        .role_bindings
        .insert(ResourceKey("admin".to_string()), RoleId(10));
    let mut roles = BTreeMap::new();
    roles.insert(RoleId(GUILD.0), Permissions::ADMINISTRATOR);
    roles.insert(RoleId(10), Permissions::ADMINISTRATOR);
    let snap = StubSnapshot {
        trace: f.trace.clone(),
        result: Ok(GuildRoleSnapshot {
            roles,
            bot_role_ids: BTreeSet::new(),
        }),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &f.rulesets,
        &snap,
        &bindings,
        &services(&f),
        "failed",
    ))
    .unwrap_err();
    assert!(matches!(failure.cause, DispatchError::NotReady(_)));
    assert!(f.mutation.calls().is_empty());
}

#[test]
fn instance_lookup_failure_stops_before_version() {
    let f = fixture(false, false, Some(InstanceStoreError::Backend("db down".to_string())));
    let snap = StubSnapshot {
        trace: f.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &f.rulesets,
        &snap,
        &ResourceBindingMap::default(),
        &services(&f),
        "failed",
    ))
    .unwrap_err();
    assert!(matches!(failure.cause, DispatchError::InstanceLookup(_)));
    assert!(!f.trace.lock().unwrap().contains(&"ruleset.get_version"));
    assert!(!f.trace.lock().unwrap().contains(&"snapshot"));
}

#[test]
fn failure_edit_failure_preserves_primary_cause() {
    let f = fixture(false, true, None);
    let v1 = publish(&f.rulesets.inner, join_rule("v1"));
    block_on(f.rulesets.inner.activate(GUILD, &key(), v1)).unwrap();
    block_on(f.instances.inner.register(instance("room_a", 99, InstanceStatus::Active))).unwrap();
    let snap = StubSnapshot {
        trace: f.trace.clone(),
        result: Ok(admin_snapshot()),
    };
    let failure = block_on(dispatch_instance_action(
        &join_event("room_a"),
        &InstanceId::parse("room_a").unwrap(),
        "join",
        &f.rulesets,
        &snap,
        &ResourceBindingMap::default(),
        &services(&f),
        "failed",
    ))
    .unwrap_err();
    assert_eq!(failure.cause, DispatchError::PinnedVersionMissing);
    assert!(matches!(
        failure.failure_response,
        FailureResponseOutcome::Failed(_)
    ));
}
```

- [ ] **Step 8: Write the no-AI-gateway guard**

Create `crates/automation-ruleset-dispatch/tests/no_ai_gateway.rs`:

```rust
#[test]
fn dispatch_crate_runtime_deps_are_pure() {
    let manifest = include_str!("../Cargo.toml");
    let runtime_deps = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!runtime_deps.contains("ai-gateway"));
    assert!(!runtime_deps.contains("twilight"));
    assert!(!runtime_deps.contains("sqlx"));
}
```

The guard checks only the `[dependencies]` block (everything before `[dev-dependencies]`), so the Task 4 Postgres dev-dependency does not trip it while regular runtime purity stays enforced.

- [ ] **Step 9: Run the crate's tests**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset-dispatch`
Expected: all tests pass.

- [ ] **Step 10: Commit**

```bash
git add crates/automation-ruleset-dispatch Cargo.toml
git commit -m "feat(automation-ruleset-dispatch): pinned-version InstanceAction dispatcher"
```

---

## Task 2: `automation-core` — reject InstanceAction, enforce mandatory defer

**Files:**
- Modify: `crates/automation-core/src/adapter.rs`, `src/run.rs`, `src/validate.rs`
- Modify: `crates/automation-ruleset-readiness/src/gate.rs`, `src/hydrate.rs` (fixture defer-wrap)
- Modify: `crates/automation-core/tests/*` (relocate/adjust InstanceAction tests)

**Interfaces:**
- Produces: `AdapterErrorKind::InvalidEventRoute`; `ValidationError::InstanceActionRuleMustDefer { rule: String }`; `handle_event` rejecting `InstanceAction`.
- Consumes: nothing new.

- [ ] **Step 1: Add the `InvalidEventRoute` error kind**

In `crates/automation-core/src/adapter.rs`, add `InvalidEventRoute,` to `AdapterErrorKind` (between `BadRequest` and `Unknown`):

```rust
pub enum AdapterErrorKind {
    Forbidden,
    NotFound,
    RateLimited,
    Network,
    Unsupported,
    BadRequest,
    InvalidEventRoute,
    Unknown,
}
```

- [ ] **Step 2: Write the failing regression test for the guard**

In `crates/automation-core/tests/run.rs` (or a new `tests/instance_action_routing.rs`), add:

```rust
struct PanicInstances;

impl InstanceStore for PanicInstances {
    async fn register(&self, _: AutomationInstance) -> Result<(), InstanceStoreError> {
        panic!("register must not be called on a misrouted InstanceAction")
    }
    async fn get(
        &self,
        _: GuildId,
        _: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        panic!("get must not be called on a misrouted InstanceAction")
    }
    async fn list_by_guild(&self, _: GuildId) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        panic!("list_by_guild must not be called on a misrouted InstanceAction")
    }
    async fn update_status(
        &self,
        _: GuildId,
        _: &InstanceId,
        _: InstanceStatus,
    ) -> Result<(), InstanceStoreError> {
        panic!("update_status must not be called on a misrouted InstanceAction")
    }
}

#[test]
fn handle_event_rejects_instance_action() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let instances = PanicInstances;
    let ids = SequenceInstanceIdGenerator::new("room", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &ids,
    };
    let event = RuntimeEvent {
        guild_id: GuildId(7),
        actor: UserId(42),
        kind: EventKind::InstanceAction {
            instance_id: InstanceId::parse("room_a").unwrap(),
            action: "join".to_string(),
        },
    };
    let identity = RunningRuleSetIdentity {
        key: "studyroom_demo".to_string(),
        version: InstanceRuleSetVersion::new(1).unwrap(),
    };
    let ruleset = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![],
    };
    let error = block_on(handle_event(
        &event,
        &ruleset,
        &ResourceBindingMap::default(),
        &services,
        "failed",
        &identity,
    ))
    .unwrap_err();
    assert_eq!(error.kind, AdapterErrorKind::InvalidEventRoute);
    assert!(responder.calls().is_empty());
    assert!(mutation.calls().is_empty());
}
```

Run: `$HOME/.cargo/bin/cargo test -p automation-core handle_event_rejects_instance_action`
Expected: FAIL (currently handle_event executes InstanceAction).

- [ ] **Step 3: Add the guard and remove InstanceAction execution from `handle_event`**

In `crates/automation-core/src/run.rs`, replace `handle_event` and delete `resolve_instance_and_run` plus the now-dead helpers `instance_store_error`, `instance_not_found`, `instance_inactive`, `instance_ruleset_mismatch`. New `handle_event`:

```rust
pub async fn handle_event<M, R, S, G>(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G>,
    failure_message: &str,
    identity: &RunningRuleSetIdentity,
) -> Result<HandleOutcome, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
{
    if let EventKind::InstanceAction { .. } = &event.kind {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidEventRoute,
            "InstanceAction must be dispatched via automation-ruleset-dispatch",
        ));
    }
    let context = RuntimeContext::from_event(event, identity);
    match interpret(event, ruleset, bindings) {
        Some(plan) => {
            let mut steps = plan.steps;
            let defer_acked = if matches!(steps.first(), Some(PlannedAction::DeferEphemeral)) {
                services.responder.defer_ephemeral().await?;
                steps.remove(0);
                true
            } else {
                false
            };
            match run(&context, &ActionPlan { steps }, services).await {
                Ok(_) => Ok(HandleOutcome::Executed),
                Err(error) => {
                    if defer_acked {
                        if let Ok(rendered) = render(
                            failure_message,
                            &context,
                            SanitizeContext::EphemeralMessageContent,
                        ) {
                            let _ = services.responder.edit_response(rendered).await;
                        }
                    }
                    Err(error)
                }
            }
        }
        None => Ok(HandleOutcome::NoOp),
    }
}
```

Remove the `use` of `InstanceStatus` and `ResolvedInstanceContext` from `run.rs` only if the compiler reports them unused after this change (they remain used by `run`'s `RegisterInstance` arm and `context.instance`, so likely keep them). Let `cargo build` and clippy identify any now-dead imports/functions and delete exactly those.

Run: `$HOME/.cargo/bin/cargo test -p automation-core handle_event_rejects_instance_action`
Expected: PASS.

- [ ] **Step 4: Relocate InstanceAction success coverage**

The `InstanceAction` success behavior now lives in `automation-ruleset-dispatch` (Task 1). In `automation-core`, delete the tests that drove `handle_event` with an `InstanceAction` event expecting execution (in `tests/dynamic_join.rs` and any InstanceAction case in `tests/run.rs`/`tests/instance_registration.rs`). Keep tests that register instances via `RegisterInstance` inside a `ModalSubmit` flow (those still exercise `run`). Run the suite and delete only the InstanceAction-execution cases the guard now supersedes.

Run: `$HOME/.cargo/bin/cargo test -p automation-core`
Expected: PASS (after deleting the superseded InstanceAction-execution tests).

- [ ] **Step 5: Write the failing validation test**

In `crates/automation-core/tests/` (e.g. `tests/validate.rs` or wherever structural validation is tested), add:

```rust
#[test]
fn instance_action_rule_must_defer() {
    let ruleset = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "join".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![ActionSpec::GrantRole {
                role: RoleRef::Instance {
                    instance: InstanceRef::Event,
                    alias: "member_role".to_string(),
                },
                target: ActionTarget::Actor,
            }],
        }],
    };
    let errors = validate_structural(&ruleset).unwrap_err();
    assert!(errors.contains(&ValidationError::InstanceActionRuleMustDefer {
        rule: "join".to_string()
    }));
}
```

Run: `$HOME/.cargo/bin/cargo test -p automation-core instance_action_rule_must_defer`
Expected: FAIL (variant does not exist yet).

- [ ] **Step 6: Add the validation variant and check**

In `crates/automation-core/src/validate.rs`, add to `ValidationError`:

```rust
InstanceActionRuleMustDefer { rule: String },
```

In `validate_structural`, replace the empty InstanceAction trigger arm (currently `TriggerSpec::InstanceAction { .. } => {}` in the rule loop) with:

```rust
TriggerSpec::InstanceAction { .. } => {
    if !matches!(rule.actions.first(), Some(ActionSpec::DeferEphemeral)) {
        errors.push(ValidationError::InstanceActionRuleMustDefer {
            rule: rule.key.clone(),
        });
    }
}
```

This adds ONLY the "first action must be `DeferEphemeral`" requirement. Do NOT duplicate the deferred-response contract: `Defer` exactly once, exactly one `EditResponse`, `EditResponse` last, and no conflict with `RespondEphemeral`/`OpenModal` are already enforced for every deferred rule by the existing 16k checks in `validate_structural` (`DeferNotFirst`, `ConflictingInitialResponse`, `DeferredMissingEditResponse`, `EditResponseWithoutDefer`, `MultipleEditResponse`, `EditResponseNotLast`), which run for `InstanceAction` rules too once they are deferred. Reusing them keeps the instance-rule and general-deferred-rule contracts from diverging.

Run: `$HOME/.cargo/bin/cargo test -p automation-core instance_action_rule_must_defer`
Expected: PASS.

- [ ] **Step 7: Defer-wrap InstanceAction fixtures broken by the new rule**

The new rule rejects non-deferred `InstanceAction` fixtures that flow through `validate_structural` (directly or via `check_readiness`). Fix the known sites:

In `crates/automation-ruleset-readiness/src/gate.rs`, change the `ruleset` test helper so its single InstanceAction rule is deferred:

```rust
fn ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
    let mut wrapped = vec![ActionSpec::DeferEphemeral];
    wrapped.extend(actions);
    wrapped.push(ActionSpec::EditResponse {
        content: "done".to_string(),
    });
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "test".to_string(),
            },
            actions: wrapped,
        }],
    }
}
```

In `crates/automation-ruleset-readiness/src/hydrate.rs`, change `def()` the same way (prepend `DeferEphemeral`, append `EditResponse`).

Then run the full workspace test suite; for any remaining failure caused by a non-deferred InstanceAction fixture (candidates: `automation-ruleset` publish tests, `automation-core` interpret/validate tests), apply the identical wrap: first action `DeferEphemeral`, last action `EditResponse { content: ... }`.

Run: `$HOME/.cargo/bin/cargo test`
Expected: PASS across the workspace.

- [ ] **Step 8: Gate and commit**

Run: `$HOME/.cargo/bin/cargo clippy --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --check`
Expected: clean.

```bash
git add crates/automation-core crates/automation-ruleset-readiness
git commit -m "feat(automation-core): route InstanceAction to pinned dispatch, require defer"
```

---

## Task 3: Edge wiring — twilight snapshot provider + gateway routing + tool

**Files:**
- Modify: `crates/automation-runtime/` (add `TwilightGuildRoleSnapshotProvider`; route in gateway/runner)
- Modify: `crates/automation-runtime/Cargo.toml` (add `automation-ruleset-dispatch`, `automation-ruleset`, `automation-ruleset-readiness` path deps if not present)
- Modify: `tools/interaction-smoke/src/main.rs`

**Interfaces:**
- Consumes: `automation_ruleset_dispatch::{dispatch_instance_action, GuildRoleSnapshot, GuildRoleSnapshotProvider, SnapshotError}`.
- Produces: `TwilightGuildRoleSnapshotProvider` implementing `GuildRoleSnapshotProvider`; a gateway event loop that routes by `event.kind()`.

- [ ] **Step 1: Implement the twilight snapshot provider**

Add `crates/automation-runtime/src/snapshot.rs` (module name to match crate conventions), mirroring `TwilightMutationAdapter` and bot-runtime `reader.rs`. It holds the twilight `Client` and the bot user id (obtained once at construction via `current_user`), and implements `GuildRoleSnapshotProvider`:

```rust
async fn snapshot(&self, guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError> {
    let roles = self
        .http
        .roles(to_twilight_guild(guild_id))
        .await
        .map_err(|error| SnapshotError::new(format!("roles fetch failed: {error}")))?
        .model()
        .await
        .map_err(|error| SnapshotError::new(format!("roles decode failed: {error}")))?;
    let mut role_map = BTreeMap::new();
    for role in &roles {
        role_map.insert(
            RoleId(role.id.get()),
            Permissions::from_bits_retain(role.permissions.bits()),
        );
    }
    let member = self
        .http
        .guild_member(to_twilight_guild(guild_id), self.bot_user_id)
        .await
        .map_err(|error| SnapshotError::new(format!("member fetch failed: {error}")))?
        .model()
        .await
        .map_err(|error| SnapshotError::new(format!("member decode failed: {error}")))?;
    let bot_role_ids = member
        .roles
        .iter()
        .map(|id| RoleId(id.get()))
        .collect::<BTreeSet<_>>();
    Ok(GuildRoleSnapshot {
        roles: role_map,
        bot_role_ids,
    })
}
```

Use the exact twilight id-conversion helpers already present in `automation-runtime` (mirror how `TwilightMutationAdapter` converts `GuildId`/`RoleId`). Do not add new twilight version constraints.

- [ ] **Step 2: Route by event kind in the gateway/runner**

Where the gateway currently calls `handle_event` for every converted event, branch on kind. Keep the boot-hydrated active `RuntimeRuleSet` and its `RunningRuleSetIdentity` for static events; pass the injected `ruleset_store`, `snapshot_provider`, and boot `bindings` for `InstanceAction`:

```rust
match &event.kind {
    EventKind::ButtonClick { .. } | EventKind::ModalSubmit { .. } => {
        handle_event(
            &event,
            &active.definition,
            bindings,
            &services,
            failure_message,
            &active_identity,
        )
        .await
        .map(|_| ())
        .unwrap_or_else(|error| log_handle_error(error));
    }
    EventKind::InstanceAction { instance_id, action } => {
        if let Err(failure) = dispatch_instance_action(
            &event,
            instance_id,
            action,
            ruleset_store,
            snapshot_provider,
            bindings,
            &services,
            failure_message,
        )
        .await
        {
            log_dispatch_failure(failure);
        }
    }
}
```

`log_dispatch_failure` records both `failure.cause` and `failure.failure_response` (this is where the `DispatchFailure` observability is consumed). Adjust `gateway::run`/`runner` signatures to accept `ruleset_store: &impl RuleSetStore` and `snapshot_provider: &impl GuildRoleSnapshotProvider`. `active` is the boot `RuntimeRuleSet`; `active_identity` is `RunningRuleSetIdentity { key: active.ruleset_key.to_string(), version: InstanceRuleSetVersion::new(active.version.get()).expect("active version >= 1") }` (already constructed for 18c-1/18c-2 static handling — reuse it).

- [ ] **Step 3: Construct store + provider in the tool and thread them in**

In `tools/interaction-smoke/src/main.rs` `run` subcommand: after boot hydration, construct the Postgres `RuleSetStore` (already built for hydration in 18c-1) and a `TwilightGuildRoleSnapshotProvider::new(&http).await`, and pass them plus the boot `bindings` into `gateway::run`. Boot hydration and static-event handling are unchanged.

- [ ] **Step 4: Build, gate, commit**

Run: `$HOME/.cargo/bin/cargo build && $HOME/.cargo/bin/cargo clippy --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --check`
Expected: clean.

```bash
git add crates/automation-runtime tools/interaction-smoke
git commit -m "feat(automation-runtime): route InstanceAction to pinned dispatch with live snapshot"
```

---

## Task 4: Ignored Postgres integration test for pinned dispatch

**Files:**
- Create: `crates/automation-ruleset-dispatch/tests/postgres_dispatch.rs`
- Modify: `crates/automation-ruleset-dispatch/Cargo.toml` (dev-dep on `automation-ruleset-postgres` + `sqlx`/`tokio` for the test target only)

**Interfaces:**
- Consumes: `automation_ruleset_postgres::PostgresRuleSetStore`, `automation_instance_postgres::PostgresInstanceStore` (or in-memory instance store; the pin path only needs the ruleset store to be Postgres).

- [ ] **Step 1: Add dev-dependencies for the ignored test**

Because the crate must not depend on `sqlx` in normal builds, add the Postgres dev-deps under `[dev-dependencies]` only (dev-deps are excluded from the crate's own compilation and from downstream users):

```toml
[dev-dependencies]
futures = "0.3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
automation-ruleset-postgres = { path = "../automation-ruleset-postgres" }
```

Note: the `no_ai_gateway.rs` guard (Task 1 Step 8) already checks only the `[dependencies]` block (it splits the manifest at `[dev-dependencies]`), so this dev-dependency does not trip it. `sqlx` never appears in this crate's own `[dependencies]`; it stays a transitive dependency of `automation-ruleset-postgres` only. Runtime purity is preserved.

- [ ] **Step 2: Write the ignored end-to-end test**

Create `crates/automation-ruleset-dispatch/tests/postgres_dispatch.rs`. It uses `STARRING_TEST_DATABASE_URL`, publishes v1 + v2 to `PostgresRuleSetStore`, activates v2, registers an instance (in-memory store is fine) pinned to v1, and asserts `dispatch_instance_action` runs v1 (mutation grants `member_role`, responder edits `joined v1`). Mirror the harness types from `tests/dispatch.rs` (a stub snapshot returning an ADMINISTRATOR `@everyone`), reusing `MockMutationAdapter` and a recording responder. Mark the test `#[ignore]` and require the DB name to contain `test`.

Run: `STARRING_TEST_DATABASE_URL=postgres://localhost/starring_test $HOME/.cargo/bin/cargo test -p automation-ruleset-dispatch --test postgres_dispatch -- --ignored --test-threads=1`
Expected: PASS against local Postgres.

- [ ] **Step 3: Gate and commit**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset-dispatch && $HOME/.cargo/bin/cargo clippy --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --check`
Expected: clean (ignored test excluded from the default run).

```bash
git add crates/automation-ruleset-dispatch
git commit -m "test(automation-ruleset-dispatch): ignored Postgres pinned-dispatch integration"
```

---

## Live demo (after all tasks)

Reused bot/guild/local `starring`:

1. Publish + activate v1 (join grants member role, edit "v1"). Create a room (pinned v1).
2. Publish a v2 with a visibly different join edit ("v2"). Activate v2.
3. Click the existing room's join button → the v1 edit shows (pinned), not v2.
4. Escalate a **bound** role (one in `bindings.role_bindings`, granted via `RoleRef::Existing`) to ADMINISTRATOR, or revoke the bot's MANAGE_ROLES → click → fail-closed ("Bot is thinking…" resolves to the failure edit), mutation none.
5. Tamper: `psql` UPDATE the pinned v1 row's `definition` (bypassing 18a immutability) → click → fail-closed on `HashMismatch`, no active-v2 fallback, mutation none.
6. Verify `automation_instances.ruleset_version` for the room is still `1` throughout.

## Self-Review notes

- `DispatchError`/`DispatchFailure` derive `Eq`; every wrapped type (`AdapterError`, `InstanceStoreError`, `RuleSetStoreError`, `InstanceStatus`, `ReadinessError`, `ReadinessContextError`, `SnapshotError`) is `Eq` — verified.
- The dispatcher strips the leading `DeferEphemeral` only after `check_readiness` (which re-runs `validate_structural` including `InstanceActionRuleMustDefer`), so the strip is always safe.
- The snapshot is fetched after the two store lookups, so nonexistent instances/versions cost no Discord call; it is fetched every click so privilege-escalation drift is caught by the policy gate (test `privilege_escalation_in_fresh_snapshot_blocks`).
- No fallback to boot snapshot or active ruleset on any failure (tests `missing_pinned_version_has_no_active_fallback`, `snapshot_failure_is_fail_closed`).
- The active-pointer changing (v2→v3) between boot and click cannot affect a pinned dispatch: the dispatcher never reads `store.active`; it reads `store.get_version(instance.ruleset_version)`. `pinned_v1_runs_while_active_is_v2` proves this for a fixed active v2; the same code path holds for any later active change.

**Scope limitation (privilege-escalation):** the policy gate re-checks `role_permissions`, which `build_readiness_context` builds only for **bound (install-time) roles** (`bindings.role_bindings`). Escalation of a role granted via `RoleRef::Existing(bound_key)` is caught (test uses `RoleRef::Existing("admin")`). Instance-scoped roles (`RoleRef::Instance`, resolved from `instance.resources.roles`) are NOT in `role_permissions`, so the policy gate does not evaluate their live permissions; escalation of an automation-created instance role is out of scope for 18c-3 (a future cut could snapshot instance-role permissions too). This is a deliberate boundary, not a silent gap.

**Tampered pinned artifact:** an artifact whose stored `definition` was mutated out-of-band (bypassing 18a application-enforced immutability, e.g. manual SQL) fails `check_readiness` with `HashMismatch` → `NotReady`, mutation 0. This is covered by composition (`check_readiness`'s own `hash_mismatch_blocked` test plus the dispatcher's `NotReady` mapping exercised in `privilege_escalation_in_fresh_snapshot_blocks`) and is shown in the live demo via a manual `psql` edit.
