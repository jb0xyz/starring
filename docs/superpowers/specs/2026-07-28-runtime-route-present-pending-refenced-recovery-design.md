# Runtime route-present pending-drain and refenced recovery design

Date: 2026-07-28

Status: implementation contract

Extends:

- `2026-07-22-production-runtime-worker-composition-design.md`
- `2026-07-27-starring-runtime-recovery-85-handoff.md`
- `2026-07-28-runtime-expired-pending-drain-succession-design.md`

## Outcome

The runtime can finish a pending drain whose exact route remains in the current
process. A new process can also recover a routed `PendingClaimed` or
`PendingRefenced` intent left by a previous process.

Admission remains paused for this complete slice. The runtime does not create
an ordinary barrier, resume admission, install or activate a route, start a
heartbeat, certify a deployment, or mutate Product state.

The same-process route path is:

1. seal the exact slot and wait its admitted guards;
2. claim the drain intent and advance the deployment fence;
3. close and join the serialized serving lane;
4. exact-observe heartbeat, serving, and certification state;
5. refence the sealed route to the claim fence;
6. persist `Claimed` to `Refenced`;
7. drain and remove only the durably refenced target;
8. persist `RouteAbsentAcknowledged`;
9. consume the local seal only through the durable acknowledgement.

The previous-process path proves a current paused gateway, empty registry,
strictly newer owner, expired predecessor claim, exact action journals, and
recoverable serving and certification state. It then atomically advances the
fence and records a current-owner successor acknowledgement with explicit
previous-process teardown evidence.

## Scope and prerequisites

This contract supports:

- V2 `PendingUnclaimed` with an exact local route;
- V2 routed `PendingClaimed`;
- V2 `PendingRefenced`;
- same-process continuation after claim, local refence, durable refence, or
  local removal;
- previous-process restart from routed `PendingClaimed` or
  `PendingRefenced`;
- one exact replay for each dispatched result that becomes unknown;
- Production and Cutover closed recovery;
- owner loss, Discord termination, cutoff, emergency, and shutdown at every
  dispatch boundary.

Previous-process routed `PendingClaimed` is required because it is the durable
state after a crash between claim commit and refence persistence.

Existing route-absent V2 and V3 paths remain unchanged. An already
`RouteAbsentAcknowledged` intent is not runtime-resolvable and remains frozen
until the separate Product consume or cancel transaction.

The current executable stops at a startup fixed point and cannot yet create a
live local route. The same-process branch is not production-reachable until the
runtime has:

- exact target hydration and route install/activation;
- an admission-open production state;
- one serialized serving lane per slot and exact heartbeat ownership;
- process-owned finalizer registration and join;
- emergency transition from production into a paused connected epoch;
- health and shutdown supervision.

Pure, registry, worker, PostgreSQL, adapter, and fixture-backed composition can
be implemented first. No feature flag may describe this path as customer-ready
until the serving foundation exists. Product consume or cancel is also required
before customer traffic because only it releases the durable slot fence.

This slice does not batch intents, drain unrelated slots concurrently, optimize
a foreign claim before database expiry, mutate Discord, or change the engine,
validator, RuleSet, panel, or model boundaries.

## Non-negotiable invariants

### Admission and authority

- The gateway starts and remains in the exact paused connected epoch.
- This flow never calls resume and never fabricates `Ordinary` provenance.
- Reconnect invalidates every authorization derived from the older epoch.
- The slot, target, process, route identity, incarnation, lifecycle, fence,
  seal, and observation lineage are exact.
- Sealing blocks only new slot-local admission; already admitted guards remain
  counted until release.
- The claim fence is exactly the source deployment fence plus one.
- Local refence changes only the controller fence. The removal target fence
  equals the claim fence.
- Removal requires durable `Refenced`, the exact removal target, and zero
  guards.

### Durability and provenance

- Product and drain roots, target, slot, and expected deployment revision never
  change.
- Each intent revision and refence claim revision advances by exactly one.
- Claim, refence, acknowledgement, and teardown succession have distinct exact
  action identities.
- Host time is never evidence for owner, claim, serving, or certification
  expiry.
- Each unknown result gets at most one exact replay with the same borrowed
  authorization. A second unknown result stops closed.
- Same-process recovery uses exact `ClosedRecovery` or `Shutdown` provenance.
- Previous-process teardown creates fresh current-owner `ClosedRecovery`
  provenance.
