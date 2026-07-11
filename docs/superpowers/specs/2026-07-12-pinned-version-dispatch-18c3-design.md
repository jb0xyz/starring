# Phase 18c-3: Pinned Version Dispatch — Design

## Goal

Make `AutomationInstance.ruleset_version` (the pin written in 18c-2) an executed
semantic, not a stored value. Every `InstanceAction` executes against the RuleSet
version pinned on the instance, re-validated against the current Discord
environment, and fails closed. `ButtonClick` / `ModalSubmit` continue to use the
boot-hydrated active RuleSet unchanged.

## Context

18c-1 hydrates the active RuleSet at boot and runs all events against it. 18c-2
records, per instance, which version it was created under — but nothing reads that
value at dispatch. So an instance created under v1 is currently handled with
whatever version is active at click time. If v2 becomes active, a v1 instance's
`join` runs v2 rules. That is the semantic error this phase closes.

The system invariant this phase establishes:

> An `InstanceAction` is executed only after deferring, resolving the instance and
> its pinned RuleSet, and re-evaluating readiness against the current Discord role
> and bot-permission state. It never falls back to a boot snapshot or to the active
> RuleSet.

This is a prerequisite for durable rollback (18e): activate v2 while existing
rooms keep behaving as their pinned version.

## Global Constraints

- No code comments anywhere (`//`, `///`, `//!` all forbidden). Applies to every
  code block in this spec and the plan.
- Crate layering (unchanged, no cycle): `automation-state` → `automation-core` →
  `automation-ruleset` / `automation-ruleset-readiness`; `automation-instance` is
  independent of the ruleset crates.
- New crate `automation-ruleset-dispatch` is a leaf consumer. Nothing depends on
  it except edges (`automation-runtime`, `tools/interaction-smoke`).
- Forbidden deps of `automation-ruleset-dispatch`: `sqlx`, `twilight-*`,
  `ai-gateway`. Stores and the role-snapshot source are injected as generic
  traits so tests run fully in-memory.
- Safety invariant (unchanged): AI designs at install time; runtime executes
  deterministically; no event-time LLM (`tests/no_ai_gateway.rs` in the new crate).
- Postgres-or-die / fail-closed (unchanged): no fixture/InMemory fallback on the
  live path.

## Architecture

### New crate: `automation-ruleset-dispatch`

Pure orchestration crate. Owns the entire `InstanceAction` execution path.

Dependencies:

```
automation-ruleset-dispatch
├─ automation-core          (interpret, run, from_event, AutomationServices, AdapterError, HandleOutcome)
├─ automation-state         (InteractionRuleSet, trigger/action types via re-export)
├─ automation-instance      (InstanceStore, InstanceStatus, InstanceRuleSetVersion, InstanceId)
├─ automation-ruleset       (RuleSetStore, RuleSetKey, RuleSetVersionId, RuleSetVersion)
├─ automation-ruleset-readiness (build_readiness_context, check_readiness, RuntimeRuleSet, GuildCapabilities, RuleSetReadinessInput, ReadinessError, ReadinessContextError)
├─ discord-model            (GuildId, RoleId, Permissions)
├─ resource-resolution      (ResourceBindingMap)
└─ desired-state            (ResourceKey)
```

No dependency in this set depends back on `automation-ruleset-dispatch`, so no
cycle is introduced. `automation-ruleset-readiness` is reused unchanged.

### Routing split (single branch point at the edge)

```
ButtonClick / ModalSubmit
  → automation_core::handle_event(active RuntimeRuleSet, active identity)

InstanceAction
  → automation_ruleset_dispatch::dispatch_instance_action(pinned)
```

The edge (`automation-runtime` gateway/runner) performs exactly one
`match event.kind()`. There is no other public live path by which an
`InstanceAction` can reach execution.

`automation-core::handle_event` is reduced to static events. Its previous
`InstanceAction` branch (`resolve_instance_and_run`) is removed and replaced with
a top-of-function guard that fails closed (see "automation-core changes").

## Dispatch flow

