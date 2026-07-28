# Runtime Product drain terminalization design

Date: 2026-07-28

## Outcome

An exact `RouteAbsentAcknowledged` Product drain intent can leave the frozen
serving-slot boundary through one of two Product-owned terminal transitions:

1. **consume** applies the exact correlated Product lifecycle mutation and
   records the drain intent as `Consumed`;
2. **cancel** explicitly abandons that pending Product lifecycle mutation,
   preserves the current deployment target and phase, advances the deployment
   revision, and records the drain intent as `Cancelled`.

Each transition is one serializable PostgreSQL transaction. The transaction
either commits all of the following or none of them:

- the Product-owned mutation or cancellation record;
- the resulting runtime deployment mutation;
- the exact drain-intent terminal state and canonical bytes;
- the immutable terminal action receipt;
- the serving-slot writer epoch advance;
- clearing every pending-drain field from the physical slot writer fence.

The writer epoch advances when the slot is released. The epoch advance is not
an optimization. It is the durable cut that invalidates every writer that
observed the slot before or during the freeze. A new runtime writer can begin
only from the successor epoch and the successor deployment revision.

`RouteAbsentAcknowledged` remains runtime-terminal. No runtime process,
gateway owner, expired drain claimant, or startup recovery permit may consume,
cancel, or release it. Only the correlated Product apply capability or a
freshly authorized explicit Product lifecycle-cancellation capability may
perform terminalization.

This design does not require route-present draining or `PendingRefenced` to
exist. It consumes only an already persisted, byte-exact
`RouteAbsentAcknowledged` source. A source that is still `Pending`, routed,
refenced, ambiguous, or corrupt remains frozen.

## Meaning of cancellation

Cancellation in this design cancels the pending Product lifecycle mutation. It
does not reject the promotion and does not invoke the runtime convergence
machine's existing deployment `cancel` command.

The existing convergence `cancel` command changes the deployment phase to
`Cancelled` and is restricted to early runtime phases. Reusing it here would
conflate two different authorities and would prevent cancelling an
acknowledged drain that originated from `AwaitingGatewayReady` or `Live`.

Product drain cancellation instead:

- leaves the authoring promotion decision unchanged;
- leaves the current runtime deployment target, phase, runtime generation,
  installation authority, binding authority, and stable history unchanged;
- advances the runtime deployment revision exactly once;
- advances the deployment mutation clock monotonically;
- advances the slot writer epoch exactly once;
- terminally cancels only the correlated drain intent;
- records an immutable Product cancellation audit receipt;
- permits later reconciliation of the preserved target from the successor
  revision and epoch.

The old Product operation cannot be revived after cancellation. A retry of
that exact operation returns its persisted cancelled outcome. A later Product
operation must use a new idempotency identity and the successor deployment
revision.

## Non-goals

This slice does not:

- create an initial Product drain intent;
- claim, refence, drain, remove, or acknowledge a route;
- implement route-present or `PendingRefenced` recovery;
- reconstruct process-local route-absence evidence;
- treat claim or gateway-owner expiry as terminalization authority;
- open Discord admission or start serving;
- add a runtime-side terminalization API;
- delete Product operations, drain intents, acknowledgements, or audit
  receipts;
- make the runtime convergence deployment `cancel` command Product-callable;
- permit terminalization from `Pending`, `PendingClaimed`, or
  `PendingRefenced`;
- infer route absence from an empty current process registry;
- release a slot in a transaction separate from the Product mutation;
- add a background retry daemon;
- weaken current application authentication, CSRF, fresh Discord authority,
  key rotation, or idempotency boundaries.

## Safety invariants

### Exact source

- The source state is exactly `RouteAbsentAcknowledged`.
- The source intent ID, intent revision, canonical state bytes, canonical
  state digest, immutable drain root, Product operation ID, Product mutation
  digest, scope, slot, expected deployment revision, expected target, and
  mutation kind are exact.
- The complete acknowledgement, claim, provenance, route identity or absence,
  certification resolution, registry observation sequence, and acknowledged
  database time decode canonically.
- The source acknowledgement remains valid after its gateway owner or drain
  claim expires. Expiry neither invalidates the acknowledgement nor grants a
  runtime successor permission to terminalize it.
- No host clock, current empty registry, or best-effort Discord observation
  substitutes for the durable acknowledgement.

### Single terminal transition

- The intent revision advances by exactly one from the acknowledged source.
- The immutable Product and drain roots never change.
- A consume result is exactly `Consumed`.
- A cancel result is exactly `Cancelled`.
- `Consumed` and `Cancelled` never transition again.
- Exact replay returns the original terminal aggregate and receipt without
  advancing any revision, epoch, or clock again.
