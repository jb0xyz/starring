# Runtime finalizer, shutdown, and staged admission design

Date: 2026-07-28

Status: proposed implementation contract

Extends:

- `2026-07-22-production-runtime-worker-composition-design.md`
- `2026-07-28-runtime-pending-drain-indeterminate-finalizer-design.md`
- `2026-07-28-runtime-expired-pending-drain-succession-design.md`

## Outcome

This slice turns the current closed startup-recovery process into a
process-owned supervised runtime without prematurely admitting customer
traffic.

It adds three connected capabilities:

1. a process-owned mutation finalizer that retains accepted mutation authority
   across caller cancellation;
2. one bounded, signal-aware shutdown path that seals intake and joins every
   accepted finalizer before closing shared dependencies;
3. a staged empty-registry admission transition that explicitly resumes one
   exact Discord connection epoch, exact-observes its ready lease, publishes
   and exact-observes the durable ingress acknowledgement, and only then enters
   an empty `Open::Production` epoch.

The registry remains recovery-empty throughout the staged admission slice. No
route is staged or activated, no panel is reconciled, no serving heartbeat is
started, and no interaction execution adapter is composed. The staged epoch
therefore proves production ownership, supervision, resume, readiness,
acknowledgement, reconnect, and shutdown behavior without serving a customer
RuleSet.

Gateway admission and public ingress remain separate boundaries:

- Discord transport `READY` or `RESUMED` creates a connected epoch that is
  still paused.
- Explicit gateway resume opens only the exact lower-level gateway epoch.
- The maintenance ingress gate may open only inside
  `AdmissionAcknowledging`, but that local transition alone creates no public
  admission permit.
- Only a current durable acknowledgement and a final exact reread may enter
  `Open::Production`.
- This implementation slice has no consumer of the public admission permit.
  The exact empty registry is an additional runtime proof, not the primary
  security boundary.

The paused-by-default contract is unchanged. A restart, reconnect, owner loss,
readiness loss, acknowledgement loss, task failure, signal, overflow, or
uncertain transition closes admission. Nothing auto-resumes a successor
connection.

## Non-goals

This slice does not add:

- route hydration, staging, activation, replacement, or serving;
- interaction event dispatch or customer RuleSet execution;
- panel reconciliation;
- certification prepare or commit;
- serving heartbeat lanes;
- route-present pending-drain recovery;
- `PendingRefenced` recovery;
- Product consumption or cancellation of a route-absent acknowledgement;
- maintenance cutover recertification;
- multi-shard ownership;
- a general retry executor;
- a detached best-effort cleanup task.

The finalizer is not authority to invent a replacement mutation. It may finish
or exactly classify only the immutable mutation transferred into it.

## Current baseline and required change

The current runtime already proves:

- five independently credentialed database capabilities;
- an exact gateway-owner lease and startup watchdog;
- one Discord control lifetime with explicit-resume policy;
- an actual paused `READY` or `RESUMED` epoch;
- a linearly owned closed-recovery session;
- a recovery-empty local registry fixed point;
- exact pending-drain mutations, one bounded same-request finalization, and
  previous-owner succession;
- bounded cleanup on ordinary returned errors.

The current implementation deliberately stops at the fixed point. Its Discord
actor is startup-scoped, treats an admission resume as terminal
`AdmissionOpened`, and is bounded by the startup operation cutoff. Mutation
finalization is borrowed or inline in the startup driver. Dropping the outer
future fails closed, but it cannot await the explicit cleanup sequence after
that cancellation.

The next composition must not solve those limitations by:

- extending the startup cutoff into an unbounded process lifetime;
- allowing the startup caller to own production tasks;
- spawning an untracked finalizer;
- treating a queued resume as ready evidence;
- opening the public ingress gate before the durable acknowledgement;
- making `/health/ready` true at the recovery fixed point;
- dropping pools while an accepted mutation may still be using them.

## Global safety invariants

The following invariants are release blockers.

### Authority

- Every external mutation is registered with the process finalizer before its
  first dispatch.
- Registration atomically transfers the non-cloneable mutation authorization,
  local seal or registry binding, exact recovery session authority, and
  dependency lifetime into process ownership.
- A rejected registration returns the complete unconsumed job to the caller.
  It sends no database mutation.
- After registration succeeds, caller cancellation drops only a result waiter.
  It cannot cancel, duplicate, or reclaim the accepted job.
- A finalizer uses the original action identity, request, candidate, source
  digest, owner evidence, minimum database clock, and seal. It never mints a
  successor identity for retry.
- Only an `Indeterminate` result permits the one exact finalization invocation
  defined by the existing pending-drain contract.
