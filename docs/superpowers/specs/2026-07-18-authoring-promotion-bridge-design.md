# Authoring promotion bridge design

Date: 2026-07-18

Status: core bridge implemented; production edge and runtime convergence pending

Branch: `feat/authoring-promotion-bridge`

> Product approval update (2026-07-31): the distinct-approver and requester
> self-approval prohibitions in this historical design are superseded by
> `2026-07-31-solo-product-approval-design.md`. Product approval requires
> exactly one authenticated manager and permits the requester to approve. All
> other promotion, payload-binding, Apply, and runtime safety invariants remain
> in force.

## Outcome

Connect a validated conversational Intent candidate to the existing durable
RuleSet lifecycle without giving the model publication, approval, activation,
Discord, or production-database authority.

The target product flow is:

```text
authenticated authoring session
  -> verified PreviewReady artifact
  -> monotonic promotion journal with immutable identity and publication payload
  -> immutable RuleSet version
  -> approval payload
  -> pending activation request
  -> distinct authenticated approval
  -> activation precondition and readiness checks
  -> active pointer
  -> runtime hydration and panel reconciliation
  -> live acknowledgement
```

An activation pointer change is not a live acknowledgement. The product reports
`Live` only after the runtime has hydrated the exact active artifact and panel
reconciliation has completed.

## Invariants

- The model cannot publish, approve, activate, select a guild, select a durable
  RuleSet key, choose an actor, choose an approval policy, or call Discord.
- `design-harness` remains pure and has no `sqlx`, Twilight, PostgreSQL adapter,
  activation, or runtime dependency.
- A publishable artifact can be constructed only from a fully validated
  `PreviewReady` Intent session using its original authoritative bindings.
- Candidate RuleSet identity and registry content identity remain distinct and
  are both retained.
- Publication never changes the active pointer.
- Promotion never auto-approves or auto-applies.
- Requester self-approval remains forbidden.
- Approval binds the exact artifact, authoring evidence, resource bindings,
  active baseline, and server policy.
- Binding or baseline drift requires a new preview and approval.
- Known failures do not mutate the active pointer. Indeterminate mutation
  outcomes remain recoverable and are never blindly retried.
- Existing instances continue dispatching against their pinned RuleSet version.

## Current truths

`Draft.ruleset` already is `InteractionRuleSet`, which is the exact type accepted
by `PublishRuleSetRequest.definition`. The bridge performs a typed clone and no
JSON conversion.

`candidate_ruleset_hash` is an authoring identity with the
`starring.intent.candidate_ruleset.v1` domain. `RuleSetContentHash` includes the
registry schema version and definition. Equal-looking hexadecimal strings do
not have equal semantics and must never be substituted.

`ActivationService::apply` currently changes the active PostgreSQL pointer.
The gateway holds its top-level RuleSet by value from startup, while pinned
instance actions load their exact version from the registry. Runtime hydration
and declared-panel reconciliation currently happen only on gateway startup.

## Boundary 1: verified authoring artifact

`design-harness` exports an owned `PreviewReadyArtifactV1`. It is neither
serializable nor deserializable and its fields are private, so external code
cannot manufacture one from client bytes or accidentally use it as a durable
wire format. Its custom `Debug` output redacts the RuleSet and preview.

The export operation:

1. Requires Intent Recipe mode.
2. Runs the existing full snapshot validator with the original bindings.
3. Requires `PreviewReady`.
4. Requires candidate, validated, and simulated revisions to match.
5. Recomputes the candidate RuleSet and Draft identities.
6. Revalidates transcript, evidence, route, compiler, and stage bindings.
7. Returns an owned RuleSet, preview, receipt, requested outcome, versioned
   authoring-contract descriptor, binding fingerprint, external binding keys,
   and stage binding digest.

The recipe ID, version, selected-descriptor digest, and registry digest are read
from the persisted recipe evidence already covered by the PreviewReady stage
binding. Full snapshot validation proves that evidence still matches the active
closed registry before export. Compiler and simulator revisions are read from
the descriptor whose digest was just verified; they are not relabeled from an
unverified current contract.