- Predecessor provenance is evidence only and never becomes current authority.
- Existing V2 provenance constructors and process/owner checks remain
  unchanged.

### Local and asynchronous safety

- No registry lock crosses database, serving, Discord, sleep, or join awaits.
- No database transaction waits for local guards.
- Fence and sequence successors are preflighted before mutation.
- Overflow writes nothing partial and closes the affected authority path.
- A non-cloneable seal moves into a registered process-owned finalizer before a
  claim dispatch can escape.
- Outer future cancellation never detaches an accepted finalizer.

## Version decision

Canonical wire, worker API, and PostgreSQL capability versions are independent.

### Same-process state remains canonical V2

Format V2 already truthfully represents:

- routed `PendingClaimed`;
- `PendingRefenced`;
- same-process `RouteAbsentAcknowledged`.

New transition builders add exact successor checks without changing V2 bytes or
weakening V2 constructors.

### Previous-process route teardown uses canonical V3

A previous-process `PendingRefenced` cannot be acknowledged with its old claim
and current recovery provenance because V2 correctly requires acknowledgement
provenance to match the embedded claim.

Creating a current-owner V2 `Refenced` claim from an empty registry would also
fabricate a local refence. The cross-process terminal result therefore uses:

```text
format_version = 3
```

Its semantic shape is:

```rust
pub struct RuntimeRouteAbsentAcknowledgementV3 {
    pub successor_claim: RuntimeDrainClaimV2,
    pub absence_basis: RuntimePreviousProcessRouteAbsenceBasisV3,
    pub provenance: RuntimeRouteMutationProvenanceV2,
    pub registry_observation_sequence: NonZeroU64,
    pub certification: RuntimePreviousProcessDrainCertificationResolutionV3,
    pub acknowledged_at: DateTime<Utc>,
}

pub struct RuntimePreviousProcessRouteAbsenceBasisV3 {
    pub predecessor_intent_revision: NonZeroU64,
    pub predecessor_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub predecessor_progress: RuntimePreviousProcessDrainProgressV3,
    pub route_identity: RuntimeProcessIdentityV1,
    pub route_incarnation: NonZeroU64,
    pub source_route_fence: FencingToken,
    pub possible_route_fence_ceiling: FencingToken,
    pub predecessor_claim_terminal_digest: RuntimeDrainActionDigestV3,
    pub predecessor_refence_terminal_digest: Option<RuntimeDrainActionDigestV3>,
}
```

The successor claim belongs to the current process and has a fresh empty local
seal with `expected_route = null`. Historical route obligation lives only in
`absence_basis`.

For routed `PendingClaimed`:

- source fence is the seal expected-route fence;
- possible ceiling is the predecessor claim fence;
- refence terminal digest is null.

For `PendingRefenced`:

- source fence is the persisted old-route fence;
- possible ceiling is the removal-target and predecessor claim fence;
- refence terminal digest is exact and non-null.

The successor fence is the possible ceiling plus one. The range for routed
`PendingClaimed` bounds what the predecessor could have locally installed; it
does not claim that every intermediate fence existed.

The V3 certification wrapper is validated against the predecessor claim and
serving identity. A committed predecessor serving identity must not be
validated against the current successor claim. The absence basis supplies the
predecessor process, target, incarnation, and fence envelope on later standalone
decode; source and journal digests link the complete predecessor.

Canonical V3 uses the V2 root, claim, route, process, scalar, and provenance
encodings. Its acknowledgement fields are ordered:

1. `successor_claim`;
2. `absence_basis`;
3. `provenance_json`;
4. `registry_observation_sequence`;
5. `certification`;
6. `acknowledged_at_unix_microseconds`.

The absence basis fields are ordered as shown in the domain type and include
`kind = previous_process_route_teardown`. Progress is exactly
`routed_claimed` or `refenced`. Optional refence digest is a required JSON field.
Digests are lowercase 64-character hexadecimal strings.

Format V3 accepts only:

- `route_absent_acknowledged` with the teardown basis;
- its later `consumed` or `cancelled` Product terminal successors.

Pending states remain V2. Decode dispatches by exact version without fallback.
Unknown or duplicate fields, noncanonical numbers, unsupported state/version
pairs, and bytes that differ from canonical re-encoding fail closed. Existing
V2 rows are not rewritten and their digests do not change.

