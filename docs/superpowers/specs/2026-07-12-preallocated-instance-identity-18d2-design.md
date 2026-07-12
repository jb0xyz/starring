# Phase 18d-2: Preallocated Instance Identity + Complete Footprint Finalization — Design

## Goal

Make a registered `AutomationInstance` own its complete resource footprint from the
moment of registration. The instance id is preallocated before any Discord mutation,
so panels that embed the id (the hub join panel) are created before registration and
captured in the `RegisterInstance` manifest. A structural invariant enforces that
every dynamically-created ownable resource in an instance-creation rule appears in
that manifest exactly once.

## Context

The StudyRoom rule creates a role, a channel, a welcome panel, then `RegisterInstance`,
then the hub join panel (`starring:i:<instance_id>:join`). The join panel needs the
instance id, which today is generated *at* `RegisterInstance`, so the hub panel must
be posted *after* registration and cannot be in the manifest. Result: the instance's
footprint is incomplete — the hub message is untracked and would orphan on teardown.

This isn't one missed panel; it is that **an instance does not fully know what it
owns**, which blocks clean teardown, a full lifecycle, and trustworthy AI-authored
rules (created ≠ owned).

The fix is identity-first: preallocate the id, create everything (including the hub
panel) with the id available, and let `RegisterInstance` be the final step that
persists the complete manifest atomically. No post-register attach state is ever
introduced.

Explicitly OUT of scope: teardown/deletion (18d-3 Durable Instance Teardown:
Active→Deleting→Deleted, idempotent cleanup, delete seams), and RouteId (deferred
until custom_id length is an actual problem).

## Global Constraints

- No code comments anywhere (`//`, `///`, `//!`).
- **No new crate, no store change, no migration.** `RegisterInstance` captures the
  full manifest via the existing `InstanceStore::register`; the id is passed in as
  today — only *where* it is generated moves. Changes are confined to
  `automation-core` (`run.rs`, `validate.rs`), the `interaction-smoke` StudyRoom
  fixture (action reorder), and tests.
- Fail-closed and honest guarantees: identity planning runs before the first Discord
  mutation; a pre-register crash leaves no instance row (same as today's
  unregistered-creation failure); a post-register-success state always has the
  complete manifest. Discord + DB are not jointly atomic, but **no "registered but
  incomplete" durable state exists**.

## Mechanism: Preallocated Identity + Finalizing Registration

### Execution structure

`run` gains an explicit preparation pass before the action loop:

```
run(context, plan, services)
├─ prepare_instance_identities(plan, services.instance_ids) -> planned_instances   (before any mutation)
└─ execute_actions(plan, ...)
```

`prepare_instance_identities`:
1. Scans the plan for `RegisterInstance` actions.
2. For each logical instance key, generates the final id exactly once (via
   `services.instance_ids.generate()`), storing it in `planned_instances`.
3. Never changes an id after allocation.

If the id allocator fails (`InstanceIdGenerationError`), `run` returns the error
**before** executing any action — zero Discord mutations, zero instance-store calls.

### `planned_instances` vs `created_instances`

`RuntimeBindings` (the run-local execution metadata that already holds
`created_instances`) gains `planned_instances`:

```rust
struct RuntimeBindings {
    created_roles: BTreeMap<String, RoleId>,
    created_channels: BTreeMap<String, ChannelId>,
    created_messages: BTreeMap<String, MessageId>,
    planned_instances: BTreeMap<String, InstanceId>,
    created_instances: BTreeMap<String, InstanceId>,
}
```

- `planned_instances` — the final identity to use in this plan. **Temporary execution
  metadata for custom_id resolution only.** It does NOT mean the instance exists. It
  must not be used for success reporting, instance-existence judgment, a store-lookup
  substitute, or created output binding.
- `created_instances` — populated **only after `RegisterInstance`'s store call
  succeeds** (promotion). This is the sole signal that the instance actually exists.

`resolve_button_instance` for `InstanceRef::Created` resolves from `planned_instances`
(the id is known from preparation), so the hub panel's join button encodes the final
id before registration.

### `RegisterInstance` becomes finalizing

`RegisterInstance` no longer generates an id. It:
1. Looks up the preallocated id in `planned_instances` (missing → internal error,
   should be unreachable given preparation).
2. Resolves the full manifest from `created_roles`/`created_channels`/`created_messages`.
3. Calls `store.register(instance with the exact planned id and full resources)`.
4. On success, promotes the id into `created_instances` and records
   `CreatedResource::Instance`.

### ID collision — no reallocation

If `store.register` returns `DuplicateInstance` (astronomically rare with random 60-bit
ids), `RegisterInstance` fails — it does **not** reallocate a new id, because the hub
panel custom_id already embeds the planned id. Result: failure response, no promotion,
Discord panels may orphan. A panel with an orphan id is safe: the pinned dispatcher
(18c-3) resolves it to `InstanceNotFound` and fails closed. A wrong-new-id durable
instance would be worse, so this is the safe choice. Orphan detection is a future
creation-reconciliation concern.

## Structural validation (L2 — complete footprint)

A rule is an *instance-creation rule* iff it contains a `RegisterInstance`. For such
rules, `validate_structural` enforces `created_resources == manifest_resources`
(each exactly once), building on the existing `check_manifest` (17b), which already
verifies every manifest `CreatedRef` resolves to a created resource of the matching
kind (`UnknownCreatedRoleRef`/`ChannelRef`/`MessageRef`) and `EmptyInstanceResources`.

