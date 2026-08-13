# StatefulSpec R0

`StatefulSpec R0` is the first recipe-independent contract for automations that remember bounded
state between interactions. It is intentionally a pure authoring, validation, and simulation
foundation. It is not a live deployment surface yet.

The model may propose a document while a user is authoring an automation. The model never receives
runtime credentials and cannot execute code, query state, call Discord, or commit a transition.
The platform owns validation, canonical identity, event authority, state resolution, execution
planning, and persistence.

## Separate execution contract

StatefulSpec is a new wire contract with the exact identity `schema_version: 1` and
`kind: "starring.stateful-spec.v1"`. It does not add optional state fields to AutomationSpec V1 and
does not reinterpret an existing immutable RuleSet.

The current pure immutable compiler bundle has two disjoint targets:

1. panels, modals, and explicitly stateless workflows in a filtered legacy RuleSet;
2. stateful workflows in a separately versioned immutable stateful execution artifact.

A stateful workflow must never be lowered into an unconditional legacy rule. A runtime that does
not understand the stateful artifact therefore finds no matching legacy rule and performs no
business effect. This fail-closed behavior is required before live activation can be added.

The bundle binds the canonical source digest, the official filtered legacy RuleSet identity, the
compiled state schema, the separate stateful artifact, and their union source map. A decoder trusts
none of the generated JSON: it validates and recompiles the embedded source, then requires an exact
canonical match. This is artifact compilation only. Publication, promotion, approval, Apply,
state persistence, dispatch, and runtime activation remain unavailable for StatefulSpec documents.

## Closed state and expression surface

State variables use a closed type set:

- Boolean;
- bounded signed integer;
- bounded UTF-8 text.

Variables are scoped to installation, actor, instance, or actor plus instance. Tenant,
installation, guild, actor, and instance identities are derived from an authoritative durable
interaction receipt. They are never accepted from a model document or an HTTP simulation field as
live state authority.

Expressions are typed and bounded. They may read literals, normalized modal input, and declared
state; compare values; perform checked integer addition or subtraction; and combine Boolean
conditions. There is no floating point, implicit coercion, wall clock, randomness, network access,
loop, recursion, arbitrary code, or event-time model call.

Each stateful workflow has a trigger, a condition, and explicit true and false branches. A branch
contains bounded state assignments, bounded existing interaction effects, and one response. The
runtime acknowledgement strategy is fixed: a durable ephemeral defer precedes state planning and a
single response edit is the final effect.

All assignment right-hand sides read one immutable pre-state snapshot. Assignments therefore have
parallel semantics, not order-dependent mutation semantics. The same variable cannot be assigned
twice in one branch. Checked arithmetic overflow, a type mismatch, or an out-of-bound value rejects
the plan before any state or business effect is committed.

## Deterministic simulation

Simulation uses the same modal normalization and pure evaluator as the contract. Missing fixture
cells resolve to declared defaults. It reports the selected workflow and branch, normalized input,
state before and after, ordered transitions, external authored node IDs, and one separately
domain-separated simulation trace digest.

A simulation trace is not a live execution plan. It has no authoritative receipt, state row
revisions, Discord preflight snapshot, or effect journal binding, so it never exposes or claims a
live plan digest.

## Authoritative event boundary

The live event envelope begins only after the gateway has durably claimed an interaction. A
token-free verified request proof recomputes the canonical request digest from the exact Discord
application and interaction identity, guild, channel, actor, locale, custom ID, and raw modal
inputs, then compares it with the digest in the authoritative receipt claim.

The semantic trigger is derived from that verified custom ID and must match the exact static or
instance route, guild, RuleSet key, and instance identity. Normalized inputs are derived from the
pinned StatefulSpec modal definition. Callers cannot independently substitute an actor, trigger,
instance, or normalized input map.

The resulting internal event envelope binds the receipt request digest, deployment and route
identity, gateway and serving fences, actor and scope, pinned program identity, derived trigger,
and normalized inputs under a separate domain-separated digest. Raw interaction tokens are never
included.

## State transition and durable dispatch boundary

A missing state row means the declared default at logical revision zero. Stored rows start at
revision one and revisions are monotonic. An explicit assignment advances the revision even when
the typed value is unchanged, so concurrent events cannot both pass a compare-and-set guard and
emit duplicate business effects.

The eventual live persistence operation must use the existing interaction database and commit all
of the following in one database transaction:

- the full authoritative read set and compare-and-set result;
- state heads and immutable transition events;
- the receipt-bound stateful execution-plan digest;
- the existing action/effect plan and complete durable dispatch payload;
- a queued outbox head and immutable outbox event.

Splitting state and effects across transactions is not permitted. A crash must never leave accepted
state without a recoverable effect plan or an effect plan whose state decision was not accepted.
An ambiguous commit is resolved by replaying the exact request and reading the immutable receipt;
state transitions are never blindly applied a second time.