- A consume/cancel race has one committed winner. The loser receives either
  the exact replay of the same action or a stable terminal conflict.

### Product authority

- Consume is authorized only by a retry of the exact correlated Product
  operation.
- Consume requires the same Product principal, installation scope, promotion,
  expected Product revision, approval payload digest, idempotency identity,
  semantic request digest, and immutable runtime Product mutation root.
- Explicit cancel uses a distinct `CancelLifecycle` capability. It does not
  reuse `Reject`, `Apply`, a runtime controller guard, or a gateway-owner
  lease.
- Cancellation requires authenticated mutation, CSRF proof, fresh Discord
  guild authority, the exact installation scope, the exact acknowledged drain
  selector, an expected Product revision, an idempotency key, and a bounded
  non-empty reason.
- Product application code cannot fabricate a persisted acknowledgement or a
  terminal controller receipt.
- Runtime code receives no Product principal or cancellation capability.

### Atomic slot release

- The global runtime writer fence is open for every first application.
- The exact serving-slot advisory lock and physical slot writer-fence row are
  held through commit.
- Before terminalization, every `pending_*` field on the slot fence matches
  the exact acknowledged intent.
- The source slot writer epoch is finite and can advance.
- Commit advances the slot writer epoch by exactly one.
- Commit clears all pending-drain fence fields as one shape transition.
- The drain intent becomes terminal in the same transaction that clears the
  fence.
- The Product lifecycle mutation or cancellation revision commits in that
  same transaction.
- A rollback or determinate non-commit leaves the acknowledgement and complete
  slot freeze unchanged.
- No transaction can observe a terminal intent with its old freeze committed,
  or an unfrozen slot with an acknowledged intent committed.

### Stale-writer safety

- Marking the original drain advanced the slot writer epoch and invalidated
  pre-freeze writers.
- Terminalization advances the epoch again and invalidates any actor that
  observed the frozen epoch.
- A runtime claim, staging operation, certification operation, serving
  heartbeat, panel mutation, or interaction execution using either older
  epoch fails closed.
- A post-terminalization runtime operation must observe both the successor
  slot epoch and the current deployment revision.
- Cancellation advances the deployment revision even though it preserves the
  target and phase. This prevents a later operation from colliding with the
  cancelled drain's natural scope.
- An exact replay of an old terminal action never clears a newer drain intent
  or changes a newer slot epoch.

### Time and replay

- Terminal timestamps come only from PostgreSQL.
- One terminal mutation clock is used for the terminal drain state,
  deployment mutation, slot fence update, Product receipt, and audit action.
- The terminal clock is finite, no earlier than the source acknowledgement,
  and strictly newer than the locked deployment and slot-fence mutation
  clocks.
- Commit uncertainty is resolved by the immutable terminal action journal.
- A replay verifies the complete original request and persisted terminal
  projection byte-for-byte.
- A changed principal, semantic command, idempotency identity, source intent,
  Product root, action kind, or cancellation reason is not a replay.
- Database serialization and deadlock failures may be retried only when the
  driver proves the transaction rolled back.
- An unknown commit result returns `Indeterminate`; the caller retries the
  exact command.

## Existing model and required additions

The pure controller already contains:

- `RuntimeDrainIntentStateKindV2::RouteAbsentAcknowledged`;
- `RuntimeDrainIntentStateKindV2::Consumed`;
- `RuntimeDrainIntentStateKindV2::Cancelled`;
- `RuntimeRouteAbsentDrainIntentSourceV2::from_acknowledged`;
- persisted constructors for consumed and cancelled states;
- canonical V2 wire forms for both terminal states;
- mutation outcome tags for `Consumed` and `Cancelled`.

It intentionally has no public consumed or cancelled receipt transition. The
dependency guard currently proves those public constructors do not exist.
PostgreSQL also constrains persisted intents to `pending` and
`route_absent_acknowledged`, and the slot fence has no gated release
transition.

This slice fills those boundaries without changing the immutable Product or
drain preimages.

## Controller boundary

The controller remains pure. It owns canonical state and exact transition
validation, not authentication, SQL, Product decision mutation, or slot
locking.

Add two distinct checked sources:

- `RuntimeDrainConsumptionSourceV2`;
- `RuntimeDrainCancellationSourceV2`.

Both are constructed only from `RuntimeRouteAbsentDrainIntentSourceV2`.
Consumption also binds the expected resulting runtime deployment revision.
Cancellation binds the database-derived cancellation time but does not claim
that the runtime deployment itself entered the convergence `Cancelled` phase.

Add public receipt constructors:

- `RuntimeDrainIntentReceiptV2::consumed`;
- `RuntimeDrainIntentReceiptV2::cancelled`.