Ordered. Every external lookup is after `Defer` (3-second ACK safe). Cheap
store checks precede the Discord snapshot so nonexistent instances/versions are
rejected without a Discord call.

```
1. Defer (responder.defer_ephemeral)
     fail → DispatchFailure { cause: DeferFailed, failure_response: NotAttempted }, no further calls
2. InstanceStore.get(guild, instance_id)
     Err  → InstanceLookup ; None → InstanceNotFound
     status != Active → InstanceInactive(status)
3. Convert the instance's ruleset identity into a store query:
     parse instance.ruleset_key → RuleSetKey     (fail → PinnedKeyInvalid; raw key logged internally, never surfaced)
     instance.ruleset_version (InstanceRuleSetVersion) → RuleSetVersionId  (total for NonZeroU32; defensively → PinnedKeyInvalid)
   Build pinned_identity = RunningRuleSetIdentity { key: instance.ruleset_key.clone(), version: instance.ruleset_version }
4. RuleSetStore.get_version(guild, &key, version_id)
     Err → VersionLookup ; None → PinnedVersionMissing
   Defensive identity match: artifact.guild_id == event.guild_id && artifact.ruleset_key == key && artifact.version == version_id
     (query is by the instance's own identity, so this holds by construction and is
      re-verified by content-hash in step 7; a store that violates it → PinnedVersionMissing)
5. GuildRoleSnapshotProvider.snapshot(guild)
     Err → SnapshotFailed(source)         (no boot/active fallback)
6. build_readiness_context(guild, bindings, &snapshot.roles, &snapshot.bot_role_ids)
     Err → ContextInvalid(error)          (EveryoneRoleMissing / BoundRoleMissing)
7. check_readiness(RuleSetReadinessInput { artifact: pinned, bindings, guild_capabilities, role_permissions })
     Err → NotReady(error)                (schema/structural/hash/binding/policy/capability)
8. interpret(event, runtime_ruleset.definition, bindings)
     None → NoMatchingRule { action }
     Some(plan): first action is DeferEphemeral by contract → strip it (already ACKed at step 1)
9. Build context = RuntimeContext::from_event(event, &pinned_identity)
   Set context.instance = Some(ResolvedInstanceContext { instance, action })
   run(context, tail_plan, services)
     Err → Execution(error)
10. Success → the tail plan's final EditResponse renders the success message
     → Ok(HandleOutcome::Executed)
```

`pinned_identity = RunningRuleSetIdentity { key: instance.ruleset_key.clone(),
version: instance.ruleset_version }`. This is the same identity threading 18c-2
introduced; here it is sourced from the instance instead of the boot-active
version, so a `RegisterInstance` executed inside a pinned rule would pin the same
version.

### Version type conversion (the reason the crate needs both ruleset+instance deps)

```rust
let version_id = RuleSetVersionId::new(instance.ruleset_version.get())
    .map_err(|_| DispatchError::PinnedKeyInvalid)?;
```

Both are `NonZeroU32` newtypes with the same `1..=u32::MAX` domain, so the
conversion is total for any stored value; the `Result` is handled defensively.
This is the exact inverse of the tool's 18c-2 write path
(`RuleSetVersionId → InstanceRuleSetVersion`).

### Readiness re-evaluation is per-click against fresh Discord state

`check_readiness` splits into artifact-intrinsic checks (schema, structural,
content-hash recompute+compare, policy classification) and environment-dependent
checks (binding resolution, guild capability, bound-role existence). The
artifact-intrinsic checks are always fresh (they depend only on the pinned
artifact). The environment inputs (`roles`, `bot_role_ids`) are fetched fresh per
click via `GuildRoleSnapshotProvider`, because `role_permissions` feeds policy
classification: a role that gains `ADMINISTRATOR` after boot must be caught by the
gate (mutation layer would not catch a privilege-escalation drift — the grant
succeeds). `bindings` remain boot-time (install-time resource resolution).

## New interfaces (`automation-ruleset-dispatch`)

### Role snapshot seam

