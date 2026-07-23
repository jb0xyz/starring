# Production runtime worker composition design

Date: 2026-07-22

Status: accepted implementation contract

Canonical addendum date: 2026-07-22

Canonical addendum audit: accepted 2026-07-22

Extends: `2026-07-19-production-control-api-runtime-convergence-design.md`

The worker-composition contract and independently audited canonical V2
persistence addendum are authoritative. This document defines the remaining
production contract for `tools/starring-runtime`, the pure runtime worker,
gateway admission, local route replacement, V2 Live certification, restart
reconstruction, and historical instance bindings. The approval, Product Apply,
immutable artifact, exact target, strict panel, PostgreSQL authority, and
false-Live boundaries
from the earlier design remain unchanged.

## Outcome and non-goals

The runtime converges an already approved and Product-Applied deployment from
Requested to Live. It may hydrate the exact target, stage and drain local
routes, reconcile declared Discord panels, resume admission, certify Live,
heartbeat a committed serving lease, and recover stale runtime state.

It cannot approve, publish, Apply, change tenant or installation authority,
activate a model tool, infer a target, or mutate Discord outside the exact
deterministic RuleSet and panel journal. The model remains absent from the
event-time and deployment-time authority paths.

PostgreSQL is authoritative. The registry and gateway control are disposable
local serving state. A local route, Discord connection, Ready event, panel
certificate, or Product Apply alone never projects Live.

## Component boundaries

Add a pure `crates/automation-runtime-worker` crate. It owns deterministic
planning and orchestration over injected ports. It has no `sqlx`, Twilight,
HTTP client, socket, process, operating-system secret, or signal dependency.
Dependency guards enforce those exclusions and forbid model-facing crates.

| Responsibility | Owner |
| --- | --- |
| Durable phase and action session rules | `automation-runtime-controller` |
| Exact deployment and Live predicates | `automation-runtime-convergence` |
| Execution, V2 certification, observation, and recovery SQL | `automation-runtime-execution-postgres` |
| Exact target hydration | `automation-runtime-convergence-postgres` |
| Strict fenced panel journal | `automation-runtime-panel-postgres` |
| Exact serving heartbeat, observation, and disconnect | `automation-runtime-serving-postgres` |
| Pinned instance route and registration | `automation-runtime-interaction-postgres` |
| Reconnect-safe admission control | `automation-runtime` |
| Fenced staged and serving routes | `automation-runtime-registry` |
| Configuration, secrets, pools, Discord, lifecycle, health, and signals | `tools/starring-runtime` |

`tools/starring-runtime` contains no raw SQL and no second route registry. It
composes verified adapters and concrete supervisors only. Every process creates
CSPRNG controller and process identities and requires a canonical build
revision. The first release owns shard 0 of shard count 1. Multi-shard support
requires a later versioned contract.

Durable V2 DTOs live in `automation-runtime-controller` and use only pure
domain types. They define `RuntimeGatewayReadyKindV2` and
`RuntimeGatewayAdmissionSequenceV2` instead of importing gateway V3 types.
`automation-runtime-worker` owns pure gateway, registry, clock, and
finalization ports. A concrete adapter in `tools/starring-runtime` is the only
layer that converts `GatewayReadyLeaseV3` and registry-local evidence into
those pure DTOs. Neither the worker nor any PostgreSQL adapter depends on
`automation-runtime`, Twilight, or a registry-local lifecycle enum.

## Five isolated PostgreSQL capabilities

The process owns exactly five separately credentialed pools.

| Capability | Production authority |
| --- | --- |
| Convergence | claim, renew, phase mutation, V2 certification, exact observation, stale recovery, gateway ownership |
| Exact target | hydrate one fenced target and its historical authority |
| Panel | claim and execute one strict panel reconciliation journal |
| Serving | heartbeat, exact observation, and conditional disconnect of one serving identity |
| Interaction | route read, pinned artifact and binding read, pinned instance registration |

Startup uses one absolute 45-second deadline for connecting, identity anchoring,
all full readiness contracts, aggregation, and failure cleanup. It reserves the
last ten seconds as a cleanup tail and starts no new startup wait or mutation
after the 35-second operation cutoff. Timeout configuration must fit the
relevant operation or cleanup partition. Trusted secrets must name one database
and five pairwise-distinct direct-login roles. The
Convergence identity observer supplies the canonical non-zero database UUID.
All adapters then prove that UUID, the same database name, the exact expected
role, and their complete least-privilege contract.

Full schema and ACL readiness runs at startup and every five seconds. Every hot
operation separately verifies a capability-specific database identity,
`current_database`, and `session_user` inside the same transaction before its
read or mutation. Hot binding never performs the full catalog audit. A periodic
failure removes process readiness and commands emergency admission pause.

No adapter exposes a raw pool or connection. Failure closes every created pool,
drops resolved secrets, starts no Discord shard, and emits only finite stable
component codes. Shutdown closes all five pools concurrently and idempotently.

## Gateway ownership and explicit admission

One database-backed gateway-owner lease serializes ownership of a shard. An
external singleton promise is insufficient for production. Stable ownership
identity and mutable renewal state are separate.

```rust
pub struct RuntimeGatewayOwnerLeaseIdV1 {
    pub gateway_shard_id: GatewayShardIdV1,
    pub process_instance_id: ProcessInstanceId,
    pub lease_epoch: NonZeroU64,
    pub expected_build_revision: RuntimeBuildRevisionV1,
}

pub struct RuntimeGatewayOwnerLeaseReceiptV1 {
    pub lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub owner_revision: NonZeroU64,
    pub database_now: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
```

Acquisition, renewal, release, and V2 certification validate the exact stable
lease ID. Renewal uses the current owner revision as its compare-and-swap
precondition and returns its successor receipt. An immutable certification
records the observed revision, but later heartbeat and status accept a newer
current revision when the stable lease ID is unchanged and the lease is fresh.

Add an explicit production admission policy to shared gateway control.

```rust
pub enum GatewayAdmissionPolicyV3 {
    ResumeOnConnect,
    ExplicitResumeAfterEveryConnect,
}

pub fn shared_gateway_control_channel_with_policy_v3(
    config: GatewayControlConfigV3,
    policy: GatewayAdmissionPolicyV3,
) -> (SharedGatewayControlV3, SharedGatewayRuntimeControlV3);
```

`ExplicitResumeAfterEveryConnect` has these invariants:

- Initial state is `Paused { Starting }` before the gateway task can run.
- Every transport disconnect synchronously increments admission revision and
  becomes `Paused { Disconnected }`, even when admission was open.
- Every reconnect creates a new connection epoch and remains
  `Paused { Connected }`.
- `mark_connected` never opens admission under this policy.
- A ready lease cannot exist until an explicit resume of the exact epoch.
- Revision or epoch overflow permanently fails closed.
- Ordinary barrier pauses do not masquerade as transport disconnects and do
  not stop serving heartbeats.

Connection state, admission revision, transition sequence, connected-event
sequence, and resume sequence are published in one
`GatewayAdmissionSnapshotV3`. Ready-lease issuance and currentness checks read
exactly one snapshot. A pause or explicit disconnect closes state and advances
revision in the same publication. A ready lease binds the exact connected and
resume sequences; no lease assembled before resume can become current later.

Every acknowledged pause, including a repeated pause of an already-paused
connection, advances revision and transition sequence and returns an opaque
`GatewayPauseTokenV3` bound to that control lifetime, epoch, revision, and pause
sequence. Resume accepts only that token. An older workflow cannot read the
latest snapshot and cancel a newer barrier or emergency pause. Lifecycle queue
capacity is reserved first, the authoritative snapshot is published second,
and the correlated event is delivered last.

Successful enqueue authorizes and queues a resume intent. Runtime claim is the
application linearization point; cancellation that wins before claim revokes
the command, while cancellation or acknowledgement loss after claim does not
revoke an authorized resume. Its caller must exact-observe the atomic snapshot
and ready lease, or issue a new pause and continue closed recovery. Dropping
the sole control owner synchronously flips shared liveness, so ready-lease
issuance and validation fail immediately even before the runtime task consumes
channel closure. One control-channel lifetime is allowed per process and
gateway-owner lease.

An explicit `GatewayBarrierCoordinatorV2` sits above the opaque pause tokens.
Its atomically published state is `Open { generation, mode }`,
`Ordinary { generation, barrier_id, mode }`,
`Committing { generation, barrier_id, operation_id, mode }`,
`Emergency { generation, cause }`,
`RecoveryPending { generation, recovery_id }`,
`AdmissionAcknowledging { generation, resume_id, desired_mode }`, or
`Shutdown { generation }`.
Only `Open` can issue a single ordinary-barrier reservation. Emergency and
shutdown advance generation, synchronously trip a one-way fail-closed ingress
handle, invalidate every ordinary reservation, and poison an in-flight ordinary
finalizer. The runtime-side resume claim checks the same coordinator generation
at its application linearization point. Repeatedly pausing an emergency-paused
connection therefore cannot create ordinary authority to resume it.

`Open` mode is either
`Production { ingress_ack_revision, maintenance_gate_generation }` or
`Cutover { fence_generation, closed_ack_revision }`. Cutover mode requires the
exact closed maintenance gate and cutover lease, permits only scoped
recertification work, and is never healthy or publicly qualifying. Production
mode requires the fresh ingress-open acknowledgement.
`AdmissionAcknowledging.desired_mode` is selected from the exact durable writer
fence: Production requires the open acknowledgement; Cutover requires the
closed-gate acknowledgement and current cutover lease. A fence-generation
change invalidates the transition and restarts recovery.

For a Production transition, the coordinator remains
`AdmissionAcknowledging` while the maintenance gate advances from exact
`Closed` through `Opening` to `Open`. No public coordinator permit exists
during that interval. The ingress-open acknowledgement binds the resulting
gate generation as well as the writer-fence and gateway state. Only an exact
reread of all three moves the coordinator to `Open::Production`. A Cutover
transition never opens the gate; it exact-observes the closed-gate
acknowledgement and enters `Open::Cutover`.

Only `Open::Production` can issue a non-cloneable
`RuntimePublicAdmissionPermitV2` bound to coordinator generation and the
installed current ingress-ack revision and maintenance-gate generation.
`Open::Cutover`, `Ordinary`,
`Committing`, `Emergency`, `RecoveryPending`, `AdmissionAcknowledging`, and
`Shutdown` issue none. Public admission double-collects that permit with the
counted maintenance permit, ready snapshot, route witness, and active guard;
any state or ack change before the second collect aborts execution. A permit
already transferred into an admitted interaction may finish.

The coordinator starts as `Emergency { Starting }`, never `Open`. Startup and
reconnect use only the recovery-permit path. Transport disconnect, control-
owner drop, gateway-owner uncertainty, capability-readiness loss, and shutdown
enter the coordinator's synchronous arbitration section no later than they
publish an invalidating gateway snapshot. Commit handoff enters that same
section, uses the fixed lock order coordinator, gateway snapshot, then registry,
and rereads the exact ready snapshot and route witness before changing
`Ordinary` to `Committing`. If an invalidator wins, the claim fails and sends no
commit. If the claim wins, the request is authorized exactly once and every
later invalidator is classified after-dispatch.

`Ordinary` remains held after resume through commit, exact local verification,
durable ingress acknowledgement, and finalizer disarm; resume alone never
returns the coordinator to `Open`.

After the final double collect, a CAS from the exact `Ordinary` state mints one
non-cloneable `GatewayCertificationCommitPermitV2` and enters `Committing`.
That mint is the local certification-handoff linearization and the prepared
commit consumes the permit with the immutable request. If emergency wins first,
no permit exists and no commit is sent. If the permit wins first, a later
emergency still closes ingress synchronously but cannot pretend the already
dispatched commit was not sent; it makes the result unknown until exact
observation, permits no heartbeat, and conditionally disconnects a committed
receipt. It may drain unaffected routes, but it cannot drain or remove the
claimed route until the commit transaction ends and exact observation classifies
the result. No other ordinary work can enter while `Committing`.

Only the dedicated recovery supervisor may move `Emergency` to
`RecoveryPending`. It first proves exact gateway ownership, all capability
readiness, a paused connected epoch, and an empty or exactly classified local
registry. The transition binds the old Emergency generation and new recovery
ID and mints one non-cloneable `RuntimeClosedDrainRecoveryPermitV2`. That permit
authorizes only the closed startup/recovery fixed point described below and
cannot resume admission.

The pure worker coordinator implements the registry evidence input as the
non-cloneable sum `RuntimeClosedRecoveryRegistryEvidenceV2`. Its first supported
branch is only `Empty(RuntimeRegistryRecoveryEmptyObservationV2)`. Aggregate
counts, a copied sequence, or that empty branch cannot stand in for the future
exactly-classified branch. The worker permit is state authority only and
performs no I/O. Before any closed operation, the concrete
`tools/starring-runtime` adapter must still bind and revalidate the exact
registry instance cursor, serialized owner freeze, gateway control lifetime,
and compound recovery session.

After the fixed point proves serving empty and every runtime-resolvable count
zero, consuming the closed permit may mint the separate non-cloneable gateway
recovery-resume permit. Its runtime-side claim and exact ready observation move
`RecoveryPending` to `AdmissionAcknowledging`; only the exact mode-specific
acknowledgement moves that state to `Open`. No ordinary reservation or public
permit exists before then. Lost resume acknowledgement remains
`RecoveryPending` until exact observation proves that resume or a successor
recovery pause is issued. Lost durable acknowledgement remains
`AdmissionAcknowledging` and exact-replays its operation. Shutdown is terminal.
Delayed
ordinary work, repeated pause, acknowledgement loss, owner loss, and
emergency-before-runtime-claim are mandatory race tests.

Closed-recovery authority advances linearly. The permit carries a monotonic
authority revision plus the exact current owner receipt, five readiness
receipts, paused gateway snapshot, and registry observation sequence. Owner
renewal, readiness refresh, a registry seal/refence/removal, or a closed
recovery database operation consumes the current permit and returns exactly one
successor permit for the same recovery ID and coordinator generation with all
changed receipts and a successor authority revision. No two such operations
run concurrently. A stale permit cannot authorize a call. Loss, mismatch, or a
non-successor result returns no permit and re-enters Emergency or Shutdown;
only a valid current permit can eventually mint the resume permit. Tests race
owner renewal, capability refresh, registry mutation, claim dispatch, and
shutdown at every consume/return boundary.

The concrete coordinator adapter exclusively owns `SharedGatewayControlV3`,
opaque pause tokens, raw pause and resume calls, and the synchronous invalidation
hook. Workers and other supervisors receive only narrowed coordinator ports.
Private constructors and dependency/public-surface guards prevent production
code from bypassing the emergency latch through a raw repeated pause and resume.

Gateway-owner renewal uncertainty removes readiness and pauses admission before
lease expiry. A transport disconnect removes readiness, invalidates admission,
closes and joins serialized heartbeat lanes, begins exact local drains,
persists conditional disconnects from the latest proven receipts, performs
stale recovery, and leaves reconnect paused until ownership and recovery are
proven again.

## Startup ownership barrier

`recover_next_stale_live() == None` cannot distinguish an empty shard from a
fresh foreign Live lease. Startup requires a closed shard-scoped observation.

```rust
pub enum RuntimeStartupServingStateV2 {
    Empty,
    RecoverableStale { count: u32 },
    ForeignFresh {
        count: u32,
        database_now: DateTime<Utc>,
        earliest_expiry: DateTime<Utc>,
        retry_after: Duration,
    },
    Ambiguous,
}

pub struct RuntimeStartupRecoveryStateV2 {
    pub serving: RuntimeStartupServingStateV2,
    pub recoverable_awaiting_certification_count: u32,
    pub suspended_local_effect_count: u32,
    pub pending_runtime_drain_intent_count: u32,
    pub acknowledged_product_handoff_count: u32,
}
```

Startup ordering is exact:

1. Validate configuration, secrets, build revision, and process identities.
2. Compose and fully verify all five database capabilities.
3. Create an empty registry and explicitly paused gateway control.
4. Acquire the exact shard-owner lease.
5. Start a limited gateway-owner renewal watchdog while admission remains
   paused. It schedules from the local monotonic request-start instant and the
   same-statement database lease duration, subtracting response latency and a
   safety margin.
6. Start Discord and wait for a paused connected epoch.
7. Move `Emergency { Starting }` to `RecoveryPending`, mint the closed drain
   recovery permit, and enter one loop bounded by the startup operation cutoff.
8. Revalidate the exact owner receipt and all five capability readiness
   contracts on every iteration.
9. Recover stale Live, every recoverable reserved Awaiting certification scope,
   suspended process-local effect, and pending runtime drain intent to a bounded
   fixed point, then observe all classes again.
