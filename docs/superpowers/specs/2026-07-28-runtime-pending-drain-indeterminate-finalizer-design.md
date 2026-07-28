# Runtime pending-drain indeterminate finalizer design

Date: 2026-07-28

## Outcome

The runtime keeps the existing non-cloneable pending-drain authority and local
S1 registry seal alive when a PostgreSQL mutation returns `Indeterminate`.
While the Discord gateway remains paused, the gateway owner remains current,
the startup operation budget remains open, and the local S1 binding still
revalidates, the runtime invokes the exact same durable mutation once more.

The existing PostgreSQL capability decides the result:

- if the first transaction did not commit, the second invocation applies it;
- if the first transaction committed, the second invocation returns the exact
  persisted replay;
- if the persisted action, owner, candidate, seal, source, or terminal
  projection differs, the capability rejects the invocation.

This applies independently to:

1. no-candidate recording;
2. TX1 pending-drain claim;
3. TX2 route-absent acknowledgement.

There is one initial invocation and at most one finalization invocation per
mutation stage. No general retry loop is introduced.

## Corrected recovery boundary

This slice is same-process commit-uncertainty finalization. It is not process
restart recovery.

Every OS launch creates a new random process instance ID. The gateway owner
lease, durable claim, and registry seal bind the process instance ID. The local
registry is also rebuilt as empty on a new launch. A `PendingClaimed` record
left by a terminated process therefore belongs to a previous owner and cannot
be relabeled as a current-owner claim or used to mint a replacement S1
capability.

Actual restart recovery requires previous-owner succession. That later slice
must prove old-process termination or lease expiry, acquire a successor fence,
and define a new durable claim or refence transition before constructing local
authority.

## Invariants

- Discord admission stays paused throughout both invocations.
- The same borrowed worker authorization is used for both invocations.
- TX1 keeps the same candidate and the same non-cloneable local S1 seal.
- TX2 keeps the exact TX1 terminal digest and claimed-state digest.
- No new recovery ID, action identity, selection identity, seal, or minimum
  database clock is generated between invocations.
- Only `Indeterminate` permits the finalization invocation.
- `Timeout`, `Concurrency`, `Unavailable`, ownership loss, authority change,
  corruption, protocol failure, and transition failure do not use this path.
- Current Discord, owner, gateway, registry, and operation-deadline state is
  revalidated before finalization.
- The same transition-priority and session revalidation runs after the
  finalization result and before a receipt can be completed.
- A transition discovered after the first invocation wins over database
  finalization.
- A second failure is terminal for this startup process and preserves the
  existing fail-closed behavior.
- TX1 and TX2 each have their own independent one-invocation finalization
  budget.
- S1 can move to S2 only after Rust validates the durable TX2 receipt.
- The finalization is inline in the existing startup-recovery future. It is not
  a detached task or a cancellation-safe supervisor. Outer future
  cancellation and OS process loss remain fail-closed and are out of scope.

## Runtime flow

For each pending-drain mutation stage:

1. invoke the existing environment port;
2. on success, continue through the existing semantic receipt validation;
3. on a transition failure, stop and join shutdown;
4. on a database failure other than `Indeterminate`, stop and join shutdown;
5. on `Indeterminate`, revalidate the current transition and closed-recovery
   session;
6. invoke the same port once with the same borrowed authorization;
7. treat the second result exactly like an ordinary terminal result.

Selection remains read-only and is never replayed by this finalizer.

## PostgreSQL contract

Migration `202607270012_add_owner_fenced_startup_pending_drain_execution_v2.sql`
already supplies the required idempotency boundary.

The no-candidate recorder and staged pending-drain executor lock the existing
action identity, compare the complete persisted terminal projection, validate
the current owner and minimum database clock, and return `replayed` only for an
exact match. A fresh invocation under the same immutable request either
applies the missing transition or returns that exact replay.

No migration, capability grant, manifest digest, or readiness digest changes
are needed for this slice.

## Verification

Runtime tests must prove:

- TX1 `Indeterminate` followed by success invokes claim exactly twice, invokes
  TX2 once, and rolls S0 to S1 to S2;
- TX2 `Indeterminate` followed by success invokes acknowledgement exactly
  twice and rolls S1 to S2;
- no-candidate `Indeterminate` followed by success records completion without
  sealing the registry;
- two consecutive `Indeterminate` results stop after the second invocation and
  preserve S1 for TX1 and TX2;
- a second `Timeout`, `OwnershipLost`, or `PersistenceCorrupt` result stops
  after exactly two invocations;
- a non-`Indeterminate` database error receives no finalization invocation;
- a Discord, owner, deadline, or protocol transition receives no finalization
  invocation;
- TX1 finalization success followed by TX2 `Indeterminate` uses one independent
  TX2 finalization invocation and no third invocation;
- both invocations borrow the same authorization and therefore expose the same
  complete request, candidate, and seal fingerprint;
- a transition observed immediately after a successful finalization prevents
  receipt completion and the next durable stage;
- the existing success and failure matrix remains green.

The existing PostgreSQL security tests remain the database proof for exact
claim, acknowledgement, no-candidate replay, forged replay rejection, and
concurrent claim serialization.

## Follow-up

The next durable restart slice is previous-owner `PendingClaimed` succession.
It must not reuse this same-owner finalizer as evidence that the previous
process is dead or that its local registry seal can be reconstructed.