That atomic commit proves event acceptance and recoverable dispatch, not that Discord has already
applied a role, channel, permission, panel, instance, or response mutation. External effects finish
asynchronously through the journal. A definitive external failure or operator-recovery state does
not automatically roll back accepted state in R0. Future previews must state this explicitly so a
counter or balance is never presented as proof that the corresponding Discord mutation succeeded.

The outbox protocol uses explicit claimant identity, gateway and serving fence snapshots, monotonic
head and claim revisions, bounded leases, availability time, and stale-token rejection. It
distinguishes queued work, active claims, effects requiring observation, completed work, and
operator recovery. An indeterminate external effect is observed through the existing effect
journal recovery contract and is never blindly invoked again.

Only the durable defer acknowledgement may occur before the combined state/effect commit. No role,
channel, permission, panel, instance, teardown, or response-edit business mutation may occur before
that boundary.

Once the durable defer succeeds, every rejection before the combined commit—including input
normalization, evaluation, preflight, exhausted compare-and-set retries, quota, or revision
failure—must leave a receipt-fenced durable failure response tail. That tail edits the deferred
response with bounded, redacted content and carries no state transition or business mutation. A
definitive defer failure prevents state planning; an indeterminate defer is resolved by the existing
initial-response recovery contract before any business commit is allowed. This prevents a rejected
workflow from leaving Discord indefinitely in a pending state.

## R0 limits and exclusions

R0 keeps every graph, value, read set, write set, effect plan, and persistence document bounded.
It intentionally excludes timers, schedules, cooldown clocks, TTL, arbitrary HTTP, connector
secrets, runtime model calls, user code, unbounded collections, and schema-changing migrations.

The current exact pure-contract limits are:

| Boundary | R0 maximum |
| --- | ---: |
| Canonical StatefulSpec document | 64 KiB |
| Stateless plus stateful workflows | 32 |
| State variables | 64 |
| Authored nodes in one branch | 64 |
| State assignments in one branch | 32 |
| Stateful nodes in one spec | 512 |
| Condition depth / nodes per condition | 8 / 64 |
| Value-expression depth / nodes per expression | 8 / 64 |
| Integer domain | ±9,007,199,254,740,991 |
| One text value | 4,000 UTF-8 bytes and 4,000 UTF-16 units |
| Simulation cells | 64 |
| Canonical simulation fixture / total request | 64 KiB / 128 KiB |
| Canonical simulation trace | 8 MiB (8,388,608 bytes) |
| Canonical compiled state schema | 128 KiB |
| Canonical stateful artifact | 512 KiB |
| Canonical union source map | 512 KiB |
| Canonical compilation binding | 64 KiB |
| Canonical immutable compiler bundle | 2 MiB |
| Live state read set / write set | 64 / 32 |
| Reference-plan external actions | 64 |
| Reference-plan state material | 1 MiB |
| Reference outbox payload | 2 MiB |
| Reference outbox scan page | 256 entries |
| Reference outbox claim lease | 5 minutes |

The final five rows describe the current non-integrated reference protocol, not a live service
promise. They are exported beside its opaque prepared-commit and lease types. A future database
adapter must preserve or explicitly version these bounds and may not silently substitute larger or
unbounded limits.

Schema evolution for one program key is additive only in R0. Existing variable ID, scope, type,
bounds, and default must remain byte-exact. Rename, removal, ID reuse, type or scope change,
default change, downgrade, and silent reinterpretation fail closed.

The pure runtime scaffold now includes a typed evaluation proof. It derives the event/program only
from an immutable compiler bundle plus an opaque, control-plane publication binding; derives the
complete snapshot request from compiler dependency indices across both branches; and binds exact
bundle, binding, source-map, artifact, schema, claim/event, workflow, selected branch, actual read
revisions and values, source ordinals, parallel writes, and ordered external-node projection. The
shared spec simulator and runtime proof call the same transport-cap-free pure evaluation core, so
the 64 KiB simulation fixture limit cannot strand a valid bounded runtime snapshot. A verified
interaction route alone deliberately cannot construct the publication binding, and its only R0
stand-in is test-only.

This proof still cannot deploy or execute. The next milestone is the integration proof that binds
the typed evaluation projection to the Discord preflight certificate and shared typed effect
journal, then composes the durable defer result and mandatory failure-response tail. It must expose
no arbitrary bytes or caller-supplied digests. Only after that boundary is complete can a tested
composite PostgreSQL commit and leased dispatch adapter be added to the existing interaction
database. Live activation remains blocked until those boundaries, durable control-plane
publication/activation pins for the complete bundle identity, capability attestation, crash
recovery, and contextual Apply-time preview are implemented and certified.