Ownable resource outputs (must be in the manifest): `CreateRole`→Role,
`CreateChannel`→Channel, `PostPanel`→Message. Non-ownable actions (never manifest
targets, they are state changes not independently-deletable resources): `GrantRole`,
`UpsertOverwrite`, `DeferEphemeral`, `EditResponse`.

`InstanceResourceRefs` is `CreatedRef`-only, so the manifest structurally cannot name
a shared `ResourceBindingMap` entry — owning an existing shared resource (hub channel,
`@everyone`, moderator role) is impossible by construction. The hub panel's message is
this rule's own output → in the manifest; the hub *channel* is a shared binding → not.

New checks added on top of `check_manifest`:

```
created_ownable = { keys of CreateRole/CreateChannel/PostPanel in the rule }
manifest_keys   = multiset of CreatedRef.created across RegisterInstance.resources

1. completeness : every created_ownable key appears in manifest_keys
2. no-duplicate : no created key appears in manifest_keys more than once
3. one-register : at most one RegisterInstance per rule
4. register-last: no resource-producing action (CreateRole/CreateChannel/PostPanel)
                  appears after the RegisterInstance
```

Set comparison alone is insufficient (it would pass "hub twice, welcome missing"), so
duplicate detection is a separate check.

New `ValidationError` variants:

```rust
InstanceResourceMissingFromManifest { key: String, kind: CreatedKind },
InstanceResourceDeclaredMultipleTimes { key: String },
InstanceResourceProducedAfterRegister { key: String, kind: CreatedKind },
MultipleRegisterInstance { rule: String },
```

Manifest-references-uncreated and kind-mismatch remain covered by the existing
`UnknownCreatedRoleRef`/`ChannelRef`/`MessageRef` from `check_manifest`.

## Planning-failure and crash semantics

```
validate (publish/readiness time)          structural invariant holds before anything runs
prepare_instance_identities (run start)    id allocation; failure -> 0 mutations, 0 store calls
execute actions                            creates, panels (planned id), RegisterInstance (finalize)
```

- Pre-register crash → no instance row; some Discord resources may orphan → same as
  today's unregistered-creation-failure category.
- Post-register-success crash → an instance row with the complete manifest → teardown-able.
- Not atomic across Discord + DB, but the "durable instance stored incomplete" state
  is eliminated.

Precise guarantee:

> The manifest declared in `RegisterInstance` is persisted in one call at
> registration, and no post-register attach state exists. Combined with the L2
> completeness invariant, a registered instance's manifest objectively equals the set
> of ownable resources its rule created.

## StudyRoom rule reorder (fixture)

```
DeferEphemeral
CreateRole        (study_member_role)
CreateChannel     (study_channel)
UpsertOverwrite   (everyone deny view)
UpsertOverwrite   (member role allow view)
GrantRole         (member role to actor)
PostPanel         (welcome, in study_channel)
PostPanel         (hub join, in existing study_hub, InstanceAction join using the planned id)
RegisterInstance  (roles: member_role, channels: room_channel, messages: welcome_panel + hub_panel)
EditResponse
```

`RegisterInstance` moves to just before the terminal `EditResponse`, after both panels,
and its manifest gains the hub join message. This is the only fixture change; the
readiness gate, the content hash, etc. shift accordingly (a new published version of
the studyroom ruleset).

## Testing

`automation-core` unit tests:
1. `prepare_instance_identities` allocates one id per `RegisterInstance`, before the loop.
2. Hub `PostPanel` before `RegisterInstance` resolves its `InstanceRef::Created` from
   `planned_instances` (id present pre-register).
3. `RegisterInstance` registers with the exact planned id (not a freshly generated one).
4. `created_instances` is populated only after a successful `register`; a failing
   `register` leaves `created_instances` empty.
5. `store.register` receives resources containing role, channel, welcome, hub.
6. `validate_structural` rejects a rule that omits any created resource from the
   manifest (`InstanceResourceMissingFromManifest`).
7. A manifest referencing an uncreated key / a shared binding key fails
   (`UnknownCreated*Ref`).
8. A resource-producing action after `RegisterInstance` fails
   (`InstanceResourceProducedAfterRegister`); two `RegisterInstance` fail
   (`MultipleRegisterInstance`); a doubly-declared resource fails
   (`InstanceResourceDeclaredMultipleTimes`).
9. `store.register` `DuplicateInstance` → error, no id reallocation, no promotion.
10. Failure before `RegisterInstance` → no instance registered (store `register`
    call count 0 for that instance).

Live (reused bot/guild/local `starring`): create a StudyRoom, then inspect
`automation_instances.resources` for the new instance — it contains **role 1, channel 1,
welcome message 1, hub join message 1** (previously the hub message was absent).

## Roadmap

- 18d-3 Durable Instance Teardown: `Active → Deleting → Deleted`, idempotent resource
  cleanup in a safe order (messages → channel → role), distinguishing
  NotFound/Forbidden/RateLimited, resumable after restart, never deleting shared
  bindings. This is the payoff that consumes the complete footprint 18d-2 guarantees.
- RouteId: compact custom_id token, when length becomes a real constraint.
- 18e Durable RuleSet rollback live: the full arc end to end.