The artifact remains design-time data. It contains no guild authority,
authenticated principal, durable key, approval policy, or deployment method.
The first promotion service accepts only `ValidatedPreview`. A `WorkingDraft`
must be explicitly upgraded through a new validated-preview authoring turn.

## Boundary 2: publication workflow

A new pure `authoring-promotion` crate owns the workflow domain and orchestration
traits. A paired `authoring-promotion-postgres` crate owns durable storage.

The planned authenticated edge will supply:

- tenant and session ownership
- authoritative guild
- stable product-owned automation installation ID and RuleSet key
- authenticated requester
- explicit idempotency key
- server-owned policy revision, quorum, and expiry

The model and request body do not supply those values.

The implemented application contract passes the same expected generation to an
owned-artifact port and a server-authority port in sequence. Those ports are
responsible for owner and generation validation. The product activation bridge
later resolves and validates the fresh binding revision and fingerprint before
constructing the activation context. The intended production route never accepts
a client-submitted authority context or an arbitrary artifact/context pairing;
its durable adapters must provide one atomic authorized-session snapshot or an
equivalent generation-bound guarantee.

The implemented workflow journal is created before publication and advances
monotonically:

```text
Prepared
  -> Published
  -> ActivationPending

Published | ActivationPending
  -> Expired
```

Approval, rejection, withdrawal, supersession, apply completion, runtime
pending, and `Live` convergence remain authoritative on the activation side or
future workflow stages. They must be synchronized without weakening the
existing activation CAS before the product reports a complete lifecycle.

If an exact activation request is created but the journal CAS is interrupted
until that request expires, recovery records `Published -> Expired` with the
reused request identity and an update timestamp at or after its expiry. The
later activation synchronization boundary owns ordinary
`ActivationPending -> Expired` convergence.

Each transition uses a revision CAS. Retry resumes from the durable state.

The monotonic record has immutable identity and publication payload fields and
binds:

- promotion ID and idempotency digest
- tenant, owner, session ID, session generation, and candidate revision
- complete Intent receipt identities
- authoring artifact, protocol, identity, extractor, normalizer, compiler, and
  simulator revisions
- recipe ID/version, descriptor digest, registry digest, requested outcome, and
  stage binding digest
- candidate RuleSet and Draft hashes
- authoritative guild and RuleSet key
- binding revision and fingerprint
- published version and registry content hash
- publication disposition
- activation request ID
- approval payload digest
- active baseline
- policy revision, quorum, and expiry
- authenticated requester and timestamps

Publication failure leaves the workflow `Prepared` and creates no activation
request. A crash after publication is safe because retry reuses identical
registry content. Activation-request failure leaves an inactive published
artifact and a resumable `Published` workflow.

`ProductActivationBridge` now adapts the pure workflow to the existing RuleSet
registry and activation request store. A product-authored request is created
`Unlinked`; approval and apply reject it until the promotion journal reaches the
exact `ActivationPending` record. The bridge re-reads and validates that record
immediately before linking. PostgreSQL independently enforces the same request,
target, requester, policy, payload, and approval-context identities in a trigger
for every insertion or transition into the linked product state. Product
authority and link identities become immutable after insertion, closing
unauthorized direct linked-insert bypasses, authority relabeling, and
linked-context rewrites. Request-ID
convention alone is not an authorization gate. Recovery safely completes a
journaled but unlinked request, while a request without the journal remains
unapprovable.

The durable activation observation field is canonically
`request_state_at_journal`. A compatibility migration rewrites the earlier
branch-local `request_state_at_link` spelling, and deserialization retains an
alias so an interrupted rollout can still recover old records.

## Idempotency

The endpoint idempotency scope digest binds the raw key to the tenant,
authenticated principal, and versioned promotion endpoint domain. The separate
exact request digest binds that scope to the session generation, candidate,
guild, installation, RuleSet key, bindings, and server policy. Reusing the same
raw key for any different request under that tenant and principal is therefore
a hard conflict rather than a second workflow.