10. Break only when serving is `Empty` and the three runtime-resolvable counts
    are zero; continue immediately on `RecoverableStale` or recoverable
    obligations; on
    `ForeignFresh`, wait for at most the returned bounded `retry_after` and the
    remaining monotonic operation budget, then repeat recovery and observation;
    fail closed on `Ambiguous`.
11. Transfer the exact owner receipt and renewal schedule from the startup
    watchdog to the production owner supervisor without a gap or a second
    concurrent renewer.
12. Start remaining supervisors, consume the closed recovery permit, and resume
    only after the loop reaches `Empty`, using the resulting non-cloneable
    gateway recovery-resume permit to reach
    `AdmissionAcknowledging` with an exact ready lease.
13. Reread the writer fence. If it is `Open`, advance the maintenance gate from
    exact `Closed` to `Opening` and `Open`, publish or exact-observe the
    ingress-open acknowledgement bound to both generations, then enter
    `Open::Production`. If it is `Closed`, keep the gate closed, verify its
    durable closed acknowledgement and the exact cutover lease, then enter
    `Open::Cutover`.
14. Publish readiness only for the Production branch after a final aggregate
    health proof.

The closed permit exposes four narrowed fixed-point port families and no generic
convergence mutation:

| Recovery class | Closed authority |
| --- | --- |
| Stale Live | Exact serving and attestation observe, serialized lane close and join, conditional disconnect, and exact V2 stale-Live recovery |
| Reserved Awaiting | Exact scope and operation observe, certification lookup or unknown recovery, route-absence proof, reservation consume, and dedicated Awaiting reset |
| Suspended local effect | Exact sidecar observe, exact local seal and drain/remove, old-sidecar-revision CAS to `RouteAbsent`, and unknown progress observation |
| Pending drain intent | Exact intent observe/claim, serving and certification resolution, seal/refence/remove progress, and route-absence acknowledgement |

Every call requires the current owner, all five readiness receipts, paused
gateway and route-mutation provenance, and the linearly current successor
closed permit. Suspension resume, a new convergence phase, panel or activation
work, recertification, and public admission remain forbidden until the fixed
point completes and the process reaches the applicable Open mode. Production
and Cutover startup, reconnect recovery, sidecar-progress acknowledgement loss,
shutdown, and every unknown-result branch are mandatory tests for all four
families.

A `RouteAbsentAcknowledged` Product handoff remains a frozen per-slot Product
obligation, not a runtime-resolvable startup obligation. It contributes to the
reported handoff count and that slot's pending status, but it does not block
global startup or unaffected admission while waiting for the correlated Product
retry.

`Ambiguous`, lease loss, or deadline exhaustion keeps admission paused and
uses only the reserved cleanup tail to stop Discord, release exact ownership,
close all created pools concurrently, and drop secrets. The 45-second absolute
deadline then forces closed teardown and a stable nonzero code.

Any gate-open, acknowledgement, fence-reread, or mode-CAS failure issues the
one-way invalidation first, closes and joins the gate, and returns to emergency
recovery if operation time remains; otherwise it uses startup cleanup. A
writer-fence generation change during acknowledgement discards that operation.
No open gate alone authorizes admission. Tests cover an ordinary Production
restart with an initially closed gate, both fence modes, failure at each gate
transition, lost acknowledgement, and a fence change before the final mode CAS.

## Local concurrency model

The first production worker uses a keyed single-flight lane per serving slot.
Two attempts for the same slot never overlap. Hydration, drain waits, panel
work, and database operations for different slots may run concurrently under a
small configured global bound.

One global barrier coordinator serializes only admission barriers and emergency
pause transitions. No drain wait, panel call, database call, Discord HTTP call,
sleep, or retry delay runs inside a barrier. Existing Live heartbeat schedules
continue concurrently unless their exact slot or gateway ownership is lost.

Certification acquires authority in one fixed order while admission is open:
the process-wide ordinary-finalization turn, the exact controller-renewal
freeze, then the exact gateway-owner-renewal freeze. Before acquisition it
computes a conservative monotonic reservation horizon from the prepare,
250-millisecond barrier, commit, and post-commit verification budgets. Each
renewal coordinator records the database lease duration at its request-start
instant, subtracts response latency and a safety margin, and grants only when
the exact receipt has sufficient remaining duration. Neither receipt may renew
while frozen.

Freeze acquisition first closes that authority's renewal-dispatch gate. It then
joins or exact-observes any already-dispatched renewal, applies its successor
receipt, and for controller renewal installs the replacement registry token
before returning the freeze. Unknown renewal outcome enters emergency recovery;
it never freezes an old receipt. Release occurs only after finalization
transaction termination, exact observation, and route resolution.

Only the holder may open a certification transaction; at most one prepared
certification exists per process. Preparation revalidates both exact receipts
in PostgreSQL. The worker passes only the remaining monotonic reservation
duration; PostgreSQL caps the absolute `must_commit_before` at
`clock_timestamp() + remaining_duration` and both database lease margins, while
the supervisor independently enforces its monotonic horizon. The three
reservations remain held through transaction termination, exact outcome
observation, and either verified local serving or exact drain. Failure to
reserve either authority runs no transaction and no barrier.

Before Barrier B, the prepared handle, immutable operation intent, route
evidence, pre-reserved pause and resume queue capacity, lifecycle capacity,
buffers, emergency fail-closed handle, and all three reservations transfer to
one process-owned finalization supervisor. Caller cancellation only drops its
result waiter and cannot cancel the accepted job. An armed finalization guard
trips the one-way emergency latch synchronously if the supervisor future is
dropped, aborted, or panics. It also permanently poisons ordinary finalization
for that process lifetime. Cleanup after such a trip runs only through closed
recovery; no reservation becomes available to another ordinary finalizer.

This structure preserves safety without making a slow panel reconciliation
head-of-line block every tenant.

## Registry witness and fencing renewal

Every installed route has a process identity, controller fencing token,
monotonic route incarnation, lifecycle, and active interaction count.

```rust
pub struct SlotRouteWitnessV1 {
    pub identity: RuntimeProcessIdentityV1,
    pub fencing_token: FencingToken,
    pub incarnation: NonZeroU64,
    pub lifecycle: SlotLifecycleV1,
}
```

The registry type above never crosses the worker port. The pure worker owns
these registry-independent projections:

```rust
pub enum RuntimeRouteLifecycleV2 {
    Staged,
    Serving,
    DrainClaimSealed {
        intent_id: RuntimeDrainIntentIdV2,
        seal_generation: NonZeroU64,
    },
    Draining,
}

pub struct RuntimeRouteWitnessV2 {
    pub identity: RuntimeProcessIdentityV1,
    pub controller_fencing_token: FencingToken,
    pub route_incarnation: NonZeroU64,
    pub lifecycle: RuntimeRouteLifecycleV2,
    pub active_interactions: u32,
    pub admission_generation: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
}

pub struct RuntimeEmptySlotSealProjectionV2 {
    pub process_instance_id: ProcessInstanceId,
    pub intent_id: RuntimeDrainIntentIdV2,
    pub seal_generation: NonZeroU64,
}

pub enum RuntimeRouteObservationV2 {
    Present(RuntimeRouteWitnessV2),
    Absent {
        slot: RuntimeServingSlotV2,
        seal: Option<RuntimeEmptySlotSealProjectionV2>,
        admission_generation: NonZeroU64,
        registry_observation_sequence: NonZeroU64,
    },
}
```

These are non-authorizing values with no registry pointer or mutation token.
The concrete adapter in `tools/starring-runtime` alone reads registry-local V1
or V2 state, combines the per-slot seal state, and projects these values. The
worker owns only pure port traits over the projections. Mutation and seal ports
accept non-cloneable adapter-owned capabilities, so a copied projection cannot
install, activate, refence, drain, remove, seal, or unseal a route. Dependency
and public-surface tests forbid `automation-runtime-worker` from importing
`automation-runtime-registry`, `SlotLifecycleV1`, or `SlotRouteWitnessV1`.

The registry itself adds one local atomic V2 observation API:

```rust
pub struct SlotSealKeyV2([u8; 16]);

pub enum SlotAdmissionStateV2 {
    Empty,
    Staged,
    Serving,
    DrainClaimSealed {
        seal_key: SlotSealKeyV2,
        seal_generation: NonZeroU64,
    },
    Draining,
}

pub struct SlotAtomicObservationV2 {
    pub route: Option<SlotRouteWitnessV1>,
    pub admission_state: SlotAdmissionStateV2,
    pub active_interactions: u32,
    pub admission_generation: NonZeroU64,
    pub observation_sequence: NonZeroU64,
}
```

`SlotSealKeyV2` is a non-authorizing registry-local value with one checked
constructor from the canonical 16-byte intent identifier. The tools adapter may
construct it, but only a registry-issued seal capability can mutate a slot.

One slot lock returns route or empty state, seal state, active count, admission
generation, and observation sequence together. Separate route, lifecycle,
active-count, or seal reads cannot implement the V2 port. Slot tombstones retain
both counters after removal. Install, activation, seal, unseal, lifecycle
change, refence, and removal advance both counters. Active-guard acquire and
drop advance only observation sequence. Overflow permanently marks that slot
non-admitting and makes every later mutation fail closed.

The registry also exposes one registry-global startup and recovery observation.
Its sequence is a distinct typed domain and cannot be substituted with a slot
sequence.

```rust
pub struct RegistryGlobalObservationSequenceV2(NonZeroU64);

pub struct RegistryRecoveryObservationV2 {
    observation_sequence: RegistryGlobalObservationSequenceV2,
    retained_slot_count: u64,
    retained_empty_tombstone_count: u64,
    staged_route_count: u64,
    serving_route_count: u64,
    draining_route_count: u64,
    sealed_slot_count: u64,
    active_interaction_count: u64,
    failed_closed_slot_count: u64,
    registry_failed_closed: bool,
}
```

One registry lock returns the sequence and aggregate together. The aggregate
counts staged, current, and every retired route independently, including
zero-active retired routes and seals over empty slots. A high-water-only empty
tombstone is retained but does not prevent recovery emptiness. Every effective
install, activation, lifecycle mutation, refence, removal, seal, unseal,
active-guard acquire, active-guard drop, and slot fail-close advances the global
sequence exactly once. Replay and rejected precondition checks do not advance
it. The maximum sequence value is reserved for registry-global terminal close;
the mutation that reaches it is rejected without applying its intended state
change, all ordinary reads and mutations then fail closed, and only the
non-authorizing recovery observation may report the terminal diagnostic.

The aggregate proves only local recovery emptiness. It carries no registry
pointer, mutation capability, owner, readiness, gateway, or coordinator
authority. A copied aggregate or sequence cannot authorize a seal, refence,
removal, closed database call, recovery transition, or admission resume.

The registry additionally exposes a short-lived recovery observation guard
over that same registry lock. The guard returns its exact non-authorizing
aggregate by value and may be consumed into a non-cloneable
`RegistryEmptyRecoveryCursorV2` only while the aggregate is recovery-empty.
This cursor is exclusively the startup-empty fast path. It privately retains a
weak reference to the exact registry instance and the exact global observation
sequence. Revalidation takes that registry's one lock and rejects a foreign
instance, a changed sequence, a non-empty aggregate, or terminal failed-closed
state. It returns only the current non-authorizing aggregate and exposes no
mutation operation.

The tools adapter acquires this guard only after the coordinator and gateway
snapshot sections in the fixed coordinator, gateway snapshot, registry lock
order. It invokes no coordinator, gateway, database, or user callback while
the guard is live. The empty cursor is only an instance-bound input to later
revalidation of the startup-empty branch. For the existing `empty or exactly
classified` recovery choice, the classified non-empty branch requires
separate exact per-slot instance-bound witnesses. An aggregate, count, global
sequence, or empty cursor cannot replace those witnesses. The coordinator's
compound recovery-session transition may consume a revalidated empty cursor
only when selecting the startup-empty branch; it must consume the separate
exact witness set when selecting the classified branch. The empty cursor
remains non-authorizing throughout. Only the separately minted compound closed
recovery permit authorizes its bounded operations. A cursor or successful
revalidation alone never authorizes a registry mutation, database call,
recovery transition, or admission resume.

The first concrete tools slice composes an opaque registry with fixed limits of
4096 slots and 8 retired routes per slot. Its per-slot active-interaction limit
is the checked `u32` conversion of the configured process-wide gateway
admission capacity. It does not use the registry default, expose a raw registry
getter, or expose the guard or cursor. Its only public recovery read maps all
ten aggregate fields into the pure worker validator and returns the resulting
non-authorizing empty projection. The private guard-to-cursor path is added only
with the coordinator and gateway compound transition that can enforce the
required lock order.

Public admission reads atomic observation A, requires unsealed `Serving`, and
acquires its guard against the exact admission generation. Guard acquisition
returns a non-cloneable capability and its exact successor observation. Final
observation B must retain the same route identity, fence, incarnation,
`Serving` lifecycle, and admission generation; unrelated guard-count sequence
changes are allowed. A seal or authority mutation always advances admission
generation and therefore invalidates B. Tests race install, guard acquire and
drop, seal and unseal, lifecycle changes, refence, removal, empty-slot sealing,
counter overflow, and A/B collection under the one-lock API.

The registry exposes exact `route_witness` and `advance_authority` operations.
`advance_authority` requires the same registry, slot, identity, incarnation,
and current high-water token; the next fence must be exactly the successor. It
changes only the record fence and slot high-water fence. Lifecycle and active
count remain unchanged, and the old token becomes stale. The operation returns
the replacement mutation token. The worker replaces its stored token before
any later activation, drain, observation, or removal and never uses the old
token again.

Controller renewal is legal only before panels, a barrier, or prepared
certification begins. After the database renewal replay settles, the worker
applies the session receipt and immediately advances the registry authority. If
registry advancement fails after a committed renewal, the process emergency
pauses and shuts down. It never continues with the old token or reinstalls the
route.

## V2 route-admission attestation

Discord `Ready` retains its narrow meaning: an actual Ready or Resumed lifecycle
event occurred. It may predate panels and route activation. V2 separately
attests that the exact route was Serving under the current connection epoch and
admission revision after explicit resume.

```rust
pub struct RuntimeBarrierIdV1(String);

pub struct RuntimeRecoveryIdV2(String);

pub struct RuntimeCertificationOperationIdV2(String);

pub enum RuntimeGatewayReadyKindV2 {
    Ready,
    Resumed,
}

pub struct RuntimeGatewayAdmissionSequenceV2(NonZeroU64);

pub struct RuntimeGatewayReadyAttestationV2 {
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub kind: RuntimeGatewayReadyKindV2,
    pub admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub resume_sequence: RuntimeGatewayAdmissionSequenceV2,
}

pub struct RuntimePanelEvidenceV2 {
    pub certificate_id: PanelCertificateId,
    pub report_digest: PanelReportDigestV1,
    pub process_identity: RuntimeProcessIdentityV1,
    pub controller_fencing_token: FencingToken,
}

pub struct RuntimeServingRouteAttestationV2 {
    pub identity: RuntimeProcessIdentityV1,
    pub controller_fencing_token: FencingToken,
    pub route_incarnation: NonZeroU64,
    pub activation_sequence: NonZeroU64,
}

pub struct RuntimeBarrierPauseWitnessV2 {
    pub coordinator_generation: NonZeroU64,
    pub connection_epoch: NonZeroU64,
    pub paused_admission_revision: NonZeroU64,
    pub pause_sequence: RuntimeGatewayAdmissionSequenceV2,
}

pub struct RuntimeClosedRecoveryRouteWitnessV2 {
    pub recovery_id: RuntimeRecoveryIdV2,
    pub originating_emergency_generation: NonZeroU64,
    pub recovery_generation: NonZeroU64,
    pub recovery_authority_revision: NonZeroU64,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub owner_expires_at: DateTime<Utc>,
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub paused_admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub pause_sequence: RuntimeGatewayAdmissionSequenceV2,
}

pub struct RuntimeShutdownRouteWitnessV2 {
    pub shutdown_generation: NonZeroU64,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub owner_expires_at: DateTime<Utc>,
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub paused_admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub pause_sequence: RuntimeGatewayAdmissionSequenceV2,
}

pub enum RuntimeRouteMutationProvenanceV2 {
    Ordinary {
        barrier_id: RuntimeBarrierIdV1,
        pause: RuntimeBarrierPauseWitnessV2,
    },
    ClosedRecovery(RuntimeClosedRecoveryRouteWitnessV2),
    Shutdown(RuntimeShutdownRouteWitnessV2),
}

pub struct RuntimeRouteAdmissionAttestationV2 {
    pub barrier_id: RuntimeBarrierIdV1,
    pub pause: RuntimeBarrierPauseWitnessV2,
    pub gateway: RuntimeGatewayReadyAttestationV2,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub attested_owner_revision: NonZeroU64,
    pub route: RuntimeServingRouteAttestationV2,
}
```

