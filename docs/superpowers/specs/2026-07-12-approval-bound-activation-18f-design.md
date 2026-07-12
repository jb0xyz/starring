# Phase 18f: Approval-Bound Activation — Design

## Goal

Unify the authority that changes the active RuleSet pointer. Concretely:

> Operational RuleSet activation happens only through a durable approval request
> bound to a specific immutable target. After a quorum of distinct authenticated
> actors approves, a leased application service re-verifies readiness against a
> fresh Discord snapshot at execution time and then changes the active pointer.
> Crashes and concurrent execution converge through per-request and per-RuleSet
> identity CAS, and no CLI, API, or UI may call the low-level activation path
> directly.

This closes the last operational-activation bypass: 18c-4 stopped raw
`store.activate`, but `activate_if_ready` itself is still a direct operational
path. After 18f, only the activation authority's apply service mutates the active
pointer.

## Nature and Non-Goals

This is a safety-boundary phase, not a new user feature. It adds a Layer 2
activation authority (a pure core + a PostgreSQL adapter) and restructures the
tool's activation surface. It deliberately does not add: production API/UI,
CI/CURRENT_STATE.md (the next stage-1 steps), or any Layer 1 change.

Explicitly OUT of scope:

- Baseline-CAS activation (`require_unchanged_baseline`). The approval binds a
  target, not a transition path. `observed_active` is recorded as non-binding
  audit metadata only.
- Authorization (which identities may approve). The core validates distinct
  authenticated `UserId`s; a trusted edge decides who is authorized.
- Any change to the Layer 1 approval pipeline.

## Code Scope Guard

```
Allowed to modify / create:
  crates/automation-ruleset-activation/            (new, pure core)
  crates/automation-ruleset-activation-postgres/   (new, adapter)
  tools/interaction-smoke/                          (CLI restructure + edge impls)
  migrations/                                       (new activation tables)
  Cargo.toml                                        (workspace members)

Forbidden to modify:
  crates/approval-manager/  crates/policy-engine/  crates/preview/  (Layer 1)
  crates/automation-ruleset/  crates/automation-ruleset-postgres/   (RuleSet store)
  crates/automation-ruleset-readiness/                              (reuse only)
```

`automation-ruleset-readiness` is consumed through its existing public API. If
the apply service needs something not currently public there, that is a
**separate defect to report and scope**, not to silently change inside 18f.

## Context

- The existing `approval-manager` crate is Layer 1 only: `ApprovalRequest` is a
  value object coupled to `policy_engine::Verdict` and `preview::PreviewModel`,
  with no target binding and no persistence. It is not reused here (reusing it
  would drag `policy-engine` and `preview` into Layer 2 and, worse, make two
  different approval meanings look like one model). A shared `approval-core`
  extraction is deferred until the two approval surfaces actually converge.
- The current RuleSet activation surface is
  `automation-ruleset-readiness::activate_if_ready` (readiness gate +
  `store.activate`), reachable directly from the tool. That direct reachability
  is the bypass 18f removes.

## Global Constraints

- No code comments anywhere (`//`, `///`, `//!`).
- Layer 1 untouched; RuleSet store untouched; readiness reuse-only.
- PostgreSQL `NOW()` is the sole authority for time-based decisions (expiry,
  lease). Application-server clocks are never used to judge expiry or lease.
- Every state transition is a token/state CAS. No read-modify-write of a JSON
  array for approvals or state.
- Correctness must never depend on a background sweep. Every operation
  re-derives expiry from `NOW()` on access.

## Architecture

Two new crates plus the tool.

### `automation-ruleset-activation` (pure core)

Owns the domain and the application service. Regular deps: `automation-ruleset`
(for `RuleSetKey`/`RuleSetVersionId`/`RuleSetContentHash`/`RuleSetStore` to look
up the target artifact and current active), `automation-ruleset-readiness` (for
`GuildCapabilities`, `check_readiness`/`activate_if_ready`, `ActivationOutcome`),
`discord-model`, `resource-resolution` (`ResourceBindingMap`), `desired-state`
(`ResourceKey`), `serde`, `thiserror`. **Forbidden deps**: `sqlx`, `twilight`,
`approval-manager`, `policy-engine`, `preview`, `automation-ruleset-dispatch`.