## Exact state transitions

### T1: routed claim

```text
PendingUnclaimed(V2) + RoutedSealed
  -> PendingClaimed(V2) + RoutedClaimedSealed
```

The deterministic candidate, current owner, paused gateway, readiness, exact
Serving route, zero guards, seal, Product root, deployment, source fence,
serving eligibility, and certification eligibility are revalidated.

The transaction advances intent revision and deployment fence exactly once,
creates a current-owner claim whose seal expected route is the local route, and
records the claim journal.

A determinate non-commit may unseal only after exact observation proves the
intent remains unclaimed and route, incarnation, fence, lifecycle, guard count,
seal, and observations are unchanged. Unknown never unseals.

### T2: local refence

```text
RoutedClaimedSealed -> LocallyRefencedSealed
```

The serving lane is closed and joined first. Unknown heartbeat is
exact-observed, the latest serving receipt is durably disconnected or proven
absent/expired, and certification resolution is immutable.

The synchronous registry transition requires the durable claim, exact seal and
route, zero guards, current provenance, and claim fence equal to route fence
plus one. It changes only the fence and observation and returns old route,
removal target, provenance, and sequence in a non-cloneable capability.

### T3: durable refence

```text
PendingClaimed(V2) + LocallyRefencedSealed
  -> PendingRefenced(V2) + DurablyRefencedSealed
```

Source bytes/digest, claim and revision, journal, seal, old route, removal
target, provenance, and observation are exact. Intent and claim revisions each
advance by one. Deployment fence and claim identity do not change. Only the
checked receipt upgrades local authority to durably refenced.

### T4: drain and remove

```text
DurablyRefencedSealed
  -> DrainingRefencedSealed
  -> RouteAbsentSealed
```

Seal-authorized mutation observes the persisted removal target, changes it to
draining, proves zero guards, and removes only that incarnation and fence. The
seal and fence high-water remain. Ordinary unsealed registry APIs grant no
authority for these steps.

### T5: same-process acknowledgement

```text
PendingRefenced(V2) + RouteAbsentSealed
  -> RouteAbsentAcknowledged(V2) + AcknowledgedEmpty
```

The transition binds exact source bytes/digest, refence journal, removal target,
post-removal observation, certification, and provenance matching the claim
process and owner. Intent revision advances once; fence and claim revision do
not. Only its durable receipt consumes the seal.

### T6: same-process recovery within the local lineage

If local refence happened while durable state is still `PendingClaimed`, the
registered finalizer and registry may reconstruct `LocallyRefencedSealed` only
from the exact seal, old route, current claim fence, and later observation. It
then performs T3 through T5 without a replacement claim.

If durable state is already `PendingRefenced`:

- an exact sealed removal target reconstructs `DurablyRefencedSealed`;
- exact absence produced under the same seal reconstructs `RouteAbsentSealed`;
- unsealed or aggregate absence, another registry lifetime, or route drift fails
  closed.

### T7: previous-process teardown succession

```text
Previous routed PendingClaimed(V2)
  or Previous PendingRefenced(V2)
    -> RouteAbsentAcknowledged(V3)
```

This one serializable transaction proves:

- exact source bytes, digest, predecessor claim, and claim journal;
- exact refence journal when applicable;
- different predecessor process and strictly newer current owner epoch;
- predecessor expiry and current owner freshness by database time;
- current paused gateway and fresh empty-slot seal;
- current empty registry and no local serving lane;
- absent, disconnected, or recoverably expired serving state;
- eligible exact certification resolution;
- exact route envelope and successor fence.

It advances intent revision, successor claim revision, and deployment fence each
by one; creates a current-owner empty-seal successor claim; writes the V3 basis,
provenance, acknowledgement, and action journal atomically; and never persists
an intermediate current-owner pending state.

Fresh foreign claim or serving lease returns a database-derived bounded retry
without local mutation.

## Typed linear registry states