- A second uncertainty is terminal for the process. It remains durable and
  fail closed for restart recovery.
- Shutdown cannot convert an unknown mutation into success.

### Admission

- Every process and every connection epoch begins paused.
- An actual Discord `READY` or `RESUMED` event does not open admission.
- Resume requires the exact current pause token, coordinator generation,
  connection epoch, admission revision, recovery-resume permit, and owner.
- A resume acknowledgement is not a durable ingress acknowledgement.
- The maintenance ingress gate starts closed. It may move through `Opening` to
  `Open` only inside the exact `AdmissionAcknowledging` transition, and an open
  gate without the durable acknowledgement creates no public permit.
- `Open::Production` requires the current writer-fence generation, open
  maintenance-gate generation, owner receipt, process identity, complete
  gateway snapshot, and durable ingress acknowledgement.
- No public admission permit exists in `Emergency`, `RecoveryPending`,
  `AdmissionAcknowledging`, or `Shutdown`.
- A reconnect invalidates the prior ready lease, resume evidence,
  acknowledgement, process readiness, and public permit before the new epoch
  can be observed.
- The empty Open slice exposes no interaction execution consumer. A future
  consumer must be added only with its own exact route and active-guard
  double-collect.

### Shutdown

- The first terminal signal or terminal supervisor failure creates one
  process-wide absolute monotonic shutdown deadline.
- Repeated signals do not extend the deadline and do not start a second
  cleanup path.
- Readiness is removed and finalizer intake is sealed before any dependency is
  closed.
- An accepted finalizer is joined or exact-observed before its database pool,
  registry binding, Discord control lifetime, or gateway owner is released.
- Shutdown never resumes admission.
- The health listener is stopped last so liveness can report bounded cleanup
  while readiness remains false.
- Deadline exhaustion aborts remaining tasks, preserves closed ingress, emits
  one stable component code, and exits nonzero.

## Layered admission model

The word `Open` is valid only when its layer is explicit.

| Layer | Open evidence | What it authorizes |
| --- | --- | --- |
| Discord transport | actual `READY` or `RESUMED` event | a connected epoch only |
| Gateway admission | exact explicit resume applied to that epoch | gateway event delivery under the current admission revision |
| Maintenance ingress | counted local gate in exact `Open` generation | entry into the public admission double-collect |
| Coordinator | `Open::Production` with installed durable acknowledgement | minting a non-cloneable public admission permit |
| Route | exact unsealed `Serving` witness and active guard | one pinned execution target |

The staged slice implements the first four layers and requires the route layer
to remain absent. It does not install the public interaction consumer.

The durable ingress acknowledgement is
`RuntimeIngressOpenAcknowledgementV2` from the production worker contract. It
binds:

- open writer-fence generation;
- open maintenance-gate generation;
- stable gateway-owner lease ID and observed revision;
- process instance ID;
- connection epoch;
- admission revision;
- connected-event sequence;
- explicit resume sequence;
- acknowledgement revision and lease interval.

The acknowledgement is accepted only after an exact reread of the writer
fence, local gate, owner, gateway snapshot, and coordinator generation. Its
lease is not inferred from host time. A missing, expired, stale, ambiguous, or
unknown acknowledgement leaves the coordinator
`AdmissionAcknowledging` and readiness false.

## Typed process state machine

The existing `RuntimeGatewayClosedLifecycleV2` remains the closed prefix of the
accepted `GatewayBarrierCoordinatorV2` contract. The implementation adds a
single full-lifecycle owner that consumes that prefix after the fixed-point
proof. It does not copy the closed snapshot into an unrelated coordinator.

The tools-private process state is:

```rust
pub enum RuntimeSupervisedProcessStateV2 {
    StartupPaused(RuntimeStartupPausedProcessV2),
    Recovering(RuntimeClosedRecoveryProcessV2),
    FixedPoint(RuntimeStartupRecoveryFixedPointProcessV2),
    ProductionHandoff(RuntimeProductionHandoffProcessV2),
    AdmissionAcknowledging(RuntimeAdmissionAcknowledgingProcessV2),
    EmptyOpen(RuntimeEmptyOpenProcessV2),
    Emergency(RuntimeEmergencyProcessV2),
    ShuttingDown(RuntimeShuttingDownProcessV2),
    Terminated(RuntimeTerminatedProcessV2),
}
```

Authority-bearing process states implement none of `Clone`, `Copy`,
`Serialize`, `Deserialize`, or `Default`. Debug output is redacted. State
transition methods consume `self` and return exactly one successor or a failure
that retains cleanup ownership.

The process-level transitions are:

```text
StartupPaused
    -> Recovering
    -> FixedPoint
    -> ProductionHandoff
    -> AdmissionAcknowledging
    -> EmptyOpen

Any nonterminal state
    -> Emergency
    -> Recovering

Any nonterminal state
    -> ShuttingDown
    -> Terminated
```

`EmptyOpen` is the staged implementation of the canonical
`Open::Production` mode with an exact empty registry and no interaction
consumer. It contains:

```rust
pub struct RuntimeEmptyOpenEpochV2 {
    coordinator_generation: RuntimeGatewayCoordinatorGenerationV2,
    gateway_owner: RuntimeGatewayOwnerLeaseReceiptV1,
    readiness: RuntimeCapabilityReadinessSetV2,
    gateway_ready: RuntimeGatewayReadyAttestationV2,
    ingress_acknowledgement: RuntimeIngressOpenAcknowledgementV2,
    registry_empty: RuntimeRegistryRecoveryEmptyObservationV2,
    finalizer_generation: RuntimeMutationFinalizerGenerationV1,
}
```

The value is non-authorizing outside the process and is never persisted as a
substitute for its individual receipts. Every readiness or public-admission
check rereads current sources rather than trusting the bundle by age.

### Production handoff

The root process, signal latch, health listener, and mutation-finalizer
supervisor are created before the closed recovery loop can dispatch its first
mutation. During closed recovery the finalizer accepts only
`StartupPendingDrain` jobs. The fixed-point handoff does not replace that
supervisor; it proves the startup job set is settled and advances the same
supervisor generation into its process-lifetime mode.

`FixedPoint -> ProductionHandoff` is one consuming operation:

1. revalidate the fixed-point proof and exact empty-registry cursor;
2. seal startup recovery intake and prove every registered startup job is
   settled;
3. transfer the frozen startup owner receipt and renewal schedule to the
   production owner supervisor without a renewal gap;
4. transfer the Discord actor from startup-bounded mode to process-supervised
   mode without creating a second shard or control lifetime;
5. retain the same finalizer generation and start the production owner,
   readiness, ingress-acknowledgement, and maintenance-gate supervision lanes;
6. prove every required supervisor has acknowledged startup;
7. consume the closed recovery fixed point into the one recovery-resume
   transition.

Failure before step 7 remains paused and uses startup cleanup. Failure after a
supervisor accepts ownership enters process shutdown and joins that supervisor.
No partially transferred owner may be returned to the startup watchdog.

### Recovery resume

The fixed-point recovery permit may mint exactly one non-cloneable
recovery-resume permit. The resume sequence is:

1. double-observe the exact current paused connected epoch;
2. verify current owner and all five readiness receipts;
3. revalidate the exact empty registry instance and global sequence;
4. require an exact `Open` writer fence and exact closed maintenance ingress
   gate for the Production transition;
5. reserve gateway command and lifecycle capacity;
6. claim resume under the coordinator generation and exact writer-fence
   generation;
7. apply explicit resume with the exact opaque pause token;
8. exact-observe the atomic gateway snapshot and ready lease;
9. require the same connection epoch and a resume sequence strictly greater
   than the actual connected-event sequence;
10. enter `AdmissionAcknowledging`;
11. advance the exact maintenance gate from `Closed` through `Opening` to
    `Open`;
12. publish or exact-observe the durable ingress acknowledgement bound to that
    open gate generation;
13. reread writer fence, maintenance gate, owner, gateway, registry, and
    supervisor state;
14. CAS the exact coordinator state to `Open::Production`;
15. publish process readiness.

Steps 7 through 14 may not be collapsed. In particular, step 7 alone never
publishes readiness.

## Process-owned mutation finalizer

### Ownership model

`RuntimeMutationFinalizerSupervisorV1` is owned by the root process supervisor.
It is not owned by a startup request, recovery iteration, HTTP request, or
caller task. It is running before `RecoveryPending` can authorize the first
startup mutation and remains the same supervised actor through production
handoff and shutdown.

The first supported job is a closed enum variant:

```rust
pub enum RuntimeMutationFinalizerJobV1 {
    StartupPendingDrain(RuntimePendingDrainFinalizerJobV3),
}
```

Later certification and ordinary drain-claim variants require separately
versioned job types. The supervisor does not accept a caller-provided async
closure or trait object with unconstrained authority.

Registration uses ownership-preserving failure:

```rust
pub fn try_register<J>(
    &self,
    job: J,
) -> Result<
    RuntimeMutationFinalizerWaiterV1,
    RuntimeMutationFinalizerRegistrationRejectedV1<J>,
>;
```