`RuntimeBarrierIdV1` and `RuntimeRecoveryIdV2` are plain controller DTOs
containing exactly 32 lowercase hexadecimal characters generated from 128
CSPRNG bits. Parsing rejects every other length or alphabet, and there is no
unchecked public constructor. Both are included in canonical wire, digest,
golden-vector, and public-surface tests when contained by one of the six
canonical roots; neither creates a seventh digest domain.

Barrier B creates typed causal receipts. The concrete adapter first returns a
non-cloneable `GatewayBarrierPauseReceiptV2` that owns the opaque pause token,
the exact control lifetime, coordinator generation, epoch, paused admission
revision, and pause sequence. Registry activation accepts that receipt and
returns a `SlotActivationReceiptV2` containing the same pause witness, exact
route witness, and a monotonic activation sequence. The adapter consumes both
receipts in `resume_admission_after_activation_v2`; its runtime-side claim
rechecks the pause token and coordinator generation before it returns a
`GatewayResumeReceiptV2` and atomically observed ready lease. Constructors are
private, so an older activation receipt cannot be combined with a newer
ordinary, emergency, or foreign-control pause token.

Loss of a pause acknowledgement loses resume authority and requires a repeated
pause that advances revision and returns a successor token. Loss of a resume
acknowledgement requires exact observation of the atomic snapshot and ready
lease. The finalizer continues only if the expected resume applied under the
same epoch, pause witness, and coordinator generation; otherwise it enters
emergency pause and never commits that request.

Construction then uses a monotonic double collect:

1. Read atomic admission snapshot A and require the barrier's ready lease.
2. Read the exact Serving route witness and activation receipt.
3. Read atomic admission snapshot B and require exact equality with A.
4. Read the route witness again and require exact equality.
5. Require one barrier ID and activation before resume in its typed receipts.
6. Bind the closed evidence into the certification request.

The process ID must match the route, Discord evidence, and gateway-owner lease.
The controller fence must match the execution guard. The admission revision,
connection epoch, connected and resume sequences, route incarnation and
activation sequence, stable owner lease ID, and observed owner revision must
all be exact. The pause epoch and admission revision must equal the gateway
evidence, and its pause sequence must precede the explicit resume sequence
under the same coordinator generation. Locked `AwaitingGatewayReady` proves
panel acceptance precedes
preparation. Typed barrier receipts prove route activation precedes resume. The
commit statement proves certification follows request construction. Wall-clock
panel, activation, resume, and Discord event times are diagnostic only and are
not ordering evidence. Production V2 requires `resume_sequence` to be strictly
greater than `connected_event_sequence`; equality identifies legacy
`ResumeOnConnect` behavior and never qualifies V2 certification.

V2 uses a separate transition validator and the explicit
`RuntimePanelEvidenceV2` projection. It binds the existing time-derived panel
certificate ID and report digest as opaque identity, so the ID transitively
commits `reconciled_at`; it does not bind the raw timestamp again or compare it
with gateway, activation, or commit timestamps. No V1 timestamp-ordering check
is reused. Golden vectors target the exact V2 projection and request wire
types.

## Canonical V2 persistence addendum

This independently audited section supersedes any less precise reference
elsewhere in this document to a canonical domain, canonical JSON, direct DTO
serialization, timestamp formatting, duration
formatting, or Live attestation record shape.

### Identifier ownership and database shape

This is the exhaustive CSPRNG identifier inventory for this contract. Every ID
contains exactly 32 lowercase ASCII hexadecimal characters encoding 128 random
bits. Checked parsing rejects every other byte length, alphabet, case, prefix,
separator, or textual form. Parsing never generates an ID.

| Identifier | Exact scope | Sole generation owner | Unknown, crash, and replay rule |
| --- | --- | --- | --- |
| `RuntimeBarrierIdV1` | gateway shard, process instance, coordinator generation | The worker requests one ID from the injected generator immediately before the first pause dispatch for one barrier. | Lost pause or resume acknowledgement exact-observes the same control lifetime, generation, and ID. It neither reuses the ID with another generation nor mints a competing barrier until the old control lifetime is proven closed. |
| `RuntimeRecoveryIdV2` | gateway shard, process instance, recovery generation | The closed-recovery supervisor requests one ID from the injected generator when it creates the recovery permit. | Every recovery retry uses that ID. Restart exact-observes gateway ownership, coordinator generation, and durable/local recovery evidence before adopting it; a successor is legal only after the old recovery authority is proven closed. |
| `RuntimeCutoverCoordinatorIdV1` | global writer-fence generation and cutover lease epoch | The offline cutover coordinator generates it once immediately before the first close or an explicit expired-lease takeover. | Close, renewal, open commit, and acknowledgement recovery exact-observe the singleton fence and adopt only the same generation, epoch, and coordinator. A foreign coordinator or generation gap is never replay. |
| `RuntimeCertificationOperationIdV2` | `RuntimeDeploymentScopeV1` plus deployment revision and convergence attempt | `tools/starring-runtime` generates it once before certification-intent reservation. | Reservation, prepare, commit-unknown observation, and restart use scope-only observation and adopt any persisted ID. A known row with different canonical bytes or digest is typed divergence. |
| `RuntimeDrainIntentIdV2` | `RuntimeDeploymentScopeV1` plus serving slot and expected revision | The Product mutation boundary generates it once before first drain-intent create. | Create uncertainty observes the exact natural scope and adopts a persisted ID and preimage. Claim, refence, acknowledgement, consumption, and restart never replace it. |
| `RuntimeProductOperationIdV2` | `RuntimeDeploymentScopeV1` plus expected revision | The Product mutation boundary generates it once before the first Product mutation. | Product retries and runtime handoff use scope-only observation and adopt any persisted operation. Runtime cannot mint or substitute it; changed semantic or canonical input is typed divergence. |
| `RuntimeSuspensionIdV2` | `RuntimeDeploymentScopeV1` plus deployment revision and convergence attempt | The worker requests one ID from the injected generator immediately before the first sidecar create. | Unknown create, drain progress, restart, and resume observe and adopt the persisted sidecar ID. They never replace it while that scoped sidecar may exist. |

Every durable first-apply call owns its dedicated database connection. If its
result is unknown, no new ID is legal until a transaction-ended proof exists
and a scope-only observation under the same natural lock proves exact absence.
If a row exists, the caller adopts its ID, canonical bytes, and digest. If the
row differs from the expected immutable values, the result is a typed
divergence, not replay. Only proven absence permits a new CSPRNG ID and a new
first apply. Observation by a caller-proposed ID alone is insufficient.

Each persisted ID is immutably bound to its exact scope. Physical tables have
one `UNIQUE` constraint over the complete natural scope and a separate
`UNIQUE` or primary-key constraint over the ID. A composite uniqueness rule on
scope plus ID is not a substitute because it would admit two IDs for one
natural scope. No update may move a scope or ID member. Embedded barrier and
recovery evidence carries and checks its complete gateway scope. Scope-only
observation returns at most one live or reserved immutable operation for the
natural scope; more than one is `PersistenceCorrupt`.

Every persisted column for one of these identifiers is `text NOT NULL` and has
an inline PostgreSQL constraint equivalent to:

```sql
CHECK (octet_length(identifier) = 32 AND identifier ~ '^[0-9a-f]{32}$')
```

The constraint applies to each of the six ID kinds on every physical column in
which that kind appears rather than relying on Rust, a trigger, or a calling
role. Persisted SHA-256 hexadecimal columns likewise require exactly 64
lowercase hexadecimal ASCII characters. Migration and real-PostgreSQL tests
exercise every physical constraint, natural-scope uniqueness rule, and separate
ID uniqueness rule directly.

### Private versioned wire projections

Domain DTOs are not canonical merely because they implement Serde. Each
persisted or digested top-level value uses a private, purpose-specific wire
projection owned by `automation-runtime-controller`. The following six roots
are exhaustive; adding another root requires a versioned design change.

```rust
pub struct RuntimeProductSemanticRequestDigestV2(String);

pub struct RuntimeProductMutationPreimageV2 {
    pub operation_id: RuntimeProductOperationIdV2,
    pub scope: RuntimeDeploymentScopeV1,
    pub expected_revision: DeploymentRevision,
    pub slot: RuntimeServingSlotV2,
    pub expected_target: RuntimeDeploymentTargetV1,
    pub mutation_kind: RuntimeProductMutationKindV2,
    pub product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2,
}

pub struct RuntimeDrainIntentPreimageV2 {
    pub key: RuntimeDrainIntentKeyV2,
}
```

`RuntimeProductSemanticRequestDigestV2` is supplied by the Product boundary,
contains exactly 64 lowercase hexadecimal SHA-256 characters, and commits the
Product-owned semantic request. Runtime validates and binds it but cannot
compute or replace it. `RuntimeDrainIntentPreimageV2` contains only the key. It
excludes `intent_digest`, revision, state, claims, progress, acknowledgements,
and timestamps, so neither preimage is self-referential or mutable.
Checked constructors compute `RuntimeProductMutationDigestV2` only from the
Product preimage and `RuntimeDrainIntentDigestV2` only from
`RuntimeDrainIntentPreimageV2::from_key(key)`. Neither constructor accepts its
resulting digest as input; decode is the only path that reads and recomputes an
embedded or separately persisted digest.

Product drain creation accepts the Product preimage, its recomputed mutation
digest, and the drain preimage as one checked aggregate. The drain key's
`product_operation_id`, `scope`, `expected_revision`, `slot`,
`expected_target`, and `mutation_kind` must equal the corresponding Product
preimage fields, and its `product_mutation_digest` must equal the digest just
recomputed from the exact Product bytes. Their slot must equal
`RuntimeServingSlotV2::from_target(expected_target)`, and scope, revision, and
target must exactly match the deployment row locked by the authorized Product
mutation. The Product boundary cannot supply two independently valid but
cross-mismatched or consistently wrong roots. Rust has no public unchecked
aggregate constructor, and the atomic PostgreSQL first-apply procedure repeats
every equality against both the roots and locked deployment before storing
either root.

| Root and exact ordered fields after `format_version` | Immutable `bytea` column | Typed digest | Sole private decoder | Maximum octets |
| --- | --- | --- | --- | --- |
| Certification intent: `action_id`, `operation_id`, `guard`, `target`, `binding_pin`, `process_identity`, `gateway_owner_lease_id`, `observed_owner_revision`, `runtime_build_revision`, `panel`, `serving_lease_milliseconds` | `certification_intent_bytes` | `RuntimeCertificationIntentFingerprintV2` | `decode_certification_intent_v2` | 32768 |
| Certification request: `intent`, `intent_fingerprint`, `must_commit_before_unix_microseconds`, `route_admission` | `certification_request_bytes` | `RuntimeCertificationRequestDigestV2` | `decode_certification_request_v2` | 65536 |
| Live record: `request_digest`, `request` | `live_attestation_record_bytes` | `RuntimeLiveAttestationDigestV2` | `decode_live_attestation_record_v2` | 131072 |
| Product mutation preimage: `operation_id`, `scope`, `expected_revision`, `slot`, `expected_target`, `mutation_kind`, `product_semantic_request_digest` | `product_mutation_request_bytes` | `RuntimeProductMutationDigestV2` | `decode_product_mutation_preimage_v2` | 32768 |
| Drain intent preimage: `key` | `drain_intent_request_bytes` | `RuntimeDrainIntentDigestV2` | `decode_drain_intent_preimage_v2` | 65536 |
| Suspend-attempt request: `suspension_id`, `action_id`, `guard`, `source_phase`, `failure`, `disposition`, `checkpoint`, `local_effect`, `drain_obligation` | `suspend_attempt_request_bytes` | `RuntimeSuspendAttemptDigestV2` | `decode_suspend_attempt_request_v2` | 131072 |

All six columns are `bytea NOT NULL` with `octet_length` bounded by the table.
Rust rejects oversize input before JSON parsing or allocation proportional to a
declared length. PostgreSQL repeats the bound. Public APIs accept and return
checked domain DTOs, canonical bytes, and digest newtypes; wire structs and
decoders remain private.

Every top-level projection follows all of these rules:

- The first field is numeric `"format_version":2`. A string version, missing
  version, duplicate version, or any other number is rejected.
- Encoding is compact UTF-8 JSON with no insignificant whitespace. Struct field
  order is the wire order and is covered by exact-byte golden tests.
- Every declared field is present. An optional value is encoded as either its
  value or JSON `null`; no `skip_serializing_if` behavior is permitted.
- Unknown, duplicate, and missing fields are rejected at every projection
  level. Decoding must re-encode and require byte-for-byte equality with the
  input, so reordered fields, alternate escapes, and whitespace are not a
  second accepted representation.
- `flatten`, untagged enums, aliases, defaults, and implicit variant renaming
  are forbidden in every canonical projection.
- Canonical projections contain no floating-point numbers, `HashMap`,
  `BTreeMap`, `serde_json::Value`, or other unordered or dynamically keyed
  object. Collections whose order is meaningful use an explicitly defined
  vector order. No set-like collection is admitted until its projection fixes
  a byte-level sort key and duplicate rejection rule.
- Integer fields use JSON decimal integers within their declared Rust range.
  Byte identities use their one checked lowercase hexadecimal representation.
  A `Debug` string, platform formatter, locale, or database JSON rendering is
  never canonical input.
- Discord snowflake values in a V2 root use JSON strings containing their
  unsigned decimal representation. The accepted range is
  `1..=18446744073709551615`; a sign, leading zero, whitespace, non-decimal
  byte, JSON number, or zero is rejected. PostgreSQL stores this value as
  `text`, never casts it to `bigint`, and checks
  `^[1-9][0-9]{0,19}$` plus, for a 20-byte value, C-collation lexical order at
  or below `18446744073709551615`. This rule is spelled out by the private V2
  projection and does not delegate canonical identity to `discord-model`
  Serde.

Except for the textual Discord snowflakes above, every persistence-bound `u64`
and `NonZeroU64`, including values nested in V1 domain objects, is restricted
to `0..=9223372036854775807` or
`1..=9223372036854775807` respectively. Rust constructors and canonical
decoders reject larger values before SQLx binding. PostgreSQL stores them as
`bigint` with matching checks. Golden and real-database tests cover
`i64::MAX`, `i64::MAX + 1`, zero, and one.

Fieldless enums encode as one fixed lowercase snake-case JSON string. Enums
with any payload encode as an object whose first field is `"kind"` and whose
remaining fixed fields are the named payload projection in declared order.
The V2 tags covered by persistence and digests are fixed as follows:

| Enum | Exact tags |
| --- | --- |
| `RuntimeGatewayReadyKindV2` | `ready`, `resumed` |
| `RuntimeFailureKindV1` in a V2 root | `environment_unavailable`, `activation_not_observable`, `panel_reconciliation`, `gateway_start`, `gateway_ready_timeout`, `invariant_violation` |
| `RuntimeCertificationRecoveryDispositionV2` | `stop_ownership`, `drain_and_replan`, `drain_and_stop`, `emergency_halt` |
| `RuntimeProductMutationKindV2` | `apply`, `supersede`, `cancel`, `authority_change`, `teardown` |
| `RuntimeDrainIntentMutationOutcomeV2` | `inserted`, `replayed`, `claimed`, `refenced`, `acknowledged`, `consumed`, `cancelled` |
| `RuntimeResumeCheckpointV2` | `verify_preflight`, `request_drain`, `complete_drain`, `begin_activation`, `observe_activation`, `begin_panels`, `reconcile_panels` |
| `RuntimeSuspensionSourcePhaseV2` | `requested`, `preflight_ready`, `drain_requested`, `drained`, `activation_applying`, `runtime_pending_ready`, `reconciling_panels` |
| `RuntimeSuspendedRouteLifecycleV2` | `staged`, `draining` |
| `RuntimeSuspendAttemptMutationOutcomeV2` | `inserted`, `replayed`, `drain_progressed`, `resumed` |
| `RuntimeRouteMutationProvenanceV2` | `ordinary`, `closed_recovery`, `shutdown` |
| `RuntimeCertificationDivergenceV2` | `ownership_lost`, `deployment_advanced`, `authority_changed`, `superseded`, `terminal`, `reservation_mismatch`, `committed_request_mismatch`, `persistence_corrupt` |
| `RuntimeCertificationObservationV2` | `not_committed`, `committed`, `diverged` |
| `RuntimeDrainClaimProgressV2` | `claimed`, `refenced` |
| `RuntimeDrainCertificationResolutionV2` | `no_operation_reserved`, `no_attestation_for_reserved_operation`, `committed_and_disconnected` |
| `RuntimeDrainIntentStateV2` | `pending`, `route_absent_acknowledged`, `consumed`, `cancelled` |
| `RuntimeAttemptDispositionV2` | `retryable`, `blocked` |
| `RuntimeDrainObligationV2` | `none`, `exact_local_route`, `previous_serving`, `local_and_previous` |
| `RuntimeLocalRouteEffectV2` | `none`, `exact_route`, `route_absent` |