| State | Exact local truth | Successor |
| --- | --- | --- |
| `RoutedObservedV4` | Unsealed Serving route | `RoutedSealedV4` |
| `RoutedSealedV4` | Route sealed, no new guards | `RoutedClaimedSealedV4` or checked rollback |
| `RoutedClaimedSealedV4` | Durable claim bound | `LocallyRefencedSealedV4` |
| `LocallyRefencedSealedV4` | Local successor fence | `DurablyRefencedSealedV4` |
| `DurablyRefencedSealedV4` | Durable removal target | `DrainingRefencedSealedV4` |
| `DrainingRefencedSealedV4` | Draining, zero guards | `RouteAbsentSealedV4` |
| `RouteAbsentSealedV4` | Exact incarnation absent | `AcknowledgedEmptyV4` |
| `EmptySuccessionSealedV4` | Fresh empty successor seal | `AcknowledgedEmptyV4` |

Every transition consumes its source. These types implement none of `Clone`,
`Copy`, `Serialize`, `Deserialize`, or `Default`, and expose no raw-parts
constructor.

The registry stores registry lifetime, slot, seal key/generation, admission
generation, route/incarnation/fences, lifecycle, slot/global observations,
guards, and bound durable receipt digests. This rejects ABA and capabilities
from another registry.

Dedicated seal-authorized methods perform bind-claim, refence, bind-refence,
drain, observe, remove, and consume-after-ack. Existing `install`, `activate`,
`advance_authority`, `begin_drain`, and `remove` continue to reject sealed
slots. The new API is not a general sealed-slot escape hatch.

## Worker V4

V4 selection returns exactly:

```text
NoCandidate
Unclaimed
CurrentOwnerRouteAbsentClaimed
CurrentOwnerRoutedClaimed
CurrentOwnerRefenced
FreshPreviousOwnerRouteAbsentClaimed
ExpiredPreviousOwnerRouteAbsentClaimed
FreshPreviousOwnerRoutedClaimed
ExpiredPreviousOwnerRoutedClaimed
FreshPreviousOwnerRefenced
ExpiredPreviousOwnerRefenced
```

Selection classifies durable state only. Each candidate carries exact intent,
slot, target, source revision/bytes/digest, Product root, database time, current
owner, revision/fence budget, and all claim, route, expiry, journal, and
certification evidence required by its class.

Selection rejects state/progress mismatch, routed target mismatch, non-exact
refence fence, same process with another stable owner, non-newer successor
epoch, ambiguous journals, unsupported wire, and any Product, deployment,
slot-fence, serving, or certification mismatch.

The pure worker adds typed ports for:

- V4 selection;
- routed claim;
- refence progress;
- same-process refenced acknowledgement;
- previous-process teardown succession;
- exact terminal observation and one replay;
- serving-lane close/join;
- V2 serving observation and conditional disconnect;
- certification resolution;
- finalizer register/transfer/join;
- typed registry transitions.

Execution ports accept only checked non-cloneable authorizations, never raw IDs.
The worker remains free of `sqlx`, Twilight, HTTP, sockets, filesystem,
operating-system secrets, gateway implementation, registry implementation,
model-facing crates, and concrete task supervision.

For unclaimed route-present state the worker runs T1 through T5. For
`CurrentOwnerRoutedClaimed` it joins or recovers the exact seal-owning finalizer
and starts after T1. For `CurrentOwnerRefenced` it starts at T4 or T5 according
to exact typed local state. Neither current-owner case issues a replacement
claim.

Previous-process recovery seals an empty slot and runs only T7. It never
reconstructs, refences, drains, or removes a historical local route.

After a successful recovery action, all readiness receipts and paused gateway
and registry observations are refreshed before the startup loop accepts a
fixed point.

## Serving and certification boundary

The finalizer may only:

- close and join the exact serialized serving lane;
- exact-observe unknown heartbeat;
- observe the exact V2 serving receipt;
- conditionally disconnect that receipt;
- observe and classify the exact certification operation.

It cannot start or renew heartbeat, disconnect another receipt, prepare or
commit certification, infer absence from lane closure, or use host time.

The complete authorization and receipt correlation binds scope, slot, target,
revision, operation, drain intent, attestation digest, process identity, lease
epoch, receipt revision, and persisted owner evidence. No lossy conversion to
`RuntimeServingIdentityV1` is allowed.

Fresh foreign serving state returns a database-derived retry. A committed
certification requires exact durable disconnect or expiry. Ambiguous natural
scope or certification stops closed.

## PostgreSQL V4

Additive migrations after
`202607280001_add_pending_drain_succession_v3.sql` add separate capabilities
for:

1. V4 selection;
2. routed claim;
3. exact claimed-to-refenced progress;
4. refenced route-absence acknowledgement;
5. previous-process routed/refenced teardown succession;
6. exact terminal observation where replay cannot decide process loss;
7. V2 serving observation and conditional disconnect.

No generic mutation-kind capability is exposed.

### Canonical and transactional contract

The migrations:

- keep exact V2 validation for existing rows;
- replace the narrow check requiring every V2
  `PendingClaimed.expected_route` to be null;
- accept routed claimed only through full V2 route/claim validation;
- add strict V3 decode, reconstruction, digest, and state/version validation;
- rewrite no existing row and infer no backfill.

Every execution mutation is serializable and locks:

1. global writer-fence advisory lock;
2. gateway-owner advisory lock and owner row;
3. serving-slot advisory lock;
4. slot writer-fence row;
5. exact serving lease when required;
6. deployment;
7. Product root;
8. drain-intent advisory lock and row;
9. certification operation/reservation;
10. exact action journal.

Selection uses the compatible subset and deterministic oldest candidate order.
It never locks the intent before the slot.

Serving disconnect uses the serving capability's lock order and separate role.
No execution transaction stays open across a serving-pool call. The later
execution mutation revalidates the exact durable serving receipt.

### Mutation guarantees

Routed claim advances the deployment fence and writes V2 `PendingClaimed`
atomically.

Refence progress validates claimed source, journal, seal, old route, exact
successor target, provenance, and observations; advances intent and claim
revisions; and writes V2 `PendingRefenced` without advancing the deployment
fence.

Same-process acknowledgement validates refenced source/journal, sealed removal,
serving/certification, and provenance; then writes V2
`RouteAbsentAcknowledged` without releasing the slot fence.

Teardown succession validates predecessor bytes/journals, database expiry,
newer owner, paused empty recovery, serving/certification, route envelope, and
V3 successor bytes. It atomically advances the fence and writes V3
`RouteAbsentAcknowledged`.

Each immutable terminal projection binds action stage, recovery, owner, source
revision/digest, predecessor journals, target/route/fences, seal/observations,
serving/certification digests, successor bytes/digest, and database mutation
time. Exact replay requires every immutable request field. Changed evidence is
not replay.

### ACL and readiness

- Public and default execute grants are revoked.
- Execution functions are granted only to the execution role.
- Serving functions are granted only to the serving role.
- Runtime roles receive no direct relation privileges and cannot call each
  other's capabilities.
- Complete function identities, bodies, owners, ACLs, and definition digests
  are pinned in execution and serving manifests/readiness.
- Migration history, collision, missing, extra, and drift cases fail readiness.
- Fresh PostgreSQL 16 replay and restricted-role tests are mandatory.

## Finalizer, process loss, and shutdown

Claim and teardown dispatch share synchronous arbitration with emergency and
shutdown. If invalidation wins, no authorization escapes. If dispatch wins, the
exact finalizer is registered before releasing arbitration and owns the seal
and authorization.

Finalizer identity binds process, stable owner and revision, recovery/shutdown
generation, intent, source revision/digest, claim epoch/revision, fence, seal,
slot, incarnation, and action stage. Duplicate registration joins the exact
finalizer or fails closed.

Shutdown:

1. closes new claim dispatch;
2. seals and snapshots the finalizer registry;
3. joins or exact-observes every dispatched mutation;
4. hard-pauses and closes gateway control;
5. closes serving lanes and conditionally disconnects exact receipts;
6. completes a committed claim only through the checked shutdown permit;
7. closes database pools only after dependent finalizers settle.

Shutdown continuation before refence uses exact `Shutdown` provenance. Already
durable `ClosedRecovery` refence remains immutable. Shutdown never unseals a
claimed slot merely to exit.

External-await priority is:

1. operation or owner-safety cutoff;
2. terminal Discord;
3. terminal gateway owner;
4. shutdown/emergency takeover;
5. database or serving completion.

A late durable result is observed by the registered finalizer or next process;
it never reopens admission or releases a seal through the abandoned caller.

## Crash matrix