No cycle: readiness and the RuleSet store do not depend on activation.

### `automation-ruleset-activation-postgres` (adapter)

Owns: the migration (both tables, the partial unique index, all `CHECK`
constraints), `PostgresActivationRequestStore`, row↔domain conversion, every
transition as an attempt/state CAS or `SELECT ... FOR UPDATE` transaction, and
the ignored PostgreSQL integration tests. `sqlx` lives only here.

### `interaction-smoke` (tool)

- CLI restructured to `request-activation` / `approve-activation` /
  `reject-activation` / `apply-activation` / `resume-activation`; `seed-studyroom
  --activate` and the standalone `activate <version>` are removed (they bypassed
  the authority).
- Implements `ActivationEnvironmentProvider` with Twilight (reusing the existing
  `readiness_context` logic). It lives in the tool for this phase; promoting it
  to `automation-runtime` is deferred until a second consumer (the API) exists.
- The gateway boot performs the already-active bookkeeping sweep (see Boot
  Recovery), mirroring the 18d-3 teardown-resume wiring.
- `unsafe-dev-activate` is a compile-feature-gated command that calls the
  authority's feature-gated internal function (see below).

## Domain Model

### `ActivationTarget`

```rust
pub struct ActivationTarget {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub content_hash: RuleSetContentHash,
}
```

The approver authorizes making exactly this immutable target active. The target
is captured at request creation and never re-specified afterward.

### `ActivationRequest`

```rust
pub struct ActivationRequest {
    pub id: ActivationRequestId,
    pub target: ActivationTarget,
    pub requester: UserId,
    pub required_approvals: u32,
    pub approvals: Vec<Approval>,
    pub state: ActivationRequestState,
    pub rejection: Option<Rejection>,
    pub apply_attempt_id: Option<ApplyAttemptId>,
    pub apply_attempt_no: u64,
    pub last_apply_error: Option<ApplyErrorRecord>,
    pub observed_active: Option<ObservedActive>,
    pub completion: Option<Completion>,
}
```

`ActivationRequestId` and `ApplyAttemptId` are opaque validated newtypes
generated by the edge and passed in (mirroring how `InstanceId` is minted at the
edge). `apply_lease_until`, `created_at`, `expires_at`, `applied_at`,
`rejected_at` are DB-time columns surfaced into the domain as needed but written
only via `NOW()` in SQL. `Approval { approver: UserId, approved_at }`.
`Completion { applied_at, applied_by, kind: CompletionKind, notices: Option<Vec<..>> }`.

### State machine

```
Pending ──quorum reached──────────────▶ Approved
   ├──authorized reject───────────────▶ Rejected        (terminal, approvals kept)
   └──expires_at < NOW() on access────▶ Expired         (terminal)

Approved ──apply claim (CAS, before expiry)──▶ Applying
   └──expires_at < NOW() on claim──────────────▶ Expired

Applying ──activation succeeded / already-active──▶ Applied     (terminal)
   ├──known-safe failure (owned attempt)─────────▶ Approved     (approvals kept, last_apply_error)
   └──crash / indeterminate outcome──────────────▶ Applying     (recoverable)

Applied / Rejected / Expired : terminal
```

`Applying` means exactly "an attempt is mid-flight or its pointer-mutation
outcome is not yet reflected in the journal." Known failures release to
`Approved`; only indeterminate outcomes stay `Applying`. There is no separate
`Failed` state.

TTL applies to `Pending` and `Approved` (checked at every approve/reject/apply
claim). Once `Applying`, expiry never discards the request — an in-flight
attempt must remain recoverable.

### Outcomes and errors