The consumed constructor validates:

- exact immutable roots;
- source kind `RouteAbsentAcknowledged`;
- result kind `Consumed`;
- exact intent-revision successor;
- exact expected resulting runtime deployment revision;
- canonical consumed timestamp.

The cancelled constructor validates:

- exact immutable roots;
- source kind `RouteAbsentAcknowledged`;
- result kind `Cancelled`;
- exact intent-revision successor;
- canonical cancelled timestamp.

The existing V2 cancelled wire contains only `cancelled_at`. It is not changed
to embed a deployment revision. The cross-aggregate cancellation proof belongs
to the Product terminal action projection, which binds the source and
resulting deployment revisions, source and successor slot epochs, terminal
intent digest, and cancellation receipt. This preserves the established V2
wire while keeping the deployment and slot relationship exact.

The constructors reject:

- a merely newer, rather than exactly succeeding, intent revision;
- pending, claimed, refenced, consumed, or cancelled sources;
- acknowledged results;
- changed immutable roots;
- wrong resulting deployment revision;
- noncanonical timestamps;
- terminal-to-terminal transitions.

Canonical state golden vectors remain unchanged for existing variants. New
transition tests use the existing consumed and cancelled wire vectors.

## Application boundary

### Consume through Product apply

`ProductApplyPort::apply_idempotent` remains the public consume entry point.
The caller retries the same apply command that originally produced the
correlated drain intent.

The application flow becomes:

1. authenticate the mutation and obtain fresh `Apply` authority;
2. compute the existing key-rotation-aware apply idempotency and semantic
   digests;
3. lock the Product apply scope through the existing apply boundary;
4. classify the correlated drain state;
5. return structured drain-pending information for `Pending`;
6. invoke exact consume for `RouteAbsentAcknowledged`;
7. return the ordinary applied or superseded Product receipt after consume;
8. return the exact original terminal receipt for a replay;
9. return a stable lifecycle-cancelled result if that drain was explicitly
   cancelled.

The adapter must not clear the slot and then call the existing Product
finalizer. The final Product mutation and slot release are one database
capability.

The current unstructured `ProductControlPortError::RuntimeDrainRequired`
becomes or is accompanied by a structured drain-pending result containing a
server-projected `ProductDrainSelectorV1`. The selector contains checked
opaque forms of:

- drain intent ID;
- acknowledged intent revision;
- acknowledged canonical-state digest;
- original Product operation ID;
- expected runtime deployment revision.

The selector is a concurrency selector, not a bearer credential. Every
consume or cancel still requires normal application authorization.

### Explicit lifecycle cancellation

Add:

- `CapabilityV1::CancelLifecycle`;
- `CancelProductLifecycleMutationV1`;
- `AuthorizedCancelProductLifecycleV1`;
- `ProductLifecycleCancellationPort`;
- `ProductLifecycleCancellationReceiptV1`.

The command binds:

- promotion selector;
- expected approval payload digest;
- expected Product revision;
- exact `ProductDrainSelectorV1`;
- idempotency key;
- bounded non-empty reason.

The cancellation capability uses the write evidence lifetime and a distinct
authority digest domain. The Discord authority adapter evaluates the same
full runtime-capable guild snapshot required by Apply. Releasing a slot can
allow the preserved target to re-enter runtime convergence, so cancellation
must not rely on the weaker read or approval snapshot.

The application cancellation method:

1. authenticates a mutation and validates CSRF;
2. requests fresh `CancelLifecycle` authority;
3. checks tenant, installation, guild, actor, bot, and runtime environment
   correlation;
4. derives key-rotation-aware cancellation idempotency, semantic request,
   receipt, audit, and terminal action identities in distinct domains;
5. invokes the cancellation persistence port once;
6. validates the returned Product, deployment, drain, and slot projection;
7. returns the immutable cancellation receipt.

Approval rejection remains unchanged. `RejectProductPromotionV1` cannot
cancel an acknowledged runtime drain.

The authoring application crate keeps its current dependency direction. It
defines opaque checked selector strings and application commands but does not
depend on `automation-runtime-controller`. The PostgreSQL adapter translates
persisted rows into controller types and performs semantic validation.

## PostgreSQL boundary

### Additive migration

Add one migration after
`202607280001_add_pending_drain_succession_v3.sql`.

The migration:

1. snapshots the owner, definitions, ACLs, constraints, indexes, triggers,
   manifests, and readiness digests it will replace;
2. takes the global runtime writer-fence migration lock;
3. locks the affected Product, runtime deployment, drain, slot-fence,
   certification, serving, idempotency, receipt, and audit relations in the
   established order;