The accepted job privately owns:

- the linearly current closed-recovery session;
- the exact mutation authorization;
- any local S1 registry capability and binding;
- the current owner and readiness evidence;
- the effective operation and owner-safety cutoffs;
- narrowed database, Discord-terminal, owner-terminal, and shutdown ports;
- an armed fail-closed guard.

The result transfers a successor closed-recovery session back to the root
process mailbox only after the job settles. The waiter is an observation
handle, not the sole continuation owner. If its waiter was dropped, the root
still receives the completion and deterministically continues or enters
shutdown. A copied receipt or error cannot reconstruct the successor session.

### Registration and dispatch linearization

The order is exact:

1. reserve one bounded finalizer slot;
2. validate that the process accepts new mutation jobs;
3. move all non-cloneable job authority into the slot;
4. publish the slot as registered under the current finalizer generation;
5. return the result waiter;
6. dispatch the first external mutation.

Shutdown and invalidation arbitrate between steps 2 and 4. If they win, the job
is returned unconsumed and no mutation is dispatched. Once step 4 wins,
shutdown sees the registered job and must join it. There is no state in which
a mutation is dispatched but absent from the finalizer registry.

### Job states

```rust
pub enum RuntimeMutationFinalizerJobStateV1 {
    Registered,
    FirstDispatchInFlight,
    ExactFinalizationEligible,
    FinalizationInFlight,
    Settled,
    Joining,
    Joined,
    FailedClosed,
}
```

These are supervisor-internal states, not authority tokens. Transitions retain
the owned job package.

For pending drain:

- a successful first result follows normal semantic validation;
- a determinate failure settles fail closed;
- `Indeterminate` moves once to `ExactFinalizationEligible`;
- current deadline, Discord, owner, gateway, session, and registry state are
  revalidated;
- only then may the exact same borrowed authorization be invoked once more;
- every second result settles the job;
- only a validated durable acknowledgement or succession receipt can consume
  S1 into S2.

The selection read is not a mutation job and is not replayed by this
supervisor.

### Cancellation

Cancellation semantics are defined at the registration boundary.

| Boundary | Cancellation result |
| --- | --- |
| Before slot reservation | Caller retains the complete job |
| Reserved but not registered | Reservation is released and caller retains the job |
| Registered but not dispatched | Supervisor owns and deterministically settles the job |
| Dispatch in flight | Supervisor waits or exact-finalizes under the existing one-replay rule |
| Result ready but waiter dropped | Supervisor records settlement and releases resources |
| Process signal after registration | Shutdown seals intake and joins the exact registered job |

Dropping a waiter never aborts a job. Dropping or aborting the root supervisor
trips the synchronous one-way invalidator through the armed finalizer guard.
The guard removes readiness, closes public ingress, invalidates ordinary
admission authority, and leaves durable state for closed restart recovery.

An OS-level forced termination cannot join tasks. Safety then comes from the
durable writer fence, owner lease expiry, immutable action journal, exact
replay, and paused-by-default restart. Cleanliness is not claimed for that
case.

### Task supervision

No finalizer task is detached. The root process retains:

- the finalizer intake handle;
- one terminal watch receiver;
- every accepted job identity;
- a bounded join handle or task-set membership;
- the finalizer generation;
- the armed invalidation guard.

The finalizer actor has a reserved control lane separate from its bounded job
lane. Seal, shutdown, and terminal observation cannot be blocked by ordinary
job saturation. Every actor exit is classified as:

```rust
pub enum RuntimeSupervisorExitV1 {
    Commanded,
    DependencyTerminal,
    DeadlineElapsed,
    ProtocolViolation,
    Panicked,
    Aborted,
}
```

Unexpected exit is a terminal process event. The root enters shutdown and does
not restart a finalizer inside the same process generation.

## Discord lifecycle and READY evidence

The existing single Discord shard and gateway control lifetime are preserved.
Production handoff changes ownership and deadline mode; it does not reconnect
or construct a second control pair.

The Discord actor becomes explicitly modeful:

```rust
pub enum RuntimeDiscordActorModeV2 {
    StartupPaused {
        operation_cutoff: Instant,
    },
    ProcessSupervised {
        process_generation: NonZeroU64,
    },
    Draining {
        shutdown_generation: NonZeroU64,
        deadline: Instant,
    },
}
```

The mode transition is an acknowledged, one-shot command reserved before the
startup cutoff. If it is not accepted, the existing startup timeout behavior
closes the actor.

Under `StartupPaused`, an applied `AdmissionResumed` remains a protocol
violation. Under `ProcessSupervised`, it is accepted only when it is correlated
to the process-owned recovery-resume or later barrier command. An unrelated or
stale resume still terminates fail closed.