```rust
pub enum ApplyOutcome {
    Activated,
    AlreadyActive,
    RecoveredAlreadyActive,
    AlreadyApplied,
    InProgress {
        blocking_request_id: ActivationRequestId,
        lease_until: DateTime,
        lease_expired: bool,
    },
}

pub enum ApplyError {
    NotApproved,
    Expired,
    LeaseLost,
    TargetCorrupt,
    Store(ActivationStoreError),
}

pub enum ApproveError {
    SelfApprovalForbidden,
    DuplicateApproval,
    NotPending,
    Expired,
    Store(ActivationStoreError),
}

pub enum CompletionKind { Activated, AlreadyActive, CrashRecovered }
```

## Approval Semantics

Enforced in the pure core, atomically in the adapter:

- `required_approvals >= 1`, fixed at request creation. A later policy change
  never alters an existing request's requirement.
- `approver == requester` → `SelfApprovalForbidden`. The requester is never in
  the approver set; the default policy therefore requires at least two people.
- A repeat approval by the same `approver_id` → `DuplicateApproval`. Quorum
  counts **distinct** `approver_id`.
- Reaching `required_approvals` distinct approvals transitions `Pending →
  Approved`.
- Any approve/reject after `Approved` → `NotPending` (`InvalidStateTransition`).
- A single authorized reject transitions `Pending → Rejected` (terminal,
  approvals preserved). No separate rejection quorum.
- Authorization is out of core: the core accepts authenticated `UserId`s and
  validates self-approval/duplicate/quorum/state. A trusted edge decides who may
  approve. **Honest limitation (state in the spec):** the `interaction-smoke`
  CLI's manual `--actor` input is workflow-validation metadata only and provides
  no production identity assurance; a real API must derive the `UserId` from an
  authenticated principal and never trust a request-body `approver_id`.
- `applied_by` is audit metadata; the requester may apply an already-approved
  request (apply executes an already-authorized intent, it is not a new approval).

## Fresh Readiness Context (service-controlled)

The service — not the caller — decides when to load a fresh snapshot, so a caller
can never inject a stale environment.

```rust
pub struct ActivationEnvironment {
    pub bindings: ResourceBindingMap,
    pub guild_capabilities: GuildCapabilities,
    pub role_permissions: BTreeMap<ResourceKey, Permissions>,
}

#[allow(async_fn_in_trait)]
pub trait ActivationEnvironmentProvider {
    async fn load_fresh(
        &self,
        target: &ActivationTarget,
    ) -> Result<ActivationEnvironment, ActivationEnvironmentError>;
}
```

The trait is defined in the activation crate, independent of
`automation-ruleset-dispatch`'s snapshot trait (avoiding a cycle). The same
Twilight adapter type may implement both traits. The edge assembles the Twilight
snapshot and bindings; the service controls *when* `load_fresh` runs (step 7
below), and only after the already-active short-circuit so a no-op activation
performs zero snapshot and zero readiness work.

## Apply Ownership (lease)

- Claim `Approved → Applying`: CAS `WHERE state='approved' AND expires_at > NOW()`
  setting `apply_attempt_id = <new token>`, `apply_attempt_no = apply_attempt_no
  + 1`, `apply_lease_until = NOW() + 60s`.
- `apply` claims **only** `Approved`. If the request is `Applying`, `apply`
  returns `InProgress` (with the blocking request and lease info) and never
  auto-resumes.
- `resume` reclaims **only** an expired-lease `Applying`: CAS `WHERE
  state='applying' AND apply_lease_until < NOW()` with a new token, `attempt_no +
  1`, new lease. A live lease → `InProgress`. There is no `--force-resume`
  (stealing a live lease risks concurrent pointer mutation).
- Every completion/release is attempt-token-guarded: `... WHERE id=$id AND
  state='applying' AND apply_attempt_id=$attempt_id`. A lost-claim attempt cannot
  write `Applied`, release to `Approved`, or update error metadata.
- Lease 60s, total apply deadline 45s (< lease). Just before the pointer
  mutation, `renew_apply_lease(id, attempt_id, 60s)` re-validates ownership; on
  failure the service returns `LeaseLost` with **zero** `activate` calls.