The activation request ID is a full lowercase SHA-256 hexadecimal digest derived
from the promotion identity. An exact retry loads and compares the complete
immutable request. A different candidate, actor, target, binding, baseline, or
policy under the same idempotency key is a hard conflict.

After activation rejection, withdrawal, expiry, or supersession, a new promotion
attempt requires a new explicit idempotency key. Apply and resume always use
fresh attempt IDs. Rejection, withdrawal, and supersession are currently
activation-authority terminal states rather than promotion journal stages.

## Boundary 3: approval context and activation preconditions

`ActivationTarget` continues to identify only the immutable registry artifact.
An `ActivationApprovalContext` separately binds the product decision:

```text
publication ID
approval payload digest
binding fingerprint
baseline policy
expected active baseline
policy revision
```

Legacy/manual requests use an explicit `Unconstrained` baseline. Product
authoring requests must use `Unchanged` with either `Absent` or the exact active
version and content hash observed when the approval payload was rendered.

The canonical resource-context fingerprint algorithm and typed fingerprint live
in the neutral pure `resource-resolution` crate. The authoring V2 fingerprint
preserves its existing identity over the complete binding catalog. The approval
binding adds an authoritative guild, binding revision, and the exact required
`(kind,key,id)` projection, so unrelated catalog additions do not invalidate an
approval and required-resource changes always do.

Cancellation after an activation request exists is an activation-authority
transition, not only a promotion-journal update. A requester withdrawal moves a
Pending or Approved request to terminal `Withdrawn` with a state CAS. It cannot
withdraw an Applying request. Supersession is likewise persisted on the
activation request. Apply therefore cannot proceed merely because a detached
promotion row still exists.

After claiming the per-RuleSet apply lease, the activation authority:

1. Re-fetches and verifies the exact target artifact.
2. Confirms the request has not been withdrawn or superseded.
3. Reads the current active artifact.
4. Accepts an already-active exact target only after bound context validation.
5. Rejects a changed product baseline as terminal `Superseded`.
6. Loads a fresh environment under the service-controlled deadline.
7. Recomputes and checks the approval binding fingerprint.
8. Marks binding drift terminal `Superseded`.
9. Re-runs the existing readiness gate.
10. Renews the lease immediately before pointer mutation.
11. Uses the existing guarded activation operation.

Readiness failures that leave the pointer unchanged return to `Approved` for a
safe retry. Context or baseline drift cannot be retried under the old approval.
Timeouts or indeterminate store outcomes remain `Applying` for recovery.

## Approval payload

The approval payload is rendered from persisted server data, not model prose or
client-submitted RuleSet bytes. It includes:

- guild and stable automation identity
- authoring summary and exact structural preview
- authoring candidate identity
- published version and content hash
- resolved external resources
- active baseline and intended replacement
- approval policy and relative expiry

Its canonical digest is persisted in both the promotion workflow and activation
request, and journal linking requires those values to agree. Apply subsequently
verifies the immutable activation context and approved digest; it does not
re-read the promotion journal.

Fresh readiness is repeated during apply for safety. Persisting a pre-approval
readiness summary and displaying the absolute activation-request expiry remain
production-surface work; neither is currently part of the canonical payload.

## Planned Boundary 4: runtime convergence

This boundary is not implemented in the current checkpoint. The planned design
has pointer activation yield `ActivationAppliedRuntimePending`. Its first
convergence implementation uses a controlled restart and does not hot-swap. It:

1. Stops accepting and dispatching top-level interactions.
2. Drains already accepted work under a bounded deadline and terminates the old
   runtime.
3. Hydrates the exact active version through the existing readiness gate.
4. Reconciles declared panels for that exact version while dispatch is stopped.
5. Starts the new runtime with the exact identity, RuleSet, and bindings.
6. Confirms a runtime attestation containing the exact guild, key, version,
   content hash, binding fingerprint, process instance, and panel-reconciliation
   generation.
