# Runtime expired pending-drain succession design

Date: 2026-07-28

## Outcome

A newly started runtime can recover an exact route-absent `PendingClaimed`
drain intent left by a previous process.

The successor runtime keeps Discord admission paused, owns a current gateway
lease, creates a fresh non-cloneable local S1 registry seal, and invokes one
atomic database mutation. That mutation:

1. proves the persisted predecessor claim is exact and recoverable;
2. proves the predecessor claim is expired according to database time;
3. proves the current gateway lease is a strictly newer owner epoch;
4. advances the deployment fencing token exactly once;
5. constructs a new current-owner claim bound to the fresh local S1 seal;
6. constructs the route-absence acknowledgement around that new claim;
7. records the predecessor and successor in the immutable startup action
   journal;
8. commits the fence, acknowledged drain state, and journal as one unit.

Only the checked durable receipt can move the local registry from S1 to S2.

This is a direct succession-and-acknowledgement transition. It does not persist
an intermediate current-owner `PendingClaimed` state.

## Why the transition is atomic

Persisting a replacement claim and acknowledging it in a second transaction
would create another restart window:

- the predecessor claim could be succeeded;
- the process could terminate before acknowledgement;
- the next process would need to succeed the successor again.

The direct transition removes that window. Its crash outcomes are closed:

| Boundary | Durable state |
| --- | --- |
| Before dispatch | Previous-owner `PendingClaimed` |
| Transaction rollback | Previous-owner `PendingClaimed` |
| Commit result unknown | Exact replay decides old state or acknowledged state |
| Commit succeeds, local unseal fails | `RouteAbsentAcknowledged`; local S1 remains closed |
| Process exits after commit | Next launch observes no active pending intent |

The result still contains a current-owner claim. This keeps the persisted
claim, deployment fence, local seal, acknowledgement provenance, and current
recovery identity aligned.

## Scope

This slice supports only:

- `intent_state = pending`;
- canonical state kind `pending_claimed`;
- claim progress kind `claimed`;
- `expected_route = null`;
- a predecessor gateway owner different from the current process;
- a predecessor claim expired by the database clock;
- a current lease epoch strictly greater than the predecessor lease epoch;
- route absence in the fresh local registry;
- certification outcomes that do not prove a committed live operation.

The existing two-transaction path remains unchanged for
`PendingUnclaimed`:

1. TX1 creates `PendingClaimed`;
2. TX2 creates `RouteAbsentAcknowledged`.

The following remain separate later slices:

- `PendingClaimed` with an expected route;
- `PendingRefenced`;
- draining and removing a route that is still locally present;
- Product-side consumption or cancellation of acknowledged drain intents;
- a clean-release optimization that succeeds a claim before its recorded
  expiry;
- detached shutdown finalizers across outer future cancellation.

## Safety invariants

### Predecessor proof

- The source intent revision, canonical bytes, and SHA-256 digest are exact.
- The source is a canonical route-absent `PendingClaimed`, not unclaimed,
  refenced, acknowledged, consumed, or cancelled.
- The source claim owner, process, controller, fence, claim epoch, claim
  revision, expiry, and seal are decoded and revalidated.
- The source claim process equals its owner lease process and its seal process.
- The source claim seal binds the selected intent ID and serving slot.
- The source claim has no expected route.
- Database time is greater than or equal to the source claim expiry.

### Successor owner proof

- The current gateway shard equals the predecessor shard.
- The current process differs from the predecessor process.
- The current owner lease epoch is strictly greater than the predecessor
  epoch.
- The current owner row exactly matches the runtime request.
- The current owner lease is unexpired at the mutation database time.
- The current paused Discord, readiness, recovery, and empty-registry evidence
  is exact.
- Host time is never evidence for predecessor expiry or owner currentness.

### Successor transition

- The immutable drain root and expected deployment target do not change.
- The intent revision advances by exactly one.
- The new claim revision advances by exactly one.
- The deployment fencing token advances by exactly one.
- The deployment controller history advances to the current recovery
  controller.
- No active deployment controller lease or serving lease exists.
- The new claim owner, process, recovery generation, controller, expiry, and
  seal all belong to the current recovery.
- The new claim seal uses the current process, selected intent ID, selected
  slot, and no expected route.
- The claim seal observation equals the local S1 post-slot observation.
- The acknowledgement observation equals the local S1 post-global
  observation.
- Slot and global observations are independent sequence domains; no invented
  ordering is imposed between them.
- The acknowledgement provenance is the exact current closed-recovery
  witness.
- The acknowledgement contains the newly constructed claim, never the
  predecessor claim.
- A committed or live certification state rejects the transition.

### Local authority