4. adds an immutable terminal action journal;
5. extends the drain-intent state constraint to `consumed` and `cancelled`;
6. extends the private exact canonical-state validator for both terminal
   states;
7. adds a gated slot-fence terminal release helper;
8. extends the guarded drain-intent mutation trigger for exact terminal
   updates;
9. adds consume and cancel security-definer capabilities;
10. patches runtime readers to accept terminal rows while excluding them from
    frozen-work selection;
11. updates all affected manifests and readiness contracts;
12. restores only the intended capability grants;
13. proves the postflight schema, definitions, ACLs, and digests exactly.

No data backfill is required. Before this migration the state constraint
prevents terminal rows. Existing pending and acknowledged canonical bytes do
not change.

### Terminal action journal

Add `runtime_product_drain_terminal_actions_v2` as an append-only table. It has
one row per terminalized drain intent and contains:

- terminal action ID;
- terminal kind, `consumed` or `cancelled`;
- drain intent ID and original Product operation ID;
- Product action idempotency and semantic request digests;
- cancellation reason digest for cancellation;
- source intent revision and canonical-state digest;
- result intent revision and canonical-state digest;
- source and resulting runtime deployment revisions;
- resulting deployment snapshot digest;
- source and successor slot writer epochs;
- terminal database time;
- Product receipt ID and audit event ID;
- authority observation digest and installation authority revision used for
  the first application;
- terminal projection bytes and SHA-256 digest.

The table has:

- a primary key on terminal action ID;
- a unique key on drain intent ID;
- a unique exact action identity appropriate to each terminal kind;
- foreign keys to the immutable Product operation and drain intent;
- bounded canonical byte and digest checks;
- a closed terminal-kind constraint;
- finite timestamp and monotonic revision/epoch checks;
- reject-update, reject-delete, and reject-truncate triggers.

The terminal projection uses a distinct framed digest domain:

`starring.runtime.product_drain.terminal.v2\0`

It binds, in order:

1. terminal kind and action identity;
2. immutable Product and drain root digests;
3. source acknowledged intent revision and state digest;
4. result terminal intent revision, state bytes, and digest;
5. source and result deployment revisions and result snapshot digest;
6. source and successor slot epochs;
7. Product receipt and audit identities;
8. cancellation authority and reason digests when present;
9. terminal database time.

The projection does not duplicate the potentially large acknowledged source
bytes. The transaction locks those bytes, validates their canonical digest,
and records the source digest in the immutable projection.

### Public capabilities

Add two narrow public functions:

- `starring_product_apply_consume_runtime_drain_v2`;
- `starring_product_cancel_runtime_drain_v2`.

Both are `SECURITY DEFINER`, set `search_path = pg_catalog`, accept only scalar
or bounded byte inputs, and run only in a serializable transaction configured
with the existing Product mutation timeouts.

Consume is granted only to the Product apply executor role. Cancel is granted
only to a distinct Product lifecycle-cancellation executor role. Neither role
receives table, sequence, schema-create, private-function, or trigger-function
privileges.

The private owner-only helpers include:

- exact terminal canonical-state builders;
- exact terminal projection builder and replay validator;
- slot writer-fence release helper;
- Product cancellation deployment-revision helper;
- consume delegation into the existing Product apply mutation core.

The public functions do not call public Product or runtime functions in an
order that can reacquire locks inconsistently. Existing Product apply logic is
factored into owner-only unfenced cores where necessary, and the public
terminalization wrappers establish the complete lock order once.

## Lock order

Consume and cancel use the same order. Optional rows are skipped without
reordering later locks.

1. shared global runtime writer-fence advisory lock;
2. global writer-fence singleton row `FOR SHARE`;
3. exclusive serving-slot advisory lock;
4. physical slot writer-fence row `FOR UPDATE`;
5. serving lease and serialized serving-lane state for the slot;
6. every runtime deployment in the slot, ordered by runtime generation and
   deployment ID, `FOR UPDATE`;
7. the exact deployment named by the drain root;
8. the authoring Product promotion, approval, activation request,
   idempotency, receipt, and audit roots in their existing Product apply
   order;
9. the immutable runtime Product operation row;
10. the drain-intent advisory lock;
11. the exact drain-intent row;
12. certification reservation, terminal, and serving-attestation rows
    correlated by the acknowledgement;
13. the terminal action journal identity.

Gateway-owner locks are not acquired. A persisted acknowledgement is already
runtime-terminal, and Product terminalization is not owned by the gateway
process that produced it.

No registry lock, Discord operation, filesystem operation, external network
request, sleep, or application callback runs while the database transaction
is open.

The transaction revalidates after all locks:

- the global writer fence is open;
- the Product authority and application idempotency request are exact;
- the Product operation and drain immutable roots are canonical and
  correlated;
- the source is an exact `RouteAbsentAcknowledged`;
- the physical slot fence points to that source in every pending field;
- no other frozen intent exists for the slot;
- the deployment and Product revisions match the source operation;
- no active serving lease, controller lease, heartbeat writer,
  certification writer, or newer unresolved lane operation exists;
- the acknowledgement's certification resolution matches durable
  certification history;
- the source and result revisions and epoch can advance without overflow.

## Consume transaction

For a first consume application:

1. validate fresh Apply authority and exact application idempotency;
2. reconstruct the immutable Product and drain roots;
3. decode and validate the acknowledged canonical source through the
   controller model;
4. verify that the request is the exact correlated original Product
   operation;
5. run the existing Product lifecycle mutation through an owner-only core;
6. derive the resulting runtime deployment revision and canonical Product
   receipt;
7. derive one terminal database clock;
8. build `Consumed` with the exact successor intent revision, resulting
   runtime deployment revision, and terminal clock;
9. encode and independently validate the canonical terminal state;
10. advance and clear the slot writer fence through the gated release helper;
11. insert the immutable consumed action and terminal projection;
12. update the drain intent through its exact mutation gate;
13. validate deferred slot/drain symmetry;
14. return the complete terminal projection.

The Product mutation is logically prepared before the fence is released.
External transactions cannot observe the internal order because all changes
commit atomically.

If the exact terminal action already exists, the function:

- validates the complete request against the journal;
- validates the persisted terminal drain state;
- validates the original terminal projection digest;
- allows the current deployment and slot epoch to have advanced
  monotonically through later legitimate operations;
- does not require the old slot to remain empty;
- does not touch a newer pending drain;
- returns the original Product receipt with `exact_replay = true`.

## Cancellation transaction

For a first cancellation:

1. validate fresh `CancelLifecycle` authority and exact cancellation
   idempotency;
2. lock and validate the Product promotion and exact drain selector;
3. reconstruct and validate the acknowledged source;
4. prove the requested cancellation action is absent;
5. prove the preserved deployment is still the exact acknowledged target and
   has no active runtime writer;
6. derive one terminal database clock;
7. rebuild the deployment snapshot with revision plus one while preserving
   phase, target, runtime generation, installation authority, binding
   authority, convergence history, and stable failure history;
8. update the deployment row and snapshot under exact source-revision CAS;
9. build `Cancelled` with the exact successor intent revision and terminal
   clock;
10. encode and independently validate the canonical terminal state;
11. advance and clear the slot writer fence through the gated release helper;
12. insert the Product cancellation receipt and audit event;
13. insert the immutable cancelled action and terminal projection;
14. update the drain intent through its exact mutation gate;
15. validate deferred slot/drain symmetry;
16. return the complete cancellation projection.

The cancellation deployment mutation changes no target or phase. Tests
byte-compare every protected snapshot field and permit differences only in the
revision and schema-defined mutation-time projection.

An exact cancellation replay returns the original receipt. A different
reason, selector, principal, Product revision, semantic digest, or
idempotency identity is a conflict and does not change the slot.

## Slot writer-fence release gate

Extend `reject_runtime_slot_writer_fence_mutation_v2` with one closed
`terminal_release` gate action. Add an owner-only helper that:

- requires the exact slot and expected source writer epoch;
- requires every pending field to match the terminalizing intent;
- requires the locked drain row to be the exact acknowledged source;
- requires an exact requested terminal kind;
- sets a transaction-local one-shot gate containing the complete old and new
  identity;
- updates `writer_epoch = writer_epoch + 1`;
- clears every `pending_*` field;
- advances `updated_at` to the terminal mutation clock;
- proves the trigger consumed and cleared every gate setting;
- returns the successor epoch.

The trigger accepts only:

- unchanged slot identity;
- exact epoch successor;
- the complete non-null pending shape becoming the complete null shape;
- no unrelated field change;
- the exact terminal mutation clock.

Direct updates remain rejected. The existing `advance` and `mark_drain` gates
remain unchanged.

The deferred symmetry trigger treats only `Pending` and
`RouteAbsentAcknowledged` as frozen. It requires:

- one exact slot fence for either frozen state;
- no old fence reference after a transition to `Consumed` or `Cancelled`;
- no terminal state to occupy the one-frozen-intent-per-slot index.

## Idempotency and replay

### Consume identity

Consume reuses the current Product apply idempotency domains and the immutable
runtime Product operation identity. It adds a derived consumed terminal action
identity in a separate domain. The caller cannot select a different
acknowledgement while keeping the same apply command.

### Cancellation identity