Payload variants use these exact semantic field names and order after `kind`:

| Enum variant | Ordered payload fields |
| --- | --- |
| `RuntimeRouteMutationProvenanceV2::Ordinary` | `barrier_id`, `pause` |
| `RuntimeRouteMutationProvenanceV2::ClosedRecovery` | `witness` |
| `RuntimeRouteMutationProvenanceV2::Shutdown` | `witness` |
| `RuntimeCertificationDivergenceV2::{DeploymentAdvanced,AuthorityChanged,Superseded,Terminal}` | `snapshot` |
| Other `RuntimeCertificationDivergenceV2` variants | no payload fields |
| `RuntimeCertificationObservationV2::NotCommitted` | `snapshot`, `convergence_attempt`, `operation_id`, `request_digest`, `observed_deployment_revision`, `observed_at_unix_microseconds` |
| `RuntimeCertificationObservationV2::Committed` | `receipt` |
| `RuntimeCertificationObservationV2::Diverged` | `divergence` |
| `RuntimeDrainClaimProgressV2::Claimed` | `seal` |
| `RuntimeDrainClaimProgressV2::Refenced` | `seal`, `provenance`, `old_route`, `removal_target`, `registry_observation_sequence`, `refenced_at_unix_microseconds` |
| `RuntimeDrainCertificationResolutionV2::NoOperationReserved` | no payload fields |
| `RuntimeDrainCertificationResolutionV2::NoAttestationForReservedOperation` | `operation_id`, `intent_fingerprint` |
| `RuntimeDrainCertificationResolutionV2::CommittedAndDisconnected` | `operation_id`, `serving_identity`, `disconnected_revision` |
| `RuntimeDrainIntentStateV2::Pending` | `claim` |
| `RuntimeDrainIntentStateV2::RouteAbsentAcknowledged` | `acknowledgement` |
| `RuntimeDrainIntentStateV2::Consumed` | `resulting_revision`, `consumed_at_unix_microseconds` |
| `RuntimeDrainIntentStateV2::Cancelled` | `cancelled_at_unix_microseconds` |
| `RuntimeAttemptDispositionV2::Retryable` | `retry_not_before_unix_microseconds` |
| `RuntimeAttemptDispositionV2::Blocked` | no payload fields |
| `RuntimeDrainObligationV2::None` | no payload fields |
| `RuntimeDrainObligationV2::ExactLocalRoute` | `route` |
| `RuntimeDrainObligationV2::PreviousServing` | `previous` |
| `RuntimeDrainObligationV2::LocalAndPrevious` | `local`, `previous` |
| `RuntimeLocalRouteEffectV2::None` | no payload fields |
| `RuntimeLocalRouteEffectV2::ExactRoute` | `route`, `lifecycle` |
| `RuntimeLocalRouteEffectV2::RouteAbsent` | `slot`, `expected_route`, `provenance`, `observed_sequence` |

No tuple payload, flattened payload, inferred field name, or alternate tag is
accepted. The private V2 projection spells out every nested field rather than
deriving order from a public domain DTO; exact-byte goldens freeze each shape.
`RuntimeFailureV1` is
projected as `failure_id`, `kind`, `code`, `message`,
`recorded_at_unix_microseconds`; its kind uses only the tags listed above.

Nested V1 domain values used by a V2 payload are expanded through private
V2-owned wire projections as well. They do not inherit a current Serde,
chrono, or Rust variant representation. Reuse is allowed only when an existing
versioned byte contract is explicitly named and exact-byte tests prove it is
identical; otherwise the consuming V2 projection fixes the nested fields,
order, and tags. No V2 projection obtains a nested tag from a Rust variant name
implicitly.

### Time and duration normalization

Every `DateTime<Utc>` in a V2 canonical projection is a signed 64-bit Unix
microsecond JSON integer in a field ending `_unix_microseconds`. Encoding
rejects a domain value with nonzero sub-microsecond nanoseconds. It also
rejects Chrono's leap-second representation, identified by
`timestamp_subsec_nanos() >= 1_000_000_000`, because PostgreSQL has no
bijective representation for that value. The inclusive canonical range is
`-62135596800000000..=253402300799999999`, covering UTC years 0001 through
9999. Rust checked conversion rejects every value outside that range.

The adapter binds the checked `DateTime<Utc>` to SQLx in binary
`timestamptz(6)` form and also binds its canonical signed `bigint` Unix
microseconds. The procedure requires a finite timestamp in the same range and
compares the bigint with `extract(epoch from value) * 1000000` using exact
PostgreSQL `numeric`, never floating point or text formatting. Negative values
use Unix floor semantics. PostgreSQL already stores microsecond precision and
cannot prove whether a caller originally supplied discarded nanoseconds;
sub-microsecond rejection is therefore a Rust boundary obligation, not a
database claim. Real-PostgreSQL parity tests cover both range endpoints,
adjacent rejection, negative fractions, epoch, and database clock values.
Every physical V2 timestamp column also has a database `CHECK` requiring
`isfinite(value)` and the inclusive UTC range from
`0001-01-01 00:00:00.000000+00` through
`9999-12-31 23:59:59.999999+00`; its paired canonical bigint has the numeric
range check above.

`RuntimeCertificationIntentV2::serving_lease_for` is encoded only as the
unsigned integer field `serving_lease_milliseconds`. It must be an exact whole
number of milliseconds in `1000..=300000`. Encoding rejects a `Duration` with
sub-millisecond remainder; decoding rejects zero, 999, 300001, overflow, and
non-integers. Its persisted normalized column is an integer with the same
inclusive PostgreSQL check. An interval string or floating-point seconds is
never canonical input.

### Framed digest domains

For exact domain bytes `D` and canonical payload bytes `P`, every V2 digest is:

```text
SHA256(u64be(octet_length(D)) || D || u64be(octet_length(P)) || P)
```

Lengths are unsigned 64-bit big-endian octet counts. Each domain below includes
the final NUL byte shown as `\0`; the same text without that byte is a different
and invalid domain for this contract.

| Digest or fingerprint | Exact domain bytes | Canonical payload |
| --- | --- | --- |
| `RuntimeCertificationIntentFingerprintV2` | `starring.runtime.certification_intent.v2\0` | certification intent wire |
| `RuntimeCertificationRequestDigestV2` | `starring.runtime.certification_request.v2\0` | certification request wire |
| `RuntimeLiveAttestationDigestV2` | `starring.runtime.live_attestation.v2\0` | Live attestation record wire |
| `RuntimeProductMutationDigestV2` | `starring.runtime.product_mutation.v2\0` | Product mutation wire |
| `RuntimeDrainIntentDigestV2` | `starring.runtime.drain_intent.v2\0` | drain intent wire |
| `RuntimeSuspendAttemptDigestV2` | `starring.runtime.suspend_attempt.v2\0` | suspend-attempt request wire |

Rust exposes six internal typed digest helpers and no function accepting a
caller-supplied domain. PostgreSQL has six fixed-domain wrappers over one
owner-only framing helper. The private schema and every helper revoke all from
`PUBLIC`; application, Product, and runtime login roles receive no direct
execute. High-level procedures are owned by a non-login role, schema-qualify
all relations and functions, and fix `search_path` to `pg_catalog` plus the
private runtime schema. They persist exact canonical `bytea` and never digest a
`jsonb` rendering. SQL privilege and hostile-`search_path` tests prove a caller
cannot select another domain, shadow a helper, or call it directly.

For the framing payload bytes `{"format_version":2}`, the mandatory SHA-256
goldens are:

| Domain suffix | Exact lowercase hexadecimal digest |
| --- | --- |
| `certification_intent` | `2065f317b4f1ff6e4b66dfc47ea8d77db8e825984c00c3acc8dd24681cf40bd6` |
| `certification_request` | `d50aa91c84f365fa336357c307b8f2613c1be377cee6f5db82510ffc195c0a6d` |
| `live_attestation` | `8216ef56961340a2f4220a43bded1079fed038af95261bbd46f91e4df8ecc759` |
| `product_mutation` | `558cb8a7f9190dfc7a7784750bf4e0d053ed7c2bb6c36c6ba6b7fd80c39bff81` |
| `drain_intent` | `08ae4fb2781f1d8f841912af5b0397468ba19fb2f41278933cce30f229943564` |
| `suspend_attempt` | `4d36fe1ee130959adbf77dd0df4ae5c49b36b188a12bfeb25fa0325a63e72c85` |

### Non-self-referential Live attestation

The sole preimage for `RuntimeLiveAttestationDigestV2` is the private-field
domain record:

```rust
pub struct RuntimeLiveAttestationRecordV2 {
    request_digest: RuntimeCertificationRequestDigestV2,
    request: RuntimeCertificationRequestV2,
}
```

Its sole public constructor is
`RuntimeLiveAttestationRecordV2::from_request(request)`. It accepts no digest.
Before hashing, it recomputes and verifies the intent fingerprint, validates
route admission, and requires exact equality across scope, target, serving
slot, process identity, controller fence, gateway-owner lease and revision,
runtime build revision, panel process and fence, route process and fence, and
binding-pin scope and target. It then canonicalizes the request and computes
the request digest internally. Decode alone reads an embedded request digest,
recomputes it, performs the same cross-field validation, and rejects mismatch.
The record exposes read-only accessors only. The Live record and its wire projection
contain neither their own attestation digest nor a serving identity, serving
receipt, `certified_at`, deployment snapshot, or transition receipt. Those
values are outputs of the atomic commit and may bind the resulting attestation
digest, but cannot enter that digest's own preimage. The database stores the
canonical record bytes, request digest, and resulting attestation digest and
checks them before committing Live. The commit procedure recomputes the framed
request digest from the exact request `bytea`, requires the record `bytea` to
equal the fixed record projection built from that request and recomputed digest,
then recomputes and compares the framed Live digest. It never trusts a caller's
digest or a parsed `jsonb` rendering.

The exact record bytes are the concatenation of these ASCII and byte segments,
with no whitespace or escaping substitution:

```text
{"format_version":2,"request_digest":"
<64 lowercase ASCII hexadecimal request-digest bytes>
","request":
<exact canonical certification-request object bytes>
}
```

The line breaks above separate segments and are not bytes. The actual prefix is
`{"format_version":2,"request_digest":"`, the middle delimiter is
`","request":`, and the suffix is `}`. Because the digest alphabet is fixed
and the request is one canonical JSON object, direct concatenation is the sole
record encoder in Rust and PostgreSQL.

### Immutable byte persistence

Every high-level first-apply or commit procedure independently constructs each
affected canonical root from its already validated typed arguments with one
owner-only, fixed-version projection builder. It compares that expected bytea
with the caller's canonical bytea before hashing or storing it. Reordered
fields, whitespace, alternate escaping, a cross-field mismatch, or any other
byte difference rejects the transaction even if the caller also supplies the
matching hash of those noncanonical bytes. The certification reserve builds
the intent; Live commit builds request and record; Product first apply builds
the Product and drain preimages; suspension create builds its request.

Projection builders are fixed-schema functions, not a generic JSON or
caller-selected domain facility. They assemble reviewed constant UTF-8
segments and typed field projections, use one private byte-exact JSON string
escape primitive, and never feed `jsonb::text` or another database JSON object
rendering into a digest. Rust and PostgreSQL goldens cover empty strings,
quotes, reverse solidus, control bytes, BMP and non-BMP Unicode, every nested
variant, and all six complete roots. High-level procedures are the only
granted entry points; login roles cannot execute builders or the escape
primitive directly.

Certification reservation atomically stores operation scope and ID,
`certification_intent_bytes`, and its fingerprint. Live commit atomically adds
`certification_request_bytes`, request digest,
`live_attestation_record_bytes`, Live digest, and the typed attestation and
serving outputs. No procedure reconstructs a missing root from mutable columns
or writes only a digest.

Product first apply atomically stores Product scope and operation ID,
`product_mutation_request_bytes`, its digest, drain scope and intent ID,
`drain_intent_request_bytes`, its digest, and initial drain state. Product
status and drain revision, claim, progress, acknowledgement, and timestamps are
separate mutable columns and cannot alter either root. Suspension create
atomically stores scope and suspension ID, `suspend_attempt_request_bytes`, its
digest, and initial sidecar state; current local effect, obligation, sidecar
revision, and completion are separate mutable columns.

Each immutable root record has `NOT NULL` bytes, typed digest, ID, and scope
fields in the transaction that first makes that record visible; no partially
filled root record exists. Certification intent is one insert-only record, and
the request plus Live record are an insert-only child created by Live commit.
Updates deny changes to either. Exact replay compares stored bytes before
mutable processing; the same scope and ID with different bytes or digest is
typed divergence. Row ACLs deny direct DML to every login role.

### Required conformance tests

Rust release gates include exact-byte goldens and decode/re-encode equality for
all six top-level projections; the six framing goldens above; domain-separation
tests using the same payload; and compile-time or public-surface guards proving
the wire structs and unchecked constructors are inaccessible. Negative tests
cover unknown, duplicate, missing, reordered, and wrong-version fields;
noncanonical JSON; every invalid ID class; every fixed enum tag; omitted versus
explicit-null options; pre-epoch, epoch, maximum supported, and
sub-microsecond timestamps; and 999, 1000, 300000, 300001, overflow, and
sub-millisecond leases. Changing any request field must change its request
digest and checked Live record digest. Injecting an attestation digest,
serving output, commit timestamp, or snapshot into the Live preimage must be
impossible through the typed API. Root-size limits, `i64::MAX` integer bounds,
all payload variant shapes, no-flatten guards, Product semantic digest format,
Product and drain non-self-reference, `from_request` cross-field mismatches,
and exact Live-record concatenation are mandatory cases. Snowflake cases accept
`"1"`, `"9223372036854775808"`, and `"18446744073709551615"`; they reject an
empty string, `"0"`, `"01"`, signs, whitespace, non-decimal bytes,
`"18446744073709551616"`, JSON numbers, and alternate escapes. Timestamp cases
also reject a microsecond-aligned Chrono leap second with
`timestamp_subsec_nanos() == 1_000_000_000`.

Real PostgreSQL 16 release gates create the migrated schema and directly prove
all identifier and digest checks, including uppercase and wrong-length
rejection. They compare the database framing helper with all six Rust goldens,
persist and reload exact canonical `bytea`, and prove no `jsonb` round trip is
used. Direct hostile calls submit reordered, whitespace-padded,
alternate-escaped, and cross-field-mismatched roots together with the correct
hash of those hostile bytes; every procedure must reject without a write.
Snowflake SQL cases accept `9223372036854775808` and
`18446744073709551615` as text while rejecting zero, leading zeros, JSON
numbers, and `18446744073709551616` without a `bigint` cast.
Timestamp parity covers negative microseconds, epoch, positive values,
database-produced `clock_timestamp()`, boundary rejection, and exact
microsecond round trips. Lease parity covers both accepted boundaries and each
adjacent rejection. Certification, Product drain, and suspension procedure
tests prove exact replay preserves the original ID and bytes while any changed
ID, digest, canonical payload, fixed tag, or normalized time/duration is a
closed divergence or constraint failure. Fault injection loses every
first-apply acknowledgement and proves transaction-ended plus scope-only
observation adopts a committed ID, creates a new ID only after exact absence,
and rejects two rows for one natural scope. ACL tests cover direct DML, helper
execution, hostile `search_path`, and all six immutable byte columns.

## Prepared V2 Live certification

Certification separates durable preparation from the post-resume commit.