- The local registry starts from the exact empty S0 observation used by the
  startup request.
- S0 to S1 happens synchronously before the database mutation.
- The S1 binding remains non-cloneable and is not held behind a detached task.
- Database waits do not hold a registry lock.
- S1 can move to S2 only through a checked durable succession receipt.
- Every transition, timeout, ownership-loss, corruption, and protocol failure
  before the durable receipt preserves S1 or fails the registry closed.

## Domain model

### Controller

Add a distinct pure transition instead of weakening the existing
acknowledgement transition.

The controller additions are:

- a checked persisted predecessor type for route-absent `PendingClaimed`;
- a checked succession-and-acknowledgement input;
- a combined canonical transition;
- a distinct receipt validator for predecessor-to-successor claim replacement.

The transition derives:

- current owner and process from the closed-recovery witness;
- successor fence from predecessor fence plus one;
- successor claim revision from predecessor claim revision plus one;
- successor intent revision from predecessor intent revision plus one;
- claim expiry from the current owner witness;
- claim epoch from the current recovery generation.

Caller-supplied values are limited to evidence that originates outside the
controller:

- database observation time;
- current recovery controller ID;
- local S1 seal generation;
- local S1 post-slot observation;
- local S1 post-global observation;
- certification resolution;
- acknowledgement database time.

The existing `RuntimeDrainIntentReceiptV2::acknowledged` contract continues to
require source-claim identity. A new succession-specific receipt constructor
validates the exact predecessor-to-successor relationship.

No new persisted state tag or wire format is required. The result uses the
existing canonical `route_absent_acknowledged` state and embeds the newly
constructed claim.

### Worker

The current V2 candidate cannot represent this path:

- it reserves two intent revisions;
- it eagerly creates a second acknowledgement action identity;
- its compound proof requires two database actions.

Add a V3 selection boundary that classifies:

- no candidate;
- unclaimed candidate using the existing two-action path;
- fresh previous-owner route-absent claim with a bounded retry;
- expired previous-owner route-absent claim using the direct path.

The direct candidate reserves only one intent revision. It owns checked
predecessor evidence and can create only a non-cloneable succession
authorization after binding the current local S1 seal.

Add:

- a succession acknowledgement execution port;
- a succession receipt;
- a durable succession acknowledgement type;
- a one-action succession proof;
- a deferred-selection proof for a fresh predecessor claim.

The existing compound proof remains the exact two-action proof for the
unclaimed path.

## PostgreSQL contract

Add an additive migration after
`202607270012_add_owner_fenced_startup_pending_drain_execution_v2.sql`.

The migration adds:

1. a V3 owner-fenced pending-drain selector;
2. one owner-fenced atomic succession capability;
3. private canonical predecessor and successor validators;
4. execution manifest and readiness updates;
5. exact capability grants without direct relation privileges.

The V3 selector reuses the existing deterministic candidate ordering and
returns a closed source classification. It exposes enough checked predecessor
evidence for Rust to validate:

- predecessor owner lease identity;
- observed owner revision;
- predecessor process;
- controller identity and fence;
- claim epoch and revision;
- claim expiry;
- route-absence seal identity and sequences.

The selector returns a bounded retry for a previous claim that is otherwise
valid but not expired. It does not seal the local registry for that outcome.

The succession mutation runs in one serializable read-write transaction with
the established order shared by the existing Product first-apply and
pending-drain mutation paths:

1. global writer-fence advisory lock;
2. gateway-owner advisory lock and current owner row;
3. serving-slot advisory lock, slot writer fence, and serving lease;
4. deployment;
5. Product root;
6. drain-intent advisory lock and drain intent;
7. certification state;
8. exact action journal identity.

Before mutation it revalidates:

- current owner and database clock;
- exact selected source revision and digest;
- canonical predecessor state;
- deterministic predecessor recovery and claim action identity decoded from
  the predecessor controller identity;
- immutable predecessor action journal owner, process, epoch, revision, claim
  stage, successor digest, successor bytes, and terminal digest;
- predecessor expiry and owner epoch ordering;
- deployment fence and controller history;
- absent serving route and controller lease;
- Product and certification roots;
- current paused recovery and registry evidence.

The transaction then advances the deployment fence, constructs the canonical
successor, updates the drain intent under the existing one-shot gate, records
the action journal, reconstructs the persisted transition, and returns the
terminal projection.

The action journal terminal projection binds:

- predecessor canonical digest and exact predecessor claim terminal digest;
- predecessor claim identity;
- complete successor canonical bytes and digest;
- current owner and closed-recovery evidence;
- current local S1 seal;
- source and successor deployment fences;
- certification resolution;
- mutation database time.