Discord event semantics remain literal:

- `READY` creates the first connected epoch.
- `RESUMED` creates a successor connected epoch after transport recovery.
- both remain paused under `ExplicitResumeAfterEveryConnect`;
- a control-plane resume never fabricates either Discord event;
- `RuntimeGatewayReadyAttestationV2` combines the actual event's connected
  sequence with the later explicit resume sequence;
- the resume sequence must be strictly greater than the connected sequence;
- disconnect synchronously invalidates the attestation and durable ingress
  acknowledgement.

The actor continues consuming lifecycle events after admission opens. It no
longer exits merely because the authorized resume applied. It still exits on
stream termination, fatal receive failure, control orphaning, command
rejection, or uncorrelated admission opening.

## Health and readiness

`/health/live` means only that the root event loop, health listener, and task
supervisor are responsive. It may remain true during bounded shutdown until
the health listener is deliberately stopped.

`/health/ready` is false for:

- startup composition;
- paused Discord connection;
- closed recovery;
- recovery fixed point before production handoff;
- owner or Discord handoff;
- gateway resume;
- `AdmissionAcknowledging`;
- stale or expiring ingress acknowledgement;
- finalizer intake sealing or unexpected finalizer exit;
- reconnect;
- capability-readiness loss;
- owner uncertainty;
- emergency;
- shutdown.

For this staged slice it is true only when one atomic readiness calculation
proves:

1. `EmptyOpen` under the current coordinator generation;
2. all five capability readiness receipts are current;
3. the exact gateway-owner lease is current beyond its safety margin;
4. every required supervisor is running;
5. the current Discord control lifetime is live;
6. the current ready lease is explicitly resumed for the exact epoch;
7. the writer fence is `Open`;
8. the maintenance ingress gate is open at the exact acknowledged generation;
9. the ingress acknowledgement is current and exact;
10. the registry is still recovery-empty at the exact process instance;
11. the finalizer intake is accepting and has no unresolved terminal failure;
12. no shutdown or emergency latch is set.

The readiness endpoint returns only a stable status and finite component code.
It exposes no owner ID, database role, URL, token, customer identifier, or raw
error.

Readiness removal is synchronous with the first invalidating local event.
Periodic full database readiness remains a backstop and does not replace hot
operation checks.

An empty Open process may be infrastructure-ready while it serves no
deployment. Product Live status still requires its independent route,
certification, serving lease, and ingress predicates. Empty Open must never
fabricate a Live deployment.

## Signal-aware shutdown

### Signal source

The root process owns one `RuntimeShutdownSignalLatchV1`. It accepts:

- SIGTERM;
- SIGINT;
- terminal Discord actor exit;
- terminal gateway-owner supervisor exit;
- terminal finalizer exit;
- terminal readiness or ingress-ack supervisor exit;
- explicit process shutdown from the executable owner.

The first event stores a finite cause, increments the shutdown generation, and
computes one absolute monotonic deadline. In a supervised production state it
is `now + 30 seconds`. During startup it is the minimum of that value and the
already established startup cleanup deadline, so a signal cannot extend the
45-second startup budget. Later events may add diagnostic counters but cannot
replace the primary cause or extend the deadline.

Every long wait receives:

- the shared shutdown observation;
- only the remaining monotonic budget;
- its dependency terminal observations;
- a typed timeout result.

No task performs an unbounded `await` during shutdown.

### Shutdown order

The order follows the canonical production shutdown contract even though the
empty stage makes route and heartbeat steps no-ops.

1. Atomically remove readiness, seal public ingress, stop new recovery and
   mutation admission, and seal the finalizer intake.
2. Enter coordinator `Shutdown` through the same invalidator arbitration used
   by mutation registration and resume claim.
3. Join every registered mutation finalizer or obtain its exact terminal
   classification. No new exact mutation is invented after its allowed
   finalization budget.
4. Hard-pause gateway admission and invalidate every ready lease and installed
   ingress acknowledgement.
5. Close and join serving heartbeat lanes. The empty stage proves the lane set
   is empty rather than skipping the supervisor boundary.
6. Seal the registry, drain active guards, and remove routes. The empty stage
   requires an exact empty observation with zero active guards.
7. Command Discord drain, wait for lifecycle publication, close the shard, and
   join both actor and control tasks.
8. Join finalizer, convergence, recovery, ownership, readiness, ingress-ack,
   and serving supervisors.
9. Release the exact gateway-owner lease if it is still current. An uncertain
   owner is left to expire and is never released by guessed identity.