```rust
pub struct RuntimeCertificationIntentV2 {
    pub action_id: RuntimeSessionActionIdV1,
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub guard: RuntimeExecutionGuardV1,
    pub target: RuntimeDeploymentTargetV1,
    pub binding_pin: RuntimeBindingPinV1,
    pub process_identity: RuntimeProcessIdentityV1,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub runtime_build_revision: RuntimeBuildRevisionV1,
    pub panel: RuntimePanelEvidenceV2,
    pub serving_lease_for: Duration,
}

pub struct RuntimeCertificationIntentFingerprintV2(String);

pub struct RuntimeCertificationRequestDigestV2(String);

pub struct RuntimeLiveAttestationDigestV2(String);

pub struct RuntimeCertificationRequestV2 {
    pub intent: RuntimeCertificationIntentV2,
    pub intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
    pub must_commit_before: DateTime<Utc>,
    pub route_admission: RuntimeRouteAdmissionAttestationV2,
}

pub struct RuntimeServingIdentityV2 {
    pub scope: RuntimeDeploymentScopeV1,
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub attestation_digest: RuntimeLiveAttestationDigestV2,
    pub process_identity: RuntimeProcessIdentityV1,
    pub lease_epoch: NonZeroU64,
    pub revision: NonZeroU64,
}

pub struct RuntimeServingReceiptV2 {
    pub identity: RuntimeServingIdentityV2,
    pub acquired_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub connected: bool,
    pub serving: bool,
}

pub struct RuntimeCertificationReceiptV2 {
    pub action_id: RuntimeSessionActionIdV1,
    pub outcome: TransitionOutcomeV1,
    pub snapshot: RuntimeDeploymentSnapshotV1,
    pub convergence_attempt: NonZeroU32,
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
    pub request_digest: RuntimeCertificationRequestDigestV2,
    pub attestation_digest: RuntimeLiveAttestationDigestV2,
    pub route_admission: RuntimeRouteAdmissionAttestationV2,
    pub serving: RuntimeServingReceiptV2,
    pub certified_at: DateTime<Utc>,
}

pub enum RuntimeCertificationDivergenceV2 {
    OwnershipLost,
    DeploymentAdvanced { snapshot: RuntimeDeploymentSnapshotV1 },
    AuthorityChanged { snapshot: RuntimeDeploymentSnapshotV1 },
    Superseded { snapshot: RuntimeDeploymentSnapshotV1 },
    Terminal { snapshot: RuntimeDeploymentSnapshotV1 },
    ReservationMismatch,
    CommittedRequestMismatch,
    PersistenceCorrupt,
}

pub enum RuntimeCertificationRecoveryDispositionV2 {
    StopOwnership,
    DrainAndReplan,
    DrainAndStop,
    EmergencyHalt,
}

pub struct RuntimeCertificationLookupV2 {
    pub scope: RuntimeDeploymentScopeV1,
    pub deployment_revision: DeploymentRevision,
    pub convergence_attempt: NonZeroU32,
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub request_digest: RuntimeCertificationRequestDigestV2,
}

pub trait RuntimeLiveCertificationPortV2 {
    type Error;
    type Prepared: RuntimePreparedLiveCertificationPortV2<Error = Self::Error>;

    async fn prepare_live_v2(
        &self,
        intent: RuntimeCertificationIntentV2,
    ) -> Result<Self::Prepared, Self::Error>;

    async fn observe_live_v2(
        &self,
        lookup: RuntimeCertificationLookupV2,
    ) -> Result<RuntimeCertificationObservationV2, Self::Error>;
}

pub trait RuntimePreparedLiveCertificationPortV2: Sized {
    type Error;
    type TransactionEnded;
    type AbortRecovery: RuntimeAbortRecoveryPortV2<
        Error = Self::Error,
        TransactionEnded = Self::TransactionEnded,
    >;
    type CommitRecovery: RuntimeCommitRecoveryPortV2<
        Error = Self::Error,
        TransactionEnded = Self::TransactionEnded,
    >;

    fn must_commit_before(&self) -> DateTime<Utc>;

    async fn commit_live_v2(
        self,
        authorized: RuntimeAuthorizedCertificationRequestV2,
    ) -> Result<
        RuntimeCertificationReceiptV2,
        RuntimeCommitCompletionErrorV2<
            Self::Error,
            Self::CommitRecovery,
            Self::TransactionEnded,
        >,
    >;

    async fn abort(
        self,
    ) -> Result<
        Self::TransactionEnded,
        RuntimeAbortErrorV2<Self::Error, Self::AbortRecovery>,
    >;
}

pub trait RuntimeAbortRecoveryPortV2: Sized {
    type Error;
    type TransactionEnded;

    async fn quiesce(
        self,
        timeout: Duration,
    ) -> Result<
        Self::TransactionEnded,
        RuntimeRecoveryPendingV2<Self::Error, Self>,
    >;
}

pub trait RuntimeCommitRecoveryPortV2: Sized {
    type Error;
    type TransactionEnded;

    fn lookup(&self) -> &RuntimeCertificationLookupV2;

    async fn quiesce_and_observe(
        self,
        timeout: Duration,
    ) -> Result<
        RuntimeCertificationRecoveryOutcomeV2<Self::TransactionEnded>,
        RuntimeRecoveryPendingV2<Self::Error, Self>,
    >;
}

pub struct RuntimeCertificationCommitAuthorityV2 {
    private: (),
}

pub struct RuntimeAuthorizedCertificationRequestV2 {
    request: RuntimeCertificationRequestV2,
    authority: RuntimeCertificationCommitAuthorityV2,
}

pub struct RuntimeCertificationRecoveryOutcomeV2<W> {
    pub transaction_ended: W,
    pub observation: RuntimeCertificationObservationV2,
}

pub enum RuntimeCommitCompletionErrorV2<E, R, W> {
    DefinitelyRolledBack {
        source: E,
        transaction_ended: W,
    },
    CommitUnknown {
        source: E,
        recovery: R,
    },
}

pub struct RuntimeAbortErrorV2<E, R> {
    pub source: E,
    pub recovery: R,
}

pub struct RuntimeRecoveryPendingV2<E, R> {
    pub source: E,
    pub recovery: R,
}
```

The intent, request, digest, fingerprint, receipt, lookup, observation,
divergence, and recovery-disposition domain DTOs live in the pure controller
contract. Only the six roots named by the canonical V2 addendum have canonical
`bytea` projections and digest domains. Other DTOs use typed columns or are
nested by an explicitly listed root; deriving Serde is not a substitute. The
receipt carries every identity needed by heartbeat, exact observation, and
conditional disconnect; adapters never reconstruct one from the current slot
row.
All digest and fingerprint newtypes accept only 64 lowercase hexadecimal
SHA-256 characters and expose no unchecked constructor.

The port traits, generic recovery carriers,
`RuntimeCertificationCommitAuthorityV2`, and
`RuntimeAuthorizedCertificationRequestV2` live in
`automation-runtime-worker`. The generic recovery outcome and error carriers
have public fields and may be constructed and destructured by a port
implementer. They confer no authority by themselves: construction requires
ownership of the exact associated recovery handle or transaction-ended proof.

`automation-runtime-execution-postgres` owns the concrete prepared, abort-
recovery, commit-recovery, and transaction-ended associated types and their
private constructors. Those concrete types are public only as required by the
trait implementation and expose no public field or constructor. Coordinator
permits, public admission permits, commit authority, the authorized request
wrapper, concrete prepared and recovery handles, transaction-ended proofs,
drain seals, authorized drain wrappers, closed drain-recovery permits, and
gateway recovery-resume and shutdown drain-completion permits implement none
of `Serialize`, `Deserialize`,
`Clone`, or `Default`. Compile-fail and public-surface guards prove that
external code cannot construct, deserialize, clone, or default any of these
authority-bearing types while execution-postgres can construct the generic
carriers from legitimately owned associated values.

The divergence mapping is closed: `OwnershipLost` maps to `StopOwnership`;
`DeploymentAdvanced` and `AuthorityChanged` map to `DrainAndReplan`;
`Superseded` and `Terminal` map to `DrainAndStop`; reservation mismatch,
committed-request mismatch, and persistence corruption map to `EmergencyHalt`.
No raw database error or free-form reason controls recovery.

The concrete prepared handle is non-cloneable and `#[must_use]`. It owns one
dedicated connection and an ordinary PostgreSQL transaction, not a PostgreSQL
prepared transaction and not two-phase commit. Consuming `commit_live_v2` or
`abort` resolves it exactly once. Cancellation or `Drop` quarantines and closes
the dedicated connection; it never returns an unresolved session to a pool.
An explicit abort uses bounded rollback. Abort failure means rollback is not
yet confirmed and requires connection termination, but it can never be
`CommitUnknown` because no commit was sent. `DefinitelyRolledBack` includes the
not-sent case. Once commit dispatch begins, cancellation or acknowledgement
loss preserves the pinned operation ID and request digest as `CommitUnknown`.

Every unconfirmed completion returns a non-cloneable recovery handle that owns
the quarantined dedicated connection. Abort recovery only quiesces. Commit
recovery internally pins its exact lookup and accepts no caller-supplied scope
or digest; it quiesces and then obtains the same serving-slot and deployment
locks on the observation connection. A returned observation therefore also
returns the adapter-owned opaque associated proof that the original transaction
ended. The pure worker can carry but cannot name or construct the concrete
proof. Timeout returns the still-owned
recovery handle for another bounded wait, retains no poolable connection, and
leaves the process in emergency recovery. At the process hard deadline the
handle is closed and the process exits; it is never reused by another attempt.

The handle pins every immutable prepared-intent field. The pure worker owns the
private constructors for `RuntimeCertificationCommitAuthorityV2` and
`RuntimeAuthorizedCertificationRequestV2`; creating the authorized wrapper
consumes the coordinator's non-cloneable commit permit. The execution
PostgreSQL crate depends on the worker contract only to implement that port; it
does not own or manufacture capabilities. It can commit only the authorized
wrapper and depends on no gateway or Twilight type. The wrapper exposes a
read-only request projection for binding, but no public constructor or mutable
accessor. The adapter compares the complete prepared
intent with the inner request before sending SQL.

After both authority freezes settle and before opening the prepared transaction,
`ReserveCertificationIntentV2` locks the slot and Awaiting row and persists one
operation ID, immutable canonical intent bytes, and fingerprint in a separate
insert-only operation row keyed by the unchanged Awaiting deployment revision
and convergence attempt. It does not mutate the deployment phase, revision,
guard, or mutation clock. A unique constraint permits only byte-exact replay;
replay returns the identical reservation receipt, while any different ID,
bytes, or fingerprint is divergence. Awaiting reset terminally consumes that
reservation in the same transaction that advances deployment revision.
Its fingerprint uses the framed
`starring.runtime.certification_intent.v2\0` domain from the canonical V2
addendum. The private versioned wire projection binds every field shown in
`RuntimeCertificationIntentV2` in a fixed order; golden bytes and digest
vectors are release gates. The authorized
post-resume request carries that exact intent projection and fingerprint
byte-for-byte, then adds prepared deadline, barrier, route, and gateway
evidence. Canonical request bytes, request digest, canonical Live record bytes,
and Live digest are persisted atomically only with a successful Live commit. A
crash before intent reservation uses transaction-ended proof and scope-only
observation before a new ID; reset also requires route absence. No API can
replay or first-apply a commit request after commit uncertainty.

`prepare_live_v2` runs only while holding all three finalization and authority
reservations. It starts one bounded transaction and locks the exact
serving-slot advisory key, deployment, controller, fence, attempt, generation,
current tenant and installation authority, active target, historical binding
authority, accepted V2 panel projection, and stable gateway-owner lease plus
current owner revision. It requires `AwaitingGatewayReady` with the exact
reserved operation and fingerprint and computes a database-derived absolute
`must_commit_before` from controller expiry, owner expiry, the hard barrier
budget, and the required post-commit monitor-start margin. Statement, lock, and
idle-in-transaction deadlines are strict.

Barrier B then performs:

1. Transfer the prepared job and all reservations to the finalization
   supervisor while admission remains open.
2. Pause the exact connected epoch.
3. Activate the exact staged registry token synchronously.
4. Resume the same epoch with the typed activation receipt.
5. Build and double-check V2 route-admission evidence.
6. Mint and consume the exact certification commit permit.
7. Commit through the same prepared transaction after admission has resumed,
   while ordinary-barrier exclusion remains held.
8. Recheck the ready lease and route witness after the receipt.
9. Start the exact serialized serving monitor and verify the local route while
   public admission remains closed.
10. Enter `AdmissionAcknowledging`, publish and exact-observe the required
    Production or Cutover acknowledgement, and only then enter the matching
    `Open` mode.
11. Disarm the safety guard and release all three reservations.

Acknowledgement or post-receipt failure enters `Emergency`. If commit already
succeeded, the finalizer closes and joins the new monitor, conditionally
disconnects only that receipt, and exact-drains the route before releasing any
reservation.

The commit statement obtains `certified_at` from `clock_timestamp()` after
admission evidence exists. In that same statement it revalidates the exact
controller identity and fence, stable owner lease ID, attested current owner
revision, and both current expiry values. It requires `certified_at` to precede
both expiries by the configured post-commit safety margin and requires
`certified_at <= must_commit_before`. Any mismatch or insufficient margin
writes nothing and is `DefinitelyRolledBack`; transaction start time and
PostgreSQL `now()` are not used for this check.

No SQL statement, PostgreSQL round trip, Discord HTTP request, filesystem IO,
external network IO, sleep, retry, allocation, or contended queue reservation
runs while admission is paused. Pause and resume command slots, lifecycle
slots, and buffers are reserved before the pause. Only synchronous registry
activation plus bounded in-memory runtime claim, acknowledgement, and atomic
observation waits are allowed. The already-open transaction is intentionally
idle across that bounded pause. Its lifetime span and idle overlap are measured
separately and bounded by `must_commit_before`.

Product Apply already rejects another nonterminal same-lane pending deployment.
Therefore a rolled-back certification remains `AwaitingGatewayReady` and blocks
another Apply, while a committed certification atomically creates Live,
attestation, and serving lease. No candidate table is added.

Product API authority and lifecycle procedures cannot execute local gateway or
registry effects in the runtime process. Under the serving-slot advisory lock,
an authorized Product mutation encountering `AwaitingGatewayReady` or a durable
local effect creates or exactly replays a correlated
`RuntimeDrainIntentV2`. The intent binds a CSPRNG intent ID, Product operation
ID, Product semantic-request and mutation digests, exact deployment revision,
slot, expected target, and requested lifecycle mutation through the two
immutable preimages. Its durable `Pending` or `RouteAbsentAcknowledged` state
freezes new runtime claim, refence, staging, and certification for that slot.
The Product call returns pending without applying the lifecycle mutation.

The exact runtime owner alone claims the intent. It enters emergency admission
pause for unresolved certification, otherwise uses an ordinary exact-slot
barrier, resolves any certification operation by lookup, drains and removes
the exact route, and records `RouteAbsentAcknowledged` with the exact owner,
controller fence, route identity, incarnation, barrier, and registry observation
sequence. The Product retry must use the same Product operation ID and digest;
under the same serving-slot lock it verifies and consumes that acknowledgement
atomically with the requested lifecycle mutation. Only that transaction lifts
the slot freeze. An explicit authorized cancellation may consume the intent
only while route absence is still proven. Cancellation preserves the current
target but atomically advances the deployment revision and mutation clock while
terminally consuming the intent. A later Product operation therefore uses the
successor revision and cannot collide with or revive the cancelled natural
scope. Application roles retain no direct table mutation authority, and the
runtime cannot reclaim the slot between the acknowledgement and Product
consumption.

`RuntimeDrainClaimV2` is a dedicated drain-intent claim, not an ordinary
convergence controller claim. Under the serving-slot advisory lock, it is legal
for the exact `Live` deployment named by the intent and for a frozen
nonterminal deployment with a durable local effect. Acquisition revalidates
the exact pending intent, deployment revision, target, slot, gateway owner
lease, and observed owner revision, then allocates a fresh controller identity
and a successor fencing token from the deployment fencing sequence. It grants
only certification observation, serialized serving-lane close and join, exact
serving observation and conditional disconnect, admission pause, exact-route
refence, drain/remove, and acknowledgement authority. It grants no phase
transition, panel mutation, certification prepare, heartbeat start or renewal,
staging, or activation authority.

The registry adds a per-slot `DrainClaimSealed` admission state. Under one
registry lock, the worker matches the exact intent, slot, process identity,
route incarnation, old controller fence, and observation sequence, changes
only that slot to non-admitting, and returns a non-cloneable seal capability
plus a pure seal witness. A slot with no route receives an empty-slot seal that
also rejects install and activation. Public route A/B validation requires an
unsealed `Serving` witness, so admission racing the seal either transfers its
guard before the seal and is counted or fails its final collect. Other slots
continue admitting normally.

The worker waits only the sealed slot's previously admitted guards outside any
global coordinator reservation or gateway pause. It then moves the seal
capability into a process-owned claim finalizer and mints a non-cloneable
`RuntimeAuthorizedDrainClaimV2`. Minting briefly enters the coordinator's
synchronous invalidator arbitration without changing its public mode or holding
it across any wait or IO. An invalidator or shutdown that wins first mints no
wrapper and sends no claim. A claim dispatch that wins first registers the
exact per-slot finalizer and binds the wrapper to the current coordinator
generation before releasing arbitration. Every later emergency or shutdown
must join that finalizer or exact-observe its claim result before route mutation
or pool close.

The drain-intent persistence port accepts only that wrapper and commits the
dedicated claim, durable seal witness, and
successor deployment fence outside any global barrier. The wrapper has private
fields, no wire conversion, and implements none of `Serialize`, `Deserialize`,
`Clone`, or `Default`. A determinate non-commit may unseal only after exact
observation proves the intent still has no claim and the route witness is
unchanged. An unknown result remains sealed and is exact-observed or exactly
replayed with the same claim epoch and digest; it never broadens authority.

