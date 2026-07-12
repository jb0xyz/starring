# Phase 18d-3: Durable Instance Teardown — Design

## Goal

Delete an instance's complete footprint cleanly and durably. A `TeardownInstance`
action (or, later, an admin operation) transitions the instance `Active → Deleting`,
deletes every owned resource (messages → channels → roles) idempotently, and
transitions to `Deleted` only when all resources are confirmed gone. A teardown
interrupted by a crash resumes on the next boot using the immutable footprint — no
per-resource progress is stored.

## Context

18d-2 guarantees a registered instance owns its complete footprint
(`resources = {roles, channels, messages}`, all instance-created, no shared bindings).
18d-3 consumes that footprint to tear the instance down. Today there is no teardown:
`DiscordMutationAdapter` has only create/grant/post seams, `InstanceStatus` has no
intermediate state, and rooms accumulate.

Because deletes are idempotent (`NotFound = already deleted`), resume is just
re-running the whole teardown over the preserved footprint — Discord's actual state
plus conservative `NotFound` handling *is* the progress. The only durable teardown
state is `status ∈ {Deleting, Deleted}`.

Explicitly OUT of scope (later phases): admin CLI/API surface, scheduled/auto-expiry
deletion, bulk teardown, a periodic retry worker, teardown-only re-click while
`Deleting`, and any dispatcher status-policy change.

## Global Constraints

- No code comments anywhere.
- New pure crate `automation-instance-teardown` (state machine + seams). Forbidden
  deps: `sqlx`, `twilight-*`, `automation-runtime`.
- `store.activate`-style: the teardown state-transition/progress logic lives in the
  pure crate; the Postgres store and the Twilight deleter are edge adapters.
- Fail-closed and honest guarantees: Discord + DB are not jointly atomic; a crash
  before `Deleted` leaves `Deleting`, resumed next boot. **18d-3 provides durable
  resume, NOT immediate automatic retry of a teardown that fails in a long-running
  process** — a runtime retry worker and admin retry surface are later phases.
- Single-process assumption. Multi-process safety (store-level lease/claim) is future.
- Gates: `$HOME/.cargo/bin/cargo test`, `clippy --all-targets -- -D warnings`,
  `fmt --check`. Postgres tests `#[ignore]`.

## Architecture

```
automation-instance-teardown   pure: state machine, order, idempotency, resume, types, seams
├─ automation-instance   (InstanceStore, AutomationInstance, InstanceResources, InstanceStatus, InstanceId)
└─ discord-model         (GuildId, RoleId, ChannelId, MessageId)

automation-instance-postgres   InstanceStore additions (transition_to_deleting / mark_deleted / list_deleting) + migration
automation-core                ActionSpec::TeardownInstance + run() thin seam + AutomationServices.teardown + validate
automation-runtime             TwilightInstanceDeleter + bounded boot-resume sweep
interaction-smoke              construct Teardown service + wire boot resume + StudyRoom close button/rule
```

`automation-core` gains a dependency on `automation-instance-teardown` (for the
`InstanceTeardownService` trait in its `AutomationServices` bundle). No cycle:
`automation-instance-teardown` does not depend on `automation-core`.

## Domain (`automation-instance-teardown`)

```rust
use discord_model::{ChannelId, GuildId, MessageId, RoleId};
use automation_instance::{InstanceId, InstanceStoreError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceResource {
    Role { alias: String, id: RoleId },
    Channel { alias: String, id: ChannelId },
    Message { alias: String, channel: ChannelId, id: MessageId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    AlreadyGone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleterErrorKind {
    Forbidden,
    RateLimited,
    Network,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleterError {
    pub kind: DeleterErrorKind,
    pub message: String,
}

#[allow(async_fn_in_trait)]
pub trait InstanceDeleter {
    async fn delete_message(&self, guild: GuildId, channel: ChannelId, message: MessageId)
        -> Result<DeleteOutcome, DeleterError>;
    async fn delete_channel(&self, guild: GuildId, channel: ChannelId)
        -> Result<DeleteOutcome, DeleterError>;
    async fn delete_role(&self, guild: GuildId, role: RoleId)
        -> Result<DeleteOutcome, DeleterError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeardownOutcome {
    Completed,
    ResumedAndCompleted,
    AlreadyDeleted,
    InProgress,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeardownError {
    Lookup(InstanceStoreError),
    InstanceNotFound,
    DeleteFailed { resource: InstanceResource, source: DeleterError },
    Store(InstanceStoreError),
}

#[allow(async_fn_in_trait)]
pub trait InstanceTeardownService {
    async fn teardown(&self, guild_id: GuildId, instance_id: InstanceId)
        -> Result<TeardownOutcome, TeardownError>;
}
```

