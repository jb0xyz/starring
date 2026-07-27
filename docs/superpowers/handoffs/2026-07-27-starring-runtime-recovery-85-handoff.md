# Starring runtime recovery 85% checkpoint handoff

Date: 2026-07-27

Branch: `feat/starring-runtime-recovery-85`

## Outcome

This checkpoint extends the production runtime startup fixed point through the
current-process, route-absent `PendingUnclaimed` drain-intent case.

The implemented success path is:

1. Observe startup recovery while Discord admission remains paused.
2. Select the deterministic oldest pending drain intent under the current
   gateway owner.
3. Bind the exact deployment target and local registry slot.
4. Atomically move the local empty tombstone from S0 to a sealed S1.
5. Persist TX1, changing `PendingUnclaimed` to `PendingClaimed`.
6. Persist TX2, changing `PendingClaimed` to
   `RouteAbsentAcknowledged`.
7. Accept the durable acknowledgement only after Rust reconstructs and
   validates the complete persisted transition.
8. Use that non-forgeable durable value to atomically move S1 to the empty S2.
9. Complete the gateway recovery action.
10. Refresh all database readiness receipts and reobserve startup recovery
    before accepting a fixed point.

The no-candidate branch records an exact durable no-candidate result without
sealing or otherwise changing the local registry.

This is an 85% recovery checkpoint, not a declaration that the complete
production runtime is ready to admit customer traffic.

## Safety boundary

The following invariants are now enforced by types, database transactions, and
independent runtime observations:

- Startup Discord admission remains paused throughout recovery.
- Selection authority `M` can authorize only claim action `N = M + 1`.
- The acknowledgement action is `N + 1` and selects the exact claim action
  `N`.
- The selected intent ID, source revision, source digest, slot, full expected
  target, gateway owner, recovery identity, and registry seal are bound at
  every layer.
- The seal key is exactly the selected drain-intent ID bytes at the worker,
  Rust semantic, and PostgreSQL capability boundaries.
- The registry S1 capability is linear and non-cloneable.
- There is no production API that unseals a pending drain without a borrowed
  `RuntimeDurablyAcknowledgedPendingDrainV2`.
- The stored claim terminal digest and claimed successor bytes are the only
  accepted predecessor for TX2.
- The acknowledgement binds its registry observation to the post-seal global
  sequence rather than the pre-seal observation.
- Rust decodes the terminal projection, reconstructs the domain transition,
  verifies canonical bytes and digests, and validates the resulting receipt
  before committing the SQL transaction.
- A client timeout before mutation dispatch is `Timeout`.
- A transport failure, client cutoff, or commit uncertainty after mutation
  dispatch is `Indeterminate`.
- A connection that may contain an unknown transaction result is detached
  instead of returned to the pool.
- `Timeout`, `Indeterminate`, cancellation, Discord termination, owner
  termination, semantic corruption, and protocol failure before durable ACK
  cannot call the S2 unseal transition.
- Dropping an S1 binding does not unseal the registry.
- Any post-seal failure leaves admission paused and the registry sealed or
  failed closed.
- The route-absent acknowledgement keeps the slot writer fence in place. It
  does not fabricate Product consumption or a serving route.

## PostgreSQL contract

Migration
`202607270012_add_owner_fenced_startup_pending_drain_execution_v2.sql`
adds:

- canonical drain-intent state bytes and SHA-256 digest;
- a canonical initialization trigger for newly inserted drain intents;
- exact state, projection, replay, and Product-root validators;
- a deterministic pending-drain selector;
- an owner-fenced no-candidate recorder;
- one owner-fenced staged executor for claim and acknowledgement;
- three public executor capabilities and private supporting functions;
- exact execution, exact-target, and serving manifest/readiness cascades.

The selector runs in a read-only serializable transaction. Mutation calls run
in serializable read-write transactions and invoke only scoped
`SECURITY DEFINER` functions. The executor role has execute permission on the
three new public capabilities and no direct table privileges.

The claim stage advances only deployment fence history. It does not acquire a
new active controller lease. The pre-existing deployment guards accept this
exception only when all of the following agree:

- the exact recovery-only action marker;
- the exact deployment ID;
- the exact source fence;
- the exact one-step successor fence;
- the exact successor recovery controller ID;
- no active old or new controller;
- a snapshot changed only at `last_fencing_token`;
- a row changed only at the snapshot, last fence, and last controller fields.

The five transaction-local authorization values are cleared and checked before
the executor continues. Missing, forged, stale, or partially matching values
remain rejected.

An acknowledgement request may carry a later database-clock floor than TX1.
The first acknowledgement must be no earlier than the recorded claim. An exact
replay may use a later request floor, but its action digest, projection,
recorded time, and returned minimum remain the values persisted by the original
acknowledgement.