After the claim commit, and outside every gateway pause or global coordinator
reservation, the finalizer closes and joins the exact slot's serialized serving
lane, exact-observes any unknown heartbeat, and conditionally disconnects only
the latest proven serving receipt. It also resolves a reserved certification
operation by exact lookup. The resulting immutable
`RuntimeDrainCertificationResolutionV2` is carried through every following
step and persisted in the absence acknowledgement. A committed certification
cannot proceed to refence until exact conditional disconnect is durable; no
new heartbeat can start from the sealed slot.

The finalizer then acquires global `Ordinary` only for a
bounded local barrier, pauses, CAS-refences the sealed exact route to the
successor fencing token, and resumes. After the standard resume
acknowledgement has returned the coordinator to `Open`, it persists
`Refenced` by exact claim-revision CAS outside the global barrier. Removal
requires a second bounded local barrier: pause, exact-observe the persisted
removal target with zero guards, remove it, and resume. The finalizer then
records `RouteAbsentAcknowledged` outside the barrier and consumes the local
seal only after that durable receipt. An initially absent route uses one
bounded barrier for the exact paused absence observation and skips refence.
No drain wait, drain-claim SQL, refence-progress SQL, or drain-intent
acknowledgement SQL runs during a gateway pause. The only database operation
in this drain path while the public coordinator remains non-Open after resume
is the existing bounded mode-specific ingress acknowledgement. Refence progress and
drain-intent acknowledgement wait until the coordinator is `Open`.

A mismatch does not broaden the claim and enters closed recovery. This
procedure invalidates every older controller fence while preserving both the
local seal and durable intent freeze. Failure or cancellation after claim
commit cannot unseal the slot; the process-owned finalizer completes exact
refence, durable progress, removal, acknowledgement, and resume or enters
`Emergency`. A claim that expires before acknowledgement may be replaced only
by another drain-intent claim with a still newer fencing token and an exact
successor seal.

Compile-fail tests prevent fabrication of the authorized wrapper or seal
capability. Deterministic races cover admission on the affected and unaffected
slots before and after sealing, final A/B collect, active-guard exit, claim
dispatch against emergency and shutdown, claim send, claim acknowledgement
loss, shutdown observation, heartbeat against lane close, heartbeat result
unknown, conditional disconnect, each pause and resume, refence, progress
persistence, removal, acknowledgement, unseal, crash, and restart.

Startup and closed emergency recovery use one narrower exception. The dedicated
recovery supervisor owns `RuntimeClosedDrainRecoveryPermitV2`, bound to the
exact `RecoveryPending` ID, its originating `Emergency { Starting }` or
emergency generation, current gateway owner and revision, five-capability
readiness snapshot, paused connected epoch, and registry observation sequence.
It may create an empty-slot seal, observe or claim one pending drain intent,
exact-observe an unknown claim or certification operation, persist an exact
`Claimed` to `Refenced` progress CAS from the sealed local witness, complete an
exact durably refenced local removal, and record process-local route absence.
An unknown progress CAS remains lookup-only until exact observation returns its
durable receipt. Removal still cannot start before that receipt. It may not
issue an ordinary reservation, resume, install, activate, certify, mutate a
Product lifecycle, or admit an interaction.

The closed permit also grants only the same exact serving-lane close, join,
heartbeat observation, and conditional-disconnect operations needed to produce
`RuntimeDrainCertificationResolutionV2`; it never grants heartbeat start or
renewal.

The closed permit creates a distinct non-cloneable authorized claim wrapper
whose persistence entry point revalidates every bound identity and the current
recovery ID. Shutdown arbitration either invalidates it before dispatch or
registers and joins the exact closed-recovery claim finalizer after dispatch.
A previous-process `Claimed` or `Refenced` record is acknowledged absent only
after the current empty registry, paused gateway epoch, exact serving and
certification observation, and claim fence are jointly proven. A fresh foreign
claim or serving lease returns a database-derived retry bound and remains in
the startup loop. `RouteAbsentAcknowledged` is observed but not consumed and
does not count as runtime-resolvable. Tests cover unclaimed, `Claimed`,
`Refenced`, claim-unknown, and acknowledged intents during ordinary Production
startup and Cutover startup, including shutdown at every dispatch boundary.
They also force emergency after local refence but before progress, after resume
but before ingress acknowledgement, and during unknown progress observation.

A claim dispatched before shutdown is the only drain claim eligible for
`RuntimeShutdownDrainCompletionPermitV2`. Shutdown arbitration seals new claim
dispatch, registers that exact claim finalizer, and mints the non-cloneable
permit only after exact observation proves the successor fence committed. The
permit is bound to the Shutdown generation, intent, claim revision, seal,
process, owner, and later hard-pause witness. After serving-lane close and guard
drain, it permits only exact local refence, `Claimed` to `Refenced` progress
CAS or unknown-result observation, removal after the durable Refenced receipt,
and absence acknowledgement with `Shutdown` provenance. It permits no new
claim, claim renewal, resume, heartbeat, certification, phase mutation, stage,
activation, Product mutation, or unseal before durable acknowledgement.

If shutdown cannot prove the claim result or finish that sequence within its
absolute deadline, it fabricates no progress or acknowledgement. It keeps the
intent durable, closes Discord and the process-local registry, leaves recovery
to the next closed startup fixed point, and exits with the stable nonzero
shutdown component code. Tests interrupt before claim dispatch, after dispatch,
after claim commit, before and after hard pause, refence, progress send and
unknown result, durable progress, removal, acknowledgement, and pool close.

`RouteAbsentAcknowledged` is terminal for runtime mutation even after the claim
or gateway-owner lease recorded in its evidence expires. The durable slot
freeze and exact absence evidence remain consumable by the correlated Product
operation; no successor runtime claim, refence, stage, activation,
certification, or heartbeat can reopen it. Only exact Product consumption or
authorized cancellation can leave that state.

```rust
pub struct RuntimeDrainIntentIdV2(String);

pub struct RuntimeProductOperationIdV2(String);

pub struct RuntimeProductMutationDigestV2(String);

pub struct RuntimeDrainIntentDigestV2(String);

pub enum RuntimeProductMutationKindV2 {
    Apply,
    Supersede,
    Cancel,
    AuthorityChange,
    Teardown,
}

pub struct RuntimeDrainClaimSealWitnessV2 {
    pub process_instance_id: ProcessInstanceId,
    pub slot: RuntimeServingSlotV2,
    pub intent_id: RuntimeDrainIntentIdV2,
    pub seal_generation: NonZeroU64,
    pub expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
    pub registry_observation_sequence: NonZeroU64,
}

pub enum RuntimeDrainClaimProgressV2 {
    Claimed {
        seal: RuntimeDrainClaimSealWitnessV2,
    },
    Refenced {
        seal: RuntimeDrainClaimSealWitnessV2,
        provenance: RuntimeRouteMutationProvenanceV2,
        old_route: RuntimeExactLocalRouteIdentityV2,
        removal_target: RuntimeExactLocalRouteIdentityV2,
        registry_observation_sequence: NonZeroU64,
        refenced_at: DateTime<Utc>,
    },
}

pub struct RuntimeDrainIntentKeyV2 {
    pub intent_id: RuntimeDrainIntentIdV2,
    pub product_operation_id: RuntimeProductOperationIdV2,
    pub product_mutation_digest: RuntimeProductMutationDigestV2,
    pub scope: RuntimeDeploymentScopeV1,
    pub expected_revision: DeploymentRevision,
    pub slot: RuntimeServingSlotV2,
    pub expected_target: RuntimeDeploymentTargetV1,
    pub mutation_kind: RuntimeProductMutationKindV2,
}

pub struct RuntimeDrainClaimV2 {
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub process_instance_id: ProcessInstanceId,
    pub controller_id: ControllerId,
    pub controller_fencing_token: FencingToken,
    pub claim_epoch: NonZeroU64,
    pub claim_revision: NonZeroU64,
    pub expires_at: DateTime<Utc>,
    pub progress: RuntimeDrainClaimProgressV2,
}

pub enum RuntimeDrainCertificationResolutionV2 {
    NoOperationReserved,
    NoAttestationForReservedOperation {
        operation_id: RuntimeCertificationOperationIdV2,
        intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
    },
    CommittedAndDisconnected {
        operation_id: RuntimeCertificationOperationIdV2,
        serving_identity: RuntimeServingIdentityV2,
        disconnected_revision: NonZeroU64,
    },
}

pub struct RuntimeRouteAbsentAcknowledgementV2 {
    pub claim: RuntimeDrainClaimV2,
    pub expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
    pub provenance: RuntimeRouteMutationProvenanceV2,
    pub registry_observation_sequence: NonZeroU64,
    pub certification: RuntimeDrainCertificationResolutionV2,
    pub acknowledged_at: DateTime<Utc>,
}

pub enum RuntimeDrainIntentStateV2 {
    Pending { claim: Option<RuntimeDrainClaimV2> },
    RouteAbsentAcknowledged(RuntimeRouteAbsentAcknowledgementV2),
    Consumed {
        resulting_revision: DeploymentRevision,
        consumed_at: DateTime<Utc>,
    },
    Cancelled { cancelled_at: DateTime<Utc> },
}

pub struct RuntimeDrainIntentV2 {
    pub key: RuntimeDrainIntentKeyV2,
    pub intent_digest: RuntimeDrainIntentDigestV2,
    pub revision: NonZeroU64,
    pub state: RuntimeDrainIntentStateV2,
}

pub enum RuntimeDrainIntentMutationOutcomeV2 {
    Inserted,
    Replayed,
    Claimed,
    Refenced,
    Acknowledged,
    Consumed,
    Cancelled,
}

pub struct RuntimeDrainIntentReceiptV2 {
    pub outcome: RuntimeDrainIntentMutationOutcomeV2,
    pub intent: RuntimeDrainIntentV2,
}
```

`RuntimeDrainIntentIdV2` contains exactly 32 lowercase hexadecimal characters
generated from 128 CSPRNG bits. Its checked parser is the sole public
constructor, canonical decoding yields the exact 16 bytes used by
`SlotSealKeyV2`, and no unchecked constructor or alternate textual form exists.
Wire, round-trip, rejection, and golden tests cover the ID-to-seal-key boundary.

Route mutation and absence evidence never fabricates an ordinary barrier for a
closed recovery. `Ordinary` provenance requires the exact barrier and pause
witness. `ClosedRecovery` provenance requires the exact non-cloneable recovery
permit and persists its recovery ID, originating Emergency and current recovery
generations, paused gateway epoch and sequences, process, and fresh exact owner
receipt. `Shutdown` provenance requires the pre-registered claim finalizer's
shutdown-completion permit and the exact hard-pause witness; it authorizes no
resume. The acknowledgement's registry sequence must equal the observation used
by its provenance. Constructors reject cross-variant or stale-owner
combinations. Canonical wire and golden tests cover all three variants, and
database procedures revalidate every durable identity available to their
respective authorized wrappers.

The key and digest are immutable. Claim, acknowledgement, consumption, and
cancellation CAS the intent revision and exact prior state under the same slot
lock. Create replay is keyed by Product operation ID, mutation digest, scope,
and deployment revision and returns the same CSPRNG intent ID. Runtime claim
replay requires the same owner and claim epoch; a successor claim is legal only
after database-derived expiry. Claim progress advances by exact claim-revision
CAS from `Claimed` to `Refenced`; the old route and removal target must differ
only by the strictly newer controller fence. Removal cannot begin until the
`Refenced` receipt is durable. A crash before refence restarts from the exact
persisted witness. A crash after local refence but before its progress CAS keeps
the slot sealed and either completes that CAS from the exact local witness or
proves process-local absence after gateway teardown. A successor claim derives
its expected route from the prior durable removal target and uses a still newer
fence. Consumption requires the exact Product operation and route-absence
acknowledgement. Tests restart at claim commit, local admission seal, refence,
refence persistence, drain, removal, and acknowledgement. Product mutation and
drain intent digests use their distinct framed
`starring.runtime.product_mutation.v2\0` and
`starring.runtime.drain_intent.v2\0` domains from the canonical V2 addendum;
golden vectors cover the two immutable preimages. Separate state-machine tests
cover mutable drain states and mutations without placing them in either digest.

## Exact certification observation

V2 persists a certification operation ID, canonical request digest, private
Live attestation record bytes, and record format version 2. Certification
request and Live attestation digests use their distinct framed domains from the
canonical V2 addendum. The request binds the operation ID, convergence attempt,
deployment revision, controller ID and
fence, complete target and historical authority, process identity, stable
owner lease ID and observed revision, build revision, panel evidence
projection, exact intent fingerprint, prepared `must_commit_before`, barrier and pause witness, route
identity/fence/incarnation/activation sequence, connection epoch/kind/admission
revision/connected sequence/resume sequence, normalized binding pin, and
serving lease duration. The Live record contains only that request and its
checked request digest, never its own digest or commit outputs. Golden
serialization and digest vectors are release gates; map order and platform
formatting cannot change the bytes.

```rust
pub enum RuntimeCertificationObservationV2 {
    NotCommitted {
        snapshot: RuntimeDeploymentSnapshotV1,
        convergence_attempt: NonZeroU32,
        operation_id: RuntimeCertificationOperationIdV2,
        request_digest: RuntimeCertificationRequestDigestV2,
        observed_deployment_revision: DeploymentRevision,
        observed_at: DateTime<Utc>,
    },
    Committed(RuntimeCertificationReceiptV2),
    Diverged(RuntimeCertificationDivergenceV2),
}
```

`NotCommitted` requires locked proof of the same Awaiting revision, controller,
fence, attempt, operation scope, and absence of the exact attestation. The
observation time is database-derived. It is not a shallow missing-row result.

An indeterminate commit follows this closed recovery:

1. The finalization guard synchronously enters `Emergency` and permits no new
   heartbeat, prepare, resume, or mutating certification replay.
2. Quarantine the original dedicated connection and prove its backend
   transaction ended. Until termination is proven, keep the operation unknown
   and perform lookup only.
3. `DefinitelyRolledBack`, including not-sent, may drain the route and start a
   new operation only after the old transaction ended and fresh authority and
   local evidence are obtained.
4. `CommitUnknown` is permanently lookup-only. Observe by the exact operation
   ID and request digest; there is no first-apply replay API or branch.
5. On `NotCommitted`, require proven connection termination, drain and remove
   the route, and run the fenced Awaiting reset before any new attempt.
6. On `Committed`, start no heartbeat while emergency is latched. Conditionally
   disconnect only the serving identity returned by that receipt, then drain,
   remove, stale-recover, and create fresh certification evidence later.
7. On divergence, follow its closed ownership, authority, superseded, terminal,
   or corrupt classification.
8. If observation or connection termination remains unavailable, stay in
   emergency pause and exit for restart recovery without releasing the poisoned
   finalization turn.

If post-receipt lease or route verification fails, the process starts no
heartbeat, conditionally disconnects only by the returned receipt, remains
paused, drains and removes the exact local route, and runs stale recovery.

The serving adapter exposes an exact non-leaking observation. It never
disconnects an identity inferred from the current slot row.

Each committed serving identity has one serialized monitor lane for heartbeat,
observation, and disconnect. Disconnect first closes the lane's command source,
then joins or exact-observes any in-flight heartbeat, replaces the stored
receipt with its proven successor, and conditionally disconnects that exact
successor. A heartbeat and disconnect never run concurrently. Gateway loss,
shutdown, monitor-start failure, and route replacement use this same protocol;
tests commit a heartbeat at every disconnect boundary.

## Controlled route replacement

For one fenced execution, the worker performs:

1. Hydrate the exact artifact and historical binding authority and construct a
   fenced staged V2 route carrying the validated binding pin.
2. Persist preflight and install the route as `Staged`.
3. Persist `DrainRequested` and observe the exact previous serving identity.
4. Barrier A pauses, synchronously marks the previous local route Draining, and
   resumes the same epoch, then completes the mode-specific acknowledgement
   before returning to `Open`.
5. Outside pause, stop the exact previous heartbeat, conditionally disconnect
   it when locally owned, and wait for exact active count zero.
6. A fresh foreign previous lease blocks until disconnected or expired. It is
   never represented by a fabricated local token.
7. Persist drain evidence, recheck current authority, renew and refence if
   needed, and enter strict panel reconciliation.
8. Persist the complete panel certificate.
9. Run prepared certification and barrier B exactly as specified above.
10. Only a committed V2 receipt creates the serving heartbeat schedule.

Admission double-collects the public coordinator permit, maintenance permit,
ready lease, exact route witness, active guard, and the discriminated static or
historical-instance execution target. An admitted
interaction owns that immutable target and active guard for its whole
execution. It may finish after Barrier A begins, and the drain waits for it;
it never rereads the current registry route, current top-level bindings, or the
instance record. New admission cannot cross either barrier with an old ready
lease because each pause advances admission revision. Between barriers the
slot is Draining. After Barrier B a new interaction can enter only through the
new route and a fresh lease.