The concrete `Teardown<S: InstanceStore, D: InstanceDeleter>` holds the store, the
deleter, and an in-process **keyed lock registry** (per `instance_id`), and
implements `InstanceTeardownService`.

## The teardown state machine

`teardown(guild, id)`:

```
1. keyed try-lock(instance_id)
     fail (another teardown in flight for this id) -> Ok(InProgress)      no store read, no deletes
2. instance = store.get(guild, id)                 Err -> Lookup ; None -> InstanceNotFound
3. match instance.status:
     Deleted  -> release lock, Ok(AlreadyDeleted)
     Active   -> store.transition_to_deleting(guild, id)  (CAS: SET Deleting WHERE status=Active)
                 first_owner = true
     Deleting -> first_owner = false                      (resume)
4. delete footprint in deterministic order (stable within each kind):
     for msg in instance.resources.messages  (sorted by alias):
        deleter.delete_message(guild, channel_of_hub_or_room, msg)   Err -> return DeleteFailed{resource, source}
     for ch in instance.resources.channels    (sorted by alias):
        deleter.delete_channel(guild, ch)                            Err -> return DeleteFailed{resource, source}
     for role in instance.resources.roles      (sorted by alias):
        deleter.delete_role(guild, role)                             Err -> return DeleteFailed{resource, source}
     each Ok(Deleted | AlreadyGone) -> continue
5. store.mark_deleted(guild, id)  (Deleting -> Deleted)               Err -> Store
6. release lock
     Ok(if first_owner { Completed } else { ResumedAndCompleted })
```

- **Footprint is never mutated** as a progress record — it stays the immutable "what
  this instance owned." Resume (a later call) re-runs step 4 over the same footprint;
  already-deleted resources return `AlreadyGone` and are skipped.
- **Any deleter `Err` stops immediately, leaves `status = Deleting`, does NOT mark
  `Deleted`**, and returns `DeleteFailed { resource, source }` so logs identify which
  role/channel/message blocked and why (`Forbidden` vs `RateLimited`/`Network`).
- **Conservative `AlreadyGone`**: the deleter maps ONLY exact Discord
  Unknown-Message/Channel/Role codes to `Ok(AlreadyGone)`. Any error where the
  resource's existence is uncertain is `Err` (never treated as gone), so a `Deleted`
  instance never leaves a live resource behind.
- The `Message` variant carries its channel because a hub-entry message lives in a
  shared channel and must be deleted explicitly (deleting the room channel does not
  remove it). Room-channel messages return `AlreadyGone` after the channel is deleted.

### Deletable footprint: messages carry their channel (model change)

A message cannot be deleted without its channel, and the hub-entry message lives in a
**shared** channel that channel-deletion never removes — so it must be deleted
explicitly. But today `InstanceResources.messages: BTreeMap<String, MessageId>` stores
no channel, making the footprint not independently deletable.