## Apply Algorithm

```
1.  load request by request_id
2.  terminal → AlreadyApplied / NotApproved; expires_at < NOW() → CAS to Expired, return Expired
3.  claim Approved → Applying (attempt token + 60s lease);
      Applying (other) for same (guild,key) → InProgress{blocking, lease, expired}
4.  look up target artifact by target.version; verify artifact.content_hash == target.content_hash
      missing → TargetMissing; version present but hash differs → TargetCorrupt
5.  read current active (store.active(guild,key)) and its artifact
6.  active.version == target.version AND active.content_hash == target.content_hash
      → CAS Applying → Applied (completion=AlreadyActive), zero snapshot, zero readiness
      version equal but hash differs → TargetCorrupt (store invariant violation)
7.  provider.load_fresh(target)  (fresh Discord snapshot + capabilities + role perms)
8.  enforce total operation deadline (< lease)
9.  renew_apply_lease just before mutation; failure → LeaseLost, no activate
10. activate_if_ready(store, guild, key, version, env.bindings, env.caps, env.role_permissions)
11. attempt-token CAS Applying → Applied (completion=Activated, notices from ActivationOutcome)
```

Known-safe failures (owned attempt, active pointer provably unchanged) —
`TargetMissing`, `TargetCorrupt`, snapshot/context failure, `NotReady`, an
`activate_if_ready` failure that guarantees the pointer is unchanged:

```
attempt-token CAS Applying → Approved
  clear apply_attempt_id + apply_lease_until
  keep approvals
  set last_apply_error
```

Indeterminate outcomes (timeout, connection loss, pointer-mutation result
unknown): keep `Applying` for lease-expiry `resume` or boot bookkeeping. Never
release an indeterminate attempt.

`last_apply_error` is overwritten by a new attempt without separate history
(documented audit limitation).

## Per-(guild,key) Serialization

```sql
CREATE UNIQUE INDEX activation_requests_one_applying_per_ruleset
ON activation_requests (guild_id, ruleset_key)
WHERE state = 'applying';
```

At most one `Applying` per `(guild_id, ruleset_key)`. A claim that would create a
second `Applying` for the same identity raises this specific partial-unique
violation; the adapter maps **only that named constraint** to `InProgress {
blocking_request_id, lease_until, lease_expired }` (querying the blocking row for
the detail) and leaves any other unique/DB error as a store error. An
indeterminate `Applying` therefore safely blocks subsequent activations for that
RuleSet until it is resolved (bookkeeping → `Applied`, or `resume` → `Applied` /
`Approved`); `apply B` never auto-recovers a blocking `A` — the operator must
`resume A` explicitly.

## Boot Recovery (bookkeeping only)

Before the gateway starts, sweep `Applying` requests. For each, **only** when

```
active.version == target.version AND active.content_hash == target.content_hash
```

do `CAS Applying → Applied` (`completion=CrashRecovered`, clear attempt/lease,
notices unavailable). This changes no pointer. If an old holder later tries to
complete, the row is no longer `Applying` and its token CAS fails.

All other cases (`active != target`, hash mismatch, target lookup error, store
error) keep the request `Applying`, log, and continue. Boot recovery never:
loads a Discord snapshot, runs readiness, calls `activate_if_ready`, or changes
the active pointer. A process that died before activating is therefore never
silently activated by a reboot; an operator must `resume` it.

Recovery matrix:

| Request  | actual active        | handling                          |
| -------- | -------------------- | --------------------------------- |
| Applying | exact target + hash  | bookkeeping → Applied             |
| Applying | different version    | keep Applying, no auto-activation |
| Applying | target artifact gone | keep Applying, log corruption     |
| Applying | lookup/store error   | keep Applying, next request       |
| Approved | exact target + hash  | apply completes AlreadyActive     |
| Applied  | anything             | AlreadyApplied, no re-consume     |

## The Single Pointer-Mutation Invariant