## Failure and retry contract

| Failure point | Required result |
| --- | --- |
| Before staging | Record a classified outcome; no route mutation |
| Staged before barrier A | Fenced-remove staged route; previous route unchanged |
| Barrier A pause failure | Do not drain; remove staged route |
| Barrier A or B pause acknowledgement lost | Issue a successor pause; use no lost token or receipt |
| Barrier A resume failure | Stay paused; enter gateway recovery; run no panels |
| Barrier A or B resume acknowledgement lost | Continue only after exact applied/current observation; otherwise emergency-pause |
| Drain timeout or foreign fresh lease | Keep slot non-serving; suspend attempt durably |
| Panel failure | Remove staged route; preserve journal; never activate |
| Prepare certification failure | Do not run barrier B; exact-drain, reset Awaiting to ReconcilingPanels, then suspend if needed |
| Barrier B pause failure | Abort prepared transaction; do not activate |
| Activation failure | Stay paused until exact target is proven non-serving |
| Barrier B resume failure | Abort certification; fenced-drain and remove route |
| Determinate certification failure | Pause, drain target, resume unaffected slots, remove, record outcome |
| Indeterminate certification | Pause and use the exact observation protocol |
| Monitor start failure after commit | Conditional disconnect by receipt, pause, drain, remove, stale-recover |

The durable controller adds a versioned `SuspendAttemptV2` sidecar. It preserves
the workflow phase instead of collapsing it into `RuntimePending`, and persists
the failure identity, attempt, deployment revision, typed disposition, exact
resume checkpoint, local effect, and drain obligation. V2 procedures do not use
the V1 `RuntimePending::Retryable` or `Blocked` variants as a second retry truth.

```rust
pub enum RuntimeAttemptDispositionV2 {
    Retryable { retry_not_before: DateTime<Utc> },
    Blocked,
}

pub enum RuntimeResumeCheckpointV2 {
    VerifyPreflight,
    RequestDrain,
    CompleteDrain,
    BeginActivation,
    ObserveActivation,
    BeginPanels,
    ReconcilePanels,
}

pub enum RuntimeSuspensionSourcePhaseV2 {
    Requested,
    PreflightReady,
    DrainRequested,
    Drained,
    ActivationApplying,
    RuntimePendingReady,
    ReconcilingPanels,
}

pub struct RuntimeExactLocalRouteIdentityV2 {
    pub identity: RuntimeProcessIdentityV1,
    pub controller_fencing_token: FencingToken,
    pub route_incarnation: NonZeroU64,
}

pub struct RuntimeServingSlotV2 {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
}

pub enum RuntimeSuspendedRouteLifecycleV2 {
    Staged,
    Draining,
}

pub enum RuntimeDrainObligationV2 {
    None,
    ExactLocalRoute(RuntimeExactLocalRouteIdentityV2),
    PreviousServing(RuntimePreviousServingLeaseIdentityV1),
    LocalAndPrevious {
        local: RuntimeExactLocalRouteIdentityV2,
        previous: RuntimePreviousServingLeaseIdentityV1,
    },
}

pub enum RuntimeLocalRouteEffectV2 {
    None,
    ExactRoute {
        route: RuntimeExactLocalRouteIdentityV2,
        lifecycle: RuntimeSuspendedRouteLifecycleV2,
    },
    RouteAbsent {
        slot: RuntimeServingSlotV2,
        expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
        provenance: RuntimeRouteMutationProvenanceV2,
        observed_sequence: NonZeroU64,
    },
}

pub struct RuntimeSuspensionIdV2(String);

pub struct RuntimeSuspendAttemptDigestV2(String);

pub struct RuntimeSuspendAttemptRequestV2 {
    pub suspension_id: RuntimeSuspensionIdV2,
    pub action_id: RuntimeSessionActionIdV1,
    pub guard: RuntimeExecutionGuardV1,
    pub source_phase: RuntimeSuspensionSourcePhaseV2,
    pub failure: RuntimeFailureV1,
    pub disposition: RuntimeAttemptDispositionV2,
    pub checkpoint: RuntimeResumeCheckpointV2,
    pub local_effect: RuntimeLocalRouteEffectV2,
    pub drain_obligation: RuntimeDrainObligationV2,
}

pub struct RuntimeSuspendedAttemptV2 {
    pub suspension_id: RuntimeSuspensionIdV2,
    pub request_digest: RuntimeSuspendAttemptDigestV2,
    pub source_guard: RuntimeExecutionGuardV1,
    pub source_phase: RuntimeSuspensionSourcePhaseV2,
    pub failure: RuntimeFailureV1,
    pub disposition: RuntimeAttemptDispositionV2,
    pub checkpoint: RuntimeResumeCheckpointV2,
    pub sidecar_revision: NonZeroU64,
    pub local_effect: RuntimeLocalRouteEffectV2,
    pub drain_obligation: RuntimeDrainObligationV2,
    pub suspended_at: DateTime<Utc>,
}

pub enum RuntimeSuspendAttemptMutationOutcomeV2 {
    Inserted,
    Replayed,
    DrainProgressed,
    Resumed,
}

pub struct RuntimeSuspendAttemptReceiptV2 {
    pub outcome: RuntimeSuspendAttemptMutationOutcomeV2,
    pub snapshot: RuntimeDeploymentSnapshotV1,
    pub suspended: Option<RuntimeSuspendedAttemptV2>,
    pub successor_execution: Option<RuntimeExecutionReceiptV1>,
}
```

`RuntimeSuspensionSourcePhaseV2` is the only phase admitted to the suspension
root. `RuntimePendingReady` means exactly `RuntimePending { condition: Ready }`;
retryable or blocked V1 pending payloads are rejected. This closed seven-value
projection prevents arbitrary `RuntimeDeploymentPhaseV1` payloads, terminal
phases, and embedded timestamps from entering the digest.

The suspension ID, request digest, source guard, complete source phase,
failure, disposition, and checkpoint are immutable. Only the sidecar revision,
local effect, and drain obligation advance through exact old-revision CAS.
Creation replay requires the same deployment scope, revision, convergence
attempt, suspension ID, and canonical request digest. Drain progress requires
the exact previous effect and obligation. Resume consumes the sidecar under the
same CAS and returns the successor execution receipt; it never
reconstructs a checkpoint from the current phase or free-form failure text.
`Inserted`, `Replayed`, and `DrainProgressed` require a present suspended
sidecar and absent successor execution. `Resumed` requires an absent sidecar
and present successor execution whose snapshot, controller identity, fence,
attempt, acquisition time, and expiry were produced by the same transaction.
Constructors and wire tests reject every other outcome and presence
combination.
The suspend-attempt digest uses the framed
`starring.runtime.suspend_attempt.v2\0` domain from the canonical V2 addendum.
Versioned, deny-unknown-field public-contract tests and golden vectors cover
every source phase, checkpoint, effect, obligation, and disposition. Suspension
IDs use the bounded canonical identifier constructor. Digests are lowercase
64-character SHA-256 values with no unchecked public constructor.

Constructors require every obligation and route effect to identify one slot and
compatible process. Any local route is drained and recorded absent before the
controller lease is released, even for `Blocked`; restart may prove process-
local absence from an empty registry. A resumed attempt always reacquires a
fresh controller guard, rechecks tenant and installation authority, and either
installs a new staged route or applies a returned successor refence token before
local mutation.

Drain progress CAS-replaces `LocalAndPrevious` with `PreviousServing` after the
exact local absence proof and never leaves a stale local identity as an active
obligation.

| Preserved source phase | Checkpoint | Allowed disposition | Required before resume |
| --- | --- | --- | --- |
| Requested | VerifyPreflight | Retryable, Blocked | Fresh guard and authority, exact hydration, local absence |
| PreflightReady | RequestDrain | Retryable, Blocked | Rehydrate Staged with a fresh fence |
| DrainRequested | CompleteDrain | Retryable, Blocked | Exact previous observe/disconnect or expiry, active count zero |
| Drained | BeginActivation | Retryable, Blocked | Previous absence, Staged candidate, fresh activation authority |
| ActivationApplying | ObserveActivation | Retryable, Blocked | Exact same-request activation observation or replay |
| RuntimePending Ready | BeginPanels | Retryable, Blocked | Accepted activation, non-serving Staged candidate, fresh fence |
| ReconcilingPanels | ReconcilePanels | Retryable, Blocked | Exact journal resume; no old-process certificate reuse |

`AwaitingGatewayReady`, `Live`, `Superseded`, and `Cancelled` cannot be
suspended. A determinate pre-commit failure in Awaiting exact-drains the local
route and uses the fenced reset to `ReconcilingPanels` before a retryable or
blocked sidecar is written. This forces a successor controller fence to create
a new staged route and new panel evidence. Retryable resumes
only after the database-derived `retry_not_before`. Blocked resumes only through
an exact failure-ID `RecoverBlockedV2`. Suspension terminates the active
90-second attempt after every process-local route effect is proven absent; an
exact foreign previous-serving obligation may remain for the successor attempt
to reobserve. Resume CAS-validates
the suspended deployment revision, attempt, controller ID, and fence, then
atomically claims a successor convergence attempt and clears the sidecar
without changing the preserved phase. The successor receives a new controller
guard and a new 90-second monotonic deadline. A generic `RuntimePending` label
never authorizes skipping drain, activation, panels, or certification.

Commit uncertainty never uses `SuspendAttemptV2`. It remains the same
`AwaitingGatewayReady` revision with its reserved operation ID and is
lookup-only under emergency pause. It cannot heartbeat, prepare again, or
change phase until exact observation returns `NotCommitted`, `Committed`, or a
typed divergence.

One attempt has one 90-second monotonic deadline beginning before claim. Phase
budgets are capped by remaining attempt time and controller and gateway-owner
lease safety margins. Renewal never resets the deadline. Retry uses one-second
initial delay, 300-second maximum, at most 20 percent deterministic jitter, and
no unbounded queue or busy loop.

## Restart reconstruction

The registry starts empty after every process restart. Durable PostgreSQL phase
and exact observations determine reconstruction.

| Durable state | Reconstruction |
| --- | --- |
| Requested | Hydrate exact target, install Staged, run preflight |
| PreflightReady | Rehydrate Staged, request drain |
| DrainRequested | Rehydrate Staged, classify local, foreign, or absent previous route |
| Drained | Rehydrate Staged, recheck activation authority |
| ActivationApplying | Rehydrate Staged, exact-replay activation |
| Any phase with Retryable sidecar | Install no Serving route; wait durable retry time, then resume its checkpoint |
| Any phase with Blocked sidecar | Install no route; retain its exact checkpoint and customer-facing failure |
| RuntimePending Ready | Rehydrate Staged and begin panels |
| ReconcilingPanels | Rehydrate Staged and resume a newly fenced journal session |
| AwaitingGatewayReady | Keep admission paused, observe its reserved operation scope, then monitor a commit or reset after route-absent proof |
| Live | Local committed receipt owns monitoring; foreign fresh Live blocks startup |
| Superseded or Cancelled | No route, session action, or monitor may remain |

A panel certificate pins process identity and cannot be reused by a new
process. Stale-Live recovery clears accepted panel, gateway, and Live evidence
and returns to a state that rebuilds them. Local registry conflict against
durable state triggers emergency pause, never best-effort overwrite.

A restarted process does not need the lost certification request to classify an
Awaiting deployment:

```rust
pub enum AwaitingCertificationScopeObservationV2 {
    Committed(RuntimeCertificationReceiptV2),
    NoOperationReserved {
        snapshot: RuntimeDeploymentSnapshotV1,
        observed_at: DateTime<Utc>,
    },
    NoAttestationForReservedOperation {
        snapshot: RuntimeDeploymentSnapshotV1,
        reserved_operation_id: RuntimeCertificationOperationIdV2,
        observed_at: DateTime<Utc>,
    },
    Diverged(RuntimeCertificationDivergenceV2),
}
```

`observe_awaiting_certification_scope_v2` locks and classifies the exact scope.
After either no-attestation outcome, only the runtime owner may execute
`ResetAwaitingGatewayReadyV2`, and only with paused admission plus exact local
route-absent proof and no unconsumed Product drain intent for the slot. The
fenced transition clears old-process panel, gateway, and certification
evidence, advances deployment revision, preserves preflight, drain, and
activation truth, and returns to `ReconcilingPanels`. It cannot be invoked by a
Product API process and cannot reset a committed or divergent scope.

## Historical instance binding pins

Pinned instance execution must use the binding authority present when the
instance was created, not the current top-level route bindings.

```rust
pub struct RuntimeBindingPinV1 {
    pub tenant_id: TenantId,
    pub installation_id: InstallationId,
    pub installation_authority_revision: NonZeroU64,
    pub binding_revision: BindingRevision,
    pub binding_fingerprint: ResourceBindingFingerprint,
}

pub struct ResolvedPinnedInstanceV2 {
    pub instance: AutomationInstance,
    pub artifact: RuleSetVersion,
    pub binding_pin: RuntimeBindingPinV1,
    pub bindings: ResourceBindingMap,
}

pub struct ExactServingRouteV2 {
    pub identity: RuntimeProcessIdentityV1,
    pub artifact: RuleSetVersion,
    pub binding_pin: RuntimeBindingPinV1,
    pub bindings: ResourceBindingMap,
}

pub enum AdmittedExecutionTargetV2 {
    Static {
        route: ExactServingRouteV2,
    },
    Instance {
        serving_route_identity: RuntimeProcessIdentityV1,
        resolved: ResolvedPinnedInstanceV2,
    },
}

pub struct AdmittedInteractionV2 {
    pub gateway: RuntimeGatewayReadyAttestationV2,
    pub route: RuntimeServingRouteAttestationV2,
    pub target: AdmittedExecutionTargetV2,
}
```

The concrete admitted value also privately owns the non-cloneable public
coordinator permit, maintenance ingress permit, and registry active guard;
those transient fields are never serialized into a durable DTO.

`automation_instances` stores the complete pin and explicit pin state.
Registration exact replay compares every pin field. Resolution joins the exact
historical authority, recomputes the fingerprint, and returns those bindings.
Static admission uses the registry route and its current pin. Instance
admission resolves status, artifact, historical pin, and bindings exactly once,
uses that result to select the serving slot, acquires the slot's active guard,
then double-checks the same ready lease and route witness. The immutable
discriminated target and active guard move together into execution. A status or
authority mutation after that admission linearization does not rewrite the
already-admitted target; later admissions see the mutation. Execution performs
no second instance lookup and never falls back to the current route pin or
current installation bindings. Child registration consumes the carried pin.
Race tests mutate status and authority between lookup and execution and prove
snapshot completion, no second read, and no fallback.

The single instance-resolution database read is the conditional semantic
admission linearization point only when the later gate, ready, and route B
checks equal A and active-guard acquisition succeeds. Failure of any later
check produces no admitted interaction. A status or authority mutation after
that read therefore does not revoke the counted snapshot; one that commits
before the read is reflected by the same read.

Initially, the normalized current installation binding map must exactly equal
the normalized historical map. PostgreSQL JSONB source byte order is not an
identity property. A field or map mismatch fails `PinnedAuthorityChanged`. Legacy
instances without trustworthy creation-time evidence fail
`PinnedBindingMissing`; migration never guesses current bindings.

## V2 attestation and status cutover

Add nullable V2 columns with a conditional all-null or all-present constraint.
Existing immutable V1 attestations are never rewritten. V2 status requires:

- record format version 2
- canonical V2 attestation and request digests
- exact operation ID
- exact stable gateway-owner lease ID, current revision at least the attested
  revision, and fresh current expiry
- exact connection epoch, admission revision, connected sequence, and explicit
  resume sequence
- exact route identity, controller fence, incarnation, activation sequence,
  and binding pin
- exact fresh serving lease
- no unconsumed same-slot `RuntimeDrainIntentV2` in `Pending` or
`RouteAbsentAcknowledged`
- exact current `Open` database-fence generation and a fresh ingress-open
  acknowledgement bound to that generation, gateway owner, process, connection
  epoch, admission revision, and exact current open maintenance-gate generation

The current ingress acknowledgement must exactly match the current atomic
gateway snapshot and the attested connection epoch. Its admission revision may
be a successor of the attested revision, because an unrelated ordinary barrier
advances the global revision; it must be greater than or equal to the attested
revision. A connection-epoch change never qualifies an old attestation and
enters drain and recovery.

Drain-intent creation and public status or Product qualification take the same
serving-slot advisory lock. The committed intent therefore changes that slot's
higher-precedence projection to stable Maintenance or RuntimePending before a
local seal is required, even while its old immutable Live attestation and lease
remain. `Pending` and `RouteAbsentAcknowledged` both disqualify Live until the
correlated Product transaction consumes or cancels the intent. That transaction
changes the lifecycle and intent state atomically. The slot-local predicate does
not make unrelated slots or global process readiness false. Tests race status,
qualification, intent creation, seal, conditional disconnect, acknowledgement,
Product consumption, and cancellation at every lock boundary and require zero
false-Live projection.