18d-3 makes the footprint deletable by carrying each message's channel:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceMessageRef {
    pub channel: ChannelId,
    pub id: MessageId,
}
```

`InstanceResources.messages` changes from `BTreeMap<String, MessageId>` to
`BTreeMap<String, InstanceMessageRef>`.

Threading (all channel info already exists — `PostPanel` returns
`CreatedResource::Message { channel, id }`):
- `run()`'s `created_messages: BTreeMap<String, MessageId>` → `BTreeMap<String, InstanceMessageRef>`
  (the `PostPanel` arm already has `channel_id`).
- `resolve_manifest` writes `InstanceMessageRef { channel, id }` into the manifest.
- Nothing reads `resources.messages` at runtime today (it is write-only, captured at
  register), so no runtime consumer breaks — only the persisted JSONB shape changes
  (serde handles it; no message migration) and 18d-2 tests that assert
  `resources.messages` values update (compiler-guided).

The teardown reads `resources.messages` → `InstanceResource::Message { channel, id }` →
`deleter.delete_message(guild, channel, id)`. Explicitly deleting every footprint
message first (each with its channel) handles both room-channel messages (which
return `AlreadyGone` after the room channel is later deleted) and the hub message in
the shared channel (removed only by this explicit delete).

## Status policy (Ⓐ boot-only) — no dispatcher change

```
Active   -> normal InstanceActions allowed, including TeardownInstance
Deleting -> ALL InstanceActions rejected (existing 18c-3 ensure_active: status != Active -> InstanceInactive)
Deleted  -> ALL InstanceActions rejected
```

The first `TeardownInstance` click is on an `Active` instance (ensure_active passes);
the service then transitions it to `Deleting`, which blocks all subsequent clicks.
Re-click retry is intentionally NOT supported. The dispatcher and its validator are
unchanged.

## `run()` thin seam + response separation

`AutomationServices` gains `teardown: &T` (`T: InstanceTeardownService`). The
`TeardownInstance` arm of `run()`:

```
resolve InstanceRef -> instance_id   (Event -> context.instance ; Created -> created/planned)
outcome = services.teardown.teardown(guild, instance_id).await
record outcome as the action result
```

`run()` does NOT delete resources, transition status, or track progress. The teardown
result is recorded independently of the response: with the rule shape
`[DeferEphemeral, TeardownInstance, EditResponse]`, a `Completed`/`ResumedAndCompleted`/
`AlreadyDeleted` teardown followed by a failed best-effort `EditResponse` must NOT
revert the teardown or re-run the plan. `InProgress` is a normal outcome ("already
being closed"), not an error, and issues no new deletes.

```rust
pub struct TeardownActionResult {
    pub teardown: TeardownOutcome,
    pub response: ResponseDeliveryOutcome,
}
pub enum ResponseDeliveryOutcome { Sent, Failed }
```

If the existing `run()` result model cannot carry this, minimally: record the
`TeardownInstance` step as success on any `TeardownOutcome`, record a later
`EditResponse` failure separately, and ensure the runtime does not auto-retry the
whole plan on that response failure.

## Store / model changes (`automation-instance` + `-postgres`)

- `InstanceStatus` gains `Deleting` (serde `"deleting"`).
- `InstanceResources.messages` value: `MessageId` → `InstanceMessageRef { channel, id }`
  (deletable footprint; see above). JSONB shape change only, no message migration.
- `InstanceStore` gains:
  - `transition_to_deleting(guild, id) -> Result<(), InstanceStoreError>` — CAS
    `SET status='deleting' WHERE status='active'` (single atomic statement; used
    after reading `Active` under the lock).
  - `mark_deleted(guild, id) -> Result<(), InstanceStoreError>` — `SET status='deleted'
    WHERE status='deleting'`.
  - `list_deleting(guild) -> Result<Vec<AutomationInstance>, InstanceStoreError>` (or
    `list_by_status`) — for bounded boot resume; a dedicated query, not load-all-and-filter.
- Postgres migration `202607120003_*`: add `'deleting'` to the `automation_instances`
  status CHECK constraint.

## Boot resume (bounded, best-effort, before gateway)

At boot, after DB/hydration and before the gateway starts:

```
for each guild the bot serves:
  pending = store.list_deleting(guild)
  bounded sweep over pending:
     - at most N concurrent teardowns
     - per-instance timeout T
     - teardown(guild, id) via the SAME service
     - Err / timeout -> leave Deleting, log (resource + kind for DeleteFailed)
     - finite termination guaranteed