10. Close all five database pools concurrently using the remaining budget.
11. Drop resolved secrets.
12. Stop the health listener and return one terminal process result.

Steps 3, 7, 8, 9, and 10 are independently reported. A later cleanup failure
does not erase the primary shutdown cause.

If the absolute deadline expires:

- ingress and gateway admission remain closed;
- unresolved dedicated mutation connections are quarantined and closed;
- remaining tasks are aborted through tracked handles;
- no success acknowledgement is fabricated;
- pools are closed or dropped;
- the process emits a stable timeout component code and exits nonzero.

Shutdown never transitions through `AdmissionAcknowledging` or `Open`.

## Overload and backpressure

All process queues are bounded and configured at startup. Their combined
worst-case memory and minimum shutdown time must fit the certified host budget.

### Finalizers

- Closed startup recovery permits at most one mutation job because its
  authority is linear.
- The supervisor has a bounded data lane and a separately reserved
  capacity-one control lane.
- Capacity is reserved before external dispatch.
- Saturation before registration returns `Busy` with the complete unconsumed
  job and sends no mutation.
- Saturation after a dispatch is a protocol violation because registration
  ordering forbids that state.
- Accepted jobs are never evicted, reordered behind a newer job for the same
  authority, or converted to best effort.

Future ordinary drain jobs use keyed single flight per serving slot plus a
small process-wide bound. They do not change the startup capacity.

### Resume and lifecycle

Pause, resume, lifecycle, finalizer-control, shutdown, and terminal
notification capacity is pre-reserved. An empty or saturated ordinary command
queue cannot prevent emergency pause or shutdown.

If resume capacity cannot be reserved, the process remains paused. If an
acknowledgement is lost after runtime claim, the process exact-observes the
same epoch; it does not enqueue a competing resume.

### Readiness and ingress acknowledgement

At most one ingress acknowledgement operation is in flight. A barrier or
snapshot change makes its result stale and triggers exact replay or a new
operation only after the previous transaction is proven ended. There is no
unbounded retry queue. Safety-margin exhaustion enters emergency.

Overload never degrades into:

- unbounded task spawning;
- dropped accepted mutation jobs;
- bypassing durable acknowledgement;
- extending owner or shutdown deadlines;
- opening admission to reduce queue depth.

## Crash and cancellation matrix

| Boundary | Local result | Durable or restart result |
| --- | --- | --- |
| Before finalizer registration | Caller retains authority; no dispatch | Ordinary closed recovery may retry |
| Registered before first dispatch | Supervisor owns job; signal joins without dispatch | No durable mutation |
| First dispatch definitely did not apply | Job settles determinate failure | Exact source remains recoverable |
| First dispatch result unknown | S1 and exact authorization remain owned | One same-request finalization; otherwise restart exact-observes |
| First dispatch committed, response lost | Replay returns exact persisted receipt | No replacement identity or fence |
| Second result unknown | Job fails closed and process shuts down | New process uses durable closed recovery |
| Durable acknowledgement before local unseal | S1 remains closed until checked rollover | Restart observes acknowledged durable state |
| Local S2 rollover before waiter delivery | Supervisor owns settled result | Dropped waiter cannot repeat rollover |
| Caller cancels after registration | Only waiter is dropped | Job continues under process ownership |
| Finalizer actor panics or is aborted | Armed guard removes readiness and closes ingress | Durable state is recovered on paused restart |
| Signal while mutation is in flight | Intake seals; exact registered job is joined | No pool closes before terminal classification |
| Crash before fixed-point handoff | Startup watchdog cleanup or lease expiry | Restart begins paused |
| Crash during owner handoff | One side owns the exact transfer receipt; admission stays paused | No second renewer; lease observation decides |
| Crash after supervisor handoff before resume | Process cleanup joins supervisors | Restart begins paused |
| Resume canceled before runtime claim | Command is revoked; epoch remains paused | A new recovery resume may be authorized |
| Resume applied, acknowledgement lost | Public ingress remains closed | Exact snapshot proves the same resume or recovery pauses again |
| Disconnect after resume before durable ACK | Ready evidence invalidates synchronously | Old ACK cannot qualify successor epoch |
| Durable ACK committed, response lost | `AdmissionAcknowledging` remains unready | Exact lookup adopts only byte-exact ACK |
| ACK accepted before final Open CAS | Gate may be open but no public permit exists | Reread and exact CAS or close gate |
| Crash in `EmptyOpen` | Discord, gate, and process-local registry disappear | Owner expiry and paused restart recovery |
| Owner or capability loss in `EmptyOpen` | Readiness and ingress close before recovery | New exact receipts are required |
| Signal in `EmptyOpen` | Coordinator enters `Shutdown`; no resume | Bounded ordered join |
| Discord close timeout | Actor is aborted after deadline | Stable nonzero shutdown code |
| Owner release uncertain | No guessed release | Lease expires in PostgreSQL |
| Pool close timeout | Remaining handles are dropped after finalizers stop | Stable nonzero shutdown code |