Cancellation adds distinct domains for:

- idempotency key digest;
- semantic request digest;
- receipt ID;
- audit event ID;
- terminal action ID;
- cancellation reason digest;
- session subject digest.

The keyring rotation and alias-capacity rules mirror approval, rejection, and
apply. The active digest and all accepted candidate key IDs and fingerprints
are supplied and checked. An incomplete keyring returns a closed readiness or
operation failure.

### Replay classification

The database returns a closed outcome set:

- `applied`;
- `replayed`;
- `drain_pending`;
- `cancelled`;
- `terminal_conflict`;
- `revision_conflict`;
- `idempotency_conflict`;
- `authorization_stale`;
- `scope_mismatch`;
- `persistence_corrupt`;
- `writer_fenced`;
- `indeterminate`.

Success rows contain every required projection field. Failure rows contain no
partial Product, deployment, drain, fence, or receipt projection.

An existing terminal intent without an exact terminal action journal is
`persistence_corrupt`, never an inferred replay.

A journal whose terminal digest, state bytes, deployment result, Product
receipt, or epoch relationship does not validate is
`persistence_corrupt`.

## Crash and concurrency matrix

| Boundary | Durable result |
| --- | --- |
| Before capability dispatch | Acknowledged intent and frozen slot |
| While waiting for any lock | Acknowledged intent and frozen slot |
| Authority or source validation failure | Acknowledged intent and frozen slot |
| Product mutation prepared, transaction rolls back | Acknowledged intent and frozen slot |
| Deployment row updated before terminal intent update, process exits | Transaction rollback; acknowledged intent and frozen slot |
| Terminal intent updated before slot release, process exits | Transaction rollback; acknowledged intent and frozen slot |
| Slot fence cleared before terminal journal insert, process exits | Transaction rollback; acknowledged intent and frozen slot |
| Terminal journal inserted before commit, process exits | Transaction rollback; acknowledged intent and frozen slot |
| Commit succeeds, response arrives | Terminal intent, Product result, successor deployment revision, successor slot epoch |
| Commit succeeds, response is lost | Exact replay returns the original terminal projection |
| Commit succeeds, application process exits | Later exact replay returns the original terminal projection |
| Consume and cancel race | One winner; exact replay or terminal conflict for the loser |
| Two exact consume requests race | One apply and one exact replay |
| Two exact cancel requests race | One cancel and one exact replay |
| Different cancel requests race | One cancel and one idempotency or terminal conflict |
| Runtime writer races before terminal commit | Blocked by the slot advisory lock and frozen fence |
| Runtime writer starts after terminal commit | Must use successor epoch and deployment revision |
| New drain is marked after terminal commit | Old replay observes but never clears the new drain |
| Global writer fence closes first | Terminalization writes nothing |
| Epoch or revision overflow | Terminalization writes nothing and leaves the slot frozen |
| Canonical or symmetry validation fails | Transaction rollback; acknowledged intent and frozen slot |

The application may retry a serialization failure only when PostgreSQL and the
driver prove rollback. Commit-response loss is never classified as a safe
fresh transaction retry; it returns `Indeterminate` and requires exact command
replay.

## Runtime boundary

Runtime responsibilities end at a checked durable
`RouteAbsentAcknowledged` receipt.

After acknowledgement:

- the runtime does not renew the drain claim;
- the runtime does not release the local slot for serving based on claim
  expiry;
- the runtime does not clear the physical slot fence;
- startup recovery observes the Product handoff but does not consume it;
- selectors exclude the frozen slot;
- terminalized rows are valid historical rows and are not recovery
  candidates.

After Product terminalization:

- startup and execution readers accept `Consumed` and `Cancelled` as valid
  terminal history;
- terminal rows do not contribute to pending or acknowledged handoff counts;
- the empty physical pending shape makes the successor epoch eligible for new
  runtime selection;
- every new runtime candidate must bind the current deployment revision and
  successor slot epoch;
- an old acknowledgement, claim, owner, controller fence, seal, or recovery
  permit grants no new authority.

This slice adds no route installation or serving behavior. Reopening actual
serving remains a later admission and convergence composition.

## Migration, ACL, and readiness

### Constraints and indexes

The migration updates:

- `runtime_drain_intents_v2_state_check` to allow `pending`,
  `route_absent_acknowledged`, `consumed`, and `cancelled`;
- the canonical-state exact validator for the two terminal wire shapes;
- the one-frozen-intent-per-slot partial unique index, which continues to
  include only `pending` and `route_absent_acknowledged`;
- slot/drain deferred symmetry;
- startup inventory and product-drain observation classifiers;
- all hard-coded state-constraint definition checks.