The operational active-pointer change happens only inside the activation
authority's apply path. A dependency/source guard test enforces the allowlist:

```
activate_if_ready direct call allowed:
  - inside automation-ruleset-activation
  - automation-ruleset-readiness's own tests

store.activate direct call allowed:
  - inside automation-ruleset-readiness
  - low-level store tests

forbidden everywhere else:
  interaction-smoke, future API handlers, runtime operational paths
```

## `unsafe-dev-activate`

A compile-feature-gated escape hatch that skips the request + human approval but
keeps every technical safeguard, implemented **inside** the authority so no
bypass reimplementation appears in the tool.

```rust
#[cfg(feature = "unsafe-dev-activation")]
pub async fn unsafe_dev_activate<S, P>(
    store: &S,
    provider: &P,
    target: ActivationTarget,
    applied_by: UserId,
) -> Result<ApplyOutcome, ApplyError>;
```

```
skips:  ActivationRequest, human approval, lease
keeps:  target/hash verification, already-active short-circuit,
        provider.load_fresh, readiness, activate_if_ready, notices
```

Requirements: distinct command name (never `--force` on a normal command);
absent from any build without the feature (`#[cfg(feature = ...)]` removes the
symbol and the CLI command); a startup warning + audit log when used. The tool
calls only this feature-gated authority API.

## Persistence Schema

Two tables. Approvals are their own rows (never a JSON array read-modify-write).

```
activation_requests
  id                    TEXT PRIMARY KEY
  guild_id              TEXT NOT NULL
  ruleset_key           TEXT NOT NULL
  target_version        BIGINT NOT NULL
  target_content_hash   TEXT NOT NULL
  requester_id          TEXT NOT NULL
  required_approvals    INT  NOT NULL
  state                 TEXT NOT NULL
  created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
  expires_at            TIMESTAMPTZ NOT NULL
  apply_attempt_id      TEXT
  apply_attempt_no      BIGINT NOT NULL DEFAULT 0
  apply_lease_until     TIMESTAMPTZ
  last_apply_error      JSONB
  observed_active_version   BIGINT
  observed_active_hash      TEXT
  applied_at            TIMESTAMPTZ
  applied_by            TEXT
  completion_kind       TEXT
  activation_notices    JSONB
  rejected_at           TIMESTAMPTZ
  rejected_by           TEXT
  rejection_reason      TEXT

activation_request_approvals
  request_id    TEXT NOT NULL REFERENCES activation_requests(id)
  approver_id   TEXT NOT NULL
  approved_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
  PRIMARY KEY (request_id, approver_id)
```

Constraints:

```sql
CHECK (required_approvals >= 1)
CHECK (apply_attempt_no >= 0)
CHECK (expires_at > created_at)
CHECK (state IN ('pending','approved','applying','applied','rejected','expired'))
CHECK (
  (state = 'applying' AND apply_attempt_id IS NOT NULL AND apply_lease_until IS NOT NULL)
  OR
  (state <> 'applying' AND apply_attempt_id IS NULL AND apply_lease_until IS NULL)
)
CHECK (state <> 'applied'  OR (applied_at IS NOT NULL AND applied_by IS NOT NULL AND completion_kind IS NOT NULL))
CHECK (state <> 'rejected' OR (rejected_at IS NOT NULL AND rejected_by IS NOT NULL))
```

Approve/reject atomicity — one transaction per operation:

```
BEGIN
  SELECT ... FROM activation_requests WHERE id = $id FOR UPDATE
  expires_at < NOW()  → UPDATE state='expired'; COMMIT; return Expired
  state <> 'pending'  → COMMIT; return NotPending
  (approve) approver == requester → ROLLBACK; return SelfApprovalForbidden
  (approve) INSERT INTO activation_request_approvals ...   (PK dup → DuplicateApproval)
  (approve) count distinct approvals; if >= required_approvals → UPDATE state='approved'
  (reject)  UPDATE state='rejected', rejected_* = ...
COMMIT
```