7. Records the panel reconciliation result and runtime attestation.
8. Marks the promotion `Live`.

Under the planned contract, a convergence failure never reports deployment
success. The active pointer may already have changed, so the state remains
`ActivationAppliedRuntimePending` and retry targets the same immutable artifact.

Hot swap is introduced only after top-level custom IDs or a dual-version route
table pin every click to a RuleSet version throughout panel transition, plus an
atomic runtime snapshot abstraction and concurrency tests. Reconciliation must
never expose a new panel to an old unversioned top-level dispatcher.

## Product API authority

The pure `authoring-application` contract derives promotion requester, guild,
installation, RuleSet key, binding revision, and policy from a verified
principal plus server-owned session and authority ports. Its client command
contains only an idempotency key, session ID, and expected generation. The
future HTTP API must supply the real authentication adapter and derive approver
and applied-by actors, guild membership, and guild authorization without
trusting IDs in request bodies.

`VerifiedPrincipalV1` is an in-process trust assertion, not cryptographic
authentication, and the raw workflow service remains a public core API. The
production composition must expose only the authenticated application route,
must keep direct workflow submission unreachable from transport handlers, and
must resolve the owned artifact and authority from one durable generation or an
equivalent atomic snapshot.

The planned API exposes separate actions:

```text
POST promotion
GET promotion and approval preview
POST approval
POST rejection
POST apply
GET runtime convergence status
```

No planned endpoint combines promotion, approval, and apply.

## Failure matrix

| Failure | Durable result | Pointer mutation | Retry |
| --- | --- | --- | --- |
| Candidate export invalid | none | none | new valid preview |
| Workflow create conflict | existing or conflict | none | exact replay only |
| Publish rejected | Prepared | none | after correction |
| Crash after publish | Prepared or Published | none | reuse artifact |
| Activation request failure | Published | none | resume |
| Approval rejected or withdrawn | activation terminal; promotion sync pending | none | new promotion key |
| Approval expired | activation terminal; promotion records expiry when observed | none | new promotion key |
| Binding drift | Superseded | none | new preview |
| Active baseline drift | Superseded | none | new preview |
| Readiness failure | Approved | none | after environment fix |
| Lease loss before mutation | Applying or safe error | none | recover/resume |
| Indeterminate activation | Applying | unknown | recover only |
| Planned runtime convergence failure | ActivationAppliedRuntimePending | applied | same target |

## Verification

The deterministic suite covers:

- export only from valid `PreviewReady` and reject live Draft drift
- artifact ownership independent of later session mutation
- created and reused publication with both identities retained
- no activation request on publish failure
- inactive artifact and safe resume on request failure
- exact idempotent replay and mismatched replay conflict
- request target pinned after later publications
- self-approval rejection and distinct approval requirement
- binding and baseline supersession before pointer mutation
- readiness failure preserving the prior active pointer
- guarded activation through the exact product-bound request
- no publish, approve, apply, Discord, or database tool visible to the model

PostgreSQL tests run serially and cover reconnect, concurrent identical
publication, crash-resume state transitions, exact journal-link enforcement,
product-bound approval, guarded apply, and active-pointer mutation. Runtime
convergence remains outside this checkpoint.

CI retains workspace tests, clippy, formatting, Promptfoo static checks, and the
existing PostgreSQL job. Live Luna and disposable Discord checks remain
separate evidence and never run in ordinary CI.

## Commit sequence

1. Design and verified authoring artifact export.
2. Pure publication workflow and in-memory tests.
3. PostgreSQL workflow journal and retry tests.
4. Activation approval context, binding and baseline preconditions.
5. Pure authentication-ready application boundary.
6. Runtime convergence controller and integration tests.
7. Live disposable-guild evidence and current-state handoff.

Each commit preserves a green scoped gate. The branch is not merged until the
complete workspace, PostgreSQL, clippy, formatting, and safety guards pass.