```rust
use std::collections::{BTreeMap, BTreeSet};
use discord_model::{GuildId, Permissions, RoleId};

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
}

#[allow(async_fn_in_trait)]
pub trait GuildRoleSnapshotProvider {
    async fn snapshot(&self, guild_id: GuildId) -> Result<GuildRoleSnapshot, SnapshotError>;
}
```

Snapshot invariants enforced downstream by `build_readiness_context` (already
implemented in 18c-1, reused unchanged):

- `roles` includes the `@everyone` role, whose `RoleId == GuildId`.
- Any `bot_role_ids` member absent from `roles` → context creation fails.
- A bound existing role absent from `roles` → fail closed (`BoundRoleMissing`).
- `ADMINISTRATOR` override applies in capability satisfaction.

`SnapshotError` is opaque (carries a detail string for internal logging, never a
user-facing Discord error).

### Error and failure-response types

```rust
use automation_core::AdapterError;
use automation_instance::{InstanceStatus, InstanceStoreError};
use automation_ruleset::RuleSetStoreError;
use automation_ruleset_readiness::{ReadinessContextError, ReadinessError};

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

### Entry point

```rust
use automation_core::{AutomationServices, DiscordMutationAdapter, HandleOutcome,
    InteractionResponder, RuntimeEvent};
use automation_instance::{InstanceId, InstanceIdGenerator, InstanceStore};
use automation_ruleset::RuleSetStore;
use resource_resolution::ResourceBindingMap;

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
```

`instance_id` and `action` are passed explicitly: the edge destructures
`EventKind::InstanceAction { instance_id, action }` in its match arm and forwards
them, so a non-`InstanceAction` event cannot reach this function and no defensive
kind-check is needed. `event` is still passed for `guild_id`/`actor` and
`RuntimeContext::from_event`. `services.instances` supplies the `InstanceStore`;
`ruleset_store` and `snapshot_provider` are separate injected generics.

### Instance validation (crate-internal pure fn, not extracted to core)

```rust
fn ensure_active(instance: &AutomationInstance) -> Result<(), DispatchError>
```

Checks `status == Active`, returning `InstanceInactive(status)` otherwise. There is
no ruleset-key comparison: the pinned identity is built from the instance's own
`ruleset_key`, so the instance is self-describing and a boot-active-key match would
be tautological (unlike 17c's `handle_event`, where the identity was the
boot-active version). `PinnedKeyInvalid` covers both a `ruleset_key` that fails to
parse and the (unreachable) version conversion. Kept inside the dispatch crate; not
promoted to a shared `automation-core` helper until a third caller needs it.

## Reused APIs (no change)

- `automation_core::interpret(event, &InteractionRuleSet, &ResourceBindingMap) -> Option<ActionPlan>`
- `automation_core::run(&RuntimeContext, &ActionPlan, &AutomationServices) -> Result<Vec<CreatedResource>, AdapterError>`
- `automation_core::RuntimeContext::from_event(&RuntimeEvent, &RunningRuleSetIdentity)`
- `automation_ruleset_readiness::build_readiness_context(GuildId, &ResourceBindingMap, &BTreeMap<RoleId, Permissions>, &[RoleId]) -> Result<(GuildCapabilities, BTreeMap<ResourceKey, Permissions>), ReadinessContextError>`
- `automation_ruleset_readiness::check_readiness(RuleSetReadinessInput) -> Result<RuntimeRuleSet, ReadinessError>`
- `RuleSetStore::get_version(GuildId, &RuleSetKey, RuleSetVersionId) -> Result<Option<RuleSetVersion>, RuleSetStoreError>`

`build_readiness_context` takes a `&[RoleId]` for bot roles; the dispatcher passes
`snapshot.bot_role_ids.iter().copied().collect::<Vec<_>>()` (or a slice view) — the
`BTreeSet` is the snapshot's storage; the readiness signature is unchanged.

## `automation-core` changes

### 1. `InvalidEventRoute`

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

Contract: `retryable = false`; not a Discord failure; never surfaced as a
user-facing error string; produced by `handle_event`'s entry guard before any
`from_event`/`interpret`/`defer`; zero responder and zero mutation calls.

### 2. `handle_event` guard (InstanceAction removed from core)

`resolve_instance_and_run`'s `InstanceAction` branch is deleted. `handle_event`
gains a guard at the very top:

```rust
if let EventKind::InstanceAction { .. } = &event.kind {
    return Err(AdapterError::new(
        AdapterErrorKind::InvalidEventRoute,
        "InstanceAction must be dispatched via automation-ruleset-dispatch",
    ));
}
```

After the guard, `resolve_instance_and_run` no longer needs the `InstanceAction`
arm; it simplifies to running the plan (the instance context is never set for the
remaining static kinds). The `identity` parameter is retained (used by
`from_event`; the boot-active identity for static events).

### 3. Mandatory defer for `InstanceAction` rules (structural validation)

`validate_structural` gains one variant, added to the existing `ValidationError`
enum:

```rust
InstanceActionRuleMustDefer { rule: String },
```

Rule: for any `InteractionRule` whose `trigger` is `TriggerSpec::InstanceAction`,
the first action must be `ActionSpec::DeferEphemeral`. The existing 16k defer
contract (Edit-last, no conflicting initial response, single Edit) then applies
because the rule is deferred. This makes the dispatcher's pre-defer (step 1, before
the artifact is read) always safe, and is enforced at publish time (18a
`validate_structural`) and re-checked per click (18c-1 `check_readiness` re-runs
structural validation).

Ripple: existing test fixtures containing a non-deferred `InstanceAction` rule
must be updated to `[DeferEphemeral, ..., EditResponse]`. Known sites:
`automation-ruleset-readiness/src/hydrate.rs` test fixture; any migrated 17c
dynamic-join fixtures. Compiler/test-guided.

## Edge wiring (`automation-runtime` + tool)

### `TwilightGuildRoleSnapshotProvider`

New adapter in `automation-runtime`, mirroring `TwilightMutationAdapter` and the
bot-runtime `reader.rs` pattern. Holds the twilight `Client` and the bot user id
(fetched once at boot via `current_user`). Implements `GuildRoleSnapshotProvider`:

```
snapshot(guild):
  roles      = http.roles(guild).await.model()   -> BTreeMap<RoleId, Permissions::from_bits_retain(role.permissions.bits())>
  bot_member = http.guild_member(guild, bot_user_id).await.model()
  bot_role_ids = bot_member.roles into BTreeSet<RoleId>
  map any twilight error → SnapshotError::new(detail)