Terminal rows remain in the existing natural and Product-operation unique
scopes. A later Product operation is possible because cancellation advances
the deployment revision and uses a new natural scope.

### ACL

- Revoke all privileges on the new journal from `PUBLIC`.
- Revoke all direct journal privileges from application and runtime roles.
- Revoke all private helper execution from non-owner roles.
- Grant consume capability only to the existing apply executor.
- Grant cancel capability and its identity/readiness probe only to the new
  lifecycle-cancellation executor.
- Grant neither capability to the runtime execution role.
- Preserve default ACL closure.
- Require the cancellation executor to be distinct from reader, approval,
  rejection, apply, runtime execution, and migration-owner roles.
- Reject role inheritance, membership, `BYPASSRLS`, superuser, replication,
  and schema-create authority in readiness.

### Readiness

Update every contract whose enumerated object set or definition digest changes:

- Product apply executor readiness;
- new Product lifecycle-cancellation executor readiness;
- overall Product decision boundary readiness and same-database distinct-role
  check;
- runtime interaction schema/readiness when the slot-fence function or trigger
  digest is enumerated;
- runtime exact-target schema/readiness;
- runtime serving schema/readiness;
- runtime execution schema/readiness;
- migration guards for state constraints, indexes, triggers, functions,
  grants, and manifest digests.

Readiness proves:

- both public terminalization functions have exact signatures, owners,
  volatility, strictness, security-definer status, search path, row estimate,
  and ACL;
- private helpers are owner-only;
- the terminal journal is append-only and inaccessible directly;
- all four canonical drain states validate;
- only the two nonterminal Product-handoff states freeze a slot;
- terminalization roles have no excess capability;
- runtime roles cannot terminalize;
- apply and cancellation keyrings cover every live receipt alias;
- schema and readiness functions agree on the complete object inventory.

## Failure classification

Stable application mappings are:

| Database result | Application result |
| --- | --- |
| Exact acknowledged source not yet terminalized | Drain pending or eligible consume |
| Exact consume applied | Ordinary Product apply success |
| Exact consume replayed | Ordinary Product apply success with exact replay |
| Exact cancellation applied | Lifecycle cancellation receipt |
| Exact cancellation replayed | Lifecycle cancellation receipt with exact replay |
| Original apply retried after cancellation | Lifecycle operation cancelled |
| Different terminal action already won | Terminal conflict |
| Product or deployment revision changed | Revision conflict |
| Same idempotency identity, different command | Idempotency conflict |
| Fresh authority invalid or expired | Invalid state or authorization stale |
| Global writer fence closed | Temporarily unavailable |
| Canonical, journal, slot, or symmetry mismatch | Persistence corruption |
| Commit result unknown | Indeterminate |

No failure mapping treats a malformed success projection as a user conflict.
Malformed or partial success projections are backend corruption and leave the
application fail-closed.

## Verification

### Pure controller

- acknowledged to consumed advances the intent revision exactly once;
- consumed binds the exact resulting runtime deployment revision;
- acknowledged to cancelled advances the intent revision exactly once;
- both preserve immutable roots;
- consumed and cancelled canonical bytes round-trip exactly;
- pending, claimed, refenced, consumed, and cancelled sources reject;
- acknowledged results reject;
- revision skips, overflows, timestamp noncanonicality, changed roots, and
  wrong resulting deployment revisions reject;
- terminal-to-terminal transitions reject;
- receipt outcome tags are exact;
- dependency guards permit only the intended new public constructors.

### Application

- apply retry consumes only the exact correlated acknowledgement;
- pending apply returns a structured drain selector;
- apply retry after cancellation returns stable lifecycle-cancelled;
- explicit cancel uses `CancelLifecycle`, never `Reject` or `Apply`;
- authentication, CSRF, tenant, installation, guild, actor, bot, and runtime
  environment mismatches reject before persistence;
- cancellation reason scalar, byte, whitespace, and control-character bounds
  are enforced;
- cancellation result preserves the Product promotion decision;
- fake-port tests prove invalid Product, deployment, drain, epoch, and replay
  projections are rejected;
- key rotation produces the same semantic action and valid aliases;
- changed idempotency command conflicts.

### PostgreSQL semantic tests

- exact consume applies the Product mutation, terminalizes the intent,
  advances the epoch, and clears the fence once;
- exact cancel preserves target and phase, advances deployment revision,
  terminalizes the intent, advances the epoch, and clears the fence once;
- consume and cancel use the same terminal database clock across all rows;
- terminal canonical bytes decode through Rust and re-encode byte-for-byte;
- exact consume replay returns the original receipt after later deployment
  progress;
- exact cancel replay returns the original receipt after later deployment
  progress;
