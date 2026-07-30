# Solo product approval design

Date: 2026-07-31

Status: accepted implementation contract

Supersedes for the Product control path:

- the second-approval verdict in
  `2026-07-09-policy-engine-design.md`
- the two-person approval state in
  `2026-07-09-approval-manager-design.md`
- the second-approval preview mapping in
  `2026-07-09-preview-model-design.md`
- the distinct-approver requirement in
  `2026-07-12-approval-bound-activation-18f-design.md`
- the requester self-approval prohibition in
  `2026-07-18-authoring-promotion-bridge-design.md`
- the distinct-manager and requester self-approval requirements previously
  stated in `2026-07-19-production-control-api-runtime-convergence-design.md`

All safety boundaries in those documents remain in force unless this document
explicitly changes them.

## Outcome

Starring Product approval requires exactly one authenticated Discord manager.
The manager who authored and promoted a candidate may also approve it and may
perform the later Apply action.

The canonical flow is:

```text
authenticated manager authors a request
  -> AI proposes a candidate
  -> deterministic validation and simulation
  -> immutable inactive RuleSet publication
  -> server-rendered approval preview
  -> one payload-bound manager approval
  -> separate guarded Apply
  -> runtime convergence and Live attestation
```

The requester remains the authenticated human who initiated the promotion. The
AI is an untrusted design-time proposer and is never recorded as the requester,
approver, or Apply actor.

## Product approval contract

- Product `required_approvals` is always exactly `1`.
- The requester may provide that approval.
- A different manager may provide the approval, but a second manager is never
  required.
- There is no risk-based, action-based, installation-based, or high-risk mode
  that raises the approval count.
- There is no two-administrator mode and no customer-configurable approval
  quorum.
- Promotion does not imply approval. Approval remains an explicit mutation
  after the server-rendered preview is available.
- Approval does not imply Apply. Apply remains a separate explicit mutation
  with its own fresh authorization and precondition checks.
- A second approval cannot accumulate after the request reaches `Approved`.
  Concurrent attempts converge through the existing state and revision compare
  and swap.

The persisted `required_approvals` field remains in Product records, approval
payloads, digests, projections, and audit evidence for schema and identity
compatibility. Its only valid Product value is `1`.

Historical generic activation or legacy executor types do not define Product
approval policy and must not be routed into the Product control path. In
particular, a legacy second-approval verdict cannot raise Product approval
cardinality.

## Preserved safety boundaries

Solo approval changes who may approve, not what can be approved or how Apply is
authorized. The following controls remain mandatory:

- The model has no publication, approval, Apply, deployment, PostgreSQL, or
  Discord mutation authority.
- Only a fully validated and simulated `PreviewReady` artifact can enter
  promotion.
- Publication creates or reuses an immutable inactive RuleSet version and never
  changes the active pointer.
- The approval preview is rendered from the exact server-owned promotion,
  target, bindings, baseline, and policy.
- Approval is bound to the exact payload digest and expected product revision.
- Every Product mutation requires an authenticated Starring session, exact
  Origin and CSRF proof, a bounded request, and an idempotency key.
- Approval and Apply each require a fresh server-side Discord authority
  observation. Guild owner, `ADMINISTRATOR`, or `MANAGE_GUILD` satisfies the
  manager policy; ordinary, removed, or stale members fail closed.
- Binding, active-baseline, policy, target, lifecycle, expiry, and revision
  drift fail closed before pointer mutation.
- Approval, receipt, idempotency alias, audit, Apply, deployment, and runtime
  evidence remain durable and tenant-scoped.
- Direct table mutation remains unavailable to API and runtime roles. Narrow
  guarded database procedures retain append-only and compare-and-swap
  enforcement.
- A deployment is not reported Live until exact runtime hydration, panel
  reconciliation, attestation, and a fresh serving lease succeed.

The accepted tradeoff is that compromise of one authorized manager session can
exercise that manager's Product authority without a second human. Session
lifetime, revocation, CSRF, fresh Discord authority, exact preview binding,
deterministic validation, idempotency, and audit evidence are the compensating
controls. Two-person separation of duties is not a Starring Product security
boundary.

## Persistence and migration contract

The transition is append-only:

- Existing migration files and immutable approval evidence are not rewritten.
- New Product authority and Product activation state reject
  `required_approvals` values other than `1`.
- Requester approval is valid Product evidence and must not be classified as
  persistence corruption by approval, read, replay, repair, or Apply paths.
- Existing Product rows with unsupported multi-approval state are not silently
  normalized. Migration or startup fails closed and requires explicit operator
  handling.
- Function signatures, HTTP routes, request bodies, response shapes, digest
  fields, and revision sequencing remain compatible.

A pre-launch pending promotion may be reused after this contract is deployed
only when it is unexpired, has `required_approvals = 1`, has no recorded
approval, and its payload digest, revision, authority, binding, baseline, and
target remain exact. A failed pre-contract self-approval attempt that committed
no receipt, audit event, or approval row does not require a new promotion.

An expired, rejected, superseded, already-decided, or otherwise mismatched
promotion is never repaired by mutating its immutable decision evidence. The
same validated authoring generation may be promoted again through the official
Product API with a new idempotency key.

## Acceptance evidence

The implementation is complete only when all of the following are proven:

- The requester can approve their exact payload-bound Product promotion.
- That single approval advances `PendingApproval` to `Approved`.
- The approval row, mutation receipt, idempotency evidence, and audit event are
  each persisted exactly once.
- Exact replay returns the prior result without another approval or audit event.
- A second or concurrent approval cannot create more than one accepted Product
  approval.
- Product authority, promotion, activation, and HTTP projections reject an
  approval count other than `1`.
- A requester-approved promotion survives read, replay, repair, restart, and
  Apply without being classified as corrupt.
- Wrong digest, stale revision, expired request, stale Discord authority,
  ordinary membership, invalid CSRF, cross-tenant identity, and changed
  binding, baseline, policy, or target continue to fail closed.
- The same authenticated manager can complete preview, approval, and Apply,
  after which runtime convergence reaches Live only with exact attestation and
  a fresh serving lease.

## Product UX requirement

Solo approval is not automatic approval. A production client must render the
server-owned preview and require an explicit human approval action before
Apply. A staging console script that reads the preview and immediately submits
approval and Apply is smoke-test tooling, not evidence that a human reviewed
the preview.