```

`@everyone` is included by `http.roles` (its id equals the guild id).

### Gateway / runner routing

The gateway event loop routes by kind:

```
match &event.kind {
    EventKind::ButtonClick { .. } | EventKind::ModalSubmit { .. } =>
        handle_event(&event, active_ruleset.definition, bindings, services, failure_message, active_identity),
    EventKind::InstanceAction { instance_id, action } =>
        dispatch_instance_action(&event, instance_id, action, ruleset_store, snapshot_provider,
            bindings, services, failure_message),
}
```

For static events, the boot-hydrated `RuntimeRuleSet` and its `active_identity`
(`RunningRuleSetIdentity { key, version }` of the active version) are used
unchanged. For `InstanceAction`, the gateway passes the injected `ruleset_store`
and `snapshot_provider`; the pinned identity is built inside the dispatcher.

`DispatchFailure` returned by the dispatcher is logged at the edge (cause +
`failure_response`), giving observability for both the primary cause and any
failure-edit failure without the pure crate taking a logging dependency.

### Tool (`tools/interaction-smoke`)

The `run` subcommand additionally constructs the Postgres-backed `RuleSetStore`
(already available from 18b) and the `TwilightGuildRoleSnapshotProvider`, and
threads them plus the boot `bindings` into `gateway::run`. Boot hydration
(18c-1) is unchanged and still governs static events.

## Failure and response contract

```
1. Defer executes.
     fail → DispatchFailure { DeferFailed, NotAttempted }; no instance/version/snapshot/mutation calls.