- old replay does not clear a newer frozen intent;
- missing terminal journal, mismatched journal, digest drift, malformed
  canonical bytes, partial fence shape, and incorrect epoch are corruption;
- direct table updates, deletes, truncates, and manual gate settings reject;
- state and fence deferred symmetry holds at commit;
- pending and acknowledged remain unique frozen states;
- consumed and cancelled do not occupy the frozen-slot index;
- cancellation permits a later operation only at the successor deployment
  revision;
- epoch and revision overflow leave the source frozen.

### PostgreSQL concurrency and fault injection

- two exact consumes serialize to one apply and one replay;
- two exact cancels serialize to one cancel and one replay;
- consume versus cancel produces one winner;
- terminalization versus global writer-fence close produces no write after
  close;
- terminalization versus runtime selector requires the selector to observe
  either the frozen old epoch or complete successor state;
- terminalization versus a later Product mutation cannot expose a release
  gap;
- forced failure after each Product, deployment, intent, fence, receipt,
  audit, and journal statement rolls back the entire transaction;
- commit-response loss is recovered by exact replay;
- safe serialization retry is bounded and does not duplicate audit rows;
- statement, lock, and idle-in-transaction timeouts preserve the freeze.

### PostgreSQL security

Under PostgreSQL 16 with real scoped roles:

- apply executor can consume and cannot cancel;
- cancellation executor can cancel and cannot consume ordinary apply work;
- runtime executor can do neither;
- reader, approval, and rejection roles can do neither;
- `PUBLIC` can do neither;
- no executor can select, insert, update, delete, or truncate the journal,
  drain, Product operation, deployment, or slot-fence tables;
- no executor can call owner-only helpers or forge trigger gates;
- role membership, schema creation, function replacement, search-path
  injection, default privileges, and grant-option paths remain closed;
- all readiness probes succeed only for the exact least-privilege topology.

### Runtime integration

- startup observes acknowledged handoff before terminalization;
- startup no longer counts consumed or cancelled rows as unresolved handoff;
- pending and acknowledged rows continue to block selection;
- consumed and cancelled rows permit selection only with successor epoch and
  deployment revision;
- stale pre-freeze and frozen-epoch candidates fail;
- claimant, gateway owner, and recovery permit cannot terminalize;
- no route-present or `PendingRefenced` path is required for a pre-existing
  route-absent acknowledgement;
- all existing startup pending-drain and succession tests remain green.

### Repository gates

- migration rerun and object-collision tests;
- migration guard tests for exact source fragments and digests;
- `cargo test -p automation-runtime-controller`;
- `cargo test -p authoring-application`;
- `cargo test -p authoring-application-discord`;
- `cargo test -p authoring-application-postgres`;
- `cargo test -p automation-runtime-execution-postgres`;
- `cargo test -p automation-runtime-worker`;
- `cargo test -p starring-runtime`;
- `cargo test --workspace`;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
- `cargo fmt --all -- --check`.

## Implementation order

1. Pure controller source types and exact consumed/cancelled receipts.
2. Application drain selector, structured pending result, cancellation
   command, capability, port, and fake-port tests.
3. Discord `CancelLifecycle` authority evidence and distinct digest domain.
4. Additive PostgreSQL terminal journal, canonical validators, state
   constraint, trigger gates, and slot release helper.
5. Atomic consume capability and Product apply adapter integration.
6. Atomic cancellation capability, distinct executor, adapter, and audit
   integration.
7. Runtime observation and selector support for terminal history.
8. PostgreSQL concurrency, crash, ACL, readiness, and migration-guard tests.
9. Full repository gates and disposable-database end-to-end verification.

Each item is a separate functional commit. Consume and cancellation may be
reviewed separately, but neither is production-complete until the shared slot
release, replay journal, security, readiness, and crash matrix are green.

## Acceptance boundary

This slice is complete only when all of the following are demonstrated against
a real PostgreSQL 16 database:

1. a persisted route-absent acknowledgement remains frozen across process
   restarts;
2. an exact Product apply retry consumes it and applies the Product mutation
   in one commit;
3. an explicitly authorized cancellation instead preserves the target and
   phase while advancing deployment revision;
4. either terminal action advances the slot writer epoch and clears the fence
   in that same commit;
5. stale writers from both pre-freeze and frozen epochs are rejected;
6. a lost commit response returns the exact original terminal receipt on
   replay;
7. an old replay cannot affect a newer Product operation or newer drain;
8. runtime and all unrelated application roles cannot terminalize;
9. every crash and concurrency boundary preserves either the complete frozen
   source or the complete terminal successor;
10. all repository, security, readiness, migration, and formatting gates are
    green.