gateway starts regardless of sweep failures
```

A stuck teardown (`Forbidden`, or timeout) never blocks startup — the instance stays
`Deleting` (which the dispatcher already rejects), and the bounded sweep guarantees
the boot completes. `Deleting` with an old `updated_at` is the operator's
"needs-attention" signal; no `blocked_reason` column is added. Limitation: each boot
re-attempts a `Forbidden` teardown once; no periodic in-process retry.

## Edge + fixture

- `TwilightInstanceDeleter` (in `automation-runtime`) implements `InstanceDeleter`:
  `http.delete_message`/`delete_channel`/`delete_role`; success → `Deleted`; exact
  Unknown-Message/Channel/Role → `Ok(AlreadyGone)`; Forbidden/RateLimited/Network/other
  → `Err(DeleterError { kind, .. })` via the existing error classifier.
- The tool constructs `Teardown::new(store, deleter)`, threads it into
  `AutomationServices.teardown`, and runs the bounded boot sweep before `gateway::run`.
- StudyRoom fixture gains a **"방 닫기" close button** on the welcome panel
  (`InstanceAction { instance: Created(study_room_instance), action: "close" }`) and a
  `study_close_rule`: `[DeferEphemeral, TeardownInstance { instance: Event }, EditResponse]`.
  (A new published version.)

## Validation

`validate_structural` for `TeardownInstance`:
- Allowed only in a rule with instance context (`InstanceAction` trigger, `InstanceRef`
  resolvable — `Event` inside an `InstanceAction` rule, or a `Created` ref).
- NOT allowed in an instance-creation rule (a rule containing `RegisterInstance`) — a
  rule either creates an instance or tears one down, not both.
- No teardown-only-plan validation (that was for the rejected Ⓑ re-click path).

## Completion contract

> When teardown starts, the instance atomically becomes `Deleting` and all user
> interactions are blocked. If deletion is interrupted or the process exits, the same
> service resumes on the next boot using the immutable footprint, and the instance
> becomes `Deleted` only once every resource is deleted or confirmed already gone.

## Testing

`automation-instance-teardown` unit tests (in-memory store + a scripted mock deleter):
- Active → all resources `Deleted` → status `Deleted`, outcome `Completed`, order
  messages→channels→roles asserted.
- Mock deleter returns `AlreadyGone` for some → still `Completed` (idempotent).
- Deleter `Err(Forbidden)` on the channel → `DeleteFailed { resource: Channel, kind: Forbidden }`,
  status stays `Deleting`, role NOT attempted, no `mark_deleted`.
- Resume: pre-set `Deleting` + a footprint where messages already gone → re-run →
  `ResumedAndCompleted`, `Deleted`.
- Second concurrent call while lock held → `InProgress`, no extra delete calls.
- `Deleted` instance → `AlreadyDeleted`, no delete calls.
- Crash-before-mark: all deletes succeed but `mark_deleted` errors → `Store`, status
  stays `Deleting`; a re-run finds all `AlreadyGone` → `Deleted`.
- Conservative NotFound: deleter `Err(Unknown)` (uncertain) → treated as failure, not
  `AlreadyGone`; instance not marked `Deleted`.
- `list_deleting` returns only `Deleting` instances.
- Ignored Postgres: `transition_to_deleting` CAS (only from Active), `mark_deleted`,
  `list_deleting`, status CHECK accepts `'deleting'`, reconnect durability.

Live (reused bot/guild/local `starring`): create a StudyRoom, then click **방 닫기** →
verify the member role, room channel, welcome message, and hub join message are all
deleted in Discord, and `automation_instances.status = 'deleted'` for that instance.
Then restart-resume: kill the process mid-teardown (or leave a `Deleting` instance) →
reboot → boot sweep completes it to `Deleted`.

## Roadmap

- Admin retry API/CLI (calls the same `InstanceTeardownService`).
- Bounded periodic `Deleting` sweeper (in-process retry without restart).
- Append-only teardown event log (per-resource audit/backoff), if needed.
- Multi-process execution ownership (store-level lease/claim).
- 18e Durable RuleSet rollback live.