2..7 any failure after a successful Defer
     → best-effort edit_response(render(failure_message))
     → DispatchFailure { cause, failure_response }:
          edit ok   → Sent
          edit fail → Failed(edit_error)
     → mutations that already succeeded before the failing step remain (orphan boundary = 16k/17b, documented)
8. success
     → the pinned rule's final EditResponse sends the completion message
```

Invariants:

- Defer is called exactly once.
- The failure EditResponse is sent at most once and never together with the
  success EditResponse.
- No fallback to boot snapshot or active RuleSet on any failure.
- `failure_message` is an app-provided static string; `InstanceAction` has empty
  inputs, so it renders/sanitizes with an empty input map.

## Test plan

New crate `automation-ruleset-dispatch` (in-memory stores + mock responder/mutation
+ a mock `GuildRoleSnapshotProvider`; a shared call-trace spy for ordering):

1. Order: Defer → InstanceStore.get → RuleSetStore.get_version → snapshot, asserted
   via the shared trace.
2. Defer failure → responder calls `= [DeferEphemeral]`; instance/ruleset/snapshot/
   mutation calls all empty; `cause = DeferFailed`, `failure_response = NotAttempted`.
3. Snapshot failure → mutation 0, failure edit sent; `cause = SnapshotFailed(_)`.
4. `PinnedVersionMissing` → no active fallback; mutation 0; failure edit.
5. `NoMatchingRule { action }` → failure edit; mutation 0.
6. `InstanceInactive` for Disabled and Deleted → mutation 0; failure edit.
7. Readiness blocking (e.g. `BlockingPolicy`, `MissingCapabilities`) → mutation 0;
   failure edit; `cause = NotReady(_)`.
8. Pinned v1 instance while active = v2 → the v1 rule runs (assert the plan/effects
   come from v1, not v2). Two distinct published versions with different join
   effects.
9. Failure-edit failure preserves primary cause → `cause` = original,
   `failure_response = Failed(_)`.
10. Privilege-escalation drift: boot snapshot has a plain role; the per-click
    snapshot grants that role `ADMINISTRATOR`; readiness now classifies it blocking
    → mutation 0 (proves fresh snapshot is the boundary).

`automation-core`:

- `handle_event` with an `InstanceAction` event → `Err(InvalidEventRoute)`, zero
  responder and mutation calls (regression proving the active-version path is gone).
- Static `ButtonClick` / `ModalSubmit` behavior unchanged.
- `validate_structural` rejects a non-deferred `InstanceAction` rule with
  `InstanceActionRuleMustDefer`.
- Migrated 17c/17e `InstanceAction` success tests now live in the dispatch crate.

Integration / live:

- Ignored Postgres test (dispatch against the Postgres `RuleSetStore` + a stub
  snapshot provider) proving get_version + readiness + run end to end.
- `tests/no_ai_gateway.rs` in the new crate.
- Live (reused bot/guild/local `starring`): publish v1, activate v1, create a room
  (pinned v1). Publish a v2 with a visibly different join effect, activate v2. Click
  the existing room's join button → the v1 effect runs (not v2). Then remove the
  bot's capability or escalate a role and confirm fail-closed.

## Explicit limitations

- Per-click snapshot adds one `roles` + one `guild_member` Discord call per
  `InstanceAction`, after Defer. No caching in this cut; future optimization: short
  TTL, `GuildRoleUpdate`-driven invalidation, capability/binding revision cache.
- Orphan boundary on mid-`run` failure is unchanged (a mutation may have run before
  the failing step; reconciliation is a later phase).
- `InvalidEventRoute` is a runtime guard, not a compile-time event-type split; a
  future refinement could make misrouting unrepresentable by narrowing
  `handle_event`'s event type.

## Roadmap

- 18c-4 Safe RuleSet Activation Service: `store.get_version(target)` → the same
  readiness gate → `store.activate`; on gate failure the active pointer is
  unchanged. Now safe to build because past instances keep their pinned semantics.
- 18d RouteId + idempotent installation + attach-after-register.
- 18e Version rollback live: activate v2 → new rooms v2 / existing rooms pinned v1
  → rollback.