SIGKILL and host power loss cannot run the join path. They are covered only by
durable fencing, lease expiry, exact replay, and paused restart. They do not
count as clean shutdown SLO success.

## Observability

Metrics use finite labels only.

Required gauges:

- process lifecycle state;
- gateway coordinator state and mode;
- gateway paused/open state;
- current connection epoch presence;
- readiness boolean;
- maintenance gate state;
- ingress acknowledgement installed/current boolean;
- finalizer intake open/sealed state;
- registered, in-flight, uncertain, and joined finalizer counts;
- supervisor running count;
- shutdown phase.

Required counters:

- mutation registration accepted, busy, and invalidated;
- caller waiter cancellation;
- first-dispatch outcome;
- exact finalization attempted and outcome;
- finalizer unexpected exit;
- resume attempted, applied, exact-observed, and rejected;
- ingress acknowledgement applied, replayed, stale, and failed;
- readiness removal cause;
- shutdown primary cause and terminal outcome;
- forced task abort after deadline.

Required histograms:

- registration-to-first-dispatch;
- mutation first-dispatch duration;
- indeterminate-to-terminal-classification;
- paused-to-resume-application;
- resume-to-exact-ready-observation;
- ready-observation-to-durable-acknowledgement;
- signal-to-readiness-removal;
- signal-to-hard-pause;
- each shutdown phase and total shutdown.

Logs exclude secrets, URLs, database roles, owner IDs, raw SQL errors,
customer identifiers, Discord payloads, RuleSet JSON, and arbitrary human
text. Correlation uses bounded internal operation kind, generation, phase, and
outcome labels.

## SLO and release gates

The certified-host cohort runs on the deployment Mac mini in release mode at
idle, 50 percent, and 90 percent of configured process capacity.

Safety gates:

- zero customer interaction execution before durable ingress acknowledgement;
- zero interaction execution in the empty Open slice;
- zero admission on a stale connection epoch or admission revision;
- zero accepted mutation jobs absent from the finalizer registry;
- zero dropped or duplicated accepted jobs under caller cancellation;
- zero mutation dispatch after finalizer intake seal;
- zero pool close while a registered finalizer is active;
- zero automatic resume after reconnect;
- zero false-ready and zero false-Live projection.

Latency and boundedness gates:

- the existing 45-second startup deadline, 35-second operation partition, and
  10-second cleanup tail remain unchanged;
- signal-to-readiness-removal p99 is at most 50 milliseconds;
- signal-to-public-ingress-closure p99 is at most 50 milliseconds;
- signal-to-hard-pause p99 is at most 250 milliseconds when no previously
  accepted finalizer must first settle; otherwise the finalizer join phase is
  reported separately and the 30-second absolute deadline remains the bound;
- healthy resume-to-exact-ready-observation p99 is below two seconds;
- durable ingress acknowledgement reaches a terminal result before its owner
  and acknowledgement safety margins;
- mutation finalization performs at most one second invocation;
- overload rejection is bounded and does not wait for queue capacity;
- normal shutdown completes within the canonical 30-second absolute deadline;
- no individual shutdown phase consumes more than its assigned remaining
  budget.

Performance results cannot waive a safety gate. A hard deadline miss, orphaned
job, false readiness, or pre-ack ingress blocks production admission.

## Dependency boundaries

### `automation-runtime-controller`

Owns durable, pure DTOs for ingress acknowledgement and any canonical
projection required by its PostgreSQL capability. It imports no runtime,
Twilight, SQL, signal, or task type.

### `automation-runtime-worker`

Extends the current closed-only lifecycle with the accepted coordinator suffix:

- consume exact fixed point;
- mint one recovery-resume permit;
- enter `AdmissionAcknowledging`;
- validate durable ingress acknowledgement;
- enter `Open::Production`;
- enter terminal `Shutdown`.

It owns pure transition validation and narrowed ports. It remains free of
`sqlx`, Twilight, operating-system signal, and task-supervision dependencies.

### `automation-runtime`

Keeps ownership of reconnect-safe gateway control, opaque pause tokens,
explicit resume, atomic admission snapshots, and ready leases. It does not
learn database acknowledgement or finalizer semantics.

### `automation-runtime-registry`