| Boundary | Durable state | Next-process rule |
| --- | --- | --- |
| Before claim commit | `PendingUnclaimed` | Re-select unclaimed; no old local route is reconstructed |
| Claim commit | Routed `PendingClaimed` | Retry until predecessor expiry, then T7 |
| Local refence before progress | Routed `PendingClaimed` | T7 uses expected-route through claim-fence envelope |
| Refence progress commit | `PendingRefenced` | Retry until predecessor expiry, then T7 |
| Local drain or removal | `PendingRefenced` | T7 proves current empty teardown |
| Acknowledgement commit | V2 `RouteAbsentAcknowledged` | Observe terminal; Product fence remains |
| T7 before commit | Previous pending state | Repeat exact selection with a fresh local seal |
| T7 commit unknown | Previous pending or V3 acknowledged | One exact replay decides |
| T7 committed, seal consume fails | V3 acknowledged | Observe terminal; local registry remains failed closed |

The next process never recreates a vanished route. It proves current
process-local absence and the predecessor route envelope.

## Verification

### Pure and wire

- exact `+1` intent, claim, and fence transitions;
- immutable roots and exact route/incarnation lineage;
- V2 routed claim, refence, and acknowledgement round trips;
- V3 routed-claimed and refenced teardown golden vectors;
- strict version dispatch and byte-for-byte canonical re-encoding;
- rejection of owner/process/provenance mixing, stale observations, wrong
  journals, serving/certification drift, and every overflow.

### Registry and worker

- affected-slot seal races against guard acquire/release;
- unrelated slot behavior remains unchanged;
- ordinary mutation rejects sealed slots;
- ABA, foreign registry, stale token, lifecycle, fence, and receipt mismatch;
- no removal before durable refence or with nonzero guards;
- fence high-water retained after removal and acknowledgement;
- linear capabilities cannot be fabricated, cloned, or serialized;
- every V4 selection class and invalid cross-class input;
- current claimed/refenced resumes without replacement claim;
- previous-process classes use only direct T7;
- one exact replay, second unknown closed;
- serving join before refence, durable refence before removal, durable
  acknowledgement before seal consumption;
- cutoff, Discord, owner, shutdown, and completion ordering.

### PostgreSQL and process

- deterministic oldest classification for every supported state;
- apply, exact replay, concurrent loser, rollback, and response-loss behavior
  for each capability;
- wrong source bytes/digest, Product/deployment/slot/route/fence/seal/owner,
  serving/certification, and journal rejection without partial writes;
- fresh foreign claim/serving database-time retry;
- V3 reconstruction before commit and replay;
- least-privilege ACL, manifests, readiness, migration collision, and fresh
  PostgreSQL 16;
- fault injection at selection, seal, guard wait, finalizer arbitration, every
  send/commit/response, serving join/disconnect, certification observation,
  refence, removal, acknowledgement, reconnect, owner loss, shutdown, pool
  close, and every crash-matrix row;
- admission never opens, no finalizer detaches, and no pool closes before its
  finalizers settle.

Repository gates include focused and workspace tests, compile-fail and
dependency guards, Clippy with warnings denied, formatting, secret scan,
PostgreSQL security suites, GitHub checks, certified-host restart/SLO cohorts,
and disposable-guild tests after the production serving foundation exists.
Customer guilds are never release targets.

## Functional implementation order

Each item is a separate functional commit:

1. pure V2 transitions and V3 teardown domain/wire;
2. typed registry capabilities and race/compile-fail tests;
3. worker V4 selection, authorizations, receipts, and finalizer ports;
4. V4 selector and widened strict V2 PostgreSQL validation;
5. routed claim and refence-progress PostgreSQL capabilities;
6. same-process acknowledgement and V3 teardown succession;
7. V2 serving observation/disconnect;
8. adapters, semantic projection, manifests, ACL, and readiness;
9. same-process paused composition;
10. previous-process restart composition;
11. finalizer arbitration, shutdown transfer, and join;
12. crash, PostgreSQL 16, security, workspace, CI, and certified-host gates;
13. separate production serving/admission foundation;
14. separate Product consume/cancel fence release.

No later item bypasses a failed earlier gate.

## Completion criteria

This contract is complete when routed `PendingClaimed` and `PendingRefenced`
reach a checked fixed point, removal is impossible before durable refence,
previous-process recovery records explicit V3 teardown evidence, V2 provenance
is unchanged, unknown results have one bounded finalization, shutdown joins all
accepted finalizers, admission remains paused, PostgreSQL stays least privilege,
and all required gates are green.

That completes paused pending-drain recovery. It does not alone make the
executable production-ready; serving/admission, Product fence release,
disposable-guild verification, and certified-host SLO evidence remain required.
