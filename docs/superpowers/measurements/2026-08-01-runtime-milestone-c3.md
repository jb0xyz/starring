# Runtime milestone C3 source hardening evidence

Date: 2026-08-01 KST

Status: Phase C functional source hardening evidence, not deployment or
commercial runtime certification

## Scope

This checkpoint completes the source implementation of durable Discord
interaction effects after the C1 duplicate-receipt boundary and the C2 complete
action-plan preflight boundary. It covers:

- a deterministic per-action effect model and canonical digests
- a PostgreSQL root, effect-head, event, and rollback journal
- journaled foreground execution before and after every external action
- action-specific observation of indeterminate Discord results
- exact reverse-order compensation with preserved preimages
- durable response-tail recovery
- static-route and instance-route unsafe-recovery admission barriers
- a bounded production recovery supervisor

The model remains absent from event-time and recovery-time execution. The
source candidate was not deployed to the retained staging fixture for this
checkpoint, and no live Discord failure was injected. Those are Phase D gates.

## Safety boundary

The runtime prepares and binds the complete effect plan before the first
mutation. A mutation-bearing plan must begin with `DeferEphemeral`, contain no
direct response or modal action, and end with exactly one `EditResponse` tail.
Every effect after index zero depends on its immediate predecessor. These
constraints make the mutable prefix and response tail deterministic and make
reverse compensation ordering explicit.

Each external action follows a durable intend, external call, and durable
result sequence. PostgreSQL transactions and advisory locks end before any
Discord HTTP request starts. A known success records its exact output. A known
failure records a closed failure. An indeterminate result enters observation;
it is never blindly replayed.

Observation can adopt a result only when the action-specific contract proves
one unique correlation identity, the exact target and actor identity, and the
expected postimage. Names and mutable attributes are never sufficient.
`PostPanel` has no safe exact correlation in the current Discord contract, so
an indeterminate panel create remains `recovery_required` rather than being
adopted or replayed.

Compensation runs in reverse dependency order. Deletion, grant removal, and
overwrite restoration use the exact journaled identity and preimage. Foreground
instance teardown also rechecks the full registration identity immediately
before the first delete, including RuleSet key, version, kind, creator, and
resource manifest. Identity drift produces zero delete calls. Restart recovery
adopts an absent teardown target as the exact terminal postimage; if a target is
present but its complete registration identity is unavailable, it records a
conflict instead of matching only the instance ID or resource manifest.

## Effect observation and compensation matrix

| Effect | Exact observation | Automatic compensation |
| --- | --- | --- |
| Create role | audit-log correlation plus exact role postimage | delete the exact created role |
| Create channel | audit-log correlation plus exact channel postimage | delete the exact created channel |
| Grant role | audit-log correlation plus exact membership | restore the exact prior membership |
| Upsert overwrite | audit-log correlation plus exact overwrite postimage | restore the exact recorded preimage |
| Post panel | no supported exact create correlation | none after an indeterminate result |
| Register instance | exact internal idempotency and semantic identity | exact identity-complete teardown |
| Teardown instance | exact internal teardown record | not compensable |
| Edit response | exact original-interaction response identity | not compensable |

## Durable recovery and admission

The PostgreSQL adapter exposes 22 C3 functions and four C3 tables. Eight guards
protect mutation and truncation of those new tables, and two integration
triggers protect the pre-existing receipt head and token-secret tables. Eleven
functions are granted to the narrow runtime interaction role; the remaining
helpers and all direct tables remain owner-only. The isolated
fresh catalog contains 198 owned user-schema relations and 135 capability
functions after migration 116,
`202608010001_add_runtime_interaction_effect_journal_v1.sql`.

Recovery candidates are split into sparse transient-head and required-rollback
paths. Separate partial indexes prevent permanent successful history from
becoming the periodic scan set. Static and instance route admission checks use
the same sparse unsafe-effect and required-rollback paths. Claim and intent
both enforce the route barrier, and intent takes the route-scoped advisory lock
before rechecking it. Concurrent different receipts therefore cannot both pass
the last safe boundary for one route.