```rust
pub struct RuntimeIngressOpenAcknowledgementV2 {
    pub fence_generation: NonZeroU64,
    pub maintenance_gate_generation: NonZeroU64,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub process_instance_id: ProcessInstanceId,
    pub connection_epoch: NonZeroU64,
    pub admission_revision: NonZeroU64,
    pub connected_event_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub resume_sequence: RuntimeGatewayAdmissionSequenceV2,
    pub acknowledgement_revision: NonZeroU64,
    pub acknowledged_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
```

One serialized ingress-ack supervisor publishes this row after every successful
ordinary or recovery resume and renews it before a fixed ten-second lease
expires in production mode. Publication runs outside the measured pause and
CAS-binds the previous ack revision, current `Open` writer-fence generation,
exact open maintenance-gate generation, owner ID and revision, process, and
complete atomic ready snapshot. PostgreSQL caps expiry by the owner lease. The
supervisor rereads coordinator, gate, owner, and gateway state after the write
and installs the receipt only if all inputs are unchanged.
Until then the coordinator stays `AdmissionAcknowledging`, health is unready,
and status is Maintenance.

While the writer fence is `Closed`, the same serialized step instead exact-
observes the current cutover lease and durable maintenance-closed
acknowledgement and enters `Open::Cutover`; it never writes or accepts an
ingress-open acknowledgement. When the cutover fence becomes `Open`, the
coordinator leaves cutover mode, opens the counted maintenance gate, and enters
`AdmissionAcknowledging` until the production acknowledgement is durable.

Lost publication acknowledgement exact-observes or replays the same operation;
it never assumes success. A barrier racing renewal makes the receipt stale and
the supervisor retries after that barrier's resume. Failure to obtain a fresh
receipt before its safety margin enters `Emergency` and closes ingress. Tests
cover every barrier, reconnect, writer-fence change, lost acknowledgement,
expiry, and stale CAS boundary.
The same tests close and reopen the gate without changing the gateway snapshot
and prove that the prior acknowledgement, Production mode, status predicate,
and public permit all become stale.

Production grants permit only V2 certification. V1 Live rows eventually project
as RuntimePending with a stable unqualified-attestation reason, but projection
cannot switch while Product Apply treats those rows as resolved.

Cutover has two independent fail-closed fences. A durable database writer fence
is checked by every ordinary Product Apply, status qualification, authority,
lifecycle, V1 worker, and V2 worker mutation procedure. Each procedure first
takes the shared transaction advisory fence lock. The cutover coordinator takes
the exclusive lock, waits every in-flight shared holder to finish, changes the
durable generation to `Closed`, and releases the lock. New ordinary calls then
acquire shared and reject. Fence-aware procedure wrappers are deployed and
proven in a prerequisite release, before projection changes.

`Closed` names one CSPRNG cutover coordinator, lease epoch, generation, and
expiry. A narrow cutover capability may renew that cutover lease and, for the
exact generation, run only this closed operation allowlist:

`RuntimeCutoverCoordinatorIdV1` follows the canonical 128-bit identifier rule
and has no unchecked constructor. Writer-fence generation advances on every
`Open -> Closed`, expired-lease takeover, and `Closed -> Open` transition.
Lease epoch is a durable high-water mark and advances on close or takeover but
not renewal. Expiry never opens the fence. One serialized renewal lane uses an
exact previous-expiry compare-and-swap; a successful renewal changes only the
expiry. Opening requires the exact fresh closed generation, coordinator, epoch,
and expiry. Generation or epoch exhaustion remains closed and requires an
offline repair rather than wrapping an identity counter.

- gateway-owner observe, acquire, renew, and release
- controller claim, renew, release, and the versioned worker-only convergence
  transitions needed to reach Awaiting
- maintenance-close acknowledgement and cutover state observation
- exact V1 serving observation and stale recovery, plus the dedicated
  route-absent legacy Awaiting reset
- exact target and historical binding-pin hydration
- strict panel-journal observe, claim, renew, apply, and completion
- certification-intent reserve, exact replay or observation, terminal consume,
  and route-absent Awaiting reset
- V2 Awaiting scope observation, prepared certification, commit, exact
  observation, and V2 stale-Live recovery
- suspension create, observe, drain progress, and resume
- drain-intent observe, claim or successor claim, exact-route refence, and
  route-absence acknowledgement
- serialized V2 serving observe, heartbeat, and conditional disconnect

Every listed read and write has a distinct cutover entry point. Each validates
the exact cutover lease and generation plus every owner, process, deployment,
target, controller, fence, and operation identity applicable to that call. No
entry point accepts an ordinary Product authority or a caller-selected
operation kind. Product Apply, lifecycle consumption, authority mutation,
general table access, arbitrary phase mutation, ingress-open acknowledgement,
and public interaction registration remain denied. Real-PostgreSQL privilege
tests invoke every allowed entry point under `Closed` and prove every
nonallowlisted procedure and direct table mutation is denied.

The `Open` commit atomically disables cutover-only authorization and makes the
same still-current exact V2 receipts eligible for ordinary procedures. Expiry
or coordinator uncertainty keeps the writer fence closed and trips runtime
emergency admission.

Every gateway process also has a `RuntimeMaintenanceIngressGateV2` before
gateway admission, instance resolution, and route lookup. It starts closed,
and exposes only non-cloneable counted ingress permits. Admission first acquires
the `Open::Production` public coordinator permit and then a counted gate permit
before any instance lookup, reads ready evidence A, resolves exactly one static
or historical target, acquires the route active guard, then rereads the same
coordinator generation and ack, gate generation, ready evidence B, and route
witness. It either transfers both permits, the route guard, and immutable target
together into execution or releases all without execution.

Closing atomically enters `Closing`, prevents new permits, waits every existing
permit and route guard through delayed lookup and execution, then enters
`Closed` and acknowledges that exact generation durably. Discord lifecycle
events may continue, but no interaction can outlive the closed acknowledgement.
Internal Barrier B may therefore resume gateway admission during
recertification without exposing partial cohorts. While the database fence is
not `Open`, public status has a higher-precedence stable Maintenance projection
regardless of V1 or V2 Live evidence.

After the database fence becomes `Open`, status and Product public
qualification remain Maintenance until a fresh exact ingress-open
acknowledgement exists for the current generation, owner, process, connection
epoch, and admission revision. A missing, expired, or stale acknowledgement
never projects Live.

Cutover preflight requires zero legacy V1 `RuntimePending::Retryable` or
`Blocked` rows. Their source checkpoint was never persisted, so migration never
guesses one; an operator must retire the exact deployment or complete a new
Product Apply before maintenance begins. Legacy V1 `AwaitingGatewayReady` has
no V2 operation or intent fingerprint. After the V1 process is fully stopped,
the cutover capability may run a dedicated versioned reset only with exact
route absence and no V1 Live attestation; it clears old-process panel and
gateway evidence, advances revision, and returns to `ReconcilingPanels` for new
V2 evidence.

Cutover is a coordinated maintenance barrier:

1. Deploy and verify the fence-aware procedures and maintenance gate on every
   running V1-compatible process, and pass the zero-legacy-failure preflight.
2. Remove public readiness, acquire the exclusive database fence, publish the
   exact `Closed` generation, and wait for every gateway process to acknowledge
   closed ingress and drain already-admitted interactions.
3. Seal and settle every V1 finalizer, stop and join every V1 worker and
   serialized serving-monitor lane, hard-pause and drop its gateway control,
   drain and remove all local routes, join its Discord shard, and release the
   exact shard owner or wait for its proven expiry. Prove the V1 process has no
   local route, control lifetime, connection, or owner authority.
4. Install additive V2 schema, procedures, normalized digest functions,
   readiness manifests, and grants without changing the production projection.
5. Under the separately bounded and renewed cutover lease, wait for every V1
   serving lease to expire, recover V1 Live to a closed fixed point, and prove
   the shard has no fresh V1 owner or serving evidence. Reset every exact
   route-absent legacy Awaiting row through the dedicated V2 migration branch.
   This cutover wait is not charged to a V2 process's 35-second startup
   operation budget.
6. Start the V2 runtime process paused, acquire the now-free owner, and run its
   startup fixed-point recovery.
7. Recertify every eligible target
   with bounded Barrier B openings behind the closed maintenance ingress gate.
8. Prove no legacy failure or Awaiting row, V1 Live evidence, or unqualified V2
   Live remains, then atomically
   switch status and Apply qualification to V2 and remove V1 production grants.
9. Change the database fence to `Open` at the irreversible commit point. The
   shard process exact-rechecks owner, ready lease, routes, monitors, and the
   open generation before opening its maintenance ingress gate and acknowledging
   it. Restore public readiness only after that acknowledgement.

Failure before the commit point leaves Product writers and maintenance ingress
closed. Failure after it uses normal fail-closed runtime recovery and cannot
roll projection back. This ordering prevents Apply from creating a newer
Requested candidate while an old V1 Live row is projected as pending, and it
prevents Discord traffic from reaching a partially recertified cohort.

## Health and deterministic shutdown

`/health/live` reports only a responsive process event loop. `/health/ready` is
true only with current five-capability readiness, gateway-owner lease, required
supervisors, completed startup recovery, an open database writer fence and
maintenance ingress gate, and a current exact explicitly-resumed ready lease.
Emergency pause, a certification requiring runtime recovery, paused reconnect,
maintenance, or shutdown is unready. An exact route-absent Product handoff is
only a frozen pending slot and does not make unaffected global readiness false.

An ordinary barrier may retain readiness only through a non-cloneable
`BarrierReadinessPermitV2` issued from the current ready lease before pause and
bound to the exact coordinator generation and barrier ID. It expires at the
250-millisecond hard deadline and cannot survive emergency, reconnect,
maintenance, or acknowledgement uncertainty. Metrics expose the barrier state.
Any missed deadline immediately removes readiness and keeps admission paused.
Customer RetryWait or Blocked states do not make the whole process unready.

SIGTERM, SIGINT, or terminal supervisor failure creates one absolute
30-second monotonic deadline and executes one idempotent order. Every join,
drain, Discord close, disconnect, and pool close receives only the remaining
budget; configuration whose guaranteed minimum sequence cannot fit is rejected
at startup.

1. Remove readiness, stop claim admission, and seal the finalization intake.
2. Enter shutdown through the commit-arbitration section and close ingress. If
   shutdown wins before commit claim, abort the prepared transaction and prove
   termination. If commit claim already won, mark shutdown pending and join the
   accepted finalizer through exact commit observation; start no monitor and
   conditionally disconnect only a committed receipt. Seal drain-claim dispatch,
   then join or exact-observe every registered per-slot claim finalizer. A
   committed successor fence transfers only into that finalizer's shutdown
   drain-completion permit; a non-commit sends no replacement claim. The claimed
   route cannot drain before both finalizer classes settle.
3. Hard-pause gateway admission and invalidate every ready lease.
4. Close and join each serialized heartbeat lane, exact-observe an unknown
   heartbeat, then conditionally disconnect its latest proven receipt.
5. Fenced-drain local routes and wait to the interaction deadline. For every
   pre-shutdown committed drain claim, use its shutdown permit to exact-refence,
   persist or exact-observe Refenced progress, and only then remove and
   acknowledge absence. No such path resumes admission.
6. Remove other drained routes and join Discord.
7. Join the certification and drain-claim finalizers plus convergence,
   recovery, ownership, readiness, and serving supervisors.
8. Release gateway ownership if exact and still held.
9. Close all five pools concurrently.
10. Drop secrets and stop the health listener last.

Shutdown never resumes admission, authorizes a new commit, or fabricates a
success attestation to make termination appear clean. A commit authorized
before shutdown won arbitration may still complete and is handled only by the
exact observation branch above. If any boundary exhausts the absolute deadline,
the process keeps both ingress gates closed, closes the recovery and dedicated
connections, leaves any unconfirmed serving lease to expire, aborts remaining
tasks, closes or drops pools, emits a stable timeout component code, and exits
nonzero. Tests use the maximum accepted configuration and race or hang every
shutdown, commit-claim, observation, and route-removal boundary.

## Observability and release SLO

Logs and metrics use finite operation, phase, outcome, and component labels.
They exclude secrets, URLs, database authority values, customer identifiers,
RuleSet JSON, Discord bodies, raw database errors, and human text.

The certified-host cohort is a release build on the deployment Mac mini at 0,
50, and 90 percent of configured admission capacity; one slot, 16 slots, and
the certified maximum active-slot cohort; concurrent claim, recovery, panel,
heartbeat, and ownership supervisors; empty and 75-percent gateway command
queues; and at least 10,000 executions of each barrier per cohort.

Fault injection covers disconnect at pause, activation, resume, commit, and
monitor start; prepared-transaction connection loss; delayed instance
resolution across both revisions; database timeout; and controller renewal at
every legal boundary.

Release gates are:

- zero wrong-target admission and zero false-Live projection
- no SQL statement, PostgreSQL round trip, or Discord HTTP span inside a
  measured pause span; the bounded idle prepared-transaction lifetime overlap
  is reported separately
- no admission on an invalidated revision
- no old-route admission after activation and no new-route admission or
  execution before barrier B resume
- barrier p95 below 25 ms, p99 below 100 ms, and zero 250 ms misses
- healthy resume-to-certify p99 below two seconds
- disposable-guild Apply-to-Live p95 at most 30 seconds and always below 90 seconds
- serving loss stops heartbeat immediately and removes projected Live by the
  45-second default lease expiry
- normal shutdown within 30 seconds

Any safety failure or hard barrier miss blocks multi-tenant public ingress. If
the global barrier fails only the performance cohort, the next optimization is
versioned per-slot admission generation. Extending pause around database or
panel work is forbidden.

## Acceptance matrix

Pure-worker tests cover every durable phase, local, foreign, and absent route
branch, retry, cancellation, observe-only commit uncertainty, restart, and
shutdown. Gateway and registry tests cover initial pause, every reconnect,
revision and sequence overflow, atomic snapshot publication, stale and foreign
pause tokens, delayed older resume after a newer pause, control-owner drop with
a buffered resume, acknowledgement loss, lifecycle delivery ordering, route
witness, refencing, both barriers, delayed lookup, active guard drain,
emergency-latch precedence, finalizer cancellation, maintenance ingress, and
unaffected-slot admission. They exercise startup, every barrier, reconnect, and
acknowledgement loss in both Production and Cutover coordinator modes.

Real PostgreSQL tests cover five direct roles and one UUID, denied cross-role
capabilities, full versus lightweight readiness, gateway-owner fencing, V2
prepare and commit, no-first-apply unknown recovery, every observation outcome,
durable Product drain handoff, authority mutation blocking, stale recovery, status
projection, partial startup cleanup, and periodic drift pause.

Disposable-guild tests cover strict panels, exact top-level routing, old
instance artifact and binding pins, reconnect recovery, heartbeat expiry, and
shutdown at every external boundary. Customer guilds are never release targets.

Workspace tests, real PostgreSQL, Clippy with warnings denied, formatting,
dependency and no-model guards, secret scan, and CI must be green. Until the
complete race, privilege, recovery, disposable-guild, and SLO cohorts pass, the
executable remains staging-only evidence.

## Minimal safe implementation order

1. Finish atomic gateway admission, exact pause token, registry witness, and
   replacement-token refencing.
2. Add pure V2 evidence, operation, suspension, drain-handoff, and binding-pin
   DTOs with canonical digest golden vectors and dependency guards.
3. Add controller sidecar state, exact checkpoint matrix, Awaiting operation
   reservation and reset, with exhaustive pure-machine tests.
4. Add gateway-owner, startup observation, writer-fence, and drain-intent SQL
   in independently green migrations with real-PostgreSQL privilege tests.
5. Add prepared certification, commit deadline, V2 status, exact observation,
   and stale-recovery SQL with separate digest and race gates.
6. Add historical binding-pin schema, resolver, registrar, and one-read
   interaction contract with migration and authority-race tests.
7. Implement exact PostgreSQL adapters and the pure worker ports without
   introducing database, Twilight, model, or operating-system dependencies.
8. Implement the emergency coordinator, process-owned finalizer, serialized
   serving lanes, startup transfer, health, shutdown, and Discord adapters in
   functional commits.
9. Pass race, restart, privilege, disposable-database, disposable-guild,
   full-workspace, CI, and certified-host SLO cohorts in staging.
10. Perform the coordinated writer-fence and maintenance-ingress cutover, V2
    recertification, projection switch, V1 grant removal, and traffic restore.

No later step may bypass a failed earlier gate.