Locking the request row `FOR UPDATE` serializes a racing last-approve and reject
so they cannot both succeed (never both `Approved` and `Rejected`). All
`activation_requests` state changes are CAS/`FOR UPDATE`-guarded — the apply
attempt transitions are token CAS, and approve/reject are `FOR UPDATE`
transactions, so the safety model is uniform rather than "apply is CAS but
approve is a plain update."

## request_id-only Execution Surface

```
request-activation  guild key version    → look up target hash, mint id, persist Pending
approve-activation  <request_id>          → --actor <user>
reject-activation   <request_id>          → --actor <user> --reason <text>
apply-activation    <request_id>          → --actor <user>   (claims Approved only)
resume-activation   <request_id>          → --actor <user>   (reclaims expired Applying only)
```

`apply` and `resume` accept **only** `request_id`. Re-passing
`guild/key/version/hash` to apply/resume is forbidden; the executed target is
read solely from the stored `ActivationRequest.target`, structurally preventing
target substitution.

## Testing

Pure-core unit tests (no DB, no Discord), using a spy `ActivationRequestStore`
and a spy `ActivationEnvironmentProvider`:

- Approval: self-approval rejected; one distinct approver + required=1 → Approved;
  duplicate approver rejected; required=2 needs two distinct; single reject →
  Rejected; approve/reject after Approved rejected; approve after Expired
  rejected; stored `required_approvals` survives a policy change; requester may
  apply an approved request.
- Apply state machine: `apply` on Pending → NotApproved; on Applying (same) →
  InProgress, no activate; already-active short-circuit → AlreadyActive, zero
  `load_fresh`/`activate` calls; version-equal hash-differs → TargetCorrupt;
  known-safe failure → Approved release with approvals kept + last_apply_error;
  indeterminate outcome stays Applying; success → Applied with notices; lease
  renew failure → LeaseLost with zero activate; caller cannot inject a stale
  environment (service always calls `load_fresh`).
- `unsafe_dev_activate`: keeps readiness (NotReady → pointer unchanged); absent
  without the feature.

PostgreSQL ignored tests (`STARRING_TEST_DATABASE_URL`, DB name must contain
`test`, `--test-threads=1`):

- The seven serialization/race cases: A(v1)+B(v2) concurrent apply → exactly one
  Applying, other InProgress, one activate; A lease expired + `apply B` → B
  InProgress(lease_expired), zero pointer mutation; A `resume` completes → A
  Applied, then B claimable; A known-failure → Approved, slot freed, B claimable;
  A crash then active==A.target → boot bookkeeping → A Applied, B claimable;
  different guild or ruleset_key → concurrent Applying allowed; duplicate-target
  requests still serialize on the same slot.
- Concurrency additions: approve+reject concurrent → exactly one state succeeds;
  the last two quorum approvals concurrent → exactly one `Approved`; Approved
  expiry vs apply claim race → exactly one succeeds; a stale attempt cannot
  overwrite a new attempt's Applied/Approved state.
- Constraint/CHECK coverage; reconnect durability (state restored across a new
  pool).

Guard test: no operational path (tool/API/runtime) calls `activate_if_ready` or
`store.activate` outside the allowlist.

## Known Limitations

- Manual CLI `--actor` input is workflow-validation metadata, not authenticated
  identity; the real boundary completes when an authenticated API/UI attaches.
- `last_apply_error` keeps only the latest attempt's error (no history table).
- `observed_active` is recorded but never gates apply; emergency-rollback stale
  approvals are not detected until a future `require_unchanged_baseline` policy.
- Correctness does not depend on a housekeeping sweep; an optional expiry sweep
  can be added later purely for tidiness.

## Roadmap

18f closes the top safety debt (a single, un-bypassable, approval-bound
activation authority). Remaining stage-1 close-out: CI (fmt/clippy/test, ignored
PostgreSQL with `--test-threads=1`) and `CURRENT_STATE.md`. Then stage 2 begins
with the product-fork decision (automation designer vs game) and the
conversational harness.