Observation, compensation, and response-tail failures that cannot continue
safely are persisted with one of ten closed block reasons, including explicit
attempt-budget exhaustion. The reason, path,
receipt, action, claim, process, gateway, build, and certificate identities are
bound into a SQL-recomputed digest. Exact replay returns the same terminal
checkpoint; tampering or a path/reason mismatch performs no state transition.
An observation block requires durable rollback where applicable, a
compensation block requires the existing rollback, and an unrecoverable
response tail closes the receipt without inventing a delivered response.
Production scheduling has one authoritative contract: each observation,
compensation-attempt, and compensation-observation path has a database-enforced
hard cap of 64 attempts and a minimum one-second retry delay. The earlier pure
experimental retry policy is test-only and is not exported as a production
contract.

The production supervisor starts only after serving admission is revalidated
and stops before controller and database teardown. Its fixed bounds are a
15-second cadence, 64 candidates per page, 16 pages per sweep, concurrency 8,
a 5-second scan timeout, and a 10-second candidate timeout. Task liveness and
progress feed serving revalidation and ingress acknowledgement evidence; a
terminated task or three consecutive failed sweeps makes the runtime not ready.

## Verification

The final gate commands and exact result counts are recorded from the frozen
source candidate below. Ignored PostgreSQL tests use an isolated PostgreSQL
16.14 server and create disposable per-test databases and roles.

| Gate | Result |
| --- | --- |
| `cargo test --locked -p automation-runtime-interaction --all-targets` | 55 passed, 0 failed |
| `cargo test --locked -p automation-runtime -p automation-instance-teardown --all-targets` | runtime 240 passed; teardown 16 passed; 0 failed |
| `cargo test --locked -p starring-runtime --no-fail-fast` | 702 passed, 0 failed, 1 ignored |
| `cargo test --locked -p starring-db-bootstrap --lib` | 17 passed, 0 failed |
| interaction PostgreSQL non-ignored targets | 65 passed, 0 failed, 8 ignored real-database tests |
| interaction PostgreSQL ignored real-database target | 8 passed, 0 failed on isolated PostgreSQL 16.14 |
| `cargo test --locked --workspace` | 4,536 passed, 0 failed, 400 ignored across 336 suites |
| workspace Clippy with warnings denied | passed, 0 warnings |
| formatting, diff, and added-secret checks | passed |

## Phase D boundary

This checkpoint does not certify commercial operation. The complete acceptance
contract remains D1–D4 in the authoritative plan. Its C3-related remainder
includes:

- deployment of the combined C2/C3 runtime candidate and migration 116 to a
  controlled staging environment
- process termination at every journal checkpoint
- injected Discord 403, 404, 429, timeout, connection-loss, and malformed
  response paths
- injected database failure plus gateway, owner, controller, authority,
  binding, and writer-fence loss
- live proof that exact observations converge and unsupported correlations
  remain blocked without duplicate effects
- response-tail causal timestamp ordering and a dedicated close-known tamper
  cohort
- proof that deferred response finalization does not create unbounded repeated
  observation work
- duplicate delivery, route replacement, rollback, and gateway-disconnect
  cohorts
- disposable-database and disposable-guild product E2E with complete cleanup
- concurrency, saturation, load, soak, backup/restore, and non-interactive
  reboot cohorts
- the final 13-package PostgreSQL CI manifest, JavaScript/evaluation gates,
  operations closure, merge-candidate CI, and merged-main CI

No result in this document is a commercial release certificate.

Later scope classification: this dated list records everything that was still
open at C3. The current authoritative completion plan assigns bounded restart
and injected failure to D1, the exact disposable-guild external-failure and
recovery path to D2, and exact-tree release evidence to D3. Sustained load and
soak, disaster-recovery restore, and non-interactive host reboot are separate
production-rollout certificates. This note changes no C3 result or claim.