## Runtime composition

The pure worker owns four narrow ports:

- pending-drain selection;
- durable no-candidate recording;
- claim execution;
- acknowledgement execution.

`automation-runtime-execution-postgres` is the only implementation used by the
production process. `tools/starring-runtime` owns the local registry binding
and the orchestration order.

All four database waits use one biased arbitration order:

1. effective operation or owner-safety cutoff;
2. terminal Discord observation;
3. terminal gateway-owner observation;
4. database completion.

The runtime revalidates the current Discord, owner, gateway, and registry
state between every durable stage. It does not hold a registry or gateway lock
across an asynchronous database wait.

## Verified evidence

The focused checkpoint gates passed:

| Gate | Result |
| --- | ---: |
| Worker unit tests | 50 passed |
| Worker integration/dependency guards | 14 passed |
| Execution PostgreSQL library tests | 131 passed |
| Execution PostgreSQL dependency guards | 18 passed |
| Pending migration static guards | 8 passed |
| Pending PostgreSQL security tests | 13 passed: 9 SQL and 4 adapter |
| Full execution PostgreSQL security suite | 98 passed |
| Starring runtime library and integration tests | 347 passed |
| Starring runtime Clippy with warnings denied | passed |
| Starring runtime formatting and comment guards | passed |
| Full workspace tests | passed |
| Full workspace Clippy with warnings denied | passed |
| Promptfoo JavaScript checks | 106 passed |

The PostgreSQL tests cover:

- deterministic no-candidate recording and exact replay;
- a candidate appearing after a no-candidate selection;
- exact claim, acknowledgement, and both replays;
- a later acknowledgement request clock with the original persisted replay
  clock;
- wrong seal, source digest, prior terminal digest, owner revision, and owner
  expiry;
- fresh claim rejection when the seal key differs from the selected intent ID;
- claim and acknowledgement replay rejection when a caller substitutes a
  different valid candidate ID or source digest;
- rollback before commit;
- two concurrent claims with one application and one exact replay;
- restricted-role ACL and readiness;
- missing or forged recovery-only deployment-history authorization;
- extra deployment-column and snapshot drift;
- authorization-value cleanup before commit.

A fresh PostgreSQL 16 database replayed all 82 migrations. Execution,
exact-target, and serving manifests returned true, and the restricted executor
readiness check passed.

The pinned definition digests at this checkpoint are:

| Contract | Definition digest |
| --- | --- |
| Execution manifest | `9de93ea5d565254c47533c7af43959aa873014bee385a2af775fafdcbf8118b9` |
| Execution readiness | `1c20dcc6c6e01b440d9a5813bad12b109d89a67c5d6815f9fd15551fa3c0f4e5` |
| Exact-target manifest | `bea5a930a40537f9f06f19a350d1fdba3bf21b222844eb0f442fb506d91a1ebb` |
| Exact-target readiness | `5eba72a786aebaa8afdc226d661b45132afc5aa053fab7be6a3b9737fdab0e8c` |
| Serving manifest | `c679ef7c0722416b514324936a95884d17242e6b67cdb130987e4d4f03a43758` |
| Serving readiness | `80e9f1da2a7b48610e95e2540db4c77a3daed2d53b3a2ec18de37c0767ac5380` |

## Explicit post-85 work

The following cases intentionally remain fail closed and belong to the next
recovery slices:

1. Restart after TX1 committed but before TX2 completed, where the durable
   state is already `PendingClaimed`.
2. Adoption or fenced succession of a claim created by a previous process or
   owner lease.
3. Restart recovery of `PendingRefenced`.
4. A pending drain whose old local route is still present and must be fenced,
   drained, removed, and durably acknowledged.
5. Exact observation and finalization of claim or acknowledgement commit
   uncertainty across process loss.
6. Shutdown arbitration and joining of in-flight drain-claim finalizers.
7. The later Product-side consumption or cancellation that releases a
   `RouteAbsentAcknowledged` slot fence.
8. Reconstructing a no-candidate replay after process restart when the caller
   has only a later database-clock floor rather than the original exact
   selection token.

Encountering one of these cases does not open admission or guess a successor.
The current process stops at the closed recovery boundary.

## Recommended next slice

Implement restart adoption for the exact current-owner `PendingClaimed` state
before adding route-present removal. That slice should:

- select and decode the persisted claim rather than issuing a replacement
  claim;
- prove the prior terminal journal and canonical claimed state;
- bind a newly reconstructed local S1 seal without weakening registry
  authority;
- distinguish current-owner replay from expired-owner succession;
- exact-observe an unknown TX2 result;
- keep shutdown and owner-loss paths joined behind the same finalizer.

Only after claimed-state restart is durable should the runtime add refencing,
route drain/removal, and previous-owner succession.
