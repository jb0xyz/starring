# Authoring promotion bridge design

Date: 2026-07-18

Status: implementation in progress

Branch: `feat/authoring-promotion-bridge`

## Outcome

Connect a validated conversational Intent candidate to the existing durable
RuleSet lifecycle without giving the model publication, approval, activation,
Discord, or production-database authority.

The product flow is:

```text
authenticated authoring session
  -> verified PreviewReady artifact
  -> immutable publication journal
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

The authenticated edge supplies:

- tenant and session ownership
- authoritative guild
- stable product-owned automation installation ID and RuleSet key
- authenticated requester
- explicit idempotency key
- server-owned policy revision, quorum, and expiry

The model and request body do not supply those values.

The workflow journal is created before publication and advances monotonically:

```text
Prepared
  -> Published
  -> ActivationPending
  -> ActivationAppliedRuntimePending
  -> Live

Prepared | Published
  -> Cancelled

ActivationPending
  -> Rejected | Withdrawn | Expired | Superseded
```

Each transition uses a revision CAS. Retry resumes from the durable state.

The immutable record binds:

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

## Idempotency

The endpoint idempotency key is bound to tenant, principal, session generation,
guild, installation, and server policy through a domain-separated digest.

The activation request ID is a full lowercase SHA-256 hexadecimal digest derived
from the promotion identity. An exact retry loads and compares the complete
immutable request. A different candidate, actor, target, binding, baseline, or
policy under the same idempotency key is a hard conflict.

Rejected, expired, cancelled, or superseded workflows require a new explicit
idempotency key. Apply and resume always use fresh attempt IDs.

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
- fresh readiness findings
- active baseline and intended replacement
- approval policy and expiry

Its canonical digest is persisted in both the promotion workflow and activation
request. Apply requires them to match.

Fresh readiness is shown before approval for usability and repeated during apply
for safety.

## Boundary 4: runtime convergence

Pointer activation yields `ActivationAppliedRuntimePending`. The first
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

A convergence failure never reports deployment success. The active pointer may
already have changed, so the state remains
`ActivationAppliedRuntimePending` and retry targets the same immutable artifact.

Hot swap is introduced only after top-level custom IDs or a dual-version route
table pin every click to a RuleSet version throughout panel transition, plus an
atomic runtime snapshot abstraction and concurrency tests. Reconciliation must
never expose a new panel to an old unversioned top-level dispatcher.

## Product API authority

The future API derives requester, approver, applied-by actor, guild membership,
and guild authorization from an authenticated principal. IDs in request bodies
are never trusted.

The API exposes separate actions:

```text
POST promotion
GET promotion and approval preview
POST approval
POST rejection
POST apply
GET runtime convergence status
```

No endpoint combines promotion, approval, and apply.

## Failure matrix

| Failure | Durable result | Pointer mutation | Retry |
| --- | --- | --- | --- |
| Candidate export invalid | none | none | new valid preview |
| Workflow create conflict | existing or conflict | none | exact replay only |
| Publish rejected | Prepared | none | after correction |
| Crash after publish | Prepared or Published | none | reuse artifact |
| Activation request failure | Published | none | resume |
| Approval rejected, withdrawn, or expired | terminal | none | new promotion key |
| Binding drift | Superseded | none | new preview |
| Active baseline drift | Superseded | none | new preview |
| Readiness failure | Approved | none | after environment fix |
| Lease loss before mutation | Applying or safe error | none | recover/resume |
| Indeterminate activation | Applying | unknown | recover only |
| Runtime convergence failure | ActivationAppliedRuntimePending | applied | same target |

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
- activation followed by exact hydration and panel reconciliation acknowledgement
- no publish, approve, apply, Discord, or database tool visible to the model

PostgreSQL tests run serially and cover reconnect, concurrent identical
publication, crash-resume state transitions, approval/apply CAS, supersession,
and runtime-pending recovery.

CI retains workspace tests, clippy, formatting, Promptfoo static checks, and the
existing PostgreSQL job. Live Luna and disposable Discord checks remain
separate evidence and never run in ordinary CI.

## Commit sequence

1. Design and verified authoring artifact export.
2. Pure publication workflow and in-memory tests.
3. PostgreSQL workflow journal and retry tests.
4. Activation approval context, binding and baseline preconditions.
5. Authenticated application edge.
6. Runtime convergence controller and integration tests.
7. Live disposable-guild evidence and current-state handoff.

Each commit preserves a green scoped gate. The branch is not merged until the
complete workspace, PostgreSQL, clippy, formatting, and safety guards pass.