The projection does not duplicate the complete predecessor canonical bytes.
Drain canonical state permits a larger payload than the bounded action
journal. The mutation instead revalidates the locked predecessor bytes against
their digest and verifies that the immutable predecessor action journal
committed the same successor digest. This keeps the audit chain exact without
making a valid large predecessor impossible to recover.

An exact replay returns the original projection. Any changed recovery,
authority revision, owner, source, target, seal, fence, certification, or
minimum database clock fails closed.

## Runtime flow

1. Observe startup recovery with Discord admission paused.
2. Authorize V3 pending-drain selection.
3. Select the deterministic oldest candidate under the current owner.
4. If none exists, use the existing durable no-candidate path.
5. If the source is unclaimed, use the existing TX1 and TX2 path.
6. If the source claim is fresh, return a bounded retry without registry
   mutation.
7. If the source claim is expired, synchronously seal the exact local slot,
   moving S0 to S1.
8. Revalidate deadline, Discord, owner, gateway, and registry state.
9. Invoke the atomic succession capability.
10. On `Indeterminate`, revalidate and invoke the same borrowed authorization
    exactly once.
11. Validate the complete terminal projection and durable receipt.
12. Revalidate transition priority.
13. Use the durable receipt to move S1 to S2.
14. Complete the startup action, refresh all readiness receipts, and reobserve
    before accepting a fixed point.

The arbitration order remains:

1. operation or owner-safety cutoff;
2. terminal Discord state;
3. terminal gateway-owner state;
4. database completion.

## Failure and replay behavior

| Failure | Result |
| --- | --- |
| Fresh predecessor claim | Bounded retry, no local seal |
| Same owner or regressed epoch | Protocol/corruption failure |
| Routed or refenced predecessor | Unsupported closed boundary |
| Wrong source digest or revision | Ownership/source failure |
| Wrong local seal | Protocol failure |
| Active serving or controller lease | Retry-not-ready |
| Committed certification | Higher-priority closed boundary |
| Timeout before dispatch | Timeout, S1 retained |
| Unknown result after dispatch | One exact inline finalization |
| Second unknown result | Terminal fail-closed, S1 retained |
| Discord or owner termination | Transition wins, S1 retained |
| Semantic projection mismatch | Persistence corruption, S1 retained |
| Durable success and local unseal failure | Database stays acknowledged, local registry closes |

## Verification

### Pure controller

- valid predecessor becomes one exact acknowledged successor;
- no intermediate current-owner pending state is produced;
- successor intent revision, claim revision, and fence are exact successors;
- successor claim and acknowledgement bind the current owner, process, seal,
  and recovery provenance;
- immutable roots remain exact;
- same process, shard drift, non-newer epoch, fresh predecessor, routed source,
  refenced source, expired current owner, fence overflow, revision overflow,
  seal drift, and certification drift reject;
- canonical bytes round-trip exactly.

### Worker

- V3 selection does not reserve a second action for direct succession;
- unclaimed selection retains the existing two-action proof;
- fresh predecessor produces one bounded retry and no S1;
- expired predecessor creates one direct authorization;
- durable receipt is the only constructor for local S2 rollover;
- direct proof contains one action identity and exact predecessor evidence.

### PostgreSQL

- exact direct succession advances the fence and drain state once;
- exact replay returns the original terminal projection;
- two concurrent successors produce one apply and one exact replay or one
  closed loser;
- rollback leaves the predecessor claim and deployment fence unchanged;
- wrong owner, source, fence, seal, target, recovery, certification, and
  journal identity reject without partial writes;
- executor role has only the new function capability;
- public and default grants remain closed;
- migration rerun and collision paths are atomic.

### Runtime

- process A commits TX1 and exits;
- process B acquires a newer owner after predecessor expiry;
- process B moves S0 to S1, commits direct succession acknowledgement, and
  moves S1 to S2;
- only one database mutation is used by the direct path;
- `Indeterminate` followed by replay success invokes the same authorization
  exactly twice;
- deadline, Discord termination, owner loss, and protocol failure never
  unseal S1;
- crash after database commit is observed as acknowledged by process C;
- all existing unclaimed success and failure cases remain unchanged.

## Implementation order

1. Pure controller transition and receipt.
2. Worker V3 selection, direct authorization, receipt, and proof.
3. Additive PostgreSQL selector and atomic succession migration.
4. PostgreSQL adapter and semantic projection validation.
5. Local registry durable succession rollover.
6. Production process composition and fault injection.
7. PostgreSQL 16 security tests and full repository gates.

Each item is a separate functional commit. The branch remains unmerged until
the complete restart scenario and repository gates are green.