Supplies the exact registry-instance cursor, empty observation, terminal close,
and active-guard drain. The staged slice adds no route mutation or second
registry.

### `automation-runtime-execution-postgres`

Owns the additive durable ingress-acknowledgement capability, exact replay and
lookup, writer-fence binding, execution-role grant, migration, manifest, and
readiness update. It accepts only the worker-authorized request and contains no
gateway or Twilight type.

### `tools/starring-runtime`

Owns:

- the process root and signal latch;
- finalizer actor and task registry;
- concrete Discord actor handoff;
- gateway, registry, owner, readiness, and database binding;
- maintenance ingress gate;
- health listener;
- shutdown orchestration.

It contains no raw SQL and exposes no raw gateway resume, registry mutation,
pool, token, or authority-bearing finalizer constructor.

No new third-party dependency is required. Existing Tokio tasks, channels,
watchers, timers, and tracked join handles are sufficient. Dependency guards
must reject `sqlx` or Twilight in the pure worker and reject raw SQL in
`starring-runtime`.

## Implementation sequence

Each numbered item is one functional commit and must leave its focused gates
green.

1. Add pure tests and types for the fixed-point-to-resume,
   `AdmissionAcknowledging`, `Open::Production`, and `Shutdown` transitions.
   Keep every new public authority non-cloneable and non-serializable.
2. Add the minimal root-owned bounded finalizer actor before closed recovery
   starts, plus the ownership-preserving registration API with fake-port
   cancellation, saturation, panic, and shutdown races.
3. Move pending-drain mutation execution into the finalizer. Prove exact
   request identity across one `Indeterminate` finalization and prove waiter
   cancellation cannot drop cleanup authority.
4. Add the root task supervisor, signal latch, absolute shutdown budget, and
   the complete ordered shutdown path while the process still remains paused.
5. Refactor the Discord actor into startup-paused, process-supervised, and
   draining modes. Preserve one shard and one control lifetime. Keep
   unauthorized resume terminal.
6. Implement the startup-owner and Discord production handoff. Prove no
   renewal gap, no concurrent renewer, no startup-cutoff leak, and bounded
   rollback to shutdown.
7. Add `RuntimeMaintenanceIngressGateV2` and its counted close/drain behavior.
   It starts closed and has no public interaction consumer in this slice.
8. Add the durable ingress-acknowledgement DTO, PostgreSQL capability,
   restricted grant, manifest/readiness update, exact replay, and real
   PostgreSQL security tests.
9. Compose recovery resume, exact ready observation, ingress
   acknowledgement, final `Open::Production` CAS, and empty-open readiness.
10. Add reconnect, owner-loss, capability-loss, acknowledgement-loss, signal,
    overload, and deadline fault injection at every transition boundary.
11. Run workspace tests, Clippy with warnings denied, formatting, dependency
    guards, migration replay, privilege tests, secret scan, and the certified
    host SLO cohort.

No step may expose customer ingress or add route execution. The later route,
certification, heartbeat, and interaction composition begins only after this
empty Open epoch passes all safety and shutdown gates.

## Acceptance matrix

The implementation is complete only when tests prove:

- every launch and reconnect begins paused;
- an actual `READY` and an actual `RESUMED` each create a distinct paused
  epoch;
- explicit resume never fabricates a Discord event;
- a stale pause token or recovery-resume permit cannot open admission;
- resume acknowledgement loss exact-observes one applied resume;
- durable acknowledgement loss exact-replays one immutable operation;
- readiness remains false through fixed point, handoff, resume, and
  `AdmissionAcknowledging`;
- only exact current evidence enters empty `Open::Production`;
- the empty Open process has no route, active guard, serving lane, panel job,
  heartbeat, or interaction execution;
- reconnect, owner loss, readiness loss, finalizer exit, and ACK expiry remove
  readiness and public ingress synchronously;
- caller cancellation before registration dispatches nothing;
- caller cancellation after registration cannot cancel the accepted job;
- finalizer saturation rejects before dispatch;
- first `Indeterminate` uses the exact same request once;
- second uncertainty stops without a third call;
- shutdown racing registration has exactly one winner;
- shutdown racing a dispatched mutation joins or exactly classifies it before
  pool close;
- shutdown never resumes gateway admission;
- every tracked task is joined or explicitly aborted at the absolute deadline;
- every failure emits a finite stable code and preserves the primary cause;
- all five pools close concurrently only after mutation and serving authority
  is settled;
- full restart after every crash-matrix boundary reaches only paused recovery,
  never implicit Open.

Until this matrix and the certified-host SLO cohort pass, empty Open is staging
evidence only and must not be connected to the customer interaction adapter.
